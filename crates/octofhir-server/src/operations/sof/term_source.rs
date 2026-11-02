//! Where the server finds a SQL on FHIR job's artifacts and terminology.
//!
//! [`ServerArtifacts`] is the server's [`ArtifactSource`] for the job graph
//! (`octofhir_sof::plan`): a canonical not supplied in `context` resolves to
//! a resource stored on this server (read as the caller), else one of an
//! installed package, else, for a ValueSet, the configured terminology
//! server's expansion by canonical. Without a `|version`, two stored
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

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use octofhir_sof::expansion::{
    Expander, ExpansionError, ExpansionStamp, Lookup, Resolved, TerminologySource, split_canonical,
};
use octofhir_sof::plan::{
    ArtifactSource, ContextArtifacts, PlanError, TerminologyContent, matches_canonical,
};
use octofhir_sof::relations::{MAX_RELATION_ROWS, value_set_record, value_set_rows};
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

/// Time a terminology server has to return an expansion.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);

/// Origin prefix of an artifact found in an installed package.
const PACKAGE_ORIGIN: &str = "package ";

/// Origin prefix of a ValueSet the terminology server expanded.
const REMOTE_ORIGIN: &str = "terminology server ";

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
        let server = terminology_server(self.state);
        if server.is_empty() {
            return Err(not_found("not found".into()));
        }
        match remote_value_set(server, url, version).await {
            Ok(Remote::Expanded(value_set)) => Ok(Resolved {
                resource: value_set,
                origin: remote_origin(server),
            }),
            Ok(Remote::Unknown) => Err(not_found(format!(
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
        let resource = &value_set.resource;
        let named = canonical_of(resource);
        let source = ServerTerminology {
            state: self.state,
            context: self.context,
            inline: context,
        };
        let expanded_by = if value_set.origin.starts_with(REMOTE_ORIGIN) {
            value_set.origin.clone()
        } else if resource.get("expansion").is_some() {
            "the artifact's expansion".to_owned()
        } else {
            "this server".to_owned()
        };
        let (expansion, expanded_by) = match Expander::new(&source)
            .expand(resource, &expansion_stamp())
            .await
        {
            Ok(expansion) => (expansion, expanded_by),
            Err(ExpansionError::Undeterminable(local)) if may_expand_remotely(value_set) => {
                let server = terminology_server(self.state);
                let answer = remote_value_set(
                    server,
                    resource["url"].as_str().unwrap_or_default(),
                    resource["version"].as_str(),
                )
                .await;
                match answer {
                    Ok(Remote::Expanded(remote)) => {
                        (remote["expansion"].clone(), remote_origin(server))
                    }
                    Ok(Remote::Unknown) => {
                        return Err(PlanError::Unprocessable(format!(
                            "ValueSet {named} ({label}): membership cannot be determined: {local}; the terminology server {server} does not know it"
                        )));
                    }
                    Err(remote) => {
                        return Err(PlanError::Unprocessable(format!(
                            "ValueSet {named} ({label}): membership cannot be determined: {local}; the terminology server {server} could not expand it either: {remote}"
                        )));
                    }
                }
            }
            Err(ExpansionError::Undeterminable(reason)) => {
                return Err(PlanError::Unprocessable(format!(
                    "ValueSet {named} ({label}): membership cannot be determined: {reason}"
                )));
            }
            Err(ExpansionError::Source(error)) => return Err(PlanError::Host(error)),
        };
        let rows = value_set_rows(&expansion).map_err(|reason| {
            PlanError::Unprocessable(format!("ValueSet {named} ({label}): {reason}"))
        })?;
        let provenance = value_set_record(
            label,
            resource,
            &value_set.origin,
            &expanded_by,
            &expansion,
            rows.len(),
        );
        Ok(TerminologyContent { rows, provenance })
    }
}

/// The identity of an expansion this server computes.
pub(crate) fn expansion_stamp() -> ExpansionStamp {
    ExpansionStamp {
        identifier: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
        timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
    }
}

/// The configured terminology server, without a trailing slash.
fn terminology_server(state: &AppState) -> &str {
    state.config.terminology.server_url.trim_end_matches('/')
}

/// A server error carried through the job graph as it is.
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

/// Order two versions of one artifact: by its `versionAlgorithm` (`semver`,
/// `integer`, `date`, `alpha`, `natural`) when both declare the same, else
/// naturally (numeric segments as numbers).
fn compare_versions(a: &Value, b: &Value) -> std::cmp::Ordering {
    let algorithm = |resource: &Value| {
        resource["versionAlgorithmString"]
            .as_str()
            .or_else(|| resource["versionAlgorithmCoding"]["code"].as_str())
            .map(str::to_owned)
    };
    let (va, vb) = (
        a["version"].as_str().unwrap_or_default(),
        b["version"].as_str().unwrap_or_default(),
    );
    let natural = || {
        let segments = |v: &str| -> Vec<(u64, String)> {
            v.split(['.', '-', '+'])
                .map(|part| (part.parse::<u64>().unwrap_or(0), part.to_owned()))
                .collect()
        };
        segments(va).cmp(&segments(vb))
    };
    match (algorithm(a), algorithm(b)) {
        (Some(x), Some(y)) if x == y => match x.as_str() {
            "semver" => match (
                semver::Version::parse(va.trim_start_matches('v')),
                semver::Version::parse(vb.trim_start_matches('v')),
            ) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                _ => natural(),
            },
            "integer" => match (va.parse::<i64>(), vb.parse::<i64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                _ => natural(),
            },
            "date" | "alpha" => va.cmp(vb),
            _ => natural(),
        },
        _ => natural(),
    }
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

/// What the terminology server answered for a ValueSet canonical.
pub(crate) enum Remote {
    /// The ValueSet, carrying its expansion.
    Expanded(Value),
    /// The server does not know the canonical (404 or 410).
    Unknown,
}

/// The ValueSet `url` (`version`) expanded by the terminology server at
/// `server`, by canonical.
pub(crate) async fn remote_value_set(
    server: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Remote, String> {
    if server.is_empty() {
        return Err("no terminology server is configured".into());
    }
    let mut query: Vec<(&str, String)> = vec![
        ("url", url.to_owned()),
        ("count", MAX_RELATION_ROWS.to_string()),
    ];
    if let Some(version) = version {
        query.push(("valueSetVersion", version.to_owned()));
    }
    let client = reqwest::Client::builder()
        .timeout(REMOTE_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("{server}/ValueSet/$expand"))
        .query(&query)
        .header(reqwest::header::ACCEPT, "application/fhir+json")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if matches!(status.as_u16(), 404 | 410) {
        return Ok(Remote::Unknown);
    }
    if !status.is_success() {
        return Err(format!("it answered {status}"));
    }
    let mut body: Value = response
        .json()
        .await
        .map_err(|error| format!("its answer is not JSON: {error}"))?;
    if body["resourceType"] != "ValueSet" || body.get("expansion").is_none() {
        return Err("its answer is not an expanded ValueSet".into());
    }
    // The answer names the artifact the caller asked for.
    body["url"] = Value::String(url.to_owned());
    if let Some(version) = version
        && !body["version"].is_string()
    {
        body["version"] = Value::String(version.to_owned());
    }
    Ok(Remote::Expanded(body))
}

/// The origin of an artifact the terminology server at `server` supplied.
pub(crate) fn remote_origin(server: &str) -> String {
    format!("{REMOTE_ORIGIN}{server}")
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

fn canonical_of(resource: &Value) -> String {
    match (resource["url"].as_str(), resource["version"].as_str()) {
        (Some(url), Some(version)) => format!("{url}|{version}"),
        (Some(url), None) => url.to_owned(),
        _ => "(inline, without url)".to_owned(),
    }
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
