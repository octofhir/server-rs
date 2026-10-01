//! Resolve FHIRPath selections to their JSON locations, preserving array identity.
//! Values alone cannot identify a location: equal values may occur several times.

use std::{future::Future, pin::Pin, sync::Arc};

use octofhir_api::ApiError;
use octofhir_fhirpath::ast::ExpressionNode;
use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, FhirPathValue};
use serde_json::Value;

use super::SharedModelProvider;

#[derive(Clone)]
pub(super) struct Location {
    pub pointer: String,
    pub value: FhirPathValue,
}

pub(super) async fn select(
    engine: &FhirPathEngine,
    provider: &SharedModelProvider,
    resource: &Value,
    path: &str,
) -> Result<Vec<Location>, ApiError> {
    let provider: Arc<dyn octofhir_fhirpath::ModelProvider + Send + Sync> = provider.clone();
    let input = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let root = Location {
        pointer: String::new(),
        value: input
            .first()
            .cloned()
            .ok_or_else(|| ApiError::bad_request("Missing resource"))?,
    };
    let context = EvaluationContext::new(input, provider, None, None, None);
    let ast = engine
        .compile_ast(path)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    resolve(engine, resource, &context, &ast, vec![root]).await
}

fn resolve<'a>(
    engine: &'a FhirPathEngine,
    resource: &'a Value,
    context: &'a EvaluationContext,
    ast: &'a ExpressionNode,
    focus: Vec<Location>,
) -> Pin<Box<dyn Future<Output = Result<Vec<Location>, ApiError>> + Send + 'a>> {
    Box::pin(async move {
        match ast {
            ExpressionNode::Identifier(node) => {
                property(resource, context, focus, &node.name).await
            }
            ExpressionNode::Variable(node)
                if matches!(
                    node.name.as_str(),
                    "$this" | "this" | "%resource" | "resource"
                ) =>
            {
                Ok(focus)
            }
            ExpressionNode::PropertyAccess(node) => {
                let parents = resolve(engine, resource, context, &node.object, focus).await?;
                property(resource, context, parents, &node.property).await
            }
            ExpressionNode::Parenthesized(node) => {
                resolve(engine, resource, context, node, focus).await
            }
            ExpressionNode::IndexAccess(node) => {
                let targets = resolve(engine, resource, context, &node.object, focus).await?;
                let result = engine
                    .evaluate_ast(&node.index, context)
                    .await
                    .map_err(|e| ApiError::bad_request(e.to_string()))?;
                let index = result
                    .value
                    .first()
                    .and_then(|v| v.to_json_value().as_i64());
                let index = index.and_then(|i| usize::try_from(i).ok()).ok_or_else(|| {
                    ApiError::bad_request("FHIRPath index must be a nonnegative integer")
                })?;
                Ok(targets.get(index).cloned().into_iter().collect())
            }
            ExpressionNode::MethodCall(node) => {
                let targets = resolve(engine, resource, context, &node.object, focus).await?;
                method(
                    engine,
                    resource,
                    context,
                    targets,
                    &node.method,
                    &node.arguments,
                )
                .await
            }
            ExpressionNode::FunctionCall(node) => {
                method(
                    engine,
                    resource,
                    context,
                    focus,
                    &node.name,
                    &node.arguments,
                )
                .await
            }
            ExpressionNode::Filter(node) => {
                let targets = resolve(engine, resource, context, &node.base, focus).await?;
                filter(engine, context, targets, &node.condition).await
            }
            ExpressionNode::Union(node) => {
                let mut targets =
                    resolve(engine, resource, context, &node.left, focus.clone()).await?;
                for target in resolve(engine, resource, context, &node.right, focus).await? {
                    if !targets.iter().any(|t| t.pointer == target.pointer) {
                        targets.push(target);
                    }
                }
                Ok(targets)
            }
            _ => Err(ApiError::bad_request(
                "Patch path must select existing resource elements",
            )),
        }
    })
}

async fn property(
    resource: &Value,
    context: &EvaluationContext,
    parents: Vec<Location>,
    name: &str,
) -> Result<Vec<Location>, ApiError> {
    let mut targets = Vec::new();
    for parent in parents {
        let value = resource
            .pointer(&parent.pointer)
            .ok_or_else(|| ApiError::bad_request("Patch target no longer exists"))?;
        if value["resourceType"].as_str() == Some(name) {
            targets.push(parent);
            continue;
        }
        let type_info = context
            .model_provider()
            .get_element_type(parent.value.type_info(), name)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        let Some(object) = value.as_object() else {
            continue;
        };
        // FHIRPath choice elements use the base name, JSON uses a type suffix.
        for (key, child) in object {
            let choice = type_info
                .as_ref()
                .is_some_and(|t| t.type_name.contains(" | "));
            if key != name
                && !(choice
                    && key
                        .strip_prefix(name)
                        .is_some_and(|s| s.starts_with(char::is_uppercase)))
            {
                continue;
            }
            let child_type = context
                .model_provider()
                .get_element_type(parent.value.type_info(), key)
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?
                .or_else(|| type_info.clone());
            let pointer = format!("{}/{}", parent.pointer, escape(key));
            let values: Vec<(String, &Value)> = match child {
                Value::Array(values) => values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (format!("{pointer}/{i}"), v))
                    .collect(),
                Value::Null => Vec::new(),
                _ => vec![(pointer, child)],
            };
            for (pointer, json) in values {
                let mut value =
                    octofhir_fhirpath::core::value::utils::json_to_fhirpath_value(json.clone());
                if let Some(info) = &child_type {
                    value = value.with_type_info(Arc::new(info.clone()));
                }
                targets.push(Location { pointer, value });
            }
        }
    }
    Ok(targets)
}

async fn filter(
    engine: &FhirPathEngine,
    context: &EvaluationContext,
    targets: Vec<Location>,
    predicate: &ExpressionNode,
) -> Result<Vec<Location>, ApiError> {
    let mut selected = Vec::new();
    for (index, target) in targets.into_iter().enumerate() {
        let child = context.create_child_context(Collection::single(target.value.clone()));
        child.set_this(target.value.clone());
        child.set_index(index);
        let result = engine
            .evaluate_ast(predicate, &child)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        if result.value.len() > 1 {
            return Err(ApiError::bad_request(
                "Patch where predicate must be a singleton boolean",
            ));
        }
        match result.value.first().map(FhirPathValue::to_json_value) {
            Some(Value::Bool(true)) => selected.push(target),
            Some(Value::Bool(false)) | None => {}
            _ => {
                return Err(ApiError::bad_request(
                    "Patch where predicate must be boolean",
                ));
            }
        }
    }
    Ok(selected)
}

async fn method(
    engine: &FhirPathEngine,
    resource: &Value,
    context: &EvaluationContext,
    mut targets: Vec<Location>,
    name: &str,
    arguments: &[ExpressionNode],
) -> Result<Vec<Location>, ApiError> {
    match (name, arguments) {
        ("where", [predicate]) => filter(engine, context, targets, predicate).await,
        ("first", []) => Ok(targets.into_iter().take(1).collect()),
        ("last", []) => Ok(targets.pop().into_iter().collect()),
        ("single", []) if targets.len() <= 1 => Ok(targets),
        ("resolve", []) => {
            let mut resolved = Vec::new();
            for target in targets {
                let reference = resource
                    .pointer(&target.pointer)
                    .and_then(|v| v.as_str().or_else(|| v["reference"].as_str()))
                    .ok_or_else(|| ApiError::bad_request("Patch resolve requires a reference"))?;
                let id = reference.strip_prefix('#').ok_or_else(|| {
                    ApiError::bad_request("Patch cannot resolve external resources")
                })?;
                if let Some(contained) = resource["contained"].as_array() {
                    for (index, value) in contained
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v["id"].as_str() == Some(id))
                    {
                        let input = Collection::from_json_resource(
                            value.clone(),
                            Some(context.model_provider().clone()),
                        )
                        .await
                        .map_err(|e| ApiError::bad_request(e.to_string()))?;
                        if let Some(value) = input.first() {
                            resolved.push(Location {
                                pointer: format!("/contained/{index}"),
                                value: value.clone(),
                            });
                        }
                    }
                }
            }
            Ok(resolved)
        }
        _ => Err(ApiError::bad_request(format!(
            "FHIRPath function {name} cannot select a patch location"
        ))),
    }
}

pub(super) fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
