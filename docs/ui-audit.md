# OctoFHIR Web Console — UI Audit (Phase 0)

> Read-only audit, 2026-06-21. 22 parallel agents (18 pages + 4 cross-cutting:
> ui-kit inventory, tokens, legacy/Mantine, a11y). Stack: React 19 + Vite +
> `@gravity-ui/uikit` v7, design kit in `@octofhir/ui-kit`. Mantine is **gone
> from all `package.json` deps and from every `.tsx`** — no `createTheme` /
> `MantineProvider` / `--app-*` tokens exist in code. Remaining "Mantine" is
> (a) stale docs and (b) a deprecated compat *prop* layer.

## TL;DR — the one root cause

`@octofhir/ui-kit` ships an **incomplete Mantine-compat shim layer**:

- **Real shims** (map Mantine props → Gravity): `Button`, `ActionIcon` (partial),
  `TextInput`, `Badge`.
- **Raw Gravity re-exports** (NO shim — Mantine props silently dropped or crash):
  `Text`, `Tooltip`, `Alert`, `Switch`, `Modal`, `Menu`, `Progress`, `ThemeIcon`
  (partial), `NumberInput`, `PasswordInput`, `TextArea`, `CopyButton`, `Spin`/`Loader`.
- **Semantically-wrong aliases** (render the *wrong widget*): `RingProgress`→`Spin`,
  `LoadingOverlay`→`Spin`, `ScrollArea`→`Box` (no scroll), `Stack`/`Center`→plain
  `Flex` (no direction/centering), `DateTimePicker`→date-only `DatePicker`,
  `JsonInput`→plain `TextArea`.

Pages were written assuming the full Mantine API exists everywhere. Result: 26 P0
findings — runtime crashes, components that never render, and ~45 TS errors on the
operations + logs pages **masked by `tsconfig` excludes** (`pnpm typecheck` is green
but those files don't compile).

## Scorecard (per page)

| Page | loading | empty | error | header | resp. | tokens | a11y | theme |
|------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| apps | ◐ | ◐ | ◐ | ✓ | ◐ | ◐ | ◐ | ◐ |
| audit | ◐ | ◐ | ✗ | ✓ | ◐ | ◐ | ◐ | ◐ |
| auth | ◐ | ◐ | ✗ | ✓ | ◐ | ◐ | ◐ | ◐ |
| automations | ◐ | ◐ | ◐ | ✓ | ✓ | ✓ | ◐ | ✓ |
| console (REST) | — | ◐ | — | ✓ | ✓ | ◐ | ✗ | ✓ |
| dashboard | ◐ | ◐ | ✗ | ✓ | ✓ | ✓ | ◐ | ✓ |
| db-console | ◐ | ◐ | ◐ | n/a | ◐ | ◐ | ◐ | ◐ |
| fhirpath-console | ◐ | ◐ | ◐ | ✓ | ◐ | ◐ | ✗ | ✓ |
| graphql-console | ◐ | ◐ | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ |
| login | ✓ | n/a | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ |
| logs | ◐ | ◐ | ◐ | ◐ | ✓ | ◐ | ◐ | ◐ |
| metadata | ◐ | ✗ | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ |
| operations | ◐ | ◐ | ◐ | ✓ | ✓ | ✓ | ✗ | ◐ |
| packages | ◐ | ◐ | ◐ | ✓ | ◐ | ◐ | ◐ | ◐ |
| resource-browser | ◐ | ◐ | ✗ | ✓ | ◐ | ◐ | ◐ | ✓ |
| sessions | ◐ | ◐ | ✗ | ✓ | ◐ | ✓ | ◐ | ✓ |
| settings | ◐ | ◐ | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ |
| viewdefinition | ◐ | ◐ | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ |

✓ ok · ◐ partial · ✗ missing/broken · n/a

## P0 — production blockers (26)

### A. Broken / crashing components (shim mismatch)
- **CommandPalette never opens** — `Modal opened=` but Gravity wants `open=`.
  `console/components/CommandPalette/CommandPalette.tsx:134`. Cmd+K does nothing.
- **Compound `Menu.Target/Dropdown/Item` is undefined** → "Element type is invalid"
  crash. `console/components/HistoryPanel.tsx:172` (per-entry delete),
  `viewdefinition/ui/components/ColumnBuilder.tsx:102` (Columns tab + Add Collection).
- **`ScrollArea.Autosize` is undefined** → audit detail Changes/Raw tabs crash.
  `audit/ui/AuditEventDetail.tsx:184,201,224,399`.
- **Audit donut chart is a spinner** — `RingProgress`→`Spin` alias.
  `audit/ui/AuditAnalytics.tsx:107`. Plus `Progress` (raw Gravity) ignores
  `color/size/radius` so all analytics bars render uncolored (`:182`).
- **`ThemeIcon` numeric size + out-of-union color** → collapsed badge boxes, lost
  color coding. `audit/ui/AuditEventList.tsx:142,197`, `logs/LogStream.tsx:58`.

### B. Files that don't compile (tsconfig-excluded → ships broken)
- **operations** (`OperationsPage.tsx`, `OperationDetailPage.tsx`) — ~45 TS errors:
  `Text` (`c/fw/size/ta`), `Alert` (`icon/color/variant/children`), `Switch`
  (`label/onChange`), `TextArea` (`minRows/autosize`), `Anchor`/Link missing `href`
  (non-navigable), `SectionPanel` missing required `title`, `SegmentedControl`
  `onChange` vs `onUpdate`. Public-Access toggle renders unlabeled.
- **logs** (`LogEntry.tsx`, `LogStream.tsx`, `LogFilters.tsx`, `LogsViewerPage.tsx`)
  — `Text` Mantine props, `Tooltip label/position/withArrow`, `CopyButton`
  render-prop API, `ThemeIcon` color/size, Gravity icons `size=` (want `width/height`),
  `Alert icon/theme` shape.

### C. Data / logic blockers
- **Sessions targets a fake user** — `userId` hardcoded `'current-user-id'` FIXME.
  `sessions/ui/SessionsPage.tsx:43`. List + Revoke-All operate on bogus subject.
- **No error state anywhere data is fetched** — resource-browser, sessions, audit,
  auth, dashboard read only `{data,isLoading}`; failures render as "no records".

### D. Keyboard-unusable controls (a11y P0)
- Expand/collapse + row-select built as click-only `<div>` wrapping a decorative
  `ActionIcon` with no `onClick`: `automations/.../PlaygroundPanel.tsx:229,252`,
  `logs/LogEntry.tsx:73`, `fhirpath-console/.../ResultItem.tsx:29`,
  `db-console/components/HistoryTab.tsx:30`.
- Empty `<ActionIcon size={12}/>` used as decoration injects a nameless focusable
  button into every audit row. `audit/ui/AuditEventList.tsx:202`.

## P1 — important (themes that repeat across pages)

1. **Loading = spinner, not skeleton.** Nearly every page uses a centered
   `Loader`/`Spin` (or plain "Loading…" text) instead of `Skeleton`. Causes layout
   shift; `EmptyState`/`Skeleton`/`DataTable` exist in the kit but are unused in `ui/src`.
2. **Empty states are hand-rolled** `Text`/`div`, no illustration, no CTA. The kit's
   `EmptyState` is **never imported** by the app.
3. **No retry on errors** — even where an error branch exists it's bare `Text`/`Alert`
   with no refetch button.
4. **Native `window.confirm()`** for destructive delete (not themed, not focus-trapped):
   apps, auth (AccessPolicies, IdentityProviders), viewdefinition.
5. **Icon-only controls with no `aria-label`** — 68 `ActionIcon` instances; 41 rely on
   Tooltip only (not an accessible name), 27 fully bare. Search inputs use placeholder
   only. Row-action `Menu.Target` buttons unlabeled.
6. **Raw semantic colors in CSS** — `rgba(...)` red/green/amber for diff + log-level
   tints (audit, logs) bypass tokens and break in dark mode.
7. **Mid-migration prop drops** — `Text size/c/fw`, `Tooltip label`, `Alert color`,
   `Badge variant/gradient`, `ActionIcon color/variant`, `size="sm"` (not a Gravity
   size) silently no-op across console, viewdefinition, resource-browser, apps.

## P2 — polish & hygiene

- Hardcoded `px` radius/spacing/font-size that map 1:1 to tokens
  (`8px=--octo-radius-md`, `6px=sm`, `12px=lg`, `16px=xl`; `12/14/16px` font →
  `--octo-typography-size-xs/sm/md`) across ~25 page CSS modules. `4px/2px/11px/9px/13px`
  have no exact token — need a stop or nearest.
- Mixing `--g-*` Gravity vars with `--octo-*` surface scale on the same page.
- Hand-rolled `Metric`/tiles duplicating `MetricTile`; `Badge` instead of `StatusBadge`.
- Per-row shared mutation state (one Deploy spinner lights all rows) — automations.
- Hardcoded `box-shadow`, `cubic-bezier`, `100vh` in embedded layouts.

## UI-kit inventory (60 components in `shared/ui`)

- **Story coverage**: 52/60 have a story. Missing: `AppShell`, `Burger`, `Collapse`,
  `DataTable`, `EmptyState`, `Form`, `FormRow`, `PageLayout` (EmptyState/PageLayout/Form
  are user-facing — must add).
- **Dead in `ui/src`** (15 + 3 zero-use aliases): `Accordion`, `AppLayout`, `AppShell`,
  `Burger`, `CommandCard`, `Container`, `DataTable`, `EmptyState`, `FormRow`, `Grid`,
  `MetricTile`, `Portal`, `StatGrid`, `StatusBadge`, `Surface`; aliases `LoadingOverlay`,
  `NavLink`, `SimpleGrid`. (EmptyState/MetricTile/StatusBadge are dead because pages
  *should* use them but hand-roll instead — wire them, don't delete.)
- **Duplicates / overlapping**:
  - `Button` = `ActionIcon` = `UnstyledButton` (one impl, 3 names).
  - `Select`/`MultiSelect`, `Card`/`Paper`, `Hotkey`/`Kbd`, `Grid`/`SimpleGrid`,
    `TextArea`/`Textarea`/`JsonInput`, `Flex`/`Stack`/`Group`/`Center` — alias chains.
  - `Container` vs `PageLayout.PageContainer`; `Table` (custom) vs `DataTable` (TanStack)
    — two competing systems, one unused.
  - Input family inconsistent: only `TextInput` shims `leftSection/rightSection`;
    `NumberInput`/`PasswordInput`/`TextArea` drop them silently.
- **Broken kit components**: `Collapse` ignores `transitionDuration` (animates nothing);
  `Stack`/`Center` don't stack/center; `RingProgress`/`LoadingOverlay`/`ScrollArea`
  render wrong widget.

## Legacy / docs

- `CLAUDE.md` "Design System Rules" (L215-230) + L125 still describe **Mantine v8,
  `createTheme`, `MantineProvider`, `themeCssVars.tsx`, `--app-glass-*`/`--app-surface-*`/
  `--app-border-subtle`** — none exist. Actively misdirects contributors.
- OAuth server-rendered pages (`crates/octofhir-auth/src/http/authorize_templates.rs`)
  + `docs/.../authentication.mdx` describe the retired "glassmorphism" language; they
  use their own local `--glass-*`/`--surface-*` (valid, but drifted from ui-kit tokens).
- Deprecated "Mantine compatibility" prop comments remain on `Button`, `TextInput`,
  `Hotkey` — intentional bridges, remove after call-site migration.

## Token reference (for fixes)

`--octo-radius-{xs=3,sm=6,md=8,lg=12,xl=16}px` · `--octo-spacing-{xs..xl}` ·
`--octo-typography-size-{xs=.75rem,sm=.875rem,md=1rem}` · `--octo-shadow-{xs..xl}` ·
`--octo-typography-mono` · `--octo-motion-ease-out`. Gravity aliases: `--g-border-radius-{s/m/l/xl}`, `--g-spacing-*`.
