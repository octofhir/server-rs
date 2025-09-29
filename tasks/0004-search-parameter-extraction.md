# Task 0004 — Search Parameter Extraction

## Objective
Kick off Stage B Milestone 1 from [Roadmap v1](../docs/ROADMAP.md) by extracting search parameters from Canonical Manager packages and the [FHIR Search Parameter Registry](https://hl7.org/fhir/searchparameter-registry.html) to drive automated index planning.

## Context & Rationale
- **Roadmap Alignment**: Provides metadata for search handler implementation and CapabilityStatement exposure.
- **Gap Closure**: Advances `Partial` status for search metadata in the [GAP analysis](../docs/roadmap-prep/gap-analysis.md) toward completion.
- **Dependencies**: Relies on Tasks 0001-0003 for schema introspection and validation signals; references ADR-003 (Index Strategy) drafts.

## Deliverables
1. Metadata extractor that composes R5 StructureDefinition and SearchParameter resources into canonical search descriptors.
2. Persistence layer storing parameter definitions with versioning for hot-reload compatibility.
3. Reporting that highlights unsupported modifiers/chains for prioritization.
4. Documentation describing mapping between Canonical Manager data and [FHIR Search](https://build.fhir.org/search.html) semantics.

## Acceptance Criteria (DoD)
- Coverage includes all core resource types targeted in Stage A (e.g., Patient, Observation) plus capability to extend dynamically.
- Output feeds Stage B indexing tasks and populates CapabilityStatement `rest.resource.searchParam` entries.
- Hot-reload pipeline refreshes descriptors when Canonical Manager publishes new packages.

## Implementation Outline
- Parse canonical packages to collect relevant SearchParameter definitions, fallback to registry lookup for gaps.
- Normalize expressions (FHIRPath, composite definitions) into an internal model aligned with ADR-003.
- Store descriptors in PostgreSQL with audit metadata for later comparison and UI surfacing.

## Risks & Mitigations
- **Expression complexity**: Validate FHIRPath expressions via linting and flag unsupported constructs with OperationOutcome diagnostics.
- **Registry drift**: Cache retrieval timestamps and add CI guard to detect upstream changes.
- **Reload storms**: Rate-limit descriptor updates and bundle with configuration snapshots (Stage H).

## References
- [FHIR Search](https://build.fhir.org/search.html)
- [Search Parameter Registry](https://hl7.org/fhir/searchparameter-registry.html)
- [FHIR StructureDefinition](https://build.fhir.org/structuredefinition.html)
- [FHIR CapabilityStatement](https://build.fhir.org/capabilitystatement.html)
