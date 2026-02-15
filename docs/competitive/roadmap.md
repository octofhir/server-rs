# Improvement Roadmap

**Team**: 3-5 developers, 3 parallel workstreams (Track A, B, C).

**Effort scale**: S (1-2 weeks), M (2-4 weeks), L (4-8 weeks), XL (8-16+ weeks).

---

## P0 — Production Readiness (Weeks 1-6)

Goal: Establish credibility and unblock early adopters.

| Epic | Stories | Track | Effort | Acceptance Criteria |
|------|---------|-------|--------|---------------------|
| **Conformance Testing** | 1. Set up Inferno R4 test suite in CI<br>2. Run initial test pass, catalog failures<br>3. Fix conformance failures (search, capabilities, error responses)<br>4. Publish pass rate in README and docs | A | L | Pass rate >90% on Inferno R4 core tests |
| **Integration Tests** | 1. Add testcontainers-rs for PostgreSQL<br>2. Write integration tests: CRUD, search, auth, transactions, subscriptions<br>3. Add to CI pipeline with coverage reporting | A | L | >80% handler code coverage; all tests pass in CI |
| **Helm Chart** | 1. Create Helm chart with `values.yaml`<br>2. Add health/readiness probes (`/healthz`, `/readyz`)<br>3. Configure HPA and PDB<br>4. Add ConfigMap for `octofhir.toml`<br>5. Document PostgreSQL + Redis dependencies | B | M | `helm install octofhir ./charts/octofhir` works on k8s 1.28+ |
| **Security Audit** | 1. Document TLS termination (nginx/Traefik examples)<br>2. Review secrets management (env vars, sealed secrets)<br>3. Audit RBAC and policy engine defaults<br>4. Review rate limiting configuration<br>5. Document security posture | B | M | Security guide published; no critical findings in OWASP top 10 review |
| **Documentation** | 1. Getting started guide (5-minute quickstart)<br>2. Deployment guide (Docker, Docker Compose, k8s)<br>3. Configuration reference (all `octofhir.toml` options)<br>4. API examples (CRUD, search, auth, bulk export)<br>5. Architecture overview with diagrams | C | M | Docs cover all public endpoints; getting started works on fresh machine |
| **Performance Baseline** | 1. Implement benchmark plan from `benchmarks/plan.md`<br>2. Run OctoFHIR vs HAPI FHIR<br>3. Publish p50/p95/p99 results with methodology<br>4. Add benchmark scripts to repo | C | M | Published benchmark results with reproducible methodology |

### P0 Dependencies
```
Conformance Testing ──┐
Integration Tests ────┤──→ Production Readiness (unblocks P1)
Helm Chart ───────────┤
Security Audit ───────┤
Documentation ────────┘
```

---

## P1 — Competitive Parity (Weeks 7-16)

Goal: Close critical gaps that block enterprise evaluation.

| Epic | Stories | Track | Effort | Acceptance Criteria |
|------|---------|-------|--------|---------------------|
| **Multi-tenancy** | 1. Design tenant isolation model (schema-level vs row-level)<br>2. Add `tenant_id` to resource tables<br>3. Implement tenant-aware routing middleware<br>4. Add tenant management admin API<br>5. Update search to scope by tenant<br>6. Add integration tests | A | XL | Tenants cannot access each other's data; admin can manage tenants via API |
| **Bulk Import ($import)** | 1. NDJSON file parsing and streaming<br>2. Async job integration for progress tracking<br>3. Error reporting per resource<br>4. Resumable imports (track position in file)<br>5. Add k6 load test for import | B | L | Import 100K resources via NDJSON; progress visible via async status endpoint |
| **$convert Operation** | 1. Implement R4→R5 resource structure mapping<br>2. Implement R5→R4 resource structure mapping<br>3. Handle extension conversions<br>4. Add tests for common resource types | C | M | Convert Patient, Observation, Encounter between R4 and R5 |
| **Reindexing Strategy** | 1. Add `$reindex` operation endpoint<br>2. Background job for GIN index rebuild<br>3. Zero-downtime (CONCURRENTLY option)<br>4. Progress reporting via async jobs | B | L | Reindex 100K resources without downtime; progress visible via admin API |
| **XML Support** | 1. Add XML serialization for FHIR resources<br>2. Content negotiation (`Accept: application/fhir+xml`)<br>3. Read-only initially (XML responses for JSON-stored data)<br>4. Add XML input parsing (Phase 2) | C | M | `Accept: application/fhir+xml` returns valid FHIR XML |

### P1 Dependencies
```
P0 (Production Readiness) ──→ Multi-tenancy (needs stable test infrastructure)
P0 (Helm Chart) ──→ Multi-tenancy (tenant routing needs k8s-aware config)
```

---

## P2 — Differentiation (Months 5-12)

Goal: Build competitive moat and unlock new markets.

| Epic | Stories | Track | Effort | Acceptance Criteria |
|------|---------|-------|--------|---------------------|
| **ONC g10 Certification** | 1. Run Inferno g10 test suite<br>2. Fix all g10 failures (SMART 2.0, US Core IG, Bulk Data)<br>3. Implement required US Core profiles<br>4. Prepare certification documentation<br>5. Submit for ONC certification | A | XL | Pass Inferno g10 test suite; certification submitted |
| **Plugin/Extension SDK** | 1. Define plugin API (custom operations, interceptors, storage backends)<br>2. Implement plugin loading (dynamic libraries or WASM)<br>3. Create plugin template/starter<br>4. Document plugin development guide<br>5. Build 2-3 example plugins | A | XL | Third-party developers can create and load custom plugins |
| **Read Replicas / Sharding** | 1. Add read replica connection pool<br>2. Implement query routing (writes → primary, reads → replica)<br>3. Add configuration for replica endpoints<br>4. Test replication lag handling | B | XL | Read queries routed to replicas; write queries to primary; tested with pg replication |
| **Advanced Terminology** | 1. Pre-indexed SNOMED CT import<br>2. Pre-indexed LOINC import<br>3. Optimized $expand for large code systems (100K+ concepts)<br>4. Hierarchical subsumption queries | B | L | $expand on SNOMED value set completes in <1s for 1000 concepts |
| **Subscription Delivery Retry/DLQ** | 1. Configurable retry policies (exponential backoff, max retries)<br>2. Dead letter queue for permanently failed deliveries<br>3. Admin UI for DLQ inspection and replay<br>4. Metrics for delivery success/failure rates | C | M | Failed subscriptions retry with backoff; DLQ viewable in admin UI |
| **Multi-version Endpoints** | 1. Route `/fhir/r4/*` and `/fhir/r5/*` to different FHIR versions<br>2. Version-specific CapabilityStatement per endpoint<br>3. Version-specific search parameters<br>4. Shared storage with version-aware serialization | C | L | Clients can query R4 and R5 endpoints simultaneously |

---

## Timeline Summary

```
Week 1-6:   P0 — Production Readiness
            ├── Track A: Conformance Testing + Integration Tests
            ├── Track B: Helm Chart + Security Audit
            └── Track C: Documentation + Performance Baseline

Week 7-16:  P1 — Competitive Parity
            ├── Track A: Multi-tenancy (XL)
            ├── Track B: Bulk Import + Reindexing
            └── Track C: $convert + XML Support

Month 5-12: P2 — Differentiation
            ├── Track A: ONC g10 Certification + Plugin SDK
            ├── Track B: Read Replicas + Advanced Terminology
            └── Track C: Subscription DLQ + Multi-version Endpoints
```

## Success Metrics

| Milestone | Metric | Target | Timeline |
|-----------|--------|--------|----------|
| Production-ready | Inferno R4 pass rate | >90% | Week 6 |
| Production-ready | Integration test coverage | >80% handlers | Week 6 |
| Production-ready | Helm chart available | `helm install` works | Week 4 |
| Competitive parity | Multi-tenant isolation | Data isolation verified | Week 16 |
| Competitive parity | Bulk import throughput | 10K resources/min | Week 12 |
| Differentiation | ONC g10 certification | Submitted | Month 9 |
| Differentiation | Read replica latency | <5ms reads from replica | Month 8 |

## Assumptions

- Team of 3-5 developers with Rust and FHIR domain knowledge
- PostgreSQL 16 as primary database (no plan to add other backends)
- Redis available for caching and pub/sub (horizontal scaling)
- CI/CD pipeline with GitHub Actions (existing)
- Access to Inferno test suite (open source, free)
- Aidbox trial license available for benchmark comparison
