//! Where the server finds a SQL on FHIR job's artifacts and terminology.
//!
//! [`ServerArtifacts`] is the server's [`ArtifactSource`] for the job graph
//! (`octofhir_sof::plan`): a canonical not supplied in `context` resolves to
//! a resource stored on this server (read as the caller), else one of an
//! installed package, else, for a ValueSet, the configured terminology
//! server's expansion by canonical, asked through the server's terminology
//! provider (the one `memberOf()` in views uses). Without a `|version`, two stored
//! resources are an ambiguity the request is rejected for (404); among
//! installed packages the version the server's package resolution selects is
//! used, and recorded.
//!
//! A ValueSet's membership is its own expansion when it carries one, else
//! computed by the strict [`Expander`] over [`ServerTerminology`] (`context`,
//! the server's resources, packages). Only a ValueSet published in an
//! installed package whose definition is beyond the local expander (a SNOMED
//! CT or LOINC filter, say) goes to the terminology server, by canonical; the
//! server's own and the caller's artifacts never leave it.
//!
//! The job records, per terminology dependency, what produced its content
//! (`octofhir_sof::relations::value_set_record`); the record is logged and
//! returned in the [`PROVENANCE_HEADER`] response header.

use async_trait::async_trait;
use octofhir_fhir_model::terminology::{ExpandedValueSet, TerminologyProvider};
use octofhir_sof::catalog::{
    RemoteFallback, compare_versions, provider_name, remote_origin, value_set_content,
};
use octofhir_sof::expansion::{
    ExpansionError, Lookup, Resolved, TerminologySource, split_canonical,
};
use octofhir_sof::plan::{
    ArtifactSource, ContextArtifacts, PlanError, TerminologyContent, matches_canonical,
};
use octofhir_sof::relations::MAX_RELATION_ROWS;
use serde_json::{Value, json};

use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

/// Response header carrying the terminology provenance of a `$sql-run` job
/// (a JSON array, one object per terminology dependency).
pub(crate) const PROVENANCE_HEADER: &str = "x-sof-terminology";

/// Key under which the handler hands the provenance to the router; never
/// part of a response body.
pub(crate) const PROVENANCE_KEY: &str = "__sofTerminologyProvenance";

/// Largest header value written; a longer record drops expansion parameters.
const MAX_HEADER_BYTES: usize = 8 * 1024;

/// Origin prefix of an artifact found in an installed package.
const PACKAGE_ORIGIN: &str = "package ";

/// The server's value set and code system lookups for one job.
pub(crate) struct ServerTerminology<'a> {
    pub state: &'a AppState,
    pub context: &'a OperationContext,
    /// `context` artifacts, preferred over the server's.
    pub inline: &'a ContextArtifacts,
}

#[async_trait]
impl TerminologySource for ServerTerminology<'_> {
    async fn find(
        &self,
        resource_type: &str,
        url: &str,
        version: Option<&str>,
    ) -> Result<Lookup, ExpansionError> {
        if let Some(found) = self.inline.find(Some(resource_type), url, version) {
            return Ok(Lookup::Found(Resolved {
                resource: found,
                origin: "context".into(),
            }));
        }
        let lookup = match stored(self.state, self.context, resource_type, url, version).await {
            Ok(Lookup::Missing) => packaged(resource_type, url, version).await,
            other => other,
        };
        lookup.map_err(|error| ExpansionError::Source(Box::new(error)))
    }
}

/// The server's [`ArtifactSource`] for one job.
pub(crate) struct ServerArtifacts<'a> {
    pub state: &'a AppState,
    pub context: &'a OperationContext,
}

#[async_trait]
impl ArtifactSource for ServerArtifacts<'_> {
    async fn artifact(&self, canonical: &str) -> Result<Resolved, PlanError> {
        let (url, version) = split_canonical(canonical);
        let not_found =
            |reason: String| PlanError::NotFound(format!("Dependency {canonical}: {reason}"));
        for resource_type in ["Library", "ViewDefinition", "ValueSet", "ConceptMap"] {
            match stored(self.state, self.context, resource_type, url, version)
                .await
                .map_err(host)?
            {
                Lookup::Found(resolved) => return Ok(resolved),
                Lookup::Ambiguous(reason) => return Err(not_found(reason)),
                Lookup::Missing => {}
            }
        }
        for resource_type in ["ValueSet", "ConceptMap"] {
            match packaged(resource_type, url, version).await.map_err(host)? {
                Lookup::Found(resolved) => return Ok(resolved),
                Lookup::Ambiguous(reason) => return Err(not_found(reason)),
                Lookup::Missing => {}
            }
        }
        // Not here: a value set the terminology server knows by canonical.
        let Some(provider) = terminology_server(self.state) else {
            return Err(not_found("not found".into()));
        };
        let server = provider_name(provider);
        match provider
            .expand_valueset_resource(url, version, Some(REMOTE_COUNT))
            .await
        {
            Ok(ExpandedValueSet::Found(value_set)) => Ok(Resolved {
                resource: value_set,
                origin: remote_origin(provider),
            }),
            Ok(ExpandedValueSet::Unknown) => Err(not_found(format!(
                "not found here nor on the terminology server {server}"
            ))),
            Err(error) => Err(not_found(format!(
                "not found here, and the terminology server {server} could not be asked: {error}"
            ))),
        }
    }

    async fn value_set(
        &self,
        label: &str,
        value_set: &Resolved,
        context: &ContextArtifacts,
    ) -> Result<TerminologyContent, PlanError> {
        let source = ServerTerminology {
            state: self.state,
            context: self.context,
            inline: context,
        };
        // Only a value set published in an installed package is named to the
        // terminology server; the server's own and the caller's never leave it.
        let remote = terminology_server(self.state)
            .filter(|_| may_expand_remotely(value_set))
            .map(|provider| RemoteFallback {
                provider,
                send_definitions: false,
            });
        value_set_content(label, value_set, &source, "this server", remote).await
    }
}

/// Members asked of the terminology server: as many as a relation may hold.
pub(crate) const REMOTE_COUNT: u32 = MAX_RELATION_ROWS as u32;

/// The configured terminology server, as the server's terminology provider.
pub(crate) fn terminology_server(state: &AppState) -> Option<&dyn TerminologyProvider> {
    if state.config.terminology.server_url.trim().is_empty() {
        return None;
    }
    state
        .terminology_provider
        .as_deref()
        .map(|provider| provider as &dyn TerminologyProvider)
}

fn host(error: OperationError) -> PlanError {
    PlanError::Host(Box::new(error))
}

/// A job graph error as the operations answer it: 400, 404, 422, or the
/// server's own error passed through.
pub(crate) fn plan_error(error: PlanError) -> OperationError {
    match error {
        PlanError::Invalid(message) => OperationError::InvalidParameters(message),
        PlanError::NotFound(message) => OperationError::NotFound(message),
        PlanError::Unprocessable(message) => super::sql_plan::unprocessable(message),
        PlanError::Host(error) => match error.downcast::<OperationError>() {
            Ok(error) => *error,
            Err(error) => OperationError::Internal(error.to_string()),
        },
    }
}

/// The server's `resource_type` with `url` (and `version`), as the caller
/// may read it; two matches are an ambiguity.
pub(crate) async fn stored(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Lookup, OperationError> {
    let matching = stored_all(state, context, resource_type, url, version, 2).await?;
    Ok(match matching.as_slice() {
        [] => Lookup::Missing,
        [one] => found_stored(resource_type, one),
        _ => Lookup::Ambiguous(match version {
            None => format!(
                "{url} matches more than one {resource_type} on this server; pin url|version"
            ),
            Some(version) => {
                format!("{url}|{version} matches more than one {resource_type} on this server")
            }
        }),
    })
}

/// Like [`stored`], but without a version the most recent version is
/// selected, as FHIR canonical resolution does (by `versionAlgorithm` where
/// declared, else comparing versions segment by segment); two resources with
/// the same version are still an ambiguity.
pub(crate) async fn stored_latest(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Lookup, OperationError> {
    if version.is_some() {
        return stored(state, context, resource_type, url, version).await;
    }
    let mut matching = stored_all(state, context, resource_type, url, None, MAX_VERSIONS).await?;
    matching.sort_by(compare_versions);
    Ok(match matching.as_slice() {
        [] => Lookup::Missing,
        [.., previous, latest] if previous["version"] == latest["version"] => {
            Lookup::Ambiguous(format!(
                "{url} has two {resource_type} resources with version {} on this server",
                latest["version"].as_str().unwrap_or("(none)")
            ))
        }
        [.., latest] => found_stored(resource_type, latest),
    })
}

/// Versions of one canonical considered when selecting the latest.
const MAX_VERSIONS: u32 = 100;

fn found_stored(resource_type: &str, resource: &Value) -> Lookup {
    Lookup::Found(Resolved {
        origin: format!(
            "{resource_type}/{}",
            resource["id"].as_str().unwrap_or_default()
        ),
        resource: resource.clone(),
    })
}

/// Up to `limit` of the server's `resource_type` with `url` (and `version`),
/// as the caller may read them.
async fn stored_all(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    url: &str,
    version: Option<&str>,
    limit: u32,
) -> Result<Vec<Value>, OperationError> {
    let registry = state.search_config.config().registry.clone();
    Ok(if registry.get(resource_type, "url").is_some() {
        let mut search = octofhir_storage::SearchParams::new()
            .with_count(limit)
            .with_param("url", url);
        if let Some(version) = version
            && registry.get(resource_type, "version").is_some()
        {
            search = search.with_param("version", version);
        }
        context
            .search(state, resource_type, &search)
            .await?
            .entries
            .into_iter()
            .map(|entry| entry.resource)
            .filter(|resource| matches_canonical(resource, url, version))
            .collect()
    } else {
        // No `url` search parameter for this type: find the ids, then read
        // each as the caller.
        let ids = octofhir_db_postgres::queries::sql_job::ids_by_canonical(
            &state.read_db_pool,
            resource_type,
            url,
            version,
            i64::from(limit),
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        let mut readable = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(stored) = context.read(state, resource_type, &id).await? {
                readable.push(stored.resource);
            }
        }
        readable
    })
}

/// `resource_type` with `url` (and `version`) from the installed packages.
pub(crate) async fn packaged(
    resource_type: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Lookup, OperationError> {
    let Some(manager) = crate::canonical::get_manager() else {
        return Ok(Lookup::Missing);
    };
    let result = manager
        .search()
        .await
        .resource_type(resource_type)
        .canonical_pattern(&format!("^{}$", regex::escape(url)))
        .limit(50)
        .execute()
        .await
        .map_err(|error| OperationError::Internal(format!("Package lookup of {url}: {error}")))?;
    // One candidate per version: the same artifact in several packages is
    // one artifact.
    let mut candidates: Vec<(Option<String>, Resolved)> = Vec::new();
    for found in result.resources {
        let content = found.resource.content;
        if !matches_canonical(&content, url, version) {
            continue;
        }
        let artifact_version = content["version"].as_str().map(str::to_owned);
        if candidates
            .iter()
            .any(|(known, _)| *known == artifact_version)
        {
            continue;
        }
        candidates.push((
            artifact_version,
            Resolved {
                origin: format!(
                    "{PACKAGE_ORIGIN}{}#{}",
                    found.index.package_name, found.index.package_version
                ),
                resource: content,
            },
        ));
    }
    if candidates.len() > 1 && version.is_none() {
        // Several installed versions (a core package and the terminology
        // package both publish many code systems): the version the server
        // resolves the bare canonical to everywhere else, and only an exact
        // match of it.
        if let Ok(chosen) = manager.resolve(url).await
            && chosen.resource.content["url"] == url
            && let Some(position) = candidates.iter().position(|(candidate, _)| {
                candidate.as_deref() == chosen.resource.content["version"].as_str()
            })
        {
            return Ok(Lookup::Found(candidates.swap_remove(position).1));
        }
    }
    Ok(match candidates.len() {
        0 => Lookup::Missing,
        1 => Lookup::Found(candidates.remove(0).1),
        _ => {
            let versions: Vec<String> = candidates
                .iter()
                .map(|(version, _)| version.clone().unwrap_or_else(|| "(none)".into()))
                .collect();
            Lookup::Ambiguous(format!(
                "{url} is installed in versions {}; pin url|version",
                versions.join(", ")
            ))
        }
    })
}

/// A published ValueSet (from a package, without an expansion) the local
/// expander could not determine may be expanded by the terminology server.
fn may_expand_remotely(value_set: &Resolved) -> bool {
    value_set.origin.starts_with(PACKAGE_ORIGIN)
        && value_set.resource.get("expansion").is_none()
        && value_set.resource["url"].is_string()
}

/// The provenance header value for `records`: ASCII JSON, at most
/// [`MAX_HEADER_BYTES`] (expansion parameters are dropped first, then
/// everything but each dependency's identity).
pub(crate) fn provenance_header(records: &[Value]) -> String {
    let full = ascii_json(&Value::Array(records.to_vec()));
    if full.len() <= MAX_HEADER_BYTES {
        return full;
    }
    let without_parameters: Vec<Value> = records
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if let Some(expansion) = record["expansion"].as_object_mut() {
                expansion.remove("parameter");
            }
            record["truncated"] = json!(true);
            record
        })
        .collect();
    let shorter = ascii_json(&Value::Array(without_parameters));
    if shorter.len() <= MAX_HEADER_BYTES {
        return shorter;
    }
    let identities: Vec<Value> = records
        .iter()
        .map(|record| {
            json!({
                "label": record["label"],
                "resourceType": record["resourceType"],
                "url": record["url"],
                "version": record["version"],
                "truncated": true,
            })
        })
        .collect();
    ascii_json(&Value::Array(identities))
}

/// JSON with every non-ASCII character escaped, as a header value requires.
fn ascii_json(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0_u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_is_ascii_and_bounded() {
        let record = json!({"label": "cvd", "url": "http://x/vs", "source": "context",
            "expansion": {"identifier": "urn:uuid:1", "parameter": [
                {"name": "displayLanguage", "valueCode": "ru"},
                {"name": "note", "valueString": "Сердце ✓ 𝄞"}]}});
        let header = provenance_header(std::slice::from_ref(&record));
        assert!(header.is_ascii());
        let back: Value = serde_json::from_str(&header).unwrap();
        assert_eq!(back[0], record);

        let mut large = record.clone();
        large["expansion"]["parameter"] = json!(vec![
            json!({"name": "version",
            "valueUri": "x".repeat(200)});
            100
        ]);
        let header = provenance_header(&[large]);
        assert!(header.len() <= MAX_HEADER_BYTES);
        let back: Value = serde_json::from_str(&header).unwrap();
        assert_eq!(back[0]["truncated"], true);
        assert_eq!(back[0]["expansion"]["identifier"], "urn:uuid:1");
    }
}
