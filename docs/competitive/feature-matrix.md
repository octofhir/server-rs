# Feature Matrix: OctoFHIR vs Competitors

Comparison across 6 axes (~80 rows). Each cell: `✅ full / ⚠️ partial / ❌ missing` + 1-line evidence.

**Legend**: OctoFHIR evidence references `file:line` in the codebase. Competitor evidence references official docs/URLs.

---

## A. FHIR Conformance

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **FHIR Versions** | ✅ R4/R4B/R5/R6 `server.rs:551-558` | ✅ DSTU2/STU3/R4/R4B | ⚠️ R4/R4B only | ⚠️ R4 only | ✅ DSTU2/STU3/R4/R5 | ✅ DSTU2/STU3/R4/R5 | ⚠️ R4 only | ✅ STU3/R4/R5 | ✅ STU3/R4/R5 |
| **read** | ✅ `handlers.rs:616` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **vread** | ✅ `queries/crud.rs:433-516` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **create** | ✅ `handlers.rs:652` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **update** | ✅ `handlers.rs:1481` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **patch (JSON Patch)** | ✅ `handlers.rs:1829-2100` + `patch.rs` | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ Limited | ✅ | ✅ |
| **patch (FHIRPath Patch)** | ✅ `handlers.rs:1829-2100` | ✅ | ⚠️ Limited | ❌ | ⚠️ | ❌ | ❌ | ✅ | ⚠️ |
| **delete** | ✅ `handlers.rs:1762` (soft delete) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **history (instance)** | ✅ `schema.rs:353-405` (auto-trigger) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **history (type)** | ✅ History tables per resource type | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **history (system)** | ✅ `traits.rs:140-152` system_history | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ❌ | ✅ | ✅ |
| **batch** | ✅ `transaction.rs` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **transaction** | ✅ `transaction.rs` (atomic, rollback) | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **conditional create** | ✅ `handlers.rs:652` (If-None-Exist) | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **conditional update** | ✅ `handlers.rs:1481` | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **conditional delete** | ✅ `handlers.rs:1762` | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **capabilities** | ✅ `handlers.rs:203-452` (dynamic) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

### Search Parameters

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **Basic search params** | ✅ `octofhir-search/src/` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_include** | ✅ Search crate, parallel resolution | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_revinclude** | ✅ `reverse_chaining.rs` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Chained params** | ✅ Search SQL builder | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Reverse chaining (_has)** | ✅ `reverse_chaining.rs` | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ |
| **_sort** | ✅ Search crate | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_count** | ✅ Config: default 10, max 100 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_summary** | ✅ `handlers.rs:455-497` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_elements** | ✅ Search crate | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **_filter** | ✅ `filter.rs` | ✅ | ⚠️ | ❌ | ⚠️ | ⚠️ | ❌ | ✅ | ✅ |
| **Compartment search** | ✅ Search crate | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **Custom search params** | ✅ Via canonical manager | ✅ | ✅ | ⚠️ | ❌ | ❌ | ❌ | ✅ | ✅ |
| **_total** | ✅ Search crate | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |

### Operations

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **$validate** | ✅ `octofhir-fhirschema` crate | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **$everything (Patient)** | ✅ Handlers + async jobs | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **$export (Bulk Data)** | ✅ `operations/bulk/export.rs` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **$import (Bulk Data)** | ❌ Not implemented | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **$graphql** | ✅ `octofhir-graphql/` (dynamic) | ⚠️ Basic | ❌ | ⚠️ | ❌ | ❌ | ❌ | ⚠️ | ❌ |
| **$convert** | ❌ Not implemented | ⚠️ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |

---

## B. Terminology

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **CodeSystem storage** | ✅ Via canonical manager (JSONB) | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **ValueSet storage** | ✅ Via canonical manager | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **ConceptMap storage** | ✅ Via canonical manager | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| **$expand** | ✅ `operations/terminology/` | ✅ | ✅ | ⚠️ | ✅ | ✅ | ❌ | ✅ | ✅ |
| **$lookup** | ✅ `operations/terminology/` | ✅ | ✅ | ⚠️ | ✅ | ✅ | ❌ | ✅ | ✅ |
| **$translate** | ✅ `operations/terminology/` | ✅ | ⚠️ | ❌ | ⚠️ | ⚠️ | ❌ | ✅ | ✅ |
| **Terminology versioning** | ✅ Canonical manager versions | ✅ | ✅ | ⚠️ | ✅ | ✅ | ❌ | ✅ | ✅ |
| **SNOMED/LOINC native** | ⚠️ Loaded via packages | ✅ Native | ✅ | ⚠️ | ✅ | ✅ | ✅ NLP-mapped | ✅ | ✅ |
| **Search-time expansion** | ✅ `terminology.rs` expand_valueset_for_search | ✅ | ✅ | ⚠️ | ✅ | ✅ | ⚠️ | ✅ | ✅ |

---

## C. Security

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **SMART on FHIR (auth code)** | ✅ `octofhir-auth/` full OAuth 2.0 | ⚠️ Needs config | ✅ | ✅ Medplum Auth | ✅ | ✅ | ✅ SMART 2.0 | ✅ | ✅ Firely Auth |
| **SMART backend services** | ✅ Client credentials grant | ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **OAuth 2.0 / OIDC** | ✅ JWT RS256/RS384/ES384 | ⚠️ External | ✅ | ✅ | ✅ Entra ID | ✅ IAM | ✅ | ✅ | ✅ |
| **Scopes (resource/user)** | ✅ SMART scopes | ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Fine-grained ACL** | ✅ QuickJS policy engine | ⚠️ Interceptors | ✅ Tenant-level | ⚠️ Access policies | ✅ RBAC | ✅ IAM | ✅ | ✅ RBAC | ✅ |
| **AuditEvent auto-gen** | ✅ `hooks/audit.rs` AsyncAuditHook | ⚠️ Configurable | ✅ Kafka CADF | ✅ | ✅ | ✅ Cloud Logging | ✅ CloudTrail | ✅ | ✅ |
| **Rate limiting** | ✅ 60/min token, 30/min auth | ❌ External | ⚠️ | ❌ External | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| **External IdP federation** | ✅ JWKS cache, auto-refresh | ⚠️ | ✅ | ✅ | ✅ Entra ID | ✅ | ✅ Cognito | ✅ | ✅ |
| **Key rotation** | ✅ 90-day rotation, 3 old keys | ⚠️ | ⚠️ | ✅ | ✅ Managed | ✅ Managed | ✅ Managed | ✅ | ✅ |
| **TLS** | ⚠️ Reverse proxy required | ⚠️ | ⚠️ | ✅ Managed | ✅ Managed | ✅ Managed | ✅ Managed | ✅ | ✅ |

---

## D. Performance & Scale

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **Indexing strategy** | ✅ GIN (jsonb_path_ops) + B-tree `schema.rs:248-321` | ✅ JPA search indexes | ✅ Custom indexes | ✅ PostgreSQL | ✅ Managed | ✅ Managed | ✅ Managed | ✅ JPA | ✅ MongoDB/SQL |
| **Two-tier cache** | ✅ DashMap L1 + Redis L2 `cache/backend.rs` | ⚠️ In-memory only | ⚠️ | ⚠️ | ✅ Managed | ✅ Managed | ✅ Managed | ⚠️ | ⚠️ |
| **Cross-instance invalidation** | ✅ Redis pub/sub `cache/pubsub.rs` | ❌ Single-instance cache | ❌ | ⚠️ | ✅ Managed | ✅ Managed | ✅ Managed | ⚠️ | ⚠️ |
| **Connection pooling** | ✅ sqlx pool, 50 max `pool.rs` | ✅ HikariCP | ✅ Liberty pool | ✅ | ✅ Managed | ✅ Managed | ✅ Managed | ✅ | ✅ |
| **Multi-tenancy** | ⚠️ Feature flags + SMART context, no data isolation | ✅ Partitioning v5.0+ | ✅ Native, physical isolation | ⚠️ Limited | ✅ Workspace-level | ✅ Dataset-level | ✅ Native | ✅ Partitioning | ✅ Configurable |
| **Async jobs** | ✅ `async_jobs.rs` (Prefer: respond-async) | ✅ | ✅ | ⚠️ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Reindexing** | ⚠️ No zero-downtime reindex | ✅ $reindex operation | ✅ | ⚠️ | ✅ Managed | ✅ Managed | ✅ Managed | ✅ | ✅ |
| **Read replicas** | ❌ Not implemented | ⚠️ External | ⚠️ | ✅ Aurora replicas | ✅ Managed | ✅ Managed | ✅ Managed | ⚠️ | ⚠️ |
| **Horizontal scaling** | ⚠️ Redis cache sync only | ⚠️ Limited | ⚠️ | ✅ ECS Fargate | ✅ Managed | ✅ Managed | ✅ Managed | ✅ | ⚠️ |
| **Raw JSON optimization** | ✅ `queries/crud.rs` read_raw() skips deser | ❌ | ❌ | ❌ | N/A | N/A | N/A | ❌ | ❌ |
| **Memory allocator** | ✅ mimalloc `main.rs:6-7` | ❌ JVM GC | ❌ JVM GC | ❌ V8 | N/A | N/A | N/A | ❌ JVM GC | ❌ .NET GC |

---

## E. Operations & Developer Experience

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **Prometheus metrics** | ✅ `metrics.rs` HTTP/DB/cache/FHIR | ✅ | ⚠️ | ⚠️ | ✅ Azure Monitor | ✅ Cloud Monitoring | ✅ CloudWatch | ✅ | ⚠️ |
| **Structured logging** | ✅ `observability.rs` tracing + WS | ✅ SLF4J | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Distributed tracing** | ✅ OpenTelemetry OTLP | ⚠️ External | ⚠️ | ⚠️ | ✅ App Insights | ✅ Cloud Trace | ✅ X-Ray | ⚠️ | ⚠️ |
| **Admin UI** | ✅ React 19 + 15 consoles `ui/` | ⚠️ Basic overlays | ❌ | ✅ Medplum App | ✅ Azure Portal | ✅ Console | ✅ Console | ✅ Admin | ⚠️ |
| **SQL console** | ✅ Monaco + LSP `DbConsolePage.tsx` | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Athena | ❌ | ❌ |
| **GraphQL playground** | ✅ GraphiQL `GraphQLConsolePage.tsx` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **FHIRPath console** | ✅ `FhirPathConsolePage.tsx` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **CQL console** | ✅ `CqlConsole.tsx` | ⚠️ Plugin | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| **CLI tool** | ✅ `octofhir-cli/` multi-profile | ❌ | ❌ | ✅ medplum CLI | ✅ az cli | ✅ gcloud | ✅ aws cli | ❌ | ❌ |
| **Helm chart** | ❌ Not available | ✅ Community | ✅ Official | ⚠️ CDK | N/A Managed | N/A Managed | N/A Managed | ✅ Official | ✅ Official |
| **Config hot reload** | ✅ File watcher + pg_notify `octofhir-config/` | ❌ | ❌ | ❌ | N/A | N/A | N/A | ❌ | ⚠️ |
| **Docs quality** | ⚠️ Early-stage, deployment docs | ✅ Extensive | ✅ Comprehensive | ✅ Modern, dev-friendly | ✅ Microsoft Learn | ✅ Google Cloud | ✅ AWS docs | ✅ | ✅ |
| **Live log streaming** | ✅ WebSocket `LogsViewerPage.tsx` | ❌ | ❌ | ❌ | ✅ Azure Monitor | ✅ Cloud Logging | ✅ CloudWatch | ⚠️ | ❌ |
| **Conformance tests** | ❌ No published results | ✅ Touchstone | ✅ | ✅ Inferno | ✅ | ✅ | ✅ Inferno/g10 | ✅ | ✅ g10 certified |

---

## F. Extensibility

| Feature | OctoFHIR | HAPI FHIR | IBM FHIR | Medplum | Azure FHIR | Google FHIR | AWS HealthLake | Smile CDR | Firely |
|---------|----------|-----------|----------|---------|------------|-------------|----------------|-----------|--------|
| **Interceptors/hooks** | ✅ `hooks/audit.rs` hook system | ✅ Extensive interceptors | ✅ | ✅ Subscriptions→Bots | ⚠️ Logic Apps | ⚠️ Functions | ⚠️ Lambda | ✅ | ✅ Plugins |
| **Scripting engine** | ✅ QuickJS (policies) + JS automations | ❌ Java only | ❌ | ✅ TypeScript Bots | ⚠️ Azure Functions | ⚠️ Cloud Functions | ⚠️ Lambda | ⚠️ | ❌ |
| **Custom operations** | ✅ `gateway/handler.rs` HandlerRegistry | ✅ @Operation | ✅ | ✅ Via Bots | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ |
| **Custom resources** | ✅ Dynamic schema from IGs | ⚠️ | ⚠️ | ✅ | ❌ | ❌ | ❌ | ⚠️ | ⚠️ |
| **Plugin/Extension SDK** | ❌ Not available | ✅ Mature | ⚠️ | ✅ SDK packages | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ |
| **SQL on FHIR** | ✅ `octofhir-sof/` ViewDefinition | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Apache Iceberg | ❌ | ❌ |
| **CQL support** | ✅ `octofhir-cql-service` crate | ⚠️ Plugin | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ |
| **FHIRPath evaluation** | ✅ `octofhir-fhirpath` crate | ✅ Built-in | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ |
| **Subscriptions** | ✅ R5 topic-based, rest-hook/WS/email `subscriptions/` | ✅ REST Hook/WS/email | ⚠️ WebSocket only | ✅ Bots-based | ✅ Pub/Sub | ✅ Pub/Sub | ⚠️ | ✅ | ✅ |
| **Automations** | ✅ JS workflows, cron/event triggers | ❌ | ❌ | ✅ Bots | ⚠️ Logic Apps | ⚠️ Workflows | ⚠️ Step Functions | ⚠️ | ❌ |
| **Data transformations** | ⚠️ FHIRPath only | ⚠️ | ⚠️ | ⚠️ | ❌ | ❌ | ✅ NLP | ⚠️ | ⚠️ |
| **XML format** | ❌ JSON only | ✅ JSON + XML + RDF | ✅ JSON + XML | ✅ JSON + XML | ✅ | ✅ | ⚠️ | ✅ Auto-convert | ✅ |
| **Notification service** | ✅ `octofhir-notifications/` email/SMTP | ⚠️ | ⚠️ | ✅ | ⚠️ | ⚠️ | ⚠️ | ✅ | ⚠️ |
