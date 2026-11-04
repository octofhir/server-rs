#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
manifest=tests/library-consumer/Cargo.toml
for feature in api config notifications cql; do
  cargo run --manifest-path "$manifest" --no-default-features --features "$feature"
  tree=$(cargo tree --manifest-path "$manifest" --no-default-features --features "$feature" --edges normal,build --prefix none --format '{p}')
  if printf '%s\n' "$tree" | grep -E '^(axum|reqwest|hyper|hyper-util|rustls|native-tls|openssl|sqlx|sqlx-core|sqlx-postgres|lettre|aws-lc-rs) v'; then
    echo "Unexpected transport/database dependency in $feature consumer" >&2
    exit 1
  fi
done
