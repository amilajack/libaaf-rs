#!/usr/bin/env python3
"""Build development-only C oracles on macOS/Linux using upstream's archive name."""
import os, pathlib, shlex, subprocess
root = pathlib.Path(__file__).resolve().parents[1]
build = root / 'tooling/reference-build'
archive = next((p for p in (build / 'lib/aaf', build / 'lib/libaaf.a') if p.is_file()), None)
if archive is None:
    raise SystemExit('Build the pinned C reference with BUILD_STATIC_LIB=ON first.')
for name in ('inspection', 'benchmark', 'extraction'):
    subprocess.run([*shlex.split(os.environ.get('CC', 'cc')), '-O3', '-DNDEBUG', '-I', str(root / 'tooling/reference/include'), str(root / f'tooling/{name}.c'), str(archive), '-lm', '-o', str(build / name)], check=True)
