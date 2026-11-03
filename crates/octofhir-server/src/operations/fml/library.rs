//! Maps and ConceptMaps from folders on disk (`fml.map_folders`), such as a checkout of the HL7
//! cross-version maps (`fhir-cross-version/input`), which are not published as a package.
//!
//! Every `.fml` / `.map` file is compiled and every `.json` StructureMap or ConceptMap read, in
//! the folders and their subfolders, once per server. They are found after the server's own
//! resources and the installed packages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fml_model::StructureMap;
use serde_json::Value;

use super::resolve::Published;

/// Folder entries read at most: a misconfigured root such as `/` must not walk the disk.
const MAX_FILES: usize = 50_000;

/// What the configured folders hold, by canonical URL, and what transforms found in the
/// installed packages.
#[derive(Default)]
pub(crate) struct Library {
    maps: HashMap<String, Arc<StructureMap>>,
    concept_maps: HashMap<String, Arc<Value>>,
    published: Published,
}

impl Library {
    /// Reads the folders. Files that do not compile or parse are skipped with a warning.
    pub(crate) fn load(folders: &[String]) -> Self {
        let mut library = Self::default();
        let mut files = Vec::new();
        for folder in folders {
            collect(Path::new(folder), &mut files);
        }
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match path.extension().and_then(|e| e.to_str()) {
                Some("fml" | "map") => {
                    let compiled = fml_model::compile(&text);
                    if compiled.has_errors() {
                        tracing::warn!(path = %path.display(), "FML map does not compile; skipped");
                        continue;
                    }
                    library.add_map(compiled.map);
                }
                Some("json") => {
                    let Ok(resource) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    match resource["resourceType"].as_str() {
                        Some("StructureMap") => match fml_model::from_json(resource) {
                            Ok(map) => library.add_map(map),
                            Err(error) => tracing::warn!(
                                path = %path.display(), %error, "StructureMap skipped"
                            ),
                        },
                        Some("ConceptMap") => library.add_concept_map(resource),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        tracing::info!(
            maps = library.maps.len(),
            concept_maps = library.concept_maps.len(),
            "FML map folders loaded"
        );
        library
    }

    fn add_map(&mut self, map: StructureMap) {
        if let Some(url) = map.url.clone() {
            self.maps.entry(url).or_insert_with(|| Arc::new(map));
        }
    }

    /// A ConceptMap by its URL, `url|version`, and the canonicals it replaces.
    fn add_concept_map(&mut self, resource: Value) {
        let resource = Arc::new(resource);
        let mut keys = Vec::new();
        if let Some(url) = resource["url"].as_str() {
            keys.push(url.to_owned());
            if let Some(version) = resource["version"].as_str() {
                keys.push(format!("{url}|{version}"));
            }
        }
        for artifact in resource["relatedArtifact"].as_array().into_iter().flatten() {
            if artifact["type"] == "predecessor"
                && let Some(predecessor) = artifact["resource"].as_str()
            {
                keys.push(predecessor.to_owned());
            }
        }
        for key in keys {
            self.concept_maps
                .entry(key)
                .or_insert_with(|| resource.clone());
        }
    }

    /// Cached answers about the packages and these folders.
    pub(crate) fn published(&self) -> &Published {
        &self.published
    }

    pub(crate) fn map(&self, canonical: &str) -> Option<Arc<StructureMap>> {
        let url = canonical.split('|').next().unwrap_or(canonical);
        self.maps.get(url).cloned()
    }

    /// The maps whose URL `matches`.
    pub(crate) fn maps_matching<'a>(
        &'a self,
        matches: impl Fn(&str) -> bool + 'a,
    ) -> impl Iterator<Item = Arc<StructureMap>> + 'a {
        self.maps
            .iter()
            .filter(move |(url, _)| matches(url))
            .map(|(_, map)| map.clone())
    }

    pub(crate) fn concept_map(&self, canonical: &str) -> Option<Arc<Value>> {
        self.concept_maps
            .get(canonical)
            .or_else(|| {
                self.concept_maps
                    .get(canonical.split('|').next().unwrap_or(canonical))
            })
            .cloned()
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        tracing::warn!(folder = %dir.display(), "FML map folder cannot be read");
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.') || n == "node_modules");
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}
