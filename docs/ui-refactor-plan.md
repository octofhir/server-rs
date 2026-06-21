# OctoFHIR Web Console — Production-Ready Refactor Plan

> Goal: production-ready console + hardened `@octofhir/ui-kit`, **Mantine fully
> removed in every form** (deps already gone; remove the compat-prop layer + stale
> docs). Companion to [ui-audit.md](./ui-audit.md). Driven from the Phase 0 audit.

## Strategy: sequence that never leaves the app broken

The deprecated compat aliases are load-bearing for ~150 call sites. We can't delete
them first, and we can't migrate pages first against missing/wrong primitives.
**Order matters:**

```
Stage 1  Foundation (ui-kit)      ── make canonical API solid; FIX broken primitives;
                                     keep deprecated aliases working as a bridge.
Stage 2  Stop the bleeding (P0)   ── crash + compile fixes; un-exclude tsconfig.
Stage 3  Page migration (P1)      ── per-page: Mantine props → canonical, add
                                     skeleton/empty/error states, aria-labels, tokens.
Stage 4  Kill the bridge          ── delete deprecated aliases once 0 refs; lint guard.
Stage 5  Polish (P2) + docs       ── tokens sweep, dedup, CLAUDE.md, a11y contrast.
```

Stage 1 is sequential (shared files). Stage 3 parallelizes per-page (each page dir is
isolated → safe concurrent agents). Stage 4 only after Stage 3 reports zero references.

---

## Stage 1 — Foundation: fix the ui-kit primitives (P0, sequential)

Make the kit correct *before* migrating call sites. Each item: implement + Storybook
story + `pnpm typecheck && pnpm lint` in `packages/ui-kit`.

1. **Modal** — add a real wrapper mapping `opened→open` and rendering `title`/`size`/
   `withCloseButton`/`trapFocus`, OR document Gravity API and fix the 2 call sites.
   *(Recommend wrapper — many call sites assume `opened`+`title`.)*
2. **Menu** — add a compound `Menu.Target/Dropdown/Item` compat over Gravity
   `DropdownMenu`, OR provide a clear `items[]`+`switcher` helper. *(2 crashing sites.)*
3. **RingProgress** — real determinate ring (Gravity `Progress` circular / SVG donut on
   tokens). Stop aliasing to `Spin`.
4. **ScrollArea** — real overflow container (not `Box`); support `Autosize`/max-height.
5. **Stack / Center / Group** — pass `direction='column'` / center alignment, or remove.
6. **ThemeIcon** — accept token sizes + a color mapper to its union; ignore-safe `radius`.
7. **Progress** — wrapper accepting `color`/`size` (mapped to Gravity `theme`/size) so
   bars color correctly.
8. **Input family** — extract `leftSection/rightSection→startContent/endContent` shim
   and apply uniformly to `NumberInput`/`PasswordInput`/`TextArea` (or drop everywhere).
9. **Collapse** — implement the height transition (honor `transitionDuration`) or remove
   the prop.
10. Add missing stories: `EmptyState`, `PageLayout`, `Form`.

**Decision needed (D1):** for Modal/Menu/Progress/ThemeIcon — *complete the compat
wrapper* (faster, keeps Mantine-ish API) **or** *go canonical Gravity + migrate call
sites* (aligns with "kill Mantine"). Recommendation: **canonical wrappers under
ui-kit's own names** (e.g. real `Modal`/`Menu`/`RingProgress` with a clean OctoFHIR API,
not Mantine's), so Stage 4 deletes Mantine props without re-breaking these.

## Stage 2 — Stop the bleeding (P0, after Stage 1)

1. Fix the crash sites now resolvable against fixed primitives: CommandPalette Modal,
   HistoryPanel + ColumnBuilder menus, AuditEventDetail ScrollArea, AuditAnalytics
   donut + bars, ThemeIcon usages.
2. **Make operations + logs compile**: migrate their Mantine props to canonical API.
3. **Remove the `tsconfig` excludes** that hide operations/logs (and audit) so
   `pnpm typecheck` actually gates the whole `ui/src`. This is the guardrail that
   prevents regression — do it as soon as those files compile.
4. **Sessions real user id** — wire `useCurrentUser()`/auth context; gate query.
5. Keyboard-P0: convert click-only `<div>` toggles to `<button>`/`role+tabIndex+onKeyDown`
   with `aria-expanded` (PlaygroundPanel, LogEntry, ResultItem, HistoryTab); remove the
   decorative empty `ActionIcon` in AuditEventList.

## Stage 3 — Page migration (P1, parallel per page)

Per page, a uniform checklist (one agent per page dir, isolated):

- [ ] Replace dropped Mantine props with canonical API (`Text variant/color/ellipsis`,
      `Tooltip content/placement`, `Alert theme/title/message`, `Badge color`,
      `ActionIcon view`, `size="sm"`→`"s"`).
- [ ] `Skeleton` for loading (shaped like content), not spinner/“Loading…” text.
- [ ] `EmptyState` (title + description + CTA) for empty; distinguish empty-vs-filtered.
- [ ] Error branch: read `isError/error/refetch`, render message + **Retry**.
- [ ] Replace `window.confirm()` with kit confirm Modal.
- [ ] `aria-label` on every icon-only control; `aria-label`/visually-hidden label on
      search inputs; `aria-haspopup="menu"` on row-action triggers.
- [ ] CSS: `px`→tokens (radius/spacing/font); kill raw `rgba()` semantic colors via
      `color-mix` on `--octo-accent-*`.
- [ ] One surface system per page (`--octo-surface-*`, not mixed `--g-*`).
- [ ] `notifications.show` → `notify(...)`; legacy color strings → semantic
      (`success`/`fire`).

**Priority order** (worst first): operations, logs, audit, resource-browser, sessions,
auth (5 list pages), apps → then console, db-console, viewdefinition, dashboard,
automations, metadata, packages, fhirpath-console, settings, graphql-console, login.

## Stage 4 — Kill the Mantine bridge

1. Verify **0 references** to each deprecated alias/prop in `ui/src` (grep gate).
2. Delete the `~22` deprecated aliases block in `shared/ui/index.ts`; delete the
   `leftSection/rightSection/leftIcon/color` Mantine-compat props from `Button`,
   `TextInput`, `Hotkey`.
3. Remove dead components/aliases: `Container`, `AppLayout`, `AppShell`, `Burger`,
   `LoadingOverlay`/`NavLink`/`SimpleGrid`, `QueryBuilder`/`RawRequestInput`/
   `DeleteConfirmModal` (dead console files), db-console `LeftPanel`/`EditorPane`/
   `ResultsPane`. Collapse `Card/Paper`, `Hotkey/Kbd`, `Select/MultiSelect` to one name.
4. Decide `Table` vs `DataTable` (one grid system); `Container` vs `PageContainer`.
5. **Add a lint rule** (Biome/oxlint or custom) requiring `aria-label` on icon-only
   `ActionIcon` — prevents a11y regressions.

## Stage 5 — Polish + docs (P2)

1. Token sweep across remaining ~25 CSS modules (P2 tokens findings). Add `2xs` font
   token + a `2px` radius stop if we want exact coverage for 11px/9px/2px.
2. Replace hand-rolled `Metric` tiles with `MetricTile`; status `Badge` → `StatusBadge`.
3. Per-row mutation state (automations Deploy), shared shadow/motion tokens.
4. **Rewrite `CLAUDE.md`** "Design System Rules" + L125 for Gravity UI + ui-kit tokens
   (drop Mantine/`createTheme`/`themeCssVars`/`--app-*`).
5. Align OAuth `authorize_templates.rs` + `authentication.mdx` wording/colors to tokens.
6. **WCAG contrast pass** on `--octo-text-secondary`/dimmed over `--octo-surface-2`.

---

## Effort & sequencing

| Stage | Scope | Size | Parallel? |
|-------|-------|------|-----------|
| 1 Foundation | ~10 ui-kit primitives + stories | M | no (shared) |
| 2 Stop-bleed | crashes + operations/logs compile + tsconfig + sessions + kbd P0 | M | partial |
| 3 Page migration | 18 pages × checklist | L | yes (per page) |
| 4 Kill bridge | delete aliases + dead code + lint | S | no (shared) |
| 5 Polish + docs | tokens, dedup, docs, contrast | M | partial |

**DoD**: 0 Mantine refs (code + docs + comments); `pnpm typecheck` + `pnpm lint` green
in `ui` and `packages/ui-kit` with **no tsconfig excludes**; every page passes the
Stage-3 checklist; `EmptyState`/`Skeleton`/`MetricTile`/`StatusBadge` actually used;
all icon-only controls labeled; tokens-only CSS; stories for user-facing primitives;
CLAUDE.md current.

## Open decisions

- **D1** (Stage 1): canonical OctoFHIR wrappers vs. completing Mantine-compat shims.
  *Recommend canonical.*
- **D2** (Stage 4): `Table` (custom CSS) or `DataTable` (TanStack) as the single grid?
- **D3** (Stage 5): introduce `2xs` font + `2px` radius tokens, or snap to nearest stop?
