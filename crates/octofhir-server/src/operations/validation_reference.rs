//! Request-local reference resolution for `$validate`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use octofhir_core::{FhirReference, UnresolvableReference, parse_reference};
use octofhir_fhirschema::reference::{
    ReferenceError, ReferenceResolutionResult, ReferenceResolver, ReferenceResult,
};
use serde_json::Value;

use super::{OperationContext, OperationError};
use crate::{reference_resolver::StorageReferenceResolver, server::AppState};

#[async_trait]
trait ReferenceRead: Send + Sync {
    async fn read(&self, reference: &FhirReference) -> Result<Option<Arc<Value>>, OperationError>;
}

struct InvocationRead {
    state: AppState,
    context: OperationContext,
}

#[async_trait]
impl ReferenceRead for InvocationRead {
    async fn read(&self, reference: &FhirReference) -> Result<Option<Arc<Value>>, OperationError> {
        let stored = match &reference.version {
            Some(version) => {
                self.context
                    .vread_reference(
                        &self.state,
                        &reference.resource_type,
                        &reference.id,
                        version,
                    )
                    .await
            }
            None => {
                self.context
                    .read_reference(&self.state, &reference.resource_type, &reference.id)
                    .await
            }
        }?;
        Ok(stored.map(|stored| Arc::new(stored.resource)))
    }
}

/// All existence/profile/body checks share the same authorized snapshot.
/// The upstream validator ignores some resolver errors, so retain failures for
/// the HTTP handler instead of reporting successful validation of unknown data.
pub(crate) struct ValidationReferenceResolver {
    reader: Arc<dyn ReferenceRead>,
    base: String,
    external: Arc<dyn ReferenceResolver>,
    snapshots: tokio::sync::Mutex<HashMap<String, Option<Arc<Value>>>>,
    error: Mutex<Option<OperationError>>,
}

impl ValidationReferenceResolver {
    pub(crate) fn new(state: &AppState, context: &OperationContext) -> Self {
        let base = format!("{}/fhir", state.base_url.trim_end_matches('/'));
        Self {
            reader: Arc::new(InvocationRead {
                state: state.clone(),
                context: context.clone(),
            }),
            external: Arc::new(StorageReferenceResolver::with_options(
                state.storage.clone(),
                base.clone(),
                state.config.validation.check_target_profile
                    && state.config.validation.fetch_external_references,
                state.config.validation.reference_fetch_timeout_ms,
            )),
            base,
            snapshots: tokio::sync::Mutex::new(HashMap::new()),
            error: Mutex::new(None),
        }
    }

    pub(crate) fn take_error(&self) -> Option<OperationError> {
        self.error.lock().unwrap().take()
    }

    fn failure(&self, error: OperationError) -> ReferenceError {
        let message = error.to_string();
        self.error.lock().unwrap().get_or_insert(error);
        ReferenceError::ServiceUnavailable { message }
    }

    async fn local(&self, reference: &FhirReference) -> ReferenceResult<Option<Arc<Value>>> {
        // Hold this invocation-local lock across the read to coalesce aliases
        // and repeated requests from the validator's concurrent batch methods.
        let mut snapshots = self.snapshots.lock().await;
        let key = reference.to_versioned();
        if let Some(snapshot) = snapshots.get(&key) {
            return Ok(snapshot.clone());
        }
        let snapshot = self
            .reader
            .read(reference)
            .await
            .map_err(|error| self.failure(error))?;
        snapshots.insert(key, snapshot.clone());
        Ok(snapshot)
    }
}

#[async_trait]
impl ReferenceResolver for ValidationReferenceResolver {
    async fn resource_exists(&self, resource_type: &str, id: &str) -> ReferenceResult<bool> {
        Ok(self
            .local(&FhirReference::new(resource_type, id))
            .await?
            .is_some())
    }

    async fn resolve_reference(
        &self,
        reference: &str,
    ) -> ReferenceResult<ReferenceResolutionResult> {
        let Ok(parsed) = parse_reference(reference, Some(&self.base)) else {
            return Ok(ReferenceResolutionResult::skipped());
        };
        if self.local(&parsed).await?.is_some() {
            Ok(ReferenceResolutionResult::found(
                parsed.resource_type,
                parsed.id,
            ))
        } else {
            Ok(ReferenceResolutionResult::not_found())
        }
    }

    async fn fetch_resource(&self, reference: &str) -> ReferenceResult<Option<Arc<Value>>> {
        match parse_reference(reference, Some(&self.base)) {
            Ok(parsed) => self.local(&parsed).await,
            Err(UnresolvableReference::External(_)) => self
                .external
                .fetch_resource(reference)
                .await
                .map_err(|error| self.failure(OperationError::Internal(error.to_string()))),
            Err(_) => Ok(None),
        }
    }

    // Do not use declared meta.profile as proof of actual conformance: writes
    // can accept unavailable profiles with a warning. The default empty profile
    // map makes the validator verify an authorized body against the target.
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_api::ApiError;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ChangingRead {
        calls: AtomicUsize,
        deny: bool,
        missing: bool,
        birth_date: bool,
    }

    #[async_trait]
    impl ReferenceRead for ChangingRead {
        async fn read(
            &self,
            reference: &FhirReference,
        ) -> Result<Option<Arc<Value>>, OperationError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.deny {
                return Err(ApiError::forbidden("reference read denied").into());
            }
            if self.missing {
                return Ok(None);
            }
            let mut body = json!({"resourceType":reference.resource_type, "id":reference.id,
                "meta":{"versionId":reference.version.clone().unwrap_or_else(||call.to_string()), "profile":["urn:test:claim"]},
                "active": call == 0
            });
            if self.birth_date {
                body["birthDate"] = json!("2000-01-01");
            }
            Ok(Some(Arc::new(body)))
        }
    }

    fn resolver(reader: Arc<ChangingRead>) -> ValidationReferenceResolver {
        ValidationReferenceResolver {
            reader,
            base: "https://example.test/fhir".into(),
            external: Arc::new(octofhir_fhirschema::reference::NoOpReferenceResolver::new()),
            snapshots: tokio::sync::Mutex::new(HashMap::new()),
            error: Mutex::new(None),
        }
    }

    fn reader(deny: bool, missing: bool) -> Arc<ChangingRead> {
        Arc::new(ChangingRead {
            calls: AtomicUsize::new(0),
            deny,
            missing,
            birth_date: false,
        })
    }

    #[tokio::test]
    async fn existence_and_body_use_one_snapshot_across_reference_aliases() {
        let reader = reader(false, false);
        let resolver = resolver(reader.clone());
        assert!(
            resolver
                .resolve_reference("Patient/p1")
                .await
                .unwrap()
                .exists
        );
        let body = resolver
            .fetch_resource("https://example.test/fhir/Patient/p1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(body["active"], true);
        assert_eq!(body["meta"]["versionId"], "0");
        assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
        let references = vec![
            "Patient/p1".into(),
            "https://example.test/fhir/Patient/p1".into(),
        ];
        let bodies = resolver.fetch_resources_batch(&references).await;
        assert!(Arc::ptr_eq(
            bodies[0].as_ref().unwrap().as_ref().unwrap(),
            bodies[1].as_ref().unwrap().as_ref().unwrap()
        ));
        // A declared profile is not proof that the body actually conforms.
        assert!(resolver.fetch_profiles_batch(&references).await.is_empty());
        assert!(resolver.take_error().is_none());
    }

    #[tokio::test]
    async fn current_and_historical_reference_snapshots_are_distinct() {
        let reader = reader(false, false);
        let resolver = resolver(reader.clone());
        let current = resolver
            .fetch_resource("Patient/p1")
            .await
            .unwrap()
            .unwrap();
        let previous = resolver
            .fetch_resource("Patient/p1/_history/old")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current["meta"]["versionId"], "0");
        assert_eq!(previous["meta"]["versionId"], "old");
        assert!(
            resolver
                .resolve_reference("https://example.test/fhir/Patient/p1/_history/old")
                .await
                .unwrap()
                .exists
        );
        assert_eq!(reader.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ignored_upstream_lookup_errors_remain_available_to_handler() {
        let resolver = Arc::new(resolver(reader(true, false)));
        let validator =
            octofhir_fhirschema::validation::FhirValidator::from_schemas(HashMap::new(), None)
                .with_reference_resolver(resolver.clone());
        // Upstream validation continues after resolver errors. The HTTP handler
        // must still reject this result, preserving the original status.
        validator
            .validate(
                &json!({"resourceType":"Observation", "subject":{"reference":"Patient/p1"}}),
                vec!["Observation".into()],
            )
            .await;
        assert!(matches!(
            resolver.take_error(),
            Some(OperationError::Api(ApiError::Forbidden(_)))
        ));
        assert!(resolver.take_error().is_none());
    }

    #[tokio::test]
    async fn definite_missing_reference_is_a_validation_result() {
        let reader = reader(false, true);
        let resolver = resolver(reader.clone());
        assert!(
            !resolver
                .resolve_reference("Patient/p1")
                .await
                .unwrap()
                .exists
        );
        assert!(
            resolver
                .fetch_resource("Patient/p1")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
        assert!(resolver.take_error().is_none());
        assert!(
            resolver
                .resolve_reference("https://foreign.test/fhir/Patient/p1")
                .await
                .unwrap()
                .exists
        );
        assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn declared_profile_cannot_replace_actual_target_conformance() {
        use octofhir_fhirschema::{
            embedded::{FhirVersion, get_schemas},
            validation::FhirValidator,
        };
        let mut schemas = get_schemas(FhirVersion::R4).clone();
        let mut profile = schemas.get("Patient").unwrap().clone();
        profile.name = "PatientWithBirthDate".into();
        profile.url = "urn:test:claim".into();
        profile.base = Some("http://hl7.org/fhir/StructureDefinition/Patient".into());
        profile.derivation = Some("constraint".into());
        profile
            .required
            .get_or_insert_with(Vec::new)
            .push("birthDate".into());
        profile
            .elements
            .as_mut()
            .unwrap()
            .get_mut("birthDate")
            .unwrap()
            .min = Some(1);
        schemas.insert(profile.name.clone(), profile);
        schemas
            .get_mut("Observation")
            .unwrap()
            .elements
            .as_mut()
            .unwrap()
            .get_mut("subject")
            .unwrap()
            .refers = Some(vec!["urn:test:claim".into()]);
        for conforms in [false, true] {
            let reader = Arc::new(ChangingRead {
                calls: AtomicUsize::new(0),
                deny: false,
                missing: false,
                birth_date: conforms,
            });
            let resolver = Arc::new(resolver(reader.clone()));
            let validator = FhirValidator::from_schemas(schemas.clone(), None)
                .with_reference_resolver(resolver.clone())
                .with_target_profile_validation(true);
            let result = validator.validate(&json!({"resourceType":"Observation", "status":"final", "code":{"text":"fixture"}, "subject":{"reference":"Patient/p1"}}), vec!["Observation".into()]).await;
            assert_eq!(result.valid, conforms, "{result:?}");
            assert_eq!(
                result
                    .errors
                    .iter()
                    .any(|error| error.error_type == "FS1017"),
                !conforms,
                "{result:?}"
            );
            assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
            assert!(resolver.take_error().is_none());
        }
    }
}
