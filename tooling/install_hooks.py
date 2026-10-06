#!/usr/bin/env python3
"""Install versioned hooks without replacing an existing hook setup."""
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
configured = subprocess.run(['git', 'config', '--get', 'core.hooksPath'], cwd=root, text=True, capture_output=True).stdout.strip()
if configured and configured != '.githooks':
    raise SystemExit(f'Existing hooksPath {configured!r}; preserve and integrate that setup before installing.')
legacy = pathlib.Path(subprocess.check_output(['git', 'rev-parse', '--git-path', 'hooks/pre-commit'], cwd=root, text=True).strip())
if not legacy.is_absolute():
    legacy = root / legacy
if legacy.exists() and not configured:
    raise SystemExit(f'Existing pre-commit hook at {legacy}; preserve and integrate it before installing.')
(root / '.githooks/pre-commit').chmod(0o755)
subprocess.run(['git', 'config', '--local', 'core.hooksPath', '.githooks'], cwd=root, check=True)
print('Installed .githooks/pre-commit; all CI verification runs before commits.')
