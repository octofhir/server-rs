//! Criterion micro-benchmarks for the date-search pipeline.
//!
//! Scope: pure-Rust components — parsing FHIR date strings, extracting date
//! ranges from FHIR resource JSON, building SQL fragments per prefix, and the
//! string-normalisation hot path used by every text-shaped search parameter.
//! These are the steps that run on every write (extractor) and every read
//! (parser + SQL builder); regression here shows up immediately under load.
//!
//! Out of scope: anything that needs a live Postgres. Live-DB throughput
//! (bulk INSERT, GiST search latency on partitioned tables) belongs in a
//! separate bench file gated behind testcontainers — that bench measures
//! Postgres performance, not Rust code, and runs at a different cadence.
//!
//! Run with:
//!     cargo bench -p octofhir-search --bench date_search
//!
//! Targets a 2024-era laptop. Treat absolute numbers as wall-clock for the
//! current commit; the value of this bench is comparison between commits.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use octofhir_core::search_index::{extract_dates, normalize_string, parse_date_to_range};
use octofhir_search::parameters::{SearchModifier, SearchPrefix};
use octofhir_search::parser::{ParsedParam, ParsedValue, SearchParameterParser};
use octofhir_search::sql_builder::SqlBuilder;
use octofhir_search::types::date::build_index_date_search;
use serde_json::{Value, json};
use std::hint::black_box;

// ---------------------------------------------------------------------------
// parse_date_to_range — every FHIR `date`/`dateTime`/`instant` string goes
// through this on the write path (extractor) and write-path latency is
// dominated by the parser cost when many date-typed search params apply.
// ---------------------------------------------------------------------------

fn bench_parse_date_to_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_date_to_range");

    let cases: &[(&str, &str)] = &[
        ("year", "2024"),
        ("year_month", "2024-03"),
        ("day", "2024-03-15"),
        ("dt_second_utc", "2024-03-15T11:00:00Z"),
        ("dt_millis_utc", "2024-03-15T11:00:00.123Z"),
        ("dt_micros_utc", "2024-03-15T11:00:00.123456Z"),
        ("dt_nanos_utc", "2024-03-15T11:00:00.123456789Z"),
        ("dt_tz_offset", "2024-03-15T11:00:00+05:30"),
        ("dt_negative_offset", "2024-03-15T11:00:00-08:00"),
    ];

    for (label, input) in cases {
        group.bench_with_input(BenchmarkId::from_parameter(label), input, |b, &s| {
            b.iter(|| parse_date_to_range(black_box(s)));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// extract_dates — runs once per `(resource, SearchParameter)` on every write.
// The shapes below cover the common FHIR date-typed elements:
//   - scalar string (Patient.birthDate)
//   - Period (Encounter.period) — both closed and open
//   - dateTime polymorphic (Observation.effectiveDateTime)
//   - Timing.event[] arrays
//   - repeating Periods (BackboneElement[] each carrying a Period)
// ---------------------------------------------------------------------------

fn patient_with_birthdate() -> Value {
    json!({
        "resourceType": "Patient",
        "id": "p1",
        "birthDate": "1980-06-15"
    })
}

fn encounter_with_closed_period() -> Value {
    json!({
        "resourceType": "Encounter",
        "id": "e1",
        "period": {
            "start": "2024-03-15T08:00:00Z",
            "end":   "2024-03-15T16:30:00Z"
        }
    })
}

fn encounter_with_open_period() -> Value {
    json!({
        "resourceType": "Encounter",
        "id": "e2",
        "period": { "start": "2024-03-15T08:00:00Z" }
    })
}

fn observation_effective_datetime() -> Value {
    json!({
        "resourceType": "Observation",
        "id": "o1",
        "effectiveDateTime": "2024-03-15T11:00:00.123Z"
    })
}

fn medication_statement_with_timing_events() -> Value {
    // MedicationStatement.effectiveTiming.event[5]
    json!({
        "resourceType": "MedicationStatement",
        "id": "ms1",
        "effectiveTiming": {
            "event": [
                "2024-03-01T08:00:00Z",
                "2024-03-02T08:00:00Z",
                "2024-03-03T08:00:00Z",
                "2024-03-04T08:00:00Z",
                "2024-03-05T08:00:00Z"
            ]
        }
    })
}

fn patient_contact_periods() -> Value {
    // Patient.contact[3].period — repeating periods inside a BackboneElement
    // array. Useful for measuring per-element extraction overhead.
    json!({
        "resourceType": "Patient",
        "id": "p2",
        "contact": [
            { "period": { "start": "2020-01-01", "end": "2020-12-31" } },
            { "period": { "start": "2021-01-01", "end": "2021-12-31" } },
            { "period": { "start": "2022-01-01", "end": "2022-12-31" } }
        ]
    })
}

type ExtractScenario = (&'static str, &'static str, &'static str, fn() -> Value);

fn bench_extract_dates(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_dates");

    let scenarios: &[ExtractScenario] = &[
        (
            "patient_birthdate",
            "Patient",
            "Patient.birthDate",
            patient_with_birthdate,
        ),
        (
            "encounter_period_closed",
            "Encounter",
            "Encounter.period",
            encounter_with_closed_period,
        ),
        (
            "encounter_period_open",
            "Encounter",
            "Encounter.period",
            encounter_with_open_period,
        ),
        (
            "observation_effective_dt",
            "Observation",
            "Observation.effective",
            observation_effective_datetime,
        ),
        (
            "medication_timing_5events",
            "MedicationStatement",
            "MedicationStatement.effective",
            medication_statement_with_timing_events,
        ),
        (
            "patient_contact_periods_3",
            "Patient",
            "Patient.contact.period",
            patient_contact_periods,
        ),
    ];

    for (label, resource_type, expression, builder) in scenarios {
        let resource = builder();
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(resource, *resource_type, *expression),
            |b, (res, rt, expr)| {
                b.iter(|| extract_dates(black_box(res), black_box(rt), "x", black_box(expr)));
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// build_index_date_search — every search request with a date-typed parameter
// goes through this once per parameter. The SQL body is short but it's run
// at every request, so any allocation regression compounds.
// ---------------------------------------------------------------------------

fn make_date_param(prefix: SearchPrefix, value: &str) -> ParsedParam {
    ParsedParam {
        name: "birthdate".to_string(),
        modifier: None,
        values: vec![ParsedValue {
            prefix: Some(prefix),
            raw: value.to_string(),
        }],
    }
}

fn bench_build_index_date_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_index_date_search");

    // Each prefix produces a structurally different SQL template, so they're
    // benchmarked independently — `ap` does extra `±10%` range arithmetic,
    // `ne` wraps the EXISTS in NOT, the simple range operators are all
    // similar but landed in a single match arm so they share a code path.
    let prefixes: &[(&str, SearchPrefix)] = &[
        ("eq", SearchPrefix::Eq),
        ("ne", SearchPrefix::Ne),
        ("gt", SearchPrefix::Gt),
        ("lt", SearchPrefix::Lt),
        ("ge", SearchPrefix::Ge),
        ("le", SearchPrefix::Le),
        ("sa", SearchPrefix::Sa),
        ("eb", SearchPrefix::Eb),
        ("ap", SearchPrefix::Ap),
    ];

    for (label, prefix) in prefixes {
        group.bench_with_input(BenchmarkId::from_parameter(label), prefix, |b, p| {
            b.iter(|| {
                let mut builder = SqlBuilder::with_resource_column("r.resource");
                let param = make_date_param(*p, "2024-03-15");
                build_index_date_search(black_box(&mut builder), black_box(&param), "Patient")
                    .unwrap();
                builder.build_where_clause()
            });
        });
    }

    // Also exercise a :missing query — a structurally different branch that
    // skips the date parser entirely.
    group.bench_function("missing_true", |b| {
        b.iter(|| {
            let mut builder = SqlBuilder::with_resource_column("r.resource");
            let param = ParsedParam {
                name: "birthdate".to_string(),
                modifier: Some(SearchModifier::Missing),
                values: vec![ParsedValue {
                    prefix: None,
                    raw: "true".to_string(),
                }],
            };
            build_index_date_search(black_box(&mut builder), black_box(&param), "Patient").unwrap();
            builder.build_where_clause()
        });
    });

    group.finish();
}

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

criterion_group!(
    benches,
    bench_parse_date_to_range,
    bench_extract_dates,
    bench_build_index_date_search,
    bench_parse_query,
    bench_normalize_string,
);
criterion_main!(benches);
