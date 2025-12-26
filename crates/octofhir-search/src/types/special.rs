//! Special search parameter type implementation.
//!
//! Special search parameters: _near, _text, _content, _filter, _list, _query

use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Special parameter types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialParameterType {
    Near,
    Text,
    Content,
    Filter,
    List,
    Query,
}

/// Parsed _near parameter.
#[derive(Debug, Clone)]
pub struct NearParameter {
    pub latitude: f64,
    pub longitude: f64,
    pub distance: f64,
    pub unit: String,
}

/// Parse a _near parameter: `latitude|longitude|distance|units`
pub fn parse_near_parameter(value: &str) -> Result<NearParameter, SqlBuilderError> {
    let parts: Vec<&str> = value.split('|').collect();

    if parts.len() < 2 {
        return Err(SqlBuilderError::InvalidSearchValue(
            "_near requires at least latitude|longitude".to_string(),
        ));
    }

    let parse_f64 = |s: &str, name: &str| {
        s.parse::<f64>()
            .map_err(|_| SqlBuilderError::InvalidSearchValue(format!("Invalid {name}: {s}")))
    };

    Ok(NearParameter {
        latitude: parse_f64(parts[0], "latitude")?,
        longitude: parse_f64(parts[1], "longitude")?,
        distance: parts
            .get(2)
            .map_or(Ok(10.0), |s| parse_f64(s, "distance"))?,
        unit: parts.get(3).map_or("km", |s| *s).to_string(),
    })
}

/// Build SQL for _near geolocation search using Haversine formula.
pub fn build_near_search(
    builder: &mut SqlBuilder,
    value: &str,
    position_path: &str,
) -> Result<(), SqlBuilderError> {
    let near = parse_near_parameter(value)?;

    let distance_km = match near.unit.as_str() {
        "mi" => near.distance * 1.60934,
        "m" => near.distance / 1000.0,
        _ => near.distance,
    };

    let lat_p = builder.add_text_param(near.latitude.to_string());
    let lon_p = builder.add_text_param(near.longitude.to_string());
    let dist_p = builder.add_text_param(distance_km.to_string());

    // Haversine formula
    builder.add_condition(format!(
        "(6371.0 * 2 * asin(sqrt(\
            power(sin(radians(({position_path}->>'latitude')::numeric - ${lat_p}::numeric) / 2), 2) + \
            cos(radians(${lat_p}::numeric)) * \
            cos(radians(({position_path}->>'latitude')::numeric)) * \
            power(sin(radians(({position_path}->>'longitude')::numeric - ${lon_p}::numeric) / 2), 2)\
        ))) <= ${dist_p}::numeric"
    ));

    Ok(())
}

/// Build SQL for _text search on narrative.
pub fn build_text_search(builder: &mut SqlBuilder, value: &str) -> Result<(), SqlBuilderError> {
    let p = builder.add_text_param(value);
    builder.add_condition(format!(
        "to_tsvector('english', regexp_replace(resource->'text'->>'div', '<[^>]*>', '', 'g')) \
         @@ plainto_tsquery('english', ${p})"
    ));
    Ok(())
}

/// Build SQL for _content search on entire resource.
pub fn build_content_search(builder: &mut SqlBuilder, value: &str) -> Result<(), SqlBuilderError> {
    let p = builder.add_text_param(value);
    builder.add_condition(format!(
        "to_tsvector('english', resource::text) @@ plainto_tsquery('english', ${p})"
    ));
    Ok(())
}

/// Build SQL for _filter parameter (basic expression parsing).
pub fn build_filter_search(
    builder: &mut SqlBuilder,
    value: &str,
    _resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(SqlBuilderError::InvalidSearchValue(
            "_filter requires 'field op value' format".to_string(),
        ));
    }

    let field = parts[0];
    let op = parts[1];
    let filter_value = parts[2..].join(" ").trim_matches('"').to_string();

    let json_path = format!("resource->>'{field}'");
    let p = builder.add_text_param(&filter_value);

    let condition = match op {
        "eq" => format!("{json_path} = ${p}"),
        "ne" => format!("{json_path} != ${p}"),
        "gt" => format!("{json_path} > ${p}"),
        "lt" => format!("{json_path} < ${p}"),
        "ge" => format!("{json_path} >= ${p}"),
        "le" => format!("{json_path} <= ${p}"),
        "co" => {
            let like_p = builder.add_text_param(format!("%{filter_value}%"));
            format!("{json_path} ILIKE ${like_p}")
        }
        "sw" => {
            let like_p = builder.add_text_param(format!("{filter_value}%"));
            format!("{json_path} ILIKE ${like_p}")
        }
        "ew" => {
            let like_p = builder.add_text_param(format!("%{filter_value}"));
            format!("{json_path} ILIKE ${like_p}")
        }
        _ => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "Filter operator '{op}' not supported"
            )));
        }
    };

    builder.add_condition(condition);
    Ok(())
}

/// Build SQL for _list parameter.
pub fn build_list_search(
    builder: &mut SqlBuilder,
    list_id: &str,
    base_type: &str,
) -> Result<(), SqlBuilderError> {
    let p = builder.add_text_param(list_id);
    let type_p = builder.add_text_param(base_type.to_string());

    builder.add_condition(format!(
        "EXISTS (SELECT 1 FROM list l, jsonb_array_elements(l.resource->'entry') AS entry \
         WHERE l.id::text = ${p} AND l.status != 'deleted' \
         AND fhir_ref_type(entry->'item'->>'reference') = ${type_p} \
         AND fhir_ref_id(entry->'item'->>'reference') = {}.id::text)",
        builder.resource_column()
    ));

    Ok(())
}

/// Detect special parameter type from name.
pub fn detect_special_type(name: &str) -> Option<SpecialParameterType> {
    match name {
        "_near" | "near" => Some(SpecialParameterType::Near),
        "_text" => Some(SpecialParameterType::Text),
        "_content" => Some(SpecialParameterType::Content),
        "_filter" => Some(SpecialParameterType::Filter),
        "_list" => Some(SpecialParameterType::List),
        "_query" => Some(SpecialParameterType::Query),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_near_parameter() {
        let r = parse_near_parameter("40.7128|-74.0060|10|km").unwrap();
        assert!((r.latitude - 40.7128).abs() < 0.0001);
        assert!((r.longitude - (-74.0060)).abs() < 0.0001);
        assert!((r.distance - 10.0).abs() < 0.0001);
        assert_eq!(r.unit, "km");

        // Minimal
        let r = parse_near_parameter("40.7|-74.0").unwrap();
        assert!((r.distance - 10.0).abs() < 0.0001);
        assert_eq!(r.unit, "km");

        // Invalid
        assert!(parse_near_parameter("invalid").is_err());
    }

    #[test]
    fn test_detect_special_type() {
        assert_eq!(
            detect_special_type("_near"),
            Some(SpecialParameterType::Near)
        );
        assert_eq!(
            detect_special_type("_text"),
            Some(SpecialParameterType::Text)
        );
        assert_eq!(
            detect_special_type("_content"),
            Some(SpecialParameterType::Content)
        );
        assert_eq!(detect_special_type("name"), None);
    }

    #[test]
    fn test_build_near_search() {
        let mut builder = SqlBuilder::new();
        build_near_search(
            &mut builder,
            "40.7128|-74.0060|10|km",
            "resource->'position'",
        )
        .unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("latitude") && clause.contains("asin"));
    }

    #[test]
    fn test_build_text_search() {
        let mut builder = SqlBuilder::new();
        build_text_search(&mut builder, "headache").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("to_tsvector") && clause.contains("text"));
    }

    #[test]
    fn test_build_content_search() {
        let mut builder = SqlBuilder::new();
        build_content_search(&mut builder, "patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource::text"));
    }

    #[test]
    fn test_build_filter_search() {
        let mut builder = SqlBuilder::new();
        build_filter_search(&mut builder, "name eq John", "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->>'name'") && clause.contains("="));

        let mut builder = SqlBuilder::new();
        build_filter_search(&mut builder, "name co Smith", "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("ILIKE"));
    }

    #[test]
    fn test_build_list_search() {
        let mut builder = SqlBuilder::new();
        build_list_search(&mut builder, "list123", "Patient").unwrap();
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("EXISTS") && clause.contains("entry"));
    }
}
