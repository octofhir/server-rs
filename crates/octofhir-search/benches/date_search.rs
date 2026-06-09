//! Criterion micro-benchmarks for the search pipeline.
//!
//! Scope: pure-Rust components — parsing query strings, the
//! string-normalisation hot path used by every text-shaped search parameter,
//! and compiling representative FHIR searches into parameterized SQL over the
//! resource JSONB. These run on every request; regression here shows up
//! immediately under load.
//!
//! Out of scope: anything that needs a live Postgres. Live-DB throughput
//! belongs in a separate bench file gated behind testcontainers — that bench
//! measures Postgres performance, not Rust code, and runs at a different
//! cadence.
//!
//! Run with:
//!     cargo bench -p octofhir-search --bench date_search
//!
//! Targets a 2024-era laptop. Treat absolute numbers as wall-clock for the
//! current commit; the value of this bench is comparison between commits.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use octofhir_core::text::normalize_string;
use octofhir_search::parameters::{
    ElementTypeHint, SearchParameter, SearchParameterComponent, SearchParameterType,
};
use octofhir_search::parser::SearchParameterParser;
use octofhir_search::registry::SearchParameterRegistry;
use octofhir_search::{
    ParamsSearchConfig, UnknownParamHandling, build_native_ir_query_from_params,
    build_native_ir_query_from_params_with_config, parse_query_string, register_common_parameters,
};
use std::hint::black_box;

// ---------------------------------------------------------------------------
// SearchParameterParser::parse_query — runs once per request. Cheap, but
// shows up under high QPS. Bench mixed-shape query strings to keep the path
// representative (single param, comma-OR, &-AND, modifier, prefix, URL-enc).
// ---------------------------------------------------------------------------

fn bench_parse_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_query");

    let queries: &[(&str, &str)] = &[
        ("simple_eq", "name=John"),
        ("with_modifier", "family:exact=Smith&given:contains=Jo"),
        (
            "and_repeated_prefix",
            "birthdate=ge1980-01-01&birthdate=lt2000-01-01",
        ),
        ("comma_or", "category=medication,biologic,food"),
        (
            "mixed_full",
            "name:exact=John%20Doe&birthdate=ge1980-01-01&_count=50&_sort=-_lastUpdated",
        ),
    ];

    for (label, query) in queries {
        group.bench_with_input(BenchmarkId::from_parameter(label), query, |b, q| {
            b.iter(|| SearchParameterParser::parse_query(black_box(q)));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// normalize_string — runs on every default-modifier string search value.
// The Unicode NFD + combining-mark filter is more work than the previous
// `to_lowercase` call; bench guards against regression and shows the
// overhead per-character vs ASCII fast path.
// ---------------------------------------------------------------------------

fn bench_normalize_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalize_string");

    let cases: &[(&str, &str)] = &[
        ("ascii_short", "smith"),
        ("ascii_long", "the quick brown fox jumps over the lazy dog"),
        ("latin_diacritics", "García-Müller"),
        ("mixed_unicode", "Renée Smith-Jönsson"),
        ("multi_combining", "n\u{0303}n\u{0303}n\u{0303}n\u{0303}"), // pre-decomposed
    ];

    for (label, input) in cases {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), input, |b, s| {
            b.iter(|| normalize_string(black_box(s)));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Native-IR query build/render — representative FHIR search requests compiled
// into parameterized SQL without touching Postgres. This is the read-path Rust
// baseline that complements live DB EXPLAIN/latency measurements.
// ---------------------------------------------------------------------------

fn representative_registry() -> SearchParameterRegistry {
    let registry = SearchParameterRegistry::new();
    register_common_parameters(&registry);

    registry.register(
        SearchParameter::new(
            "birthdate",
            "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
            SearchParameterType::Date,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.birthDate"),
    );
    registry.register(
        SearchParameter::new(
            "family",
            "http://hl7.org/fhir/SearchParameter/Patient-family",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family")
        .with_element_type_hint(ElementTypeHint::Array("string".to_string())),
    );
    registry.register(
        SearchParameter::new(
            "identifier",
            "http://hl7.org/fhir/SearchParameter/Patient-identifier",
            SearchParameterType::Token,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.identifier")
        .with_element_type_hint(ElementTypeHint::Identifier),
    );
    registry.register(
        SearchParameter::new(
            "gender",
            "http://hl7.org/fhir/SearchParameter/Patient-gender",
            SearchParameterType::Token,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.gender")
        .with_element_type_hint(ElementTypeHint::SimpleCode),
    );
    registry.register(
        SearchParameter::new(
            "code",
            "http://hl7.org/fhir/SearchParameter/Observation-code",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.code")
        .with_element_type_hint(ElementTypeHint::Token),
    );
    registry.register(
        SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".to_string()]),
    );
    registry.register(
        SearchParameter::new(
            "value-quantity",
            "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
            SearchParameterType::Quantity,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.valueQuantity"),
    );
    registry.register(
        SearchParameter::new(
            "code-value-quantity",
            "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
            SearchParameterType::Composite,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.component")
        .with_components(vec![
            SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                expression: "Observation.component.code".to_string(),
            },
            SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                    .to_string(),
                expression: "Observation.component.valueQuantity".to_string(),
            },
        ]),
    );

    registry
}

fn representative_query_cases() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("patient_id", "Patient", "_id=pat-1&_count=10"),
        (
            "patient_last_updated_window",
            "Patient",
            "_lastUpdated=ge2024-01-01&_lastUpdated=le2024-12-31&_count=10",
        ),
        (
            "patient_birthdate_window",
            "Patient",
            "birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
        ),
        ("patient_family_prefix", "Patient", "family=Smith&_count=10"),
        (
            "patient_family_exact",
            "Patient",
            "family:exact=Smith&_count=10",
        ),
        (
            "patient_identifier",
            "Patient",
            "identifier=http://hospital.example/mrn|12345&_count=10",
        ),
        ("patient_gender", "Patient", "gender=female&_count=10"),
        (
            "patient_gender_system_only",
            "Patient",
            "gender=http://example.org|&_count=10",
        ),
        (
            "observation_code",
            "Observation",
            "code=http://loinc.org|8480-6&_count=10",
        ),
        (
            "observation_subject",
            "Observation",
            "subject=Patient/pat-1&_count=10",
        ),
        (
            "observation_subject_patient_family",
            "Observation",
            "subject:Patient.family=Smith&_count=10",
        ),
        (
            "patient_has_observation_code",
            "Patient",
            "_has:Observation:subject:code=http://loinc.org|8480-6&_count=10",
        ),
        (
            "observation_quantity",
            "Observation",
            "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        ),
        (
            "observation_composite_code_quantity",
            "Observation",
            "code-value-quantity=http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        ),
    ]
}

fn bench_native_ir_query_build_render(c: &mut Criterion) {
    let registry = representative_registry();
    let mut group = c.benchmark_group("native_ir_query_build_render");

    for (label, resource_type, query) in representative_query_cases() {
        let params = parse_query_string(query, 10, 100);
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(*resource_type, params),
            |b, (rt, parsed)| {
                b.iter(|| {
                    let converted = build_native_ir_query_from_params(
                        black_box(rt),
                        black_box(parsed),
                        black_box(&registry),
                        "public",
                    )
                    .unwrap();
                    converted.builder.with_raw_resource(true).build().unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_native_ir_query_build_render_with_debug_plan(c: &mut Criterion) {
    let registry = representative_registry();
    let config = ParamsSearchConfig {
        unknown_param_handling: UnknownParamHandling::Lenient,
        collect_debug_plan: true,
    };
    let mut group = c.benchmark_group("native_ir_query_build_render_debug_plan");

    for (label, resource_type, query) in representative_query_cases() {
        let params = parse_query_string(query, 10, 100);
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(*resource_type, params),
            |b, (rt, parsed)| {
                b.iter(|| {
                    let converted = build_native_ir_query_from_params_with_config(
                        black_box(rt),
                        black_box(parsed),
                        black_box(&registry),
                        "public",
                        black_box(&config),
                    )
                    .unwrap();
                    black_box(
                        converted
                            .debug_plan
                            .as_ref()
                            .map(|plan| plan.predicates.len()),
                    );
                    converted.builder.with_raw_resource(true).build().unwrap()
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_query,
    bench_normalize_string,
    bench_native_ir_query_build_render,
    bench_native_ir_query_build_render_with_debug_plan,
);
criterion_main!(benches);
