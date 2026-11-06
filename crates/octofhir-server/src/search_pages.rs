//! Continuation of long searches. A POST `_search` can carry more criteria than fit in a
//! URL, and its paging links would have to repeat them all. Above the limit the links
//! carry a token instead, and the server keeps the query for a while.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use octofhir_api::{ApiError, BundleLink};
use sha2::{Digest, Sha256};

/// URLs longer than this are unsafe with common proxies and clients.
const MAX_LINK_QUERY: usize = 2048;

/// How long a search can be paged after its last page was read.
const PAGE_TTL: Duration = Duration::from_secs(1800);

/// Queries kept at most, oldest expiring first.
const MAX_PAGES: usize = 10_000;

/// Paging parameters that stay in the link.
const PAGE_CONTROLS: [&str; 3] = ["_count", "_offset", "_cursor"];

/// Queries of searches being paged, by token.
#[derive(Default)]
pub struct SearchPages(DashMap<String, (String, Instant)>);

impl SearchPages {
    /// The query behind `raw_q`'s `_page` token, with the paging parameters of the
    /// request, or `raw_q` itself when it has no token.
    pub(crate) fn expand(&self, raw_q: String) -> Result<String, ApiError> {
        let mut token = None;
        let mut rest = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in url::form_urlencoded::parse(raw_q.as_bytes()) {
            if name == "_page" {
                token = Some(value.into_owned());
            } else {
                rest.append_pair(&name, &value);
            }
        }
        let Some(token) = token else {
            return Ok(raw_q);
        };
        let now = Instant::now();
        let mut entry = self
            .0
            .get_mut(&token)
            .filter(|entry| entry.1 > now)
            .ok_or_else(|| ApiError::gone("This search is no longer available; run it again"))?;
        entry.1 = now + PAGE_TTL;
        let rest = rest.finish();
        Ok(if rest.is_empty() {
            entry.0.clone()
        } else {
            format!("{}&{rest}", entry.0)
        })
    }

    /// Swaps long links for a token holding the query.
    pub(crate) fn shorten(&self, links: &mut [BundleLink], raw_q: &str) {
        if links.iter().all(|link| link.url.len() <= MAX_LINK_QUERY) {
            return;
        }
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in url::form_urlencoded::parse(raw_q.as_bytes()) {
            if !PAGE_CONTROLS.contains(&name.as_ref()) && name != "_page" {
                query.append_pair(&name, &value);
            }
        }
        let query = query.finish();
        let token = hex::encode(&Sha256::digest(query.as_bytes())[..16]);
        let now = Instant::now();
        if self.0.len() >= MAX_PAGES {
            self.0.retain(|_, entry| entry.1 > now);
        }
        self.0.insert(token.clone(), (query, now + PAGE_TTL));
        for link in links {
            let Ok(mut url) = url::Url::parse(&link.url) else {
                continue;
            };
            let kept: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(name, _)| PAGE_CONTROLS.contains(&name.as_ref()))
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect();
            url.query_pairs_mut()
                .clear()
                .append_pair("_page", &token)
                .extend_pairs(kept);
            link.url = url.into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(relation: &str, url: String) -> BundleLink {
        BundleLink {
            relation: relation.into(),
            url,
        }
    }

    #[test]
    fn short_links_stay_as_they_are() {
        let pages = SearchPages::default();
        let url = "https://example.org/fhir/Patient?_count=2&name=a".to_owned();
        let mut links = [link("self", url.clone())];
        pages.shorten(&mut links, "name=a");
        assert_eq!(links[0].url, url);
    }

    #[test]
    fn long_links_become_a_token_that_expands_to_the_query() {
        let pages = SearchPages::default();
        let ids = (0..600)
            .map(|n| format!("id-{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let raw_q = format!("_id={ids}&_count=2");
        let url = format!("https://example.org/fhir/Patient?_count=2&_offset=2&_id={ids}");
        let mut links = [link("next", url)];
        pages.shorten(&mut links, &raw_q);
        assert!(links[0].url.len() < 200, "{}", links[0].url);
        let query = links[0].url.split_once('?').unwrap().1.to_owned();
        assert!(query.contains("_offset=2"));

        let expanded = pages.expand(query).unwrap();
        let pairs: Vec<_> = url::form_urlencoded::parse(expanded.as_bytes()).collect();
        assert!(
            pairs
                .iter()
                .any(|(name, value)| name == "_id" && *value == ids)
        );
        assert!(
            pairs
                .iter()
                .any(|(name, value)| name == "_offset" && value == "2")
        );
        assert!(!pairs.iter().any(|(name, _)| name == "_page"));
    }

    #[test]
    fn an_unknown_token_is_gone() {
        let pages = SearchPages::default();
        assert!(pages.expand("_page=abc".into()).is_err());
        assert_eq!(pages.expand("name=a".into()).unwrap(), "name=a");
    }
}
