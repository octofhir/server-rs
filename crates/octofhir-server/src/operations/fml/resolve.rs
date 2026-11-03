//! Finding what a transform needs before it runs: StructureMaps by canonical (with the maps they
//! import), the StructureDefinitions their `uses` name, and ConceptMaps for `translate()`.
//!
//! Everything is looked up as the caller: the server's own resources through
//! [`OperationContext`] (so grants and policies apply), then the installed packages. The engine
//! itself is synchronous about maps and definitions, so they are gathered here up front.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fml_check::{Definitions, PackageDefinitions, Structure};
use fml_model::StructureMap;
use octofhir_sof::expansion::{Lookup, split_canonical};
use serde_json::Value;

use super::library::Library;
use crate::operations::sof::term_source::{packaged, stored_latest};
use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

/// Maps one transform may load through `imports`, so a wildcard cannot pull in without end.
const MAX_MAPS: usize = 2_000;

/// A StructureMap resource as the engine's model.
pub(crate) fn map_from_json(resource: Value, origin: &str) -> Result<StructureMap, OperationError> {
    fml_model::from_json(resource).map_err(|error| {
        OperationError::InvalidParameters(format!("{origin} is not a valid StructureMap: {error}"))
    })
}

/// The canonical resource `canonical` (`url` or `url|version`) of `resource_type`: the server's
/// own first, then the installed packages. `Ok(None)` when neither has it.
pub(crate) async fn canonical_resource(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    canonical: &str,
) -> Result<Option<Value>, OperationError> {
    let (url, version) = split_canonical(canonical);
    for lookup in [
        stored_latest(state, context, resource_type, url, version).await?,
        packaged(resource_type, url, version).await?,
    ] {
        match lookup {
            Lookup::Found(found) => return Ok(Some(found.resource)),
            Lookup::Ambiguous(reason) => {
                return Err(OperationError::InvalidParameters(format!(
                    "{resource_type} {reason}"
                )));
            }
            Lookup::Missing => {}
        }
    }
    Ok(None)
}

/// The StructureMap `canonical`: the server's own, then the installed packages, then the
/// configured map folders.
pub(crate) async fn structure_map(
    state: &AppState,
    context: &OperationContext,
    library: &Library,
    canonical: &str,
) -> Result<Option<Arc<StructureMap>>, OperationError> {
    if let Some(resource) = stored_resource(state, context, "StructureMap", canonical).await? {
        return Ok(Some(Arc::new(map_from_json(
            resource,
            &format!("StructureMap {canonical}"),
        )?)));
    }
    library
        .published()
        .structure_map(canonical, || async {
            if let Some(resource) = packaged_resource("StructureMap", canonical).await? {
                return Ok(Some(Arc::new(map_from_json(
                    resource,
                    &format!("StructureMap {canonical}"),
                )?)));
            }
            Ok(library.map(canonical))
        })
        .await
}

/// The ConceptMap `canonical`, from the same places as [`structure_map`].
pub(crate) async fn concept_map(
    state: &AppState,
    context: &OperationContext,
    library: &Library,
    canonical: &str,
) -> Result<Option<Arc<Value>>, OperationError> {
    if let Some(resource) = stored_resource(state, context, "ConceptMap", canonical).await? {
        return Ok(Some(Arc::new(resource)));
    }
    library
        .published()
        .concept_map(canonical, || async {
            if let Some(resource) = packaged_resource("ConceptMap", canonical).await? {
                return Ok(Some(Arc::new(resource)));
            }
            Ok(library.concept_map(canonical))
        })
        .await
}

/// The server's own `resource_type` with `canonical`, as the caller may read it.
async fn stored_resource(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    canonical: &str,
) -> Result<Option<Value>, OperationError> {
    let (url, version) = split_canonical(canonical);
    found(
        resource_type,
        stored_latest(state, context, resource_type, url, version).await?,
    )
}

/// `resource_type` with `canonical` from the installed packages.
async fn packaged_resource(
    resource_type: &str,
    canonical: &str,
) -> Result<Option<Value>, OperationError> {
    let (url, version) = split_canonical(canonical);
    found(resource_type, packaged(resource_type, url, version).await?)
}

fn found(resource_type: &str, lookup: Lookup) -> Result<Option<Value>, OperationError> {
    match lookup {
        Lookup::Found(found) => Ok(Some(found.resource)),
        Lookup::Ambiguous(reason) => Err(OperationError::InvalidParameters(format!(
            "{resource_type} {reason}"
        ))),
        Lookup::Missing => Ok(None),
    }
}

/// The StructureMaps an `imports` pattern names: an exact canonical, or one with `*` standing
/// for any run of characters (`http://hl7.org/fhir/uv/xver/StructureMap/*4to5`).
async fn imported_maps(
    state: &AppState,
    context: &OperationContext,
    library: &Library,
    pattern: &str,
) -> Result<Vec<Arc<StructureMap>>, OperationError> {
    if !pattern.contains('*') {
        return Ok(structure_map(state, context, library, pattern)
            .await?
            .into_iter()
            .collect());
    }
    let matcher = regex::Regex::new(&wildcard_regex(pattern)).map_err(|error| {
        OperationError::InvalidParameters(format!("imports `{pattern}`: {error}"))
    })?;
    let mut found: Vec<Value> = Vec::new();
    // The server's own, looked up each time: everything below the fixed part of the pattern,
    // filtered.
    let prefix = &pattern[..pattern.find('*').unwrap_or(pattern.len())];
    let below = &prefix[..prefix.rfind('/').map_or(0, |slash| slash + 1)];
    if !below.is_empty() {
        let search = octofhir_storage::SearchParams::new()
            .with_count(MAX_MAPS as u32)
            .with_param("url:below", below.trim_end_matches('/'));
        for entry in context
            .search(state, "StructureMap", &search)
            .await?
            .entries
        {
            if entry.resource["url"]
                .as_str()
                .is_some_and(|url| matcher.is_match(url))
            {
                found.push(entry.resource);
            }
        }
    }
    let mut maps = found
        .into_iter()
        .map(|resource| {
            let origin = format!(
                "StructureMap {}",
                resource["url"].as_str().unwrap_or_default()
            );
            map_from_json(resource, &origin).map(Arc::new)
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The installed packages and the configured map folders, which change rarely: cached.
    maps.extend(
        library
            .published()
            .matching(pattern, || async {
                let mut maps = Vec::new();
                if let Some(manager) = crate::canonical::get_manager() {
                    let result = manager
                        .search()
                        .await
                        .resource_type("StructureMap")
                        .canonical_pattern(&wildcard_regex(pattern))
                        .limit(MAX_MAPS)
                        .execute()
                        .await
                        .map_err(|error| {
                            OperationError::Internal(format!(
                                "Package lookup of {pattern}: {error}"
                            ))
                        })?;
                    for found in result.resources {
                        let resource = found.resource.content;
                        let origin = format!(
                            "StructureMap {}",
                            resource["url"].as_str().unwrap_or_default()
                        );
                        maps.push(Arc::new(map_from_json(resource, &origin)?));
                    }
                }
                maps.extend(library.maps_matching(|url| matcher.is_match(url)));
                Ok(maps)
            })
            .await?
            .iter()
            .cloned(),
    );
    Ok(maps)
}

/// What the installed packages and the map folders hold, as far as transforms have asked,
/// kept for a while. Packages change rarely, and looking into them is slow (every search reads
/// the whole package index), so a transform with a dozen `translate()` calls and an import of a
/// hundred and fifty maps would spend most of its time there. Answers that nothing was found are
/// kept too. The server's own resources are never cached.
#[derive(Default)]
pub(crate) struct Published {
    patterns: Cache<Arc<Vec<Arc<StructureMap>>>>,
    maps: Cache<Option<Arc<StructureMap>>>,
    concept_maps: Cache<Option<Arc<Value>>>,
}

/// How long [`Published`] keeps an answer: a package installed meanwhile shows after this.
const PUBLISHED_TTL: std::time::Duration = std::time::Duration::from_secs(300);

type Cache<T> = tokio::sync::Mutex<HashMap<String, (std::time::Instant, T)>>;

async fn cached<T, F, Fut>(cache: &Cache<T>, key: &str, load: F) -> Result<T, OperationError>
where
    T: Clone,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, OperationError>>,
{
    if let Some((at, value)) = cache.lock().await.get(key)
        && at.elapsed() < PUBLISHED_TTL
    {
        return Ok(value.clone());
    }
    let value = load().await?;
    cache
        .lock()
        .await
        .insert(key.to_owned(), (std::time::Instant::now(), value.clone()));
    Ok(value)
}

impl Published {
    /// The maps an `imports` pattern matches.
    async fn matching<F, Fut>(
        &self,
        pattern: &str,
        load: F,
    ) -> Result<Arc<Vec<Arc<StructureMap>>>, OperationError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Arc<StructureMap>>, OperationError>>,
    {
        cached(&self.patterns, pattern, || async {
            Ok(Arc::new(load().await?))
        })
        .await
    }

    async fn structure_map<F, Fut>(
        &self,
        canonical: &str,
        load: F,
    ) -> Result<Option<Arc<StructureMap>>, OperationError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Option<Arc<StructureMap>>, OperationError>>,
    {
        cached(&self.maps, canonical, load).await
    }

    async fn concept_map<F, Fut>(
        &self,
        canonical: &str,
        load: F,
    ) -> Result<Option<Arc<Value>>, OperationError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Option<Arc<Value>>, OperationError>>,
    {
        cached(&self.concept_maps, canonical, load).await
    }
}

/// `*` as any run of characters, everything else literal, anchored.
fn wildcard_regex(pattern: &str) -> String {
    let parts: Vec<String> = pattern.split('*').map(regex::escape).collect();
    format!("^{}$", parts.join(".*"))
}

/// The maps a transform can reach: `maps` and, transitively, everything they import. A map the
/// server has twice under one URL is taken once, the first found.
pub(crate) async fn with_imports(
    state: &AppState,
    context: &OperationContext,
    library: &Library,
    maps: Vec<StructureMap>,
) -> Result<Vec<Arc<StructureMap>>, OperationError> {
    let mut known: HashSet<String> = maps.iter().filter_map(|m| m.url.clone()).collect();
    let mut done_patterns: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = maps.iter().flat_map(|m| m.import.clone()).collect();
    let mut all: Vec<Arc<StructureMap>> = maps.into_iter().map(Arc::new).collect();
    while let Some(pattern) = pending.pop() {
        if !done_patterns.insert(pattern.clone()) {
            continue;
        }
        for map in imported_maps(state, context, library, &pattern).await? {
            let url = map.url.clone().unwrap_or_default();
            if url.is_empty() || !known.insert(url) {
                continue;
            }
            pending.extend(map.import.iter().cloned());
            all.push(map);
            if all.len() > MAX_MAPS {
                return Err(OperationError::Api(
                    octofhir_api::ApiError::unprocessable_entity(
                        format!("the map imports more than {MAX_MAPS} maps"),
                        None,
                    ),
                ));
            }
        }
    }
    Ok(all)
}

/// The structure definitions of a run: the core release, and the StructureDefinitions of the
/// server or its packages for every other structure the maps use (logical models, profiles).
pub(crate) async fn definitions(
    state: &AppState,
    context: &OperationContext,
    core: Arc<fml_check::CoreDefinitions>,
    maps: &[Arc<StructureMap>],
    models: &Models,
) -> Result<PackageDefinitions<SharedCore>, OperationError> {
    let core = SharedCore(core);
    let mut definitions = PackageDefinitions::new(core.clone());
    let mut seen: HashSet<String> = HashSet::new();
    // The request's own models come first, and are not looked up again.
    for resource in &models.structure_definitions {
        definitions
            .add(resource)
            .map_err(OperationError::InvalidParameters)?;
        if let Some(url) = resource["url"].as_str() {
            seen.insert(url.to_owned());
        }
    }
    let mut pending: Vec<String> = maps
        .iter()
        .flat_map(|map| map.structure.iter().map(|s| s.url.clone()))
        .collect();
    while let Some(url) = pending.pop() {
        if !seen.insert(url.clone()) || core.covers(&url) || core.structure(&url).is_some() {
            continue;
        }
        let Some(resource) =
            canonical_resource(state, context, "StructureDefinition", &url).await?
        else {
            // Unknown structures stay unknown: the engine works untyped there.
            continue;
        };
        if let Some(base) = resource["baseDefinition"].as_str() {
            pending.push(base.to_owned());
        }
        definitions
            .add(&resource)
            .map_err(OperationError::InvalidParameters)?;
    }
    Ok(definitions)
}

/// Definitions a request brings along (`model`, the IG debug page's input for isolated
/// debugging): StructureDefinitions of logical models and ConceptMaps, alone or in Bundles.
#[derive(Default)]
pub(crate) struct Models {
    pub(crate) structure_definitions: Vec<Value>,
    pub(crate) concept_maps: fml_engine::ConceptMapSet,
}

impl Models {
    /// Adds a StructureDefinition, a ConceptMap, or every one of those in a Bundle.
    pub(crate) fn add(&mut self, resource: Value) -> Result<(), OperationError> {
        match resource["resourceType"].as_str() {
            Some("StructureDefinition") => self.structure_definitions.push(resource),
            Some("ConceptMap") => {
                self.concept_maps.add(resource);
            }
            Some("Bundle") => {
                for entry in resource["entry"].as_array().into_iter().flatten() {
                    if let Some(inner) = entry.get("resource") {
                        self.add(inner.clone())?;
                    }
                }
            }
            other => {
                return Err(OperationError::InvalidParameters(format!(
                    "`model` takes StructureDefinitions, ConceptMaps or a Bundle of them, not {}",
                    other.unwrap_or("this")
                )));
            }
        }
        Ok(())
    }
}

/// The core definitions, shared between requests.
#[derive(Clone)]
pub(crate) struct SharedCore(Arc<fml_check::CoreDefinitions>);

impl Definitions for SharedCore {
    fn structure(&self, url: &str) -> Option<Arc<Structure>> {
        self.0.structure(url)
    }

    fn covers(&self, url: &str) -> bool {
        self.0.covers(url)
    }
}

/// ConceptMaps found during one transform, by canonical.
pub(crate) type ConceptMapCache = HashMap<String, Option<Arc<Value>>>;
