//! Where `$sql-run` finds its dependencies and terminology, and how a
//! terminology dependency becomes a relation with its provenance.
//!
//! A canonical resolves, in order, to an artifact supplied in `context`, a
//! resource stored on this server (read as the caller), or a resource of an
//! installed package. Without a `|version`, two stored resources are an
//! ambiguity the request is rejected for (404); among installed packages the
//! version the server's package resolution selects is used, and recorded.
//!
//! A ValueSet's membership is its own expansion when it carries one, else
//! computed by [`Expander`] from its definition. Only a ValueSet published in
//! an installed package whose definition is beyond the local expander (a
//! SNOMED CT or LOINC filter, say) is expanded by the configured terminology
//! server, by canonical: the server's own and the caller's artifacts never
//! leave it.
//!
//! The job records, per terminology dependency, what produced its content:
//! the artifact's canonical and source, and for a value set the expansion's
//! identifier, timestamp and parameters (code system versions, imported
//! value sets). The record is logged and returned in the
//! [`PROVENANCE_HEADER`] response header.

use std::time::Duration;

use async_trait::async_trait;
use octofhir_api::ApiError;
use serde_json::{Value, json};

use super::expansion::{Expander, Lookup, Resolved, TerminologySource};
use super::terminology::{
    CONCEPT_MAP_COLUMNS, MAX_RELATION_ROWS, VALUE_SET_COLUMNS, columns, concept_map_rows,
    value_set_rows,
};
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

/// A dependency's relation: its columns, rows and provenance record.
pub(crate) struct TerminologyRelation {
    pub columns: Vec<(String, &'static str)>,
    pub rows: Vec<Value>,
    pub provenance: Value,
}

/// The server's terminology lookups for one job.
pub(crate) struct ServerTerminology<'a> {
    pub state: &'a AppState,
    pub context: &'a OperationContext,
    /// `context` artifacts, preferred over the server's.
    pub inline: &'a [Value],
}

#[async_trait]
impl TerminologySource for ServerTerminology<'_> {
    async fn find(
        &self,
        resource_type: &str,
        url: &str,
        version: Option<&str>,
    ) -> Result<Lookup, OperationError> {
        if let Some(found) = self.inline.iter().find(|artifact| {
            artifact["resourceType"] == resource_type && matches_canonical(artifact, url, version)
        }) {
            return Ok(Lookup::Found(Resolved {
                resource: found.clone(),
                origin: "context".into(),
            }));
        }
        match stored(self.state, self.context, resource_type, url, version).await? {
            Lookup::Missing => packaged(resource_type, url, version).await,
            found => Ok(found),
        }
    }
}

pub(crate) fn matches_canonical(artifact: &Value, url: &str, version: Option<&str>) -> bool {
    artifact["url"] == url && version.is_none_or(|version| artifact["version"] == version)
}

/// The server's `resource_type` with `url` (and `version`), as the caller
/// may read it.
pub(crate) async fn stored(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Lookup, OperationError> {
    let registry = state.search_config.config().registry.clone();
    let matching: Vec<Value> = if registry.get(resource_type, "url").is_some() {
        let mut search = octofhir_storage::SearchParams::new()
            .with_count(2)
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
        // No `url` search parameter for this type (ViewDefinition is a
        // logical model): find the ids, then read each as the caller.
        let ids = octofhir_db_postgres::queries::sql_job::ids_by_canonical(
            &state.read_db_pool,
            resource_type,
            url,
            version,
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
    };
    Ok(match matching.as_slice() {
        [] => Lookup::Missing,
        [one] => Lookup::Found(Resolved {
            origin: format!("{resource_type}/{}", one["id"].as_str().unwrap_or_default()),
            resource: one.clone(),
        }),
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

/// The relation of a ValueSet dependency `label`.
pub(crate) async fn value_set_relation(
    source: &ServerTerminology<'_>,
    label: &str,
    value_set: &Resolved,
) -> Result<TerminologyRelation, OperationError> {
    let resource = &value_set.resource;
    let named = canonical_of(resource);
    let expanded_by = if resource.get("expansion").is_some() {
        "the artifact's expansion"
    } else {
        "this server"
    };
    let expansion = Expander::new(source).expand(resource).await;
    let (expansion, expanded_by) = match expansion {
        Ok(expansion) => (expansion, expanded_by.to_owned()),
        Err(local) if may_expand_remotely(value_set, &local) => {
            let server = source
                .state
                .config
                .terminology
                .server_url
                .trim_end_matches('/');
            match remote_expansion(server, resource).await {
                Ok(expansion) => (expansion, format!("terminology server {server}")),
                Err(remote) => {
                    return Err(unprocessable(format!(
                        "ValueSet {named} ({label}): {}; the terminology server {server} could not expand it either: {remote}",
                        message(&local)
                    )));
                }
            }
        }
        Err(error) => {
            return Err(unprocessable(format!(
                "ValueSet {named} ({label}): {}",
                message(&error)
            )));
        }
    };
    let rows = value_set_rows(&expansion)
        .map_err(|reason| unprocessable(format!("ValueSet {named} ({label}): {reason}")))?;
    let mut record = json!({
        "label": label,
        "resourceType": "ValueSet",
        "url": resource["url"],
        "source": value_set.origin,
        "expandedBy": expanded_by,
        "members": rows.len(),
    });
    if let Some(version) = resource["version"].as_str() {
        record["version"] = json!(version);
    }
    let mut provenance_expansion = serde_json::Map::new();
    for field in ["identifier", "timestamp", "parameter"] {
        if let Some(value) = expansion.get(field) {
            provenance_expansion.insert(field.to_owned(), value.clone());
        }
    }
    record["expansion"] = Value::Object(provenance_expansion);
    Ok(TerminologyRelation {
        columns: columns(&VALUE_SET_COLUMNS),
        rows,
        provenance: record,
    })
}

/// The relation of a ConceptMap dependency `label`.
pub(crate) fn concept_map_relation(
    label: &str,
    concept_map: &Resolved,
) -> Result<TerminologyRelation, OperationError> {
    let resource = &concept_map.resource;
    let rows = concept_map_rows(resource).map_err(|reason| {
        unprocessable(format!(
            "ConceptMap {} ({label}): {reason}",
            canonical_of(resource)
        ))
    })?;
    let mut record = json!({
        "label": label,
        "resourceType": "ConceptMap",
        "url": resource["url"],
        "source": concept_map.origin,
        "mappings": rows.len(),
    });
    if let Some(version) = resource["version"].as_str() {
        record["version"] = json!(version);
    }
    Ok(TerminologyRelation {
        columns: columns(&CONCEPT_MAP_COLUMNS),
        rows,
        provenance: record,
    })
}

/// A published ValueSet (from a package, without an expansion) the local
/// expander could not determine may be expanded by the terminology server.
fn may_expand_remotely(value_set: &Resolved, local: &OperationError) -> bool {
    value_set.origin.starts_with(PACKAGE_ORIGIN)
        && value_set.resource.get("expansion").is_none()
        && value_set.resource["url"].is_string()
        && matches!(local, OperationError::Api(_))
}

/// The expansion of a published ValueSet from the terminology server at
/// `server`, by canonical.
async fn remote_expansion(server: &str, value_set: &Value) -> Result<Value, String> {
    if server.is_empty() {
        return Err("no terminology server is configured".into());
    }
    let url = value_set["url"].as_str().unwrap_or_default();
    let mut query: Vec<(&str, String)> = vec![
        ("url", url.to_owned()),
        ("count", MAX_RELATION_ROWS.to_string()),
    ];
    if let Some(version) = value_set["version"].as_str() {
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
    if !status.is_success() {
        return Err(format!("it answered {status}"));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("its answer is not JSON: {error}"))?;
    if body["resourceType"] != "ValueSet" {
        return Err("its answer is not a ValueSet".into());
    }
    body.get("expansion")
        .cloned()
        .ok_or_else(|| "its answer has no expansion".into())
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

/// The text of an error, without the status wording.
fn message(error: &OperationError) -> String {
    match error {
        OperationError::Api(ApiError::UnprocessableEntity { message, .. }) => message.clone(),
        OperationError::Api(api) => api.to_string(),
        other => other.to_string(),
    }
}

fn unprocessable(message: String) -> OperationError {
    OperationError::Api(ApiError::unprocessable_entity(message, None))
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
