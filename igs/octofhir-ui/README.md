# OctoFHIR UI IG

Generic admin-UI resources, reusable across the whole web console (not feature-specific).

## Resources

- **UserPreference** - Per-user preference keyed by `(user, namespace, key)`, value JSON-encoded.
- **Dashboard** - Metrics dashboard of the Performance page: PromQL panels on a 24-column grid
  (Grafana-compatible). Only header fields are modelled; panels and variables are open JSON, so
  writes send `X-Skip-Validation: true`. Provision one with `PUT /fhir/Dashboard/{id}`.

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
