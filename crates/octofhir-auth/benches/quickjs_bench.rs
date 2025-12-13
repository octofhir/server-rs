//! Performance benchmarks for QuickJS policy evaluation.
//!
//! These benchmarks measure the performance of different aspects of QuickJS
//! policy evaluation to ensure optimal runtime behavior.
//!
//! Run with: `cargo bench -p octofhir-auth quickjs`

use std::collections::HashMap;
use std::sync::Arc;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use octofhir_auth::config::QuickJsConfig;
use octofhir_auth::policy::context::{
    ClientIdentity, ClientType, EnvironmentContext, PolicyContext, RequestContext, ResourceContext,
    ScopeSummary, UserIdentity,
};
use octofhir_auth::policy::quickjs::QuickJsRuntime;
use octofhir_auth::smart::scopes::FhirOperation;
use time::OffsetDateTime;

/// Create a minimal test context for benchmarking.
fn create_minimal_context() -> PolicyContext {
    PolicyContext {
        user: Some(UserIdentity {
            id: "user-123".to_string(),
            fhir_user: Some("Practitioner/456".to_string()),
            fhir_user_type: Some("Practitioner".to_string()),
            fhir_user_id: Some("456".to_string()),
            roles: vec!["doctor".to_string(), "admin".to_string()],
            attributes: HashMap::new(),
        }),
        client: ClientIdentity {
            id: "client-123".to_string(),
            name: "Test Client".to_string(),
            trusted: false,
            client_type: ClientType::Public,
        },
        scopes: ScopeSummary {
            raw: "user/Patient.cruds".to_string(),
            patient_scopes: vec![],
            user_scopes: vec!["user/Patient.cruds".to_string()],
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
            query_params: HashMap::new(),
            path: "/Patient/pat-123".to_string(),
            method: "GET".to_string(),
            operation_id: None,
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

/// Create a context with a small FHIR resource (simple Patient).
fn create_small_resource_context() -> PolicyContext {
    let mut ctx = create_minimal_context();
    ctx.resource = Some(ResourceContext {
        id: "pat-123".to_string(),
        resource_type: "Patient".to_string(),
        version_id: Some("1".to_string()),
        last_updated: Some("2024-01-15T10:30:00Z".to_string()),
        subject: None,
        author: None,
        resource: serde_json::json!({
            "resourceType": "Patient",
            "id": "pat-123",
            "meta": {
                "versionId": "1",
                "lastUpdated": "2024-01-15T10:30:00Z"
            },
            "name": [{
                "family": "Smith",
                "given": ["John"]
            }],
            "gender": "male",
            "birthDate": "1980-01-15"
        }),
    });
    ctx
}

/// Create a context with a large FHIR resource (complex Patient with extensions).
fn create_large_resource_context() -> PolicyContext {
    let mut ctx = create_minimal_context();

    // Create a complex patient with many extensions, identifiers, and nested data
    let complex_patient = serde_json::json!({
        "resourceType": "Patient",
        "id": "pat-complex-123",
        "meta": {
            "versionId": "5",
            "lastUpdated": "2024-01-15T10:30:00Z",
            "profile": [
                "http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"
            ],
            "security": [
                {"system": "http://terminology.hl7.org/CodeSystem/v3-Confidentiality", "code": "R"}
            ],
            "tag": [
                {"system": "http://example.org/tags", "code": "VIP"},
                {"system": "http://example.org/tags", "code": "Research"}
            ]
        },
        "extension": [
            {
                "url": "http://hl7.org/fhir/us/core/StructureDefinition/us-core-race",
                "extension": [
                    {"url": "ombCategory", "valueCoding": {"system": "urn:oid:2.16.840.1.113883.6.238", "code": "2106-3", "display": "White"}},
                    {"url": "text", "valueString": "White"}
                ]
            },
            {
                "url": "http://hl7.org/fhir/us/core/StructureDefinition/us-core-ethnicity",
                "extension": [
                    {"url": "ombCategory", "valueCoding": {"system": "urn:oid:2.16.840.1.113883.6.238", "code": "2186-5", "display": "Not Hispanic or Latino"}},
                    {"url": "text", "valueString": "Not Hispanic or Latino"}
                ]
            },
            {
                "url": "http://hl7.org/fhir/StructureDefinition/patient-birthPlace",
                "valueAddress": {
                    "city": "Boston",
                    "state": "MA",
                    "country": "USA"
                }
            },
            {
                "url": "http://hl7.org/fhir/StructureDefinition/patient-nationality",
                "extension": [
                    {"url": "code", "valueCodeableConcept": {"coding": [{"system": "urn:iso:std:iso:3166", "code": "US"}]}}
                ]
            }
        ],
        "identifier": [
            {"system": "http://hospital.example.org/patients", "value": "12345"},
            {"system": "http://hl7.org/fhir/sid/us-ssn", "value": "123-45-6789"},
            {"system": "http://hl7.org/fhir/sid/us-medicare", "value": "1EG4-TE5-MK72"},
            {"system": "urn:oid:1.2.36.146.595.217.0.1", "value": "VIC-12345678"},
            {"type": {"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0203", "code": "MR"}]}, "value": "MRN-001234"}
        ],
        "active": true,
        "name": [
            {"use": "official", "family": "Smith", "given": ["John", "Michael"], "prefix": ["Dr."], "suffix": ["Jr."]},
            {"use": "nickname", "given": ["Johnny"]},
            {"use": "maiden", "family": "Johnson"}
        ],
        "telecom": [
            {"system": "phone", "value": "555-123-4567", "use": "home"},
            {"system": "phone", "value": "555-987-6543", "use": "work"},
            {"system": "phone", "value": "555-555-5555", "use": "mobile"},
            {"system": "email", "value": "john.smith@example.com", "use": "home"},
            {"system": "email", "value": "jsmith@work.example.com", "use": "work"}
        ],
        "gender": "male",
        "birthDate": "1980-01-15",
        "deceasedBoolean": false,
        "address": [
            {
                "use": "home",
                "type": "physical",
                "line": ["123 Main Street", "Apt 4B"],
                "city": "Boston",
                "district": "Suffolk",
                "state": "MA",
                "postalCode": "02115",
                "country": "USA"
            },
            {
                "use": "work",
                "line": ["456 Business Ave", "Suite 100"],
                "city": "Cambridge",
                "state": "MA",
                "postalCode": "02139"
            }
        ],
        "maritalStatus": {
            "coding": [{"system": "http://terminology.hl7.org/CodeSystem/v3-MaritalStatus", "code": "M", "display": "Married"}]
        },
        "multipleBirthBoolean": false,
        "contact": [
            {
                "relationship": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0131", "code": "C"}]}],
                "name": {"family": "Smith", "given": ["Jane"]},
                "telecom": [{"system": "phone", "value": "555-111-2222"}]
            },
            {
                "relationship": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0131", "code": "E"}]}],
                "name": {"family": "Smith", "given": ["Robert"]},
                "telecom": [{"system": "phone", "value": "555-333-4444"}]
            }
        ],
        "communication": [
            {"language": {"coding": [{"system": "urn:ietf:bcp:47", "code": "en"}]}, "preferred": true},
            {"language": {"coding": [{"system": "urn:ietf:bcp:47", "code": "es"}]}}
        ],
        "generalPractitioner": [
            {"reference": "Practitioner/prac-123", "display": "Dr. Mary Johnson"},
            {"reference": "Organization/org-456", "display": "Primary Care Clinic"}
        ],
        "managingOrganization": {"reference": "Organization/org-789", "display": "General Hospital"},
        "link": [
            {"other": {"reference": "Patient/pat-old-123"}, "type": "replaces"}
        ]
    });

    ctx.resource = Some(ResourceContext {
        id: "pat-complex-123".to_string(),
        resource_type: "Patient".to_string(),
        version_id: Some("5".to_string()),
        last_updated: Some("2024-01-15T10:30:00Z".to_string()),
        subject: None,
        author: None,
        resource: complex_patient,
    });
    ctx
}

/// Benchmark: Pool creation time.
fn bench_pool_creation(c: &mut Criterion) {
    c.bench_function("quickjs_pool_creation", |b| {
        b.iter(|| black_box(QuickJsRuntime::new(QuickJsConfig::default()).unwrap()));
    });
}

/// Benchmark: Simple boolean script evaluation.
fn bench_simple_script(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    c.bench_function("quickjs_simple_script", |b| {
        b.iter(|| black_box(runtime.evaluate("return true;", &context)));
    });
}

/// Benchmark: allow() function call.
fn bench_allow_function(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    c.bench_function("quickjs_allow_function", |b| {
        b.iter(|| black_box(runtime.evaluate("return allow();", &context)));
    });
}

/// Benchmark: Role checking function.
fn bench_role_check(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    c.bench_function("quickjs_role_check", |b| {
        b.iter(|| black_box(runtime.evaluate(r#"return hasRole("doctor");"#, &context)));
    });
}

/// Benchmark: Complex conditional script.
fn bench_complex_script(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    let script = r#"
        if (hasRole("admin")) {
            return allow();
        } else if (request.method === "GET" && request.resourceType === "Patient") {
            if (hasAnyRole("doctor", "nurse")) {
                return allow();
            } else {
                return deny("Not authorized");
            }
        } else {
            return abstain();
        }
    "#;

    c.bench_function("quickjs_complex_script", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: Context access.
fn bench_context_access(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    let script = r#"
        const uid = user.id;
        const cid = client.id;
        const op = request.method;
        const rt = request.resourceType;
        return uid.length > 0 && cid.length > 0;
    "#;

    c.bench_function("quickjs_context_access", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: Small FHIR resource evaluation.
fn bench_small_resource(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_small_resource_context();

    let script = r#"
        if (resource && resource.id === "pat-123") {
            return allow();
        }
        return deny("Resource not found");
    "#;

    c.bench_function("quickjs_small_resource", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: Large FHIR resource evaluation.
fn bench_large_resource(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_large_resource_context();

    let script = r#"
        if (resource && resource.data && resource.data.id === "pat-complex-123") {
            const patient = resource.data;
            const hasIdentifier = patient.identifier && patient.identifier.length > 0;
            const hasExtension = patient.extension && patient.extension.length > 0;
            if (hasIdentifier && hasExtension) {
                return allow();
            }
        }
        return deny("Invalid resource");
    "#;

    c.bench_function("quickjs_large_resource", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: Large resource with deep access.
fn bench_large_resource_deep_access(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_large_resource_context();

    let script = r#"
        if (resource && resource.data) {
            const patient = resource.data;
            // Access multiple nested properties
            const name = patient.name?.[0]?.family;
            const phone = patient.telecom?.[0]?.value;
            const addr = patient.address?.[0]?.city;
            const ext = patient.extension?.[0]?.url;
            const id0 = patient.identifier?.[0]?.value;
            const id1 = patient.identifier?.[1]?.value;
            const id2 = patient.identifier?.[2]?.value;

            if (name && phone && addr) {
                return allow();
            }
        }
        return deny("Missing data");
    "#;

    c.bench_function("quickjs_large_resource_deep_access", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: ES2020 features (optional chaining, nullish coalescing).
fn bench_es2020_features(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    let script = r#"
        const patientId = user?.fhirUserId ?? "unknown";
        const clientName = client?.name ?? "anonymous";
        if (patientId !== "unknown" && clientName !== "anonymous") {
            return allow();
        }
        return deny("Unknown user");
    "#;

    c.bench_function("quickjs_es2020_features", |b| {
        b.iter(|| black_box(runtime.evaluate(script, &context)));
    });
}

/// Benchmark: Pool parallel evaluation (using sequential iteration for benchmark).
fn bench_pool_sequential(c: &mut Criterion) {
    let runtime = Arc::new(
        QuickJsRuntime::new(QuickJsConfig {
            pool_size: 4,
            ..Default::default()
        })
        .unwrap(),
    );
    let context = create_minimal_context();

    c.bench_function("quickjs_pool_sequential_10", |b| {
        b.iter(|| {
            for _ in 0..10 {
                black_box(runtime.evaluate("return allow();", &context));
            }
        });
    });
}

/// Benchmark: User type check.
fn bench_user_type_check(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let context = create_minimal_context();

    c.bench_function("quickjs_user_type_check", |b| {
        b.iter(|| black_box(runtime.evaluate("return isPractitionerUser();", &context)));
    });
}

/// Benchmark: Patient compartment check.
fn bench_compartment_check(c: &mut Criterion) {
    let runtime = QuickJsRuntime::new(QuickJsConfig::default()).unwrap();
    let mut context = create_small_resource_context();
    context.environment.patient_context = Some("pat-123".to_string());
    if let Some(ref mut resource) = context.resource {
        resource.subject = Some("Patient/pat-123".to_string());
    }

    c.bench_function("quickjs_compartment_check", |b| {
        b.iter(|| black_box(runtime.evaluate("return inPatientCompartment();", &context)));
    });
}

criterion_group!(
    benches,
    bench_pool_creation,
    bench_simple_script,
    bench_allow_function,
    bench_role_check,
    bench_complex_script,
    bench_context_access,
    bench_small_resource,
    bench_large_resource,
    bench_large_resource_deep_access,
    bench_es2020_features,
    bench_pool_sequential,
    bench_user_type_check,
    bench_compartment_check,
);

criterion_main!(benches);
