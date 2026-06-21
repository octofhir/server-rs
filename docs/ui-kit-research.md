# UI Kit Research & Recommendation (2026)

> Goal: replace the current foundation (`@gravity-ui/*` behind `@octofhir/ui-kit`) with a
> **modern** stack that lets us build a **recognizable, own visual style** (great sidebars,
> polished shell), keep our heavy custom tools (REST console, DB console, GraphQL, FHIRPath,
> ViewDefinition) and data tables. **Hard constraint: NO Tailwind CSS in any form.**

## Why move off Gravity

- Look is generic / not ours; shell + sidebars are weak and hard to restyle.
- The kit is consumed as raw re-exports, so half the "components" are just Gravity with a
  thin shim — we fight its design tokens and component APIs to get a custom look.
- We already proved (Stages 1–3) that wrapping it cleanly is possible but the ceiling is
  still "Gravity that looks slightly different".

## The decision that actually matters: **headless vs. batteries-included**

| | Batteries-included (Ant Design, MUI, PrimeReact, Mantine) | Headless primitives + own CSS (Base UI, React Aria, Radix, Ark) |
|---|---|---|
| Visual identity | Their look; you fight it to differentiate | **100% ours** — primitives ship zero styles |
| Speed to first screen | Fast | Slower (we write the CSS) |
| Sidebars / shell | Their layout components | **We build it** — exactly the pain point we want to own |
| Data tables | Built-in (Ant/Prime strong) | TanStack Table (headless) |
| A11y / keyboard | Built-in | Built-in (that's the whole point of these libs) |
| Lock-in | High | Low (primitives are swappable) |
| Tailwind? | None require it ✅ | None require it ✅ |

We want a **recognizable own style + great custom sidebars** → **headless primitives + our
own CSS Modules + the existing `--octo-*` token system.** Batteries-included kits (Ant v6,
PrimeReact) are faster but give a generic, hard-to-own look — wrong fit for the stated goal.

## Headless options compared (2026)

| Library | Components | API style | A11y/i18n | Status | Notes |
|---|---|---|---|---|---|
| **Base UI** `@base-ui/react` | 40+ (incl. Combobox, Autocomplete, Select, Menu, Dialog, Drawer, Popover, Tooltip, Tabs, Accordion, Toast, Number/OTP fields, Toolbar, Navigation Menu) | Radix-like compound parts — **clean DX** | Strong a11y | **v1.0 stable Dec 2025**, v1.1 since; by the creators of Radix + Floating UI + MUI; MUI Base deprecated in its favor | **No** data grid, **no** date picker |
| **React Aria Components** (Adobe) | 40+ incl. **Table, GridList, DatePicker/Calendar, Combobox, ListBox** | Render-props/hooks — more verbose, most flexible | **Best-in-class** a11y + i18n (30+ locales, RTL, calendars) | Mature, actively maintained | Covers our gaps (Table, DatePicker) in one family |
| Radix Primitives | 30+ | Compound parts | Strong | Acquired by WorkOS; **velocity slowed** (Combobox/multi-select lag) | What shadcn uses (but shadcn = Tailwind, excluded) |
| Ark UI (Zag.js) | 35+ | State-machine, multi-framework (React/Vue/Solid) | Strong | Growing | Multi-framework we don't need |

Gaps either way: **data grid → [TanStack Table](https://tanstack.com/table)** (headless,
we already pull it transitively via `@gravity-ui/table`), **date picker → React Aria's
`DatePicker`/`Calendar`** (or Base UI's when it ships).

## Recommendation

**Primary: Base UI** for primitives + **CSS Modules with our `--octo-*` tokens** for styling +
**TanStack Table** for grids + **React Aria `DatePicker`/`Calendar`** for dates.

Rationale:
- Base UI is the modern successor everyone is consolidating on, **style-agnostic**, clean
  compound API close to what our pages already expect (`Menu.Trigger/Popup/Item`,
  `Dialog`, `Select`) → least call-site churn.
- We keep **CSS Modules** (already in use) + the existing token layer → no new styling
  paradigm, **zero Tailwind**, full control to build a distinctive shell/sidebar.
- Our heavy tools (REST/DB/GraphQL/FHIRPath consoles) are **Monaco + custom layout**, not
  kit components — they're largely unaffected by the swap.

**Strong alternative: React Aria Components** — pick this instead if we want one coherent
headless family that *also* ships Table + DatePicker + Combobox + i18n out of the box and we
accept a more verbose API. Best if accessibility/i18n is a top priority.

**Not recommended for this goal:** Ant Design / MUI / PrimeReact (generic look, fights our
brand) and anything Tailwind-based (HeroUI/NextUI, shadcn) — excluded by constraint.

## Styling system

Keep **CSS Modules + `--octo-*` design tokens** (already in `packages/ui-kit/src/shared/theme`).
No Tailwind. Optional later upgrade: **vanilla-extract** or **StyleX** for type-safe tokens —
both are build-time CSS (not Tailwind), but only if we want typed styles; not required.

## What carries over

- The `@octofhir/ui-kit` **facade is the migration seam** — its public API stays; we swap the
  implementation underneath (Gravity → Base UI), component by component. App pages barely change.
- The `--octo-*` token system, the Stage 1–3 cleanup (Mantine props already removed from
  ~6 pages, error/empty/skeleton/a11y patterns added) — all reusable.
- TanStack Table (via gravity-table today) → migrate to raw TanStack Table.
- Monaco-based consoles, Effector stores, TanStack Query, routing — untouched.

## Sources
- [Base UI releases](https://base-ui.com/react/overview/releases) · [InfoQ: Base UI 1.0](https://www.infoq.com/news/2026/02/baseui-v1-accessible/) · [Base UI GitHub](https://github.com/mui/base-ui)
- [Base UI vs Radix vs Ark guide](https://www.pkgpulse.com/guides/base-ui-vs-radix-ui-vs-ark-ui-guide-for-headless-react-components-2026) · [LogRocket: headless alternatives](https://blog.logrocket.com/headless-ui-alternatives/) · [GreatFrontend: top headless 2026](https://www.greatfrontend.com/blog/top-headless-ui-libraries-for-react-in-2026)
- [React Aria Components](https://react-aria.adobe.com/) · [TanStack Table](https://tanstack.com/table)
