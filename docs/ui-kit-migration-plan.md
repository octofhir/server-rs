# Gradual Migration Plan — Gravity → Base UI (headless + own CSS)

> Companion to [ui-kit-research.md](./ui-kit-research.md). Strategy: **strangler pattern via
> the `@octofhir/ui-kit` facade**. The facade's public API stays stable; we re-implement each
> component on Base UI + CSS Modules underneath. Both kits coexist until the last component
> flips. App pages change little. **No Tailwind at any step.**

## Why this is safe

`ui/src` imports everything from `@octofhir/ui-kit` (≈60 components, one barrel). That barrel
is the seam. We swap implementations behind unchanged names/props, so the blast radius per step
is one component, not the whole app. We can ship continuously; the app stays green the whole way.

## Ground rules

- Both `@gravity-ui/*` and `@base-ui/react` installed during migration; remove Gravity only
  after the last consumer is gone (grep-gated).
- Keep the public prop API of each facade component **stable** when flipping its impl. If an API
  must change, migrate call sites in the same step.
- Each flipped component: Storybook story + visual check in light/dark + `typecheck`+`lint` green.
- New visual language lives in CSS Modules + `--octo-*` tokens. Define the brand first (Stage 0)
  so components are built to it, not retrofitted.

## Stage 0 — Foundation & design language (no app changes)

1. Add deps: `@base-ui/react`, `@tanstack/react-table` (raw), date lib (`react-aria` DatePicker
   or `@internationalized/date`). Add `react-resizable-panels` etc. to `ui/package.json` (the
   transitive-dep gap that already bit us).
2. **Define the brand** in `shared/theme`: finalize `--octo-*` tokens (color scales, surfaces,
   radius, spacing, typography, shadow, motion, focus ring) for light + dark. This is where the
   "recognizable style" is decided — do it deliberately (mockups for shell + sidebar + a data page).
3. Set up Base UI's portal/root + theming wiring; a `<UIProvider>` that injects tokens.
4. Spike: build **one showcase component** (e.g. `Button`) on Base UI + CSS Modules end-to-end
   to lock the patterns (class naming, token usage, variant API, story).

## Stage 1 — Primitives (highest reuse first)

Flip the leaf components every page uses, in dependency order. Per component: implement on Base
UI, keep facade props, add story, verify.

Order: `Button` → `ActionIcon` → `Text/Code` → `Tooltip` → `Badge`/`StatusBadge` →
`Input`/`TextInput`/`NumberInput`/`PasswordInput`/`TextArea` → `Select`/`Combobox` →
`Checkbox`/`Switch`/`Radio`/`SegmentedControl` → `Tabs` → `Accordion` → `Menu` (compound,
already canonical-shaped) → `Modal`/`Drawer`/`Popover` → `Alert`/`Toast`/`Progress`/`Spin`/
`Skeleton` → `Divider`/`ScrollArea`/`Collapse`.

> The Stage 1–3 work already done for Gravity (canonical `Modal`/`Menu`/`Tooltip` shapes,
> removed Mantine props on ~6 pages) means many call sites already match a clean compound API —
> Base UI's API is close, so churn is low.

## Stage 2 — Layout & shell (the pain point — own it)

Build these **custom** on Base UI primitives + CSS Modules (this is where we beat Gravity):

- `AppShell` / sidebar navigation (collapsible, sections, keyboard nav, active states) — the
  thing we most want to fix. Use Base UI `Navigation Menu`/`Menu` + our own CSS.
- `PageHeader` / `PageLayout` / `WorkspacePageLayout` / `SectionPanel` / `ResizablePanels`
  (keep `react-resizable-panels`).
- `Card`/`Surface`/`MetricTile`/`StatGrid`/`KeyValueList`/`EmptyState`.

## Stage 3 — Data grid

- Migrate `Table`/`DataTable`/`DataPreview`/`RecordList` to **raw TanStack Table** (drop the
  `@gravity-ui/table` wrapper). One canonical grid: sorting, keyboard nav, sticky header,
  overflow, row selection. Heavy consumers: audit, auth lists, sessions, resource-browser,
  DB-console results.
- `DatePicker` → React Aria `DatePicker`/`Calendar` (or Base UI when shipped).

## Stage 4 — Page validation sweep

With the facade fully on Base UI, walk each page (the 18 from the audit) and confirm
loading/empty/error/skeleton/a11y/responsive against the original audit checklist. The custom
consoles (REST/DB/GraphQL/FHIRPath/ViewDefinition) are mostly Monaco + layout — verify only
their chrome (toolbars, panels, dialogs) picks up the new primitives.

## Stage 5 — Remove Gravity

1. Grep-gate: 0 imports of `@gravity-ui/*` outside the facade internals.
2. Remove `@gravity-ui/*` deps; delete any remaining Gravity shims and the deprecated
   Mantine-compat alias block.
3. Drop dead components; add the icon-only-`aria-label` lint rule.
4. Rewrite `CLAUDE.md` design-system section for Base UI + CSS Modules + tokens.

## Sequencing & risk

| Stage | Scope | Parallel? | Risk |
|---|---|---|---|
| 0 Foundation+brand | tokens, deps, provider, 1 spike | no | low — no app change |
| 1 Primitives | ~25 leaf components | partly (per component) | low — facade keeps API |
| 2 Shell/layout | sidebar, shell, panels | no (shared) | medium — visual redesign |
| 3 Data grid | TanStack Table swap | per consumer | medium — table behavior |
| 4 Page sweep | 18 pages | yes (per page) | low |
| 5 Remove Gravity | deps + cleanup + docs | no | low (gated by grep) |

**DoD:** 0 `@gravity-ui/*` and 0 Tailwind anywhere; app on Base UI + TanStack Table + CSS
Modules + `--octo-*` tokens; distinctive shell/sidebar; every page passes the audit checklist;
`typecheck`+`lint`+`build` green; stories for primitives; CLAUDE.md current.

## Open decisions

- **F1** Foundation: **Base UI** (recommended) vs **React Aria Components** (more complete,
  verbose) vs hybrid (Base UI + React Aria for Table/DatePicker).
- **F2** Brand direction: who owns the visual language / do we need mockups for shell+sidebar
  before Stage 1?
- **F3** Typed styling: stay on plain CSS Modules (recommended) or adopt vanilla-extract/StyleX
  for typed tokens (still no Tailwind).
- **F4** Keep the cleanly-migrated Gravity pages (operations/logs/apps/resource-browser) as-is
  until Stage 4, or revert the in-flight partials now? (Build is green either way.)
