# OctoFHIR UI IG

Generic admin-UI resources, reusable across the whole web console (not feature-specific).

## Resources

- **UserPreference** - Per-user preference keyed by `(user, namespace, key)`, value JSON-encoded.

## Routes

Served WITHOUT the `/fhir` prefix (gateway fallback):

- `GET /UserPreference?user=...&namespace=console`
- `PUT /UserPreference/{id}`

## Namespaces (convention)

- `console` — REST console general prefs (default _count, default view, etc.)
- `console.keys` — rebindable keymap (vim/default)
- `console.layout` — panel split ratios / open panels
- `ui` — cross-feature prefs (theme)

## Storage

Custom `kind:"logical"` resource. Table auto-created by `SchemaManager` on IG load — no
migrations.
