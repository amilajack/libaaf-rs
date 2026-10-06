#!/usr/bin/env python3
"""Compare original property bytes and stable reference edges; no pointer bytes.

This is the user-approved exception for process-dependent C debug dumps.
Node IDs are the on-disk directory indices, shared by both implementations.
"""
import difflib, json, pathlib, subprocess
ROOT = pathlib.Path(__file__).resolve().parents[1]
output = ROOT / 'tooling/results/inspection'
output.mkdir(parents=True, exist_ok=True)
results = []
for fixture in sorted((ROOT / 'tooling/reference/test/aaf').glob('*.aaf')):
    c = subprocess.run([str(ROOT / 'tooling/reference-build/inspection'), str(fixture)], capture_output=True, check=True, text=True)
    rust = subprocess.run([str(ROOT / 'target/release/libaaf-acceptance'), 'inspect', str(fixture), '1', '.'], capture_output=True, check=True, text=True)
    a, b = sorted(c.stdout.splitlines()), sorted(rust.stdout.splitlines())
    ok = a == b
    results.append({'fixture': fixture.name, 'pass': ok, 'reference_records': len(a), 'rust_records': len(b)})
    if not ok:
        (output / (fixture.stem + '.diff')).write_text('\n'.join(difflib.unified_diff(a, b, 'C', 'Rust')) + '\n')
report = {'passed': sum(r['pass'] for r in results), 'total': len(results), 'cases': results}
(output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(f"{report['passed']}/{report['total']} stable property/reference comparisons passed")
raise SystemExit(report['passed'] != report['total'])
