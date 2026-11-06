use super::*;

#[test]
fn revinclude_guards_regexp_and_does_not_multiply_source_rows() {
    let registry = SearchParameterRegistry::new();
    registry.register(
        octofhir_search::SearchParameter::new(
            "subject",
            "urn:test:subject",
            octofhir_search::SearchParameterType::Reference,
            vec!["Observation".into()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".into()]),
    );
    for raw in [false, true] {
        let sql = build_reverse_inline_query(
            "Observation",
            "subject",
            "Patient",
            &["p1"],
            IncludeExecution {
                registry: &registry,
                limits: IncludeLimits::default(),
                visibility: None,
                snapshot: None,
            },
            raw,
        )
        .unwrap()
        .unwrap()
        .sql;
        assert!(sql.contains("CASE WHEN split_part"));
        assert!(sql.contains("THEN (regexp_match"));
        assert!(sql.contains(REFERENCE_TYPE_ID_RE));
        assert!(sql.contains("r.status != 'deleted'"));
        assert!(sql.contains("EXISTS"));
        assert!(!sql.contains("DISTINCT"));
        assert_eq!(sql.contains("r.resource::text"), raw);
    }
}
