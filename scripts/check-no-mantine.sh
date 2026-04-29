#!/usr/bin/env bash
# Fails if any Mantine traces remain in the source tree.
# Excludes: lockfiles, vendored deps, migration commit messages, this script,
# and historical docs / migration plan markdown.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ROOTS=("packages/ui-kit/src" "ui/src")
FAIL=0

# 1) `@mantine/*` package imports.
echo "[check-no-mantine] scanning @mantine package imports..."
HITS=$(grep -rEn "@mantine/" \
    --include="*.ts" --include="*.tsx" --include="*.json" --include="*.css" \
    "${ROOTS[@]}" || true)
if [ -n "$HITS" ]; then
    echo "ERROR: @mantine package references found:"
    echo "$HITS"
    FAIL=1
fi

# 2) `--mantine-*` CSS variables.
echo "[check-no-mantine] scanning --mantine-* CSS variables..."
HITS=$(grep -rEn \
    --include="*.css" --include="*.tsx" --include="*.ts" \
    "[-][-]mantine-" \
    "${ROOTS[@]}" || true)
if [ -n "$HITS" ]; then
    echo "ERROR: --mantine-* CSS variable references found:"
    echo "$HITS"
    FAIL=1
fi

# 3) Tabler icons — Mantine ecosystem, must be migrated to @gravity-ui/icons.
echo "[check-no-mantine] scanning @tabler imports..."
HITS=$(grep -rEn "@tabler/" \
    --include="*.ts" --include="*.tsx" --include="*.json" \
    "${ROOTS[@]}" || true)
if [ -n "$HITS" ]; then
    echo "ERROR: @tabler/* imports found (use @gravity-ui/icons instead):"
    echo "$HITS"
    FAIL=1
fi

# 4) Bare "mantine" identifier usage in source — exclude this script and the
#    historical MANTINE_TO_GRAVITY_MIGRATION.md plan that lives at repo root.
echo "[check-no-mantine] scanning literal 'mantine' identifier..."
HITS=$(grep -rIn -i "mantine" \
    --include="*.ts" --include="*.tsx" --include="*.css" --include="*.scss" \
    "${ROOTS[@]}" || true)
if [ -n "$HITS" ]; then
    echo "ERROR: literal 'mantine' references found:"
    echo "$HITS"
    FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
    echo
    echo "[check-no-mantine] FAILED — clean up the matches above."
    exit 1
fi

echo "[check-no-mantine] OK — no Mantine references found."
