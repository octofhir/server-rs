# Contributing to OctoFHIR

Thanks for helping. Bug reports, fixes, docs and ideas are all welcome.

- **Questions and ideas:** [GitHub Discussions](https://github.com/octofhir/server-rs/discussions)
- **Bugs and feature requests:** [Issues](https://github.com/octofhir/server-rs/issues/new/choose)
- **Security problems:** please don't open an issue; see [SECURITY.md](SECURITY.md)

## Setting up

You need Rust 1.98+, Docker, Node 22+ and pnpm.

```bash
git clone https://github.com/octofhir/server-rs.git && cd server-rs
docker compose up -d          # PostgreSQL on :5450 (and Redis on :6380)
cargo run                     # server on http://localhost:8888
cd ui && pnpm install && pnpm dev   # console with hot reload on http://localhost:4000/ui/
```

Sign in to the console as `admin` / `admin123`. `bun run scripts/seed-demo.ts` (or **Load demo
data** on an empty Overview) fills the server with sample patients, encounters and observations.

The FHIR libraries the server builds on live in sibling repositories
([fhirpath-rs](https://github.com/octofhir/fhirpath-rs),
[fhirschema](https://github.com/octofhir/fhirschema),
[canonical-manager](https://github.com/octofhir/canonical-manager)) and are expected next to this
checkout; see `Cargo.toml` for the paths.

## Before you open a pull request

Server:

```bash
cargo fmt --all
cargo clippy --all-targets
cargo test
```

Console (`ui/`) and UI kit (`packages/ui-kit/`):

```bash
cd ui
pnpm typecheck
pnpm lint
pnpm build
```

- Keep a pull request to one change, and add tests for behaviour you fix or add.
- Use debug builds while developing; release builds are slow and only needed for benchmarks.
- UI changes: follow the design rules in [CLAUDE.md](CLAUDE.md#design-system-rules) (tokens from
  `@octofhir/ui-kit`, one Marigold accent, light and dark themes both checked), and attach
  screenshots of both themes to the pull request.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/) with the area in parentheses:

```
fix(search): correlate composite components
feat(ui): recent places in the command palette
docs: getting started for Docker
```

Say what changed and why in the body when the subject isn't enough.

## License

OctoFHIR is licensed under the [Apache License 2.0](LICENSE). By contributing, you agree that your
contributions are licensed under the same terms.
