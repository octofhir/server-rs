//! UCUM canonical quantities for unit-aware quantity search.
//!
//! A quantity search with the UCUM system matches stored quantities in any
//! commensurable unit: `value-quantity=1|http://unitsofmeasure.org|g` finds
//! `1000 mg`. PostgreSQL cannot evaluate UCUM, so the conversion happens in
//! Rust on both sides: [`canonical_quantities`] runs on write and fills the
//! `ucum` column of the resource table, and the search renderer converts the
//! query bounds with [`CanonicalUnit::convert`].
//!
//! The `ucum` column holds one entry per UCUM quantity in the resource:
//! `{"p": <element path>, "u": <canonical unit>, "v": <canonical value>}`.
//! The path joins object keys from the root and skips array indices
//! (`component.valueQuantity`), which is the form search parameter paths take.
//! Values are decimal strings so conversion never goes through a float.

use std::sync::{Arc, LazyLock};

use octofhir_ucum::Number;
use serde_json::{Map, Value};

/// The UCUM code system URI.
pub const UCUM_SYSTEM: &str = "http://unitsofmeasure.org";

/// The table column that holds canonical quantities.
pub const UCUM_COLUMN: &str = "ucum";

/// A UCUM unit resolved to its canonical form: a value `v` in the unit is
/// `v × factor + offset` in [`CanonicalUnit::unit`].
#[derive(Debug, Clone)]
pub struct CanonicalUnit {
    /// Canonical unit, e.g. `g` for any mass.
    pub unit: String,
    factor: Number,
    offset: Number,
}

/// Resolved units by code. Bounded because query codes are client input.
static UNITS: LazyLock<moka::sync::Cache<String, Option<Arc<CanonicalUnit>>>> =
    LazyLock::new(|| moka::sync::Cache::new(10_000));

impl CanonicalUnit {
    /// Resolves a UCUM code, or `None` when the code is not valid UCUM or its
    /// scale is not linear (`[pH]`, `B`).
    pub fn of(code: &str) -> Option<Arc<Self>> {
        if let Some(cached) = UNITS.get(code) {
            return cached;
        }
        let resolved = Self::resolve(code).map(Arc::new);
        UNITS.insert(code.to_owned(), resolved.clone());
        resolved
    }

    fn resolve(code: &str) -> Option<Self> {
        let canonical = octofhir_ucum::get_canonical_units(code).ok()?;
        // The library computes magnitudes exactly but reports them as f64; the
        // shortest decimal of that f64 is the exact magnitude (0.001, 453.59237).
        let factor = Number::from_f64(canonical.factor)?;
        let offset = Number::from_f64(canonical.offset)?;
        if factor.is_zero() || factor.is_negative() {
            return None;
        }
        Some(Self {
            unit: canonical.unit,
            factor,
            offset,
        })
    }

    /// Converts a decimal value in this unit to the canonical unit, exactly up
    /// to 28 significant digits.
    pub fn convert(&self, value: &str) -> Option<String> {
        let canonical = Number::parse(value)?
            .checked_mul(self.factor)?
            .checked_add(self.offset)?;
        Some(canonical.to_string())
    }
}

/// Collects every UCUM quantity of `resource` in canonical form, or `None`
/// when it has none (the column then stays NULL).
pub fn canonical_quantities(resource: &Value) -> Option<Value> {
    let object = resource.as_object()?;
    let mut entries = Vec::new();
    let mut path = String::new();
    collect_object(object, &mut path, &mut entries, true);
    (!entries.is_empty()).then_some(Value::Array(entries))
}

fn collect_object(
    object: &Map<String, Value>,
    path: &mut String,
    out: &mut Vec<Value>,
    root: bool,
) {
    if let Some(entry) = canonical_entry(object, path) {
        out.push(entry);
        return;
    }
    for (key, value) in object {
        // Contained resources are not searched through the container's params.
        if root && key == "contained" {
            continue;
        }
        if !matches!(value, Value::Object(_) | Value::Array(_)) {
            continue;
        }
        let len = path.len();
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(key);
        collect_value(value, path, out);
        path.truncate(len);
    }
}

fn collect_value(value: &Value, path: &mut String, out: &mut Vec<Value>) {
    match value {
        Value::Object(object) => collect_object(object, path, out, false),
        Value::Array(items) => {
            for item in items {
                collect_value(item, path, out);
            }
        }
        _ => {}
    }
}

fn canonical_entry(object: &Map<String, Value>, path: &str) -> Option<Value> {
    if object.get("system")?.as_str()? != UCUM_SYSTEM {
        return None;
    }
    let code = object.get("code")?.as_str()?;
    let value = object.get("value")?.as_number()?;
    let unit = CanonicalUnit::of(code)?;
    let canonical = unit.convert(&value.to_string())?;
    Some(serde_json::json!({ "p": path, "u": unit.unit, "v": canonical }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn convert(value: &str, code: &str) -> (String, String) {
        let unit = CanonicalUnit::of(code).unwrap();
        (unit.unit.clone(), unit.convert(value).unwrap())
    }

    #[test]
    fn commensurable_units_share_a_canonical_value() {
        assert_eq!(convert("1000", "mg"), convert("1", "g"));
        assert_eq!(convert("1", "kg"), convert("0.001", "t"));
        assert_eq!(convert("2", "[lb_av]").1, convert("907.18474", "g").1);
    }

    #[test]
    fn conversion_is_exact_decimal() {
        assert_eq!(convert("5.4", "mg").1, convert("0.0054", "g").1);
        assert_eq!(convert("0.1", "mg").1, convert("0.0001", "g").1);
    }

    #[test]
    fn temperature_applies_the_offset() {
        assert_eq!(convert("37", "Cel"), convert("310.15", "K"));
        // 5/9 has no finite decimal, so Fahrenheit is close rather than exact.
        let kelvin: f64 = convert("32", "[degF]").1.parse().unwrap();
        assert!((kelvin - 273.15).abs() < 1e-9, "{kelvin}");
    }

    #[test]
    fn invalid_and_non_linear_units_are_rejected() {
        assert!(CanonicalUnit::of("not a unit").is_none());
    }

    #[test]
    fn collects_ucum_quantities_with_their_paths() {
        let resource = json!({
            "resourceType": "Observation",
            "valueQuantity": {"value": 1000, "unit": "mg", "system": UCUM_SYSTEM, "code": "mg"},
            "component": [
                {"valueQuantity": {"value": 120, "system": UCUM_SYSTEM, "code": "mm[Hg]"}},
                {"valueQuantity": {"value": 5, "system": "http://example.org", "code": "g"}}
            ],
            "contained": [
                {"resourceType": "Observation",
                 "valueQuantity": {"value": 1, "system": UCUM_SYSTEM, "code": "g"}}
            ]
        });
        let entries = canonical_quantities(&resource).unwrap();
        let entries = entries.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        let at = |path: &str| entries.iter().find(|e| e["p"] == path).unwrap();
        assert_eq!(at("valueQuantity")["v"], convert("1", "g").1);
        assert_eq!(at("component.valueQuantity")["u"], convert("1", "mm[Hg]").0);
    }

    #[test]
    fn resources_without_ucum_quantities_yield_none() {
        assert!(
            canonical_quantities(&json!({"resourceType": "Patient", "name": [{"family": "X"}]}))
                .is_none()
        );
    }
}
