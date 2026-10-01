#!/usr/bin/env python3
"""Compile the audit against the current workspace; run only a disposable PostgreSQL."""
import os
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
EXTERNS = {
    'octofhir_search', 'octofhir_db_postgres', 'serde_json', 'tokio', 'sqlx_core',
    'sqlx_postgres', 'octofhir_storage', 'testcontainers', 'testcontainers_modules',
    'chrono', 'async_trait',
}
# Reuse the dependencies from the audit's successful no-run build. Do not
# let this diagnostic script rewrite the user's existing Cargo.lock.
artifacts = {}
for name in EXTERNS:
    paths = [p for p in (ROOT / 'target/debug/deps').glob(f'lib{name}-*')
             if p.suffix in ('.rlib', '.dylib', '.so')]
    if paths:
        artifacts[name] = str(max(paths, key=lambda p: p.stat().st_mtime))
missing = EXTERNS - artifacts.keys()
if missing:
    raise SystemExit(f'Missing dependency artifacts: {sorted(missing)}')
with tempfile.TemporaryDirectory(prefix='octofhir-search-audit-') as temp:
    executable = Path(temp) / 'probe'
    command = ['rustc', '--edition=2024', str(HERE / 'probe.rs'), '-L',
               f'dependency={Path(artifacts["octofhir_search"]).parent}', '-o', str(executable)]
    for name, path in sorted(artifacts.items()):
        command.extend(['--extern', f'{name}={path}'])
    subprocess.run(command, cwd=ROOT, check=True)
    if os.environ.get('AUDIT_BUILD_ONLY') == '1':
        print('Audit compiled successfully; no database started.')
        raise SystemExit(0)
    # Probe creates its own container and ignores DATABASE_URL / production config.
    env = dict(os.environ, AUDIT_OUTPUT=str(HERE))
    with (HERE / 'results.jsonl').open('w') as results, (HERE / 'probe.stderr').open('w') as errors:
        subprocess.run([str(executable)], cwd=ROOT, env=env, stdout=results, stderr=errors, check=True)
print(f'Results: {HERE / "results.jsonl"}')
