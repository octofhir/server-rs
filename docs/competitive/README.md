# OctoFHIR (Abyxon) Competitive Analysis

## Purpose

This directory contains a comprehensive competitive analysis of OctoFHIR against 9 major FHIR server implementations. The analysis provides evidence-based competitive positioning, gap identification, and a prioritized improvement roadmap for a 3-5 developer team.

## How to Read Each File

| File | Purpose | Audience |
|------|---------|----------|
| [sources.md](sources.md) | ~50 annotated URLs organized by competitor | Anyone verifying claims |
| [feature-matrix.md](feature-matrix.md) | 80-row comparison table across 6 axes | Product & engineering leads |
| [feature-matrix.csv](feature-matrix.csv) | Same data in CSV for spreadsheet import | Stakeholders, presentations |
| [swot-and-gaps.md](swot-and-gaps.md) | SWOT analysis with code evidence + gap analysis | Strategy & planning |
| [benchmarks/plan.md](benchmarks/plan.md) | Reproducible benchmark methodology | Performance engineering |
| [roadmap.md](roadmap.md) | Prioritized improvement roadmap (P0/P1/P2) | Engineering management |
| [adr/ADR-0001-conformance-testing-strategy.md](adr/ADR-0001-conformance-testing-strategy.md) | Architecture decision: conformance testing | Engineering team |

## Methodology

- **OctoFHIR features**: Reconstructed from code analysis (no server runtime needed). Evidence links reference `file:line` locations in the codebase.
- **Competitor features**: Sourced from official documentation, GitHub repositories, capability statements, and pricing pages. All sources listed in [sources.md](sources.md).
- **CapabilityStatement**: Reconstructed from `handlers.rs:203-452` code analysis.

## Competitors Analyzed

| Category | Competitors |
|----------|------------|
| Open Source | HAPI FHIR (Java), IBM FHIR / LinuxForHealth (Java), Medplum (TypeScript) |
| Commercial / Open Core | Aidbox (PostgreSQL), Smile CDR (Java), Firely Server (.NET) |
| Managed Cloud | Azure Health Data Services, Google Cloud Healthcare API, AWS HealthLake |

## Date of Analysis

**February 2026**

## Caveats

- Competitor features are based on publicly available documentation. Internal/unreleased features may exist.
- OctoFHIR features are based on code analysis of the current `main` branch. Some features may be partially implemented or behind feature flags.
- Pricing for commercial products may vary. Contact vendors for current quotes.
- Performance claims are based on architectural analysis, not benchmarks (see [benchmarks/plan.md](benchmarks/plan.md) for planned benchmarks).
- "Production readiness" assessments reflect documentation claims and community feedback, not independent audits.
