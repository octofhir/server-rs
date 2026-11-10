//! Request-local, read-only FHIR server access for mapping expressions.

use std::fmt;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fml_engine::{Expressions, FhirPathExpressions, Item, ServerProvider};
use octofhir_fhir_model::error::{ModelError, Result as ModelResult};
use serde_json::Value;

use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

#[async_trait]
trait ResourceRead: Send + Sync {
    async fn read(&self, resource_type: &str, id: &str) -> Result<Option<Value>, OperationError>;
}

struct InvocationRead {
    state: AppState,
    context: OperationContext,
}

#[async_trait]
impl ResourceRead for InvocationRead {
    async fn read(&self, resource_type: &str, id: &str) -> Result<Option<Value>, OperationError> {
        self.context
            .read(&self.state, resource_type, id)
            .await
            .map(|stored| stored.map(|stored| stored.resource))
    }
}

/// Reads through the caller's operation context, never through HTTP or raw storage.
/// Errors are retained because FHIRPath otherwise turns failed reads into empty results.
#[derive(Clone)]
pub(super) struct FmlServerProvider {
    reader: Arc<dyn ResourceRead>,
    base: String,
    error: Arc<Mutex<Option<OperationError>>>,
}

impl fmt::Debug for FmlServerProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FmlServerProvider")
            .finish_non_exhaustive()
    }
}

impl FmlServerProvider {
    pub(super) fn new(state: &AppState, context: &OperationContext) -> Self {
        Self {
            reader: Arc::new(InvocationRead {
                state: state.clone(),
                context: context.clone(),
            }),
            base: state.fhir_base_url(),
            error: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn take_error(&self) -> Option<OperationError> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    fn failed(&self) -> bool {
        self.error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    fn failure(&self, error: OperationError) -> ModelError {
        self.error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(error);
        ModelError::Generic {
            message: "FHIR server access failed during map execution".into(),
        }
    }

    fn unsupported<T>(&self, operation: &str) -> ModelResult<T> {
        Err(self.failure(OperationError::NotSupported(format!(
            "%server.{operation} is not available during FML execution; only reads are allowed"
        ))))
    }
}

#[async_trait]
impl ServerProvider for FmlServerProvider {
    async fn read(&self, resource_type: &str, id: &str) -> ModelResult<Option<Value>> {
        if self.failed() {
            return Err(ModelError::Generic {
                message: "FHIR server access already failed during map execution".into(),
            });
        }
        self.reader
            .read(resource_type, id)
            .await
            .map_err(|error| self.failure(error))
    }

    async fn create(&self, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("create")
    }

    async fn update(&self, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("update")
    }

    async fn delete(&self, _: &Value) -> ModelResult<bool> {
        self.unsupported("delete")
    }

    async fn search(&self, _: bool, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("search")
    }

    async fn patch(&self, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("patch")
    }

    async fn capabilities(&self, _: Option<&str>) -> ModelResult<Option<Value>> {
        self.unsupported("capabilities")
    }

    async fn validate(&self, _: &Value, _: &str, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("validate")
    }

    async fn transform(&self, _: &Value, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("transform")
    }

    async fn everything(&self, _: &str, _: &str, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("everything")
    }

    async fn apply(&self, _: &Value, _: &str, _: &Value) -> ModelResult<Option<Value>> {
        self.unsupported("apply")
    }

    fn base_url(&self) -> &str {
        &self.base
    }

    fn with_base_url(&self, url: &str) -> Option<Arc<dyn ServerProvider>> {
        if url.trim_end_matches('/') == self.base.trim_end_matches('/') {
            return Some(Arc::new(self.clone()));
        }
        let _ = self.unsupported::<()>("at another server");
        None
    }
}

/// Keeps the shared engine's caches, but binds server access to this invocation only.
pub(super) struct ServerExpressions {
    expressions: FhirPathExpressions,
    server: Arc<FmlServerProvider>,
}

impl ServerExpressions {
    pub(super) fn new(shared: &FhirPathExpressions, server: Arc<FmlServerProvider>) -> Self {
        Self {
            expressions: shared.clone().with_server_provider(server.clone()),
            server,
        }
    }
}

impl Expressions for ServerExpressions {
    async fn evaluate(
        &self,
        expression: &str,
        focus: Option<&Item>,
        variables: &[(String, Item)],
    ) -> Result<Vec<Item>, String> {
        let result = self
            .expressions
            .evaluate(expression, focus, variables)
            .await;
        if self.server.failed() {
            // Stop the map even when FHIRPath swallowed the provider error. Runner returns
            // the original OperationError before exposing a result or debug trace.
            return Err("FHIR server access failed during map execution".into());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fml_check::{CoreDefinitions, Release};
    use fml_engine::Engine;
    use octofhir_api::ApiError;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Reader {
        caller: &'static str,
        deny: bool,
        missing: bool,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ResourceRead for Reader {
        async fn read(
            &self,
            resource_type: &str,
            id: &str,
        ) -> Result<Option<Value>, OperationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.deny {
                return Err(ApiError::forbidden("resource read denied").into());
            }
            if self.missing {
                return Ok(None);
            }
            Ok(Some(json!({
                "resourceType": resource_type,
                "id": id,
                "extension": [{"url": "http://example.test/caller", "valueString": self.caller}]
            })))
        }
    }

    fn reader(caller: &'static str, deny: bool, missing: bool) -> Arc<Reader> {
        Arc::new(Reader {
            caller,
            deny,
            missing,
            calls: AtomicUsize::new(0),
        })
    }

    fn provider(reader: Arc<Reader>) -> Arc<FmlServerProvider> {
        Arc::new(FmlServerProvider {
            reader,
            base: "https://example.test/fhir".into(),
            error: Arc::new(Mutex::new(None)),
        })
    }

    fn reference() -> Item {
        Item {
            json: json!({"reference": "BodyStructure/abc"}),
            type_name: Some("Reference".into()),
        }
    }

    #[tokio::test]
    async fn resolved_resources_keep_types_and_reach_typed_groups() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let server = provider(reader("caller", false, false));
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate("resolve()", Some(&reference()), &[])
            .await
            .unwrap();
        assert_eq!(result[0].type_name.as_deref(), Some("BodyStructure"));
        let definitions = CoreDefinitions::new(Release::R5);
        for expression in [
            "resolve()",
            "(resolve() as BodyStructure)",
            "resolve().ofType(BodyStructure)",
        ] {
            let map = fml_model::compile(&format!(
                r#"/// url = 'http://example.test/StructureMap/resolve'
/// name = 'resolve'
uses "http://hl7.org/fhir/StructureDefinition/Observation" as source
uses "http://hl7.org/fhir/StructureDefinition/Basic" as target
uses "http://hl7.org/fhir/StructureDefinition/BodyStructure" alias BS as queried
group Main(source src : Observation, target tgt : Basic) {{
  src.focus as r -> tgt.code as c, evaluate(r, {expression}) as row then Rows(row, c) "resolve";
}}
group Rows(source row : BS, target c) {{
  row.extension as e -> c.text = (e.value) "extension";
}}
"#
            ));
            assert!(!map.has_errors(), "{:?}", map.diagnostics);
            let result = Engine::new(&definitions, &expressions)
                .transform(
                    &map.map,
                    &json!({
                        "resourceType": "Observation", "focus": [{"reference": "BodyStructure/abc"}]
                    }),
                )
                .await
                .unwrap();
            assert_eq!(
                result.target,
                json!({"resourceType": "Basic", "code": {"text": "caller"}}),
                "{expression}"
            );
        }
        assert!(server.take_error().is_none());
    }

    #[tokio::test]
    async fn shared_expressions_do_not_share_callers_or_resource_bodies() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let first_reader = reader("first", false, false);
        let second_reader = reader("second", false, false);
        let first = ServerExpressions::new(&shared, provider(first_reader.clone()));
        let second = ServerExpressions::new(&shared, provider(second_reader.clone()));
        let focus = reference();
        let (first, second) = tokio::join!(
            first.evaluate("resolve().extension.value", Some(&focus), &[]),
            second.evaluate("resolve().extension.value", Some(&focus), &[])
        );
        assert_eq!(first.unwrap()[0].json, "first");
        assert_eq!(second.unwrap()[0].json, "second");
        assert_eq!(first_reader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_reader.calls.load(Ordering::SeqCst), 1);
        assert!(
            shared
                .evaluate("resolve()", Some(&focus), &[])
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn missing_resources_return_empty_without_failing_the_operation() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let server = provider(reader("caller", false, true));
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate("resolve().exists()", Some(&reference()), &[])
            .await
            .unwrap();
        assert_eq!(result[0].json, false);
        assert!(server.take_error().is_none());
    }

    #[tokio::test]
    async fn denied_reads_stop_the_map_and_preserve_the_authorization_error() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let server = provider(reader("caller", true, false));
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate(
                "iif(resolve().exists(), 'found', 'fallback')",
                Some(&reference()),
                &[],
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            server.take_error(),
            Some(OperationError::Api(ApiError::Forbidden(_)))
        ));
    }

    #[tokio::test]
    async fn server_read_uses_the_same_request_provider() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let server = provider(reader("caller", false, false));
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate("%server.read('BodyStructure', 'abc').id", None, &[])
            .await
            .unwrap();
        assert_eq!(result[0].json, "abc");
        assert!(server.take_error().is_none());
    }

    #[tokio::test]
    async fn all_other_server_operations_are_rejected_without_accessing_resources() {
        for operation in [
            "create",
            "update",
            "delete",
            "search",
            "patch",
            "capabilities",
            "validate",
            "transform",
            "everything",
            "apply",
        ] {
            let reader = reader("caller", false, false);
            let server = provider(reader.clone());
            let value = json!({"resourceType": "Patient", "id": "p1"});
            let result = match operation {
                "create" => server.create(&value).await.map(|_| ()),
                "update" => server.update(&value).await.map(|_| ()),
                "delete" => server.delete(&value).await.map(|_| ()),
                "search" => server.search(false, &value).await.map(|_| ()),
                "patch" => server.patch(&value).await.map(|_| ()),
                "capabilities" => server.capabilities(None).await.map(|_| ()),
                "validate" => server
                    .validate(&value, "create", &Value::Null)
                    .await
                    .map(|_| ()),
                "transform" => server.transform(&value, &value).await.map(|_| ()),
                "everything" => server
                    .everything("Patient", "p1", &Value::Null)
                    .await
                    .map(|_| ()),
                "apply" => server
                    .apply(&value, "Patient/p1", &Value::Null)
                    .await
                    .map(|_| ()),
                _ => unreachable!(),
            };
            assert!(result.is_err(), "{operation}");
            assert!(server.read("Patient", "p1").await.is_err(), "{operation}");
            assert_eq!(reader.calls.load(Ordering::SeqCst), 0, "{operation}");
            assert!(
                matches!(server.take_error(), Some(OperationError::NotSupported(_))),
                "{operation}"
            );
        }
    }

    #[tokio::test]
    async fn write_attempts_fail_expression_evaluation_instead_of_returning_empty() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let server = provider(reader("caller", false, false));
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate(
                "%server.create(%candidate)",
                None,
                &[(
                    "candidate".into(),
                    Item {
                        json: json!({"resourceType": "Patient"}),
                        type_name: Some("Patient".into()),
                    },
                )],
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            server.take_error(),
            Some(OperationError::NotSupported(_))
        ));
    }

    #[tokio::test]
    async fn switching_servers_never_falls_back_to_a_local_read() {
        let shared = FhirPathExpressions::new(Release::R5).await.unwrap();
        let reader = reader("caller", false, false);
        let server = provider(reader.clone());
        let expressions = ServerExpressions::new(&shared, server.clone());
        let result = expressions
            .evaluate(
                "%server.at('https://other.test/fhir').read('BodyStructure', 'abc')",
                None,
                &[],
            )
            .await;
        assert!(result.is_err());
        assert_eq!(reader.calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            server.take_error(),
            Some(OperationError::NotSupported(_))
        ));

        let same = server.with_base_url("https://example.test/fhir/").unwrap();
        assert!(same.read("BodyStructure", "abc").await.unwrap().is_some());
    }
}
