#!/usr/bin/env python3
"""Compare native library extraction results and exact bytes against pinned C."""
import hashlib
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARIES = (ROOT / 'tooling/reference-build/extraction', ROOT / 'target/release/extraction')


def run(binary, fixture, fmt, destination, offset, length):
    result = subprocess.run([str(binary), str(fixture), fmt, str(destination), str(offset), str(length)], capture_output=True, check=True)
    files = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (destination.iterdir() if destination.is_dir() else ()) if p.is_file()}
    return result.stdout, files


passed = 0
embedded = 0
embedded_fixtures = set()
for fixture in sorted((ROOT / 'tooling/reference/test/aaf').glob('*.aaf')):
    for fmt in ('original', 'wav', 'bwav'):
        for offset, length in ((0, 0), (0, 1), (1, 17), (31, 127)):
            with tempfile.TemporaryDirectory(prefix='libaaf-extract-') as tmp:
                outputs = []
                for name, binary in zip(('c', 'rust'), BINARIES):
                    destination = Path(tmp) / name
                    destination.mkdir()
                    outputs.append(run(binary, fixture, fmt, destination, offset, length))
                if outputs[0] != outputs[1]:
                    raise SystemExit(f'{fixture.name} {fmt} {offset}:{length}\nC: {outputs[0]}\nRust: {outputs[1]}')
                if outputs[0][0]:
                    embedded += 1
                    embedded_fixtures.add(fixture)
                passed += 1
for fixture in sorted(embedded_fixtures):
    for fmt in ('original', 'wav', 'bwav'):
        for failure in ('missing-parent', 'not-directory', 'unicode'):
            with tempfile.TemporaryDirectory(prefix='libaaf-extract-') as tmp:
                outputs = []
                for name, binary in zip(('c', 'rust'), BINARIES):
                    destination = Path(tmp) / name / '音声 é'
                    if failure == 'not-directory':
                        destination.parent.mkdir()
                        destination.write_text('sentinel')
                    elif failure == 'unicode':
                        destination.mkdir(parents=True)
                    outputs.append(run(binary, fixture, fmt, destination, 1, 17))
                    if failure == 'not-directory':
                        assert destination.read_text() == 'sentinel'
                assert outputs[0] == outputs[1], (fixture.name, fmt, failure, outputs)
                passed += 1
print(f'{passed} extraction API comparisons passed ({embedded} cases with embedded media); statuses, filenames, and SHA-256 hashes match')
