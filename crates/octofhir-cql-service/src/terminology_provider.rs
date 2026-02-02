//! Terminology provider adapter for CQL engine

use octofhir_cql_eval::TerminologyProvider;
use octofhir_cql_types::CqlValue;

pub struct CqlTerminologyProvider {}

impl CqlTerminologyProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for CqlTerminologyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminologyProvider for CqlTerminologyProvider {
    fn in_value_set(&self, _code: &CqlValue, _value_set_id: &str) -> Option<bool> {
        // Return None (unknown) - actual implementation would use octofhir-fhir-model
        None
    }

    fn in_code_system(&self, _code: &CqlValue, _code_system_id: &str) -> Option<bool> {
        None
    }

    fn expand_value_set(&self, _value_set_id: &str) -> Option<Vec<CqlValue>> {
        Some(Vec::new())
    }

    fn lookup_display(&self, code: &CqlValue) -> Option<String> {
        if let CqlValue::Code(c) = code {
            c.display.clone()
        } else {
            None
        }
    }
}
