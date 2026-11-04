# Dependency boundaries

The server explicitly assembles the integrations it needs. The internal crates
also provide smaller profiles so parsing, SQL generation, storage contracts and
GraphQL execution can be checked without the full server graph.

| Crate | Minimal profile | Integration features |
| --- | --- | --- |
| `octofhir-core` | Types, errors and utilities; no async event runtime | `events` |
| `octofhir-storage` | Storage contracts, DTOs and metrics | `events` for evented wrappers |
| `octofhir-search` | SearchParameter parsing, query parsing, IR, SQL, caller-provided terminology | `canonical`, `remote-terminology`, `watch` |
| `octofhir-graphql` | Schema, execution, resolvers and required `Authorizer`; use `default-features = false` | `server-auth` for policy evaluation; `http` for server Axum handlers |

Core, storage and search default to their minimal profiles. The server and
Postgres backend request integration features explicitly. GraphQL retains the
server HTTP/auth integrations by default; its HTTP feature currently includes
the server auth adapter. This does not provide a generic HTTP authentication
framework for other hosts.

`HierarchyProvider` supplies ancestor/descendant traversal independently of the
hybrid HTTP terminology provider. Existing remote hierarchy semantics, FHIR
escaping and expansion limits are preserved. ValueSet expansion continues to
use the transport-independent FHIR model terminology interface.

GraphQL contexts require an `Arc<dyn Authorizer>`. The optional server adapter
keeps authenticated identity, scopes, resource bodies, request/environment
metadata, policy decisions and FHIR error details. `Abstain` remains denied.
A custom authorizer and server policy evaluator cannot both be selected.
`GraphQLIdentity` is informational and never grants access.

The internal GraphQL operation enum is independent of the server policy engine;
the adapter exhaustively maps every operation. The existing convenience builder
methods remain available with `server-auth`. Direct internal accesses to
`GraphQLContext.policy_evaluator` and `.auth_context` migrate to `.authorizer`
and `.identity`; raw authentication context stays inside the server adapter.

Run `bash scripts/check-library-deps.sh` to execute independent consumers and
reject accidental HTTP/TLS/database dependencies in their normal/build graphs.
The core/storage profiles additionally reject the event runtime, and GraphQL
rejects the server auth engine and canonical/file-watching adapters. CI also
runs tests and strict Clippy independently with default features disabled to
catch feature-unification mistakes.
