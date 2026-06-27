# OctoFHIR Console IG

Resources backing the interactive REST console (query builder, history, variables).

## Resources

- **ConsoleCollection** - Named folder of saved console requests (shareable)
- **ConsoleSavedRequest** - A saved request (method/path/headers/body + chaining extractions)
- **ConsoleEnvironment** - Named variable set with initial/current value duality
- **ConsoleHistoryEntry** - One executed request/response, per-user, for cross-browser history

## Routes

All resources in this IG are accessible WITHOUT the `/fhir` prefix (gateway fallback):

- `GET /ConsoleCollection`
- `GET /ConsoleSavedRequest`
- `GET /ConsoleEnvironment`
- `GET /ConsoleHistoryEntry?user=...&executedAt=ge2026-01-01&status=200`

## Storage

Custom `kind:"logical"` resources. Tables, history tables, triggers and GIN indexes are
auto-created by `SchemaManager` on IG load — no SQL migrations. CRUD/search/history come
free over the FHIR REST API.

## Notes

- History is high write-volume. If FHIR-resource write overhead becomes a bottleneck under
  load, `ConsoleHistoryEntry` can be swapped for a purpose-built append-only table behind the
  same `/api/console/history` facade without changing the frontend contract.
- Secret environment variables (`ConsoleEnvironment.variable.secret = true`) hold
  client-side-encrypted ciphertext only; OAuth tokens are never persisted here.
