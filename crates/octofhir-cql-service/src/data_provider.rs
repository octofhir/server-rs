//! FHIR data provider implementation for CQL engine

use crate::error::CqlResult;
use bigdecimal::BigDecimal;
use indexmap::IndexMap;
use num_traits::ToPrimitive;
use octofhir_cql_eval::DataProvider;
use octofhir_cql_types::{CqlList, CqlTuple, CqlType, CqlValue};
use octofhir_fhirpath::{
    Collection, EvaluationContext as FhirPathContext, FhirPathEngine, FhirPathValue,
};
use octofhir_storage::{DynStorage, SearchParams};
use serde_json::Value;
use std::sync::Arc;

/// FHIR server data provider for CQL evaluation
pub struct FhirServerDataProvider {
    storage: DynStorage,
    fhirpath_engine: Arc<FhirPathEngine>,
    max_retrieve_size: usize,
}

impl FhirServerDataProvider {
    pub fn new(
        storage: DynStorage,
        fhirpath_engine: Arc<FhirPathEngine>,
        max_retrieve_size: usize,
    ) -> Self {
        Self {
            storage,
            fhirpath_engine,
            max_retrieve_size,
        }
    }

    /// Retrieve resources based on CQL retrieve parameters
    pub async fn retrieve(
        &self,
        data_type: &str,
        _context_type: Option<&str>,
        context_value: Option<&Value>,
        template_id: Option<&str>,
        code_property: Option<&str>,
        codes: Option<&Value>,
        date_property: Option<&str>,
        date_range: Option<&Value>,
    ) -> CqlResult<Vec<Value>> {
        tracing::debug!(data_type = data_type, "CQL retrieve starting");

        let mut search_params = SearchParams::new();

        // Context filtering
        if let Some(ctx_value) = context_value {
            if let Some(patient_id) = Self::extract_patient_id(ctx_value) {
                if Self::has_subject_field(data_type) {
                    search_params = search_params.with_param("subject", &patient_id);
                } else if data_type == "Patient" {
                    search_params = search_params.with_param("_id", &patient_id);
                }
            }
        }

        // Profile filtering
        if let Some(profile) = template_id {
            search_params = search_params.with_param("_profile", profile);
        }

        // Code filtering
        if let Some(codes_val) = codes {
            if let Some(code_param) = Self::build_code_param(codes_val, code_property) {
                let param_name = code_property.unwrap_or("code");
                search_params = search_params.with_param(param_name, &code_param);
            }
        }

        // Date filtering
        if let Some(range) = date_range {
            if let Some((start, end)) = Self::extract_date_range(range) {
                let param_name = date_property.unwrap_or("date");
                search_params = search_params.with_param(param_name, &format!("ge{}", start));
                search_params = search_params.with_param(param_name, &format!("le{}", end));
            }
        }

        search_params = search_params.with_count(self.max_retrieve_size as u32);

        let result = self
            .storage
            .search(data_type, &search_params)
            .await
            .map_err(|e| {
                crate::error::CqlError::DataProviderError(format!("Search failed: {}", e))
            })?;

        let resources: Vec<Value> = result
            .entries
            .into_iter()
            .map(|entry| entry.resource)
            .collect();
        Ok(resources)
    }

    fn extract_patient_id(context_value: &Value) -> Option<String> {
        if let Some(reference) = context_value.get("reference").and_then(|r| r.as_str()) {
            if reference.starts_with("Patient/") {
                return Some(reference.to_string());
            }
        }
        if let Some(id) = context_value.as_str() {
            if id.starts_with("Patient/") {
                return Some(id.to_string());
            } else {
                return Some(format!("Patient/{}", id));
            }
        }
        None
    }

    fn has_subject_field(resource_type: &str) -> bool {
        matches!(
            resource_type,
            "Observation"
                | "Condition"
                | "Procedure"
                | "MedicationRequest"
                | "DiagnosticReport"
                | "Encounter"
                | "AllergyIntolerance"
        )
    }

    fn build_code_param(codes: &Value, _code_property: Option<&str>) -> Option<String> {
        if let Some(code_str) = codes.as_str() {
            return Some(code_str.to_string());
        }
        if let Some(obj) = codes.as_object() {
            let system = obj.get("system").and_then(|s| s.as_str());
            let code = obj.get("code").and_then(|c| c.as_str());
            if let (Some(sys), Some(cd)) = (system, code) {
                return Some(format!("{}|{}", sys, cd));
            } else if let Some(cd) = code {
                return Some(cd.to_string());
            }
        }
        None
    }

    fn extract_date_range(date_range: &Value) -> Option<(String, String)> {
        if let Some(obj) = date_range.as_object() {
            let low = obj.get("low").and_then(|l| l.as_str());
            let high = obj.get("high").and_then(|h| h.as_str());
            if let (Some(start), Some(end)) = (low, high) {
                return Some((start.to_string(), end.to_string()));
            }
        }
        None
    }
}

impl DataProvider for FhirServerDataProvider {
    fn retrieve(
        &self,
        data_type: &str,
        context_type: Option<&str>,
        context_value: Option<&CqlValue>,
        template_id: Option<&str>,
        code_property: Option<&str>,
        codes: Option<&CqlValue>,
        date_property: Option<&str>,
        date_range: Option<&CqlValue>,
    ) -> Vec<CqlValue> {
        let context_json = context_value.and_then(|cv| cql_value_to_json(cv));
        let codes_json = codes.and_then(|cv| cql_value_to_json(cv));
        let date_range_json = date_range.and_then(|cv| cql_value_to_json(cv));

        let rt = tokio::runtime::Handle::current();
        let resources = rt.block_on(async {
            self.retrieve(
                data_type,
                context_type,
                context_json.as_ref(),
                template_id,
                code_property,
                codes_json.as_ref(),
                date_property,
                date_range_json.as_ref(),
            )
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "Failed to retrieve data");
                Vec::new()
            })
        });

        resources
            .into_iter()
            .filter_map(|json| json_to_cql_value(&json))
            .collect()
    }

    fn get_property(&self, resource: &CqlValue, path: &str) -> Option<CqlValue> {
        let json = cql_value_to_json(resource)?;

        // Use FHIRPath engine for proper path navigation
        // This is sync trait method, so use tokio runtime to block on async operation
        let rt = tokio::runtime::Handle::current();
        let result = rt.block_on(async {
            // Note: Using EmptyModelProvider since we don't need full model validation for property access
            let model_provider: Arc<dyn octofhir_fhirpath::ModelProvider + Send + Sync> =
                Arc::new(octofhir_fhir_model::EmptyModelProvider);

            let input = Collection::from_json_resource(json.clone(), Some(model_provider.clone()))
                .await
                .ok()?;

            let ctx = FhirPathContext::new(input, model_provider, None, None, None);

            self.fhirpath_engine.evaluate(path, &ctx).await.ok()
        })?;

        // Convert FHIRPath result to CqlValue
        fhirpath_to_cql_value(&result.value)
    }
}

pub(crate) fn cql_value_to_json(value: &CqlValue) -> Option<Value> {
    match value {
        CqlValue::Null => Some(Value::Null),
        CqlValue::Boolean(b) => Some(Value::Bool(*b)),
        CqlValue::Integer(i) => Some(Value::Number((*i).into())),
        CqlValue::Long(l) => Some(Value::Number((*l).into())),
        CqlValue::Decimal(d) => d
            .to_f64()
            .and_then(|f| serde_json::Number::from_f64(f))
            .map(Value::Number),
        CqlValue::String(s) => Some(Value::String(s.clone())),
        CqlValue::Date(d) => Some(Value::String(d.to_string())),
        CqlValue::DateTime(dt) => Some(Value::String(dt.to_string())),
        CqlValue::Time(t) => Some(Value::String(t.to_string())),
        CqlValue::Quantity(q) => {
            let mut map = serde_json::Map::new();
            if let Some(v) = q.value.to_f64() {
                map.insert(
                    "value".to_string(),
                    Value::Number(serde_json::Number::from_f64(v)?),
                );
            }
            if let Some(u) = &q.unit {
                map.insert("unit".to_string(), Value::String(u.clone()));
            }
            Some(Value::Object(map))
        }
        CqlValue::Code(c) => {
            let mut map = serde_json::Map::new();
            map.insert("system".to_string(), Value::String(c.system.clone()));
            map.insert("code".to_string(), Value::String(c.code.clone()));
            if let Some(d) = &c.display {
                map.insert("display".to_string(), Value::String(d.clone()));
            }
            Some(Value::Object(map))
        }
        CqlValue::List(list) => {
            let json_items: Vec<Value> = list
                .elements
                .iter()
                .filter_map(|item| cql_value_to_json(item))
                .collect();
            Some(Value::Array(json_items))
        }
        CqlValue::Tuple(tuple) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &tuple.elements {
                if let Some(json_v) = cql_value_to_json(v) {
                    map.insert(k.clone(), json_v);
                }
            }
            Some(Value::Object(map))
        }
        _ => None,
    }
}

pub(crate) fn json_to_cql_value(value: &Value) -> Option<CqlValue> {
    match value {
        Value::Null => Some(CqlValue::Null),
        Value::Bool(b) => Some(CqlValue::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Some(CqlValue::Integer(i as i32))
                } else {
                    Some(CqlValue::Long(i))
                }
            } else if let Some(f) = n.as_f64() {
                BigDecimal::try_from(f).ok().map(CqlValue::Decimal)
            } else {
                None
            }
        }
        Value::String(s) => Some(CqlValue::String(s.clone())),
        Value::Array(arr) => {
            let items: Vec<CqlValue> = arr
                .iter()
                .filter_map(|item| json_to_cql_value(item))
                .collect();
            Some(CqlValue::List(CqlList {
                element_type: CqlType::Any,
                elements: items,
            }))
        }
        Value::Object(map) => {
            let mut fields = IndexMap::new();
            for (k, v) in map {
                if let Some(cql_v) = json_to_cql_value(v) {
                    fields.insert(k.clone(), cql_v);
                }
            }
            Some(CqlValue::Tuple(CqlTuple { elements: fields }))
        }
    }
}

/// Convert FHIRPath Collection to CqlValue
fn fhirpath_to_cql_value(collection: &Collection) -> Option<CqlValue> {
    match collection.len() {
        0 => Some(CqlValue::Null),
        1 => {
            // Single value - convert to JSON then to CqlValue
            let value = collection.get(0)?;
            let json = fhirpath_value_to_json(value)?;
            json_to_cql_value(&json)
        }
        _ => {
            // Multiple values - convert to JSON array then to CQL List
            let json_values: Vec<Value> = collection
                .iter()
                .filter_map(fhirpath_value_to_json)
                .collect();
            json_to_cql_value(&Value::Array(json_values))
        }
    }
}

/// Convert single FHIRPath value to JSON
fn fhirpath_value_to_json(value: &FhirPathValue) -> Option<Value> {
    match value {
        FhirPathValue::Boolean(b, _, _) => Some(Value::Bool(*b)),
        FhirPathValue::String(s, _, _) => Some(Value::String(s.clone())),
        FhirPathValue::Integer(i, _, _) => Some(Value::Number((*i).into())),
        FhirPathValue::Decimal(d, _, _) => d
            .to_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number),
        FhirPathValue::Date(d, _, _) => Some(Value::String(d.to_string())),
        FhirPathValue::DateTime(dt, _, _) => Some(Value::String(dt.to_string())),
        FhirPathValue::Time(t, _, _) => Some(Value::String(t.to_string())),
        FhirPathValue::Quantity { value, unit, .. } => {
            let mut map = serde_json::Map::new();
            if let Some(v) = value.to_f64() {
                map.insert(
                    "value".to_string(),
                    Value::Number(serde_json::Number::from_f64(v)?),
                );
            }
            if let Some(u) = unit {
                map.insert("unit".to_string(), Value::String(u.clone()));
            }
            Some(Value::Object(map))
        }
        FhirPathValue::Resource(json, _, _) => Some((**json).clone()),
        FhirPathValue::Empty => Some(Value::Null),
        _ => None,
    }
}
