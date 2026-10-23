//! Deterministic search parameter registry with immutable lookup snapshots.
//!
//! Readers use ArcSwap without a lock. Writers rebuild all indices together;
//! bootstrap registers a batch so loading packages does not copy per parameter.

use arc_swap::ArcSwap;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::parameters::SearchParameter;

#[derive(Debug, Default, Clone)]
struct RegistryData {
    declarations: HashMap<String, Arc<SearchParameter>>,
    resources: HashMap<String, (i64, Option<Arc<SearchParameter>>)>,
    by_url: HashMap<String, Arc<SearchParameter>>,
    by_resource: HashMap<(String, String), Arc<SearchParameter>>,
    by_type: HashMap<String, Vec<Arc<SearchParameter>>>,
    common: HashMap<String, Arc<SearchParameter>>,
    declared: HashSet<(String, String)>,
}

impl RegistryData {
    fn rebuild(&mut self) {
        self.by_url.clone_from(&self.declarations);
        let mut resources: Vec<_> = self.resources.iter().collect();
        resources.sort_by(|a, b| a.1.0.cmp(&b.1.0).then_with(|| a.0.cmp(b.0)));
        for (_, (_, parameter)) in resources {
            if let Some(parameter) = parameter {
                self.by_url.insert(parameter.url.clone(), parameter.clone());
            }
        }
        self.by_resource.clear();
        self.by_type.clear();
        self.common.clear();
        self.declared.clear();
        for parameter in self.by_url.values() {
            for base in &parameter.base {
                self.declared.insert((base.clone(), parameter.code.clone()));
            }
        }
        let mut definitions: Vec<_> = self.by_url.values().cloned().collect();
        // A runtime conformance declaration with no executable expression must
        // not erase a package's runnable definition of the same canonical URL.
        definitions.extend(
            self.declarations
                .values()
                .filter(|parameter| {
                    self.by_url
                        .get(&parameter.url)
                        .is_some_and(|effective| !effective.is_runnable())
                })
                .cloned(),
        );
        // Explicit runtime definitions override packages. Among package aliases,
        // prefer the base FHIR definition, then canonical URL. This does not
        // depend on database row order or hash-map iteration order.
        definitions.sort_by_key(|p| {
            (
                !p.user_defined,
                !p.url.starts_with("http://hl7.org/fhir/SearchParameter/"),
                p.url.clone(),
            )
        });
        for param in definitions {
            if !param.is_runnable() {
                continue;
            }
            if param.is_common() {
                self.common
                    .entry(param.code.clone())
                    .or_insert_with(|| param.clone());
            }
            for base in &param.base {
                let key = (base.clone(), param.code.clone());
                self.by_resource.entry(key).or_insert_with(|| param.clone());
            }
        }
        for ((base, _), param) in &self.by_resource {
            self.by_type
                .entry(base.clone())
                .or_default()
                .push(param.clone());
        }
        for params in self.by_type.values_mut() {
            params.sort_by(|a, b| a.code.cmp(&b.code));
        }
    }
}

/// Package declarations remain available by URL. Executable indices contain
/// runnable definitions only, with deterministic collision selection.
#[derive(Debug, Default)]
pub struct SearchParameterRegistry {
    data: ArcSwap<RegistryData>,
    writer: Mutex<()>,
}

impl SearchParameterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep one published generation for all query resolution, without copying
    /// indices. Later mutations publish independently in either registry.
    pub fn snapshot(&self) -> Self {
        Self {
            data: ArcSwap::from(self.data.load_full()),
            writer: Mutex::new(()),
        }
    }

    pub fn register(&self, param: SearchParameter) {
        self.register_all(std::iter::once(param));
    }

    /// Publish one complete snapshot for a bootstrap or configuration batch.
    pub fn register_all(&self, params: impl IntoIterator<Item = SearchParameter>) {
        let _writer = self.writer.lock().expect("search registry writer poisoned");
        let mut data = (**self.data.load()).clone();
        for param in params {
            data.declarations.insert(param.url.clone(), Arc::new(param));
        }
        data.rebuild();
        self.data.store(Arc::new(data));
    }

    /// Apply a committed runtime revision. Retain deletion revision markers so
    /// delayed or retried events cannot resurrect a deleted definition. Runtime
    /// resources overlay package declarations, which reappear after deletion.
    pub fn apply_resource(&self, id: &str, version: i64, param: Option<SearchParameter>) -> bool {
        self.apply_resources(std::iter::once((id.to_owned(), version, param)))
    }

    /// Restore committed definitions and deletion markers with one publication.
    pub fn apply_resources(
        &self,
        resources: impl IntoIterator<Item = (String, i64, Option<SearchParameter>)>,
    ) -> bool {
        let _writer = self.writer.lock().expect("search registry writer poisoned");
        let mut data = (**self.data.load()).clone();
        let mut changed = false;
        for (id, version, param) in resources {
            if data
                .resources
                .get(&id)
                .is_some_and(|(previous, _)| *previous >= version)
            {
                continue;
            }
            data.resources.insert(
                id,
                (
                    version,
                    param.map(|param| Arc::new(param.with_user_defined(true))),
                ),
            );
            changed = true;
        }
        if changed {
            data.rebuild();
            self.data.store(Arc::new(data));
        }
        changed
    }

    pub fn upsert(&self, param: SearchParameter) {
        self.register(param);
    }

    pub fn remove_by_url(&self, url: &str) -> bool {
        let _writer = self.writer.lock().expect("search registry writer poisoned");
        let mut data = (**self.data.load()).clone();
        if data.declarations.remove(url).is_none() {
            return false;
        }
        // Removing a losing alias must not remove a different winner. Removing
        // the winner exposes the next runnable candidate rather than losing it.
        data.rebuild();
        self.data.store(Arc::new(data));
        true
    }

    pub fn get(&self, resource_type: &str, code: &str) -> Option<Arc<SearchParameter>> {
        let data = self.data.load();
        data.by_resource
            .get(&(resource_type.into(), code.into()))
            .or_else(|| data.common.get(code))
            .cloned()
    }

    pub fn get_all_for_type(&self, resource_type: &str) -> Vec<Arc<SearchParameter>> {
        let data = self.data.load();
        let mut params = data.by_type.get(resource_type).cloned().unwrap_or_default();
        for param in data.common.values() {
            if !params.iter().any(|p| p.code == param.code) {
                params.push(param.clone());
            }
        }
        params.sort_by(|a, b| a.code.cmp(&b.code));
        params
    }

    /// Distinguish an unsupported declared filter from an unknown lenient one.
    pub fn has_declaration(&self, resource_type: &str, code: &str) -> bool {
        let data = self.data.load();
        [resource_type, "Resource", "DomainResource"]
            .iter()
            .any(|base| data.declared.contains(&((*base).into(), code.into())))
    }

    pub fn get_by_url(&self, url: &str) -> Option<Arc<SearchParameter>> {
        self.data.load().by_url.get(url).cloned()
    }

    pub fn get_common_parameters(&self) -> Vec<Arc<SearchParameter>> {
        let data = self.data.load();
        let mut params: Vec<_> = data.common.values().cloned().collect();
        params.sort_by(|a, b| a.code.cmp(&b.code));
        params
    }

    pub fn len(&self) -> usize {
        self.data.load().by_url.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.load().by_url.is_empty()
    }

    pub fn count_for_type(&self, resource_type: &str) -> usize {
        self.get_all_for_type(resource_type).len()
    }

    pub fn list_resource_types(&self) -> Vec<String> {
        let mut types: Vec<_> = self.data.load().by_type.keys().cloned().collect();
        types.sort();
        types
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_registry_generation_survives_runtime_changes() {
        let registry = SearchParameterRegistry::new();
        let original = SearchParameter::new(
            "name",
            "http://example.test/name",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name");
        registry.register(original.clone());
        let snapshot = registry.snapshot();
        assert!(Arc::ptr_eq(
            &registry.get("Patient", "name").unwrap(),
            &snapshot.get("Patient", "name").unwrap()
        ));
        let updated = original.with_expression("Patient.name.family");
        registry.apply_resource("runtime-name", 2, Some(updated));
        assert_eq!(
            snapshot
                .get("Patient", "name")
                .unwrap()
                .expression
                .as_deref(),
            Some("Patient.name")
        );
        assert_eq!(
            registry
                .get("Patient", "name")
                .unwrap()
                .expression
                .as_deref(),
            Some("Patient.name.family")
        );
        registry.apply_resource("runtime-name", 3, None);
        assert_eq!(
            snapshot
                .get("Patient", "name")
                .unwrap()
                .expression
                .as_deref(),
            Some("Patient.name")
        );
    }
    use crate::parameters::SearchParameterType;

    #[test]
    fn committed_revisions_ignore_reordered_events_and_restore_packages_after_deletion() {
        let registry = SearchParameterRegistry::new();
        let core = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name");
        registry.register(core.clone());
        let custom = core.clone().with_expression("Patient.name.family");
        assert!(registry.apply_resource("runtime", 2, Some(custom.clone())));
        assert_eq!(
            registry
                .get("Patient", "name")
                .unwrap()
                .expression
                .as_deref(),
            Some("Patient.name.family")
        );
        let mut renamed = custom.clone();
        renamed.url = "http://example.org/renamed".into();
        renamed.code = "family".into();
        assert!(registry.apply_resource("runtime", 3, Some(renamed.clone())));
        assert_eq!(
            registry.get("Patient", "name").unwrap().expression,
            core.expression
        );
        assert!(!registry.apply_resource("runtime", 2, Some(custom)));
        assert!(registry.get("Patient", "family").is_some());
        assert!(registry.apply_resource("runtime", 4, None));
        assert!(!registry.apply_resource("runtime", 3, Some(renamed)));
        assert!(registry.get("Patient", "family").is_none());
        assert_eq!(registry.get("Patient", "name").unwrap().url, core.url);
    }

    #[test]
    fn aliases_are_order_independent_and_deletions_restore_the_next_candidate() {
        let core = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name");
        let alias = SearchParameter::new(
            "name",
            "http://example.org/name",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name.family");
        for reverse in [false, true] {
            let registry = SearchParameterRegistry::new();
            let mut params = vec![core.clone(), alias.clone()];
            if reverse {
                params.reverse();
            }
            registry.register_all(params);
            assert_eq!(registry.get("Patient", "name").unwrap().url, core.url);
            assert!(registry.remove_by_url(&alias.url));
            assert_eq!(registry.get("Patient", "name").unwrap().url, core.url);
            registry.register(alias.clone());
            assert!(registry.remove_by_url(&core.url));
            assert_eq!(registry.get("Patient", "name").unwrap().url, alias.url);
        }
    }

    #[test]
    fn runtime_definitions_win_and_replacement_removes_old_bases_and_codes() {
        let registry = SearchParameterRegistry::new();
        let core = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name");
        let custom = SearchParameter::new(
            "name",
            "http://example.org/runtime",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name.family")
        .with_user_defined(true);
        registry.register_all([core.clone(), custom.clone()]);
        assert_eq!(registry.get("Patient", "name").unwrap().url, custom.url);
        let mut updated = custom;
        updated.base = vec!["Practitioner".into()];
        updated.code = "family".into();
        registry.upsert(updated.clone());
        assert_eq!(registry.get("Patient", "name").unwrap().url, core.url);
        assert_eq!(
            registry.get("Practitioner", "family").unwrap().url,
            updated.url
        );
        assert_eq!(registry.get_all_for_type("Patient").len(), 1);
        assert!(registry.remove_by_url(&updated.url));
        assert!(registry.get("Practitioner", "family").is_none());
    }

    #[test]
    fn conformance_declarations_do_not_shadow_executable_definitions() {
        let registry = SearchParameterRegistry::new();
        let executable = SearchParameter::new(
            "name",
            "http://example.org/runnable",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name");
        let declaration = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".into()],
        );
        registry.register_all([executable.clone(), declaration.clone()]);
        assert_eq!(registry.get("Patient", "name").unwrap().url, executable.url);
        assert!(registry.get_by_url(&declaration.url).is_some());
        registry.remove_by_url(&executable.url);
        assert!(registry.get("Patient", "name").is_none());
        assert_eq!(registry.count_for_type("Patient"), 0);
    }

    #[test]
    fn test_register_and_get() {
        let registry = SearchParameterRegistry::new();

        let param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name")
        .with_description("A patient's name");

        registry.register(param);

        // Should find by resource type and code
        let found = registry.get("Patient", "name");
        assert!(found.is_some());
        assert_eq!(found.unwrap().code, "name");

        // Should not find for wrong resource type
        assert!(registry.get("Observation", "name").is_none());
    }

    #[test]
    fn test_common_parameters() {
        let registry = SearchParameterRegistry::new();

        // Register a common parameter
        let param = SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.id");

        registry.register(param);

        // Should find for any resource type
        assert!(registry.get("Patient", "_id").is_some());
        assert!(registry.get("Observation", "_id").is_some());
        assert!(registry.get("Condition", "_id").is_some());
    }

    #[test]
    fn test_get_all_for_type() {
        let registry = SearchParameterRegistry::new();

        // Register common parameter
        registry.register(
            SearchParameter::new(
                "_id",
                "http://hl7.org/fhir/SearchParameter/Resource-id",
                SearchParameterType::Token,
                vec!["Resource".to_string()],
            )
            .with_expression("Resource.id"),
        );

        // Register resource-specific parameter
        registry.register(
            SearchParameter::new(
                "name",
                "http://hl7.org/fhir/SearchParameter/Patient-name",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name"),
        );

        let patient_params = registry.get_all_for_type("Patient");
        assert_eq!(patient_params.len(), 2);

        let observation_params = registry.get_all_for_type("Observation");
        assert_eq!(observation_params.len(), 1); // Only common params
    }

    #[test]
    fn test_get_by_url() {
        let registry = SearchParameterRegistry::new();

        registry.register(
            SearchParameter::new(
                "name",
                "http://hl7.org/fhir/SearchParameter/Patient-name",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name"),
        );

        let found = registry.get_by_url("http://hl7.org/fhir/SearchParameter/Patient-name");
        assert!(found.is_some());

        let not_found = registry.get_by_url("http://example.org/unknown");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_upsert_and_remove() {
        let registry = SearchParameterRegistry::new();

        let param = SearchParameter::new(
            "custom",
            "http://example.org/SearchParameter/custom",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name");

        // Upsert (insert)
        registry.upsert(param);
        assert!(registry.get("Patient", "custom").is_some());

        // Remove
        let removed = registry.remove_by_url("http://example.org/SearchParameter/custom");
        assert!(removed);
        assert!(registry.get("Patient", "custom").is_none());

        // Remove non-existent
        let not_removed = registry.remove_by_url("http://example.org/nonexistent");
        assert!(!not_removed);
    }
}
