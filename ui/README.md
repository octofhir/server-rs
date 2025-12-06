# Octofhir UI

Solid + Vite application that powers the Octofhir web console. The project uses `pnpm` scripts defined in `package.json`.

## Local scripts
- `pnpm dev` – run the Vite dev server.
- `pnpm build` – produce the production bundle.
- `pnpm preview` – preview the production build locally.
- `pnpm typecheck` – run TypeScript in `--noEmit` mode.

## Biome linting & formatting
Biome is the single source of truth for linting and formatting:

- `pnpm lint` → `biome lint .`
- `pnpm format` → `biome format --write .`
- `pnpm check` → `biome check .`

Update the rules inside `biome.json` when new conventions are needed. Do not add ESLint or Prettier—Biome handles both linting and formatting for this codebase per our tooling policy.
