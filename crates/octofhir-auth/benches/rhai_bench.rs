//! Performance benchmarks for Rhai policy evaluation.
//!
//! These benchmarks measure the performance of different aspects of Rhai
//! policy evaluation to ensure optimal runtime behavior.
//!
//! Run with: `cargo bench -p octofhir-auth rhai`

use std::collections::HashMap;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use octofhir_auth::config::RhaiConfig;
use octofhir_auth::policy::context::{
    ClientIdentity, ClientType, EnvironmentContext, PolicyContext, RequestContext, ScopeSummary,
    UserIdentity,
};
use octofhir_auth::policy::rhai::RhaiRuntime;
use octofhir_auth::smart::scopes::FhirOperation;
use time::OffsetDateTime;

/// Create a realistic test context for benchmarking.
fn create_benchmark_context() -> PolicyContext {
    PolicyContext {
        user: Some(UserIdentity {
            id: "user-123".to_string(),
            fhir_user: Some("Practitioner/456".to_string()),
            fhir_user_type: Some("Practitioner".to_string()),
            fhir_user_id: Some("456".to_string()),
            roles: vec![
                "doctor".to_string(),
                "admin".to_string(),
                "researcher".to_string(),
            ],
            attributes: HashMap::new(),
        }),
        client: ClientIdentity {
            id: "client-123".to_string(),
            name: "Test Client".to_string(),
            trusted: false,
            client_type: ClientType::Public,
        },
        scopes: ScopeSummary {
            raw: "user/Patient.cruds user/Observation.rs".to_string(),
            patient_scopes: vec![],
            user_scopes: vec![
                "user/Patient.cruds".to_string(),
                "user/Observation.rs".to_string(),
            ],
            system_scopes: vec![],
            has_wildcard: false,
            launch: false,
            openid: true,
            fhir_user: true,
            offline_access: false,
        },
        request: RequestContext {
            operation: FhirOperation::Read,
            resource_type: "Patient".to_string(),
            resource_id: Some("pat-123".to_string()),
            compartment_type: None,
            compartment_id: None,
            body: None,
            query_params: {
                let mut map = HashMap::new();
                map.insert("_count".to_string(), "10".to_string());
                map
            },
            path: "/Patient/pat-123".to_string(),
            method: "GET".to_string(),
        },
        resource: None,
        environment: EnvironmentContext {
            request_time: OffsetDateTime::now_utc(),
            source_ip: Some("192.168.1.100".parse().unwrap()),
            request_id: "req-123".to_string(),
            patient_context: Some("Patient/pat-123".to_string()),
            encounter_context: None,
        },
    }
}

/// Benchmark: Engine creation time.
///
/// This should only happen once at startup, but we measure it to ensure
/// it's reasonable.
fn bench_engine_creation(c: &mut Criterion) {
    c.bench_function("rhai_engine_creation", |b| {
        b.iter(|| black_box(RhaiRuntime::new(RhaiConfig::default())));
    });
}

/// Benchmark: Simple boolean script evaluation.
///
/// This measures the hot path for simple policies.
fn bench_simple_script(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    c.bench_function("rhai_simple_script", |b| {
        b.iter(|| black_box(runtime.evaluate("true", &context)));
    });
}

/// Benchmark: Role checking function.
///
/// This measures the performance of our custom helper functions.
fn bench_role_check(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    c.bench_function("rhai_role_check", |b| {
        b.iter(|| black_box(runtime.evaluate(r#"has_role(user, "doctor")"#, &context)));
    });
}

/// Benchmark: Complex conditional script.
///
/// This measures more realistic policy logic.
fn bench_complex_script(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    let script = r#"
        if has_role(user, "admin") {
            allow()
        } else if request.operation == "Read" && request.resourceType == "Patient" {
            if has_any_role(user, ["doctor", "nurse"]) {
                allow()
            } else {
                deny("Not authorized")
            }
        } else {
            abstain()
        }
    "#;

    c.bench_function("rhai_complex_script", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: AST cache hit.
///
/// This verifies that cached scripts are faster than first-time compilation.
fn bench_cache_hit(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    // Prime the cache
    let _ = runtime.evaluate("true", &context);

    c.bench_function("rhai_cache_hit", |b| {
        b.iter(|| black_box(runtime.evaluate("true", &context)));
    });
}

/// Benchmark: First-time script compilation (cache miss).
///
/// This measures the cost of compiling a new script.
fn bench_cache_miss(c: &mut Criterion) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let counter = AtomicUsize::new(0);
    let context = create_benchmark_context();

    c.bench_function("rhai_cache_miss", |b| {
        // Create a fresh runtime for each iteration to ensure cache miss
        let runtime = RhaiRuntime::new(RhaiConfig::default());

        b.iter(|| {
            // Generate a unique script each time to force cache miss
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let script = format!("true /* unique: {} */", n);
            black_box(runtime.evaluate(&script, &context))
        });
    });
}

/// Benchmark: User type check.
fn bench_user_type_check(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    c.bench_function("rhai_user_type_check", |b| {
        b.iter(|| black_box(runtime.evaluate("is_practitioner_user(user)", &context)));
    });
}

/// Benchmark: Accessing context variables.
fn bench_context_access(c: &mut Criterion) {
    let runtime = RhaiRuntime::new(RhaiConfig::default());
    let context = create_benchmark_context();

    let script = r#"
        let uid = user.id;
        let cid = client.id;
        let op = request.operation;
        let rt = request.resourceType;
        uid.len() > 0 && cid.len() > 0
    "#;

    c.bench_function("rhai_context_access", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

criterion_group!(
    benches,
    bench_engine_creation,
    bench_simple_script,
    bench_role_check,
    bench_complex_script,
    bench_cache_hit,
    bench_cache_miss,
    bench_user_type_check,
    bench_context_access,
);

criterion_main!(benches);
