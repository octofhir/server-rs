# Security policy

OctoFHIR stores health data, so we take security reports seriously.

## Reporting a vulnerability

Please **do not** open a public issue, discussion or pull request for a security problem.

Report it privately through GitHub instead:
[**Report a vulnerability**](https://github.com/octofhir/server-rs/security/advisories/new).

Helpful details:

- what an attacker can do (read or change data, bypass authorization, crash the server, …);
- the affected version or commit, and your configuration if it matters;
- steps or a request sequence that reproduces it.

We will confirm the report, keep you updated while we work on a fix, and credit you in the
advisory unless you prefer not to be named.

## Supported versions

OctoFHIR is pre-1.0. Fixes land on `main` and in the next published image
(`ghcr.io/octofhir/octofhir-server`); older images are not patched.

## Deployment notes

- Change the bootstrap admin password (`bootstrap.admin_user`) before exposing a server.
- The default `octofhir.toml` is for local development; see the
  [security guide](https://octofhir.github.io/server-rs/security/) for production settings.
