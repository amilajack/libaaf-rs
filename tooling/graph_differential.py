#!/usr/bin/env python3
"""Compare timeline interpretation after independently perturbing graph properties."""
from pathlib import Path
import hashlib
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[1]
passed = 0
with tempfile.TemporaryDirectory(prefix='libaaf-graphs-') as tmp:
    work = Path(tmp)
    for fixture in sorted((ROOT / 'tooling/reference/test/aaf').glob('*.aaf')):
        variants = work / fixture.stem
        subprocess.run([str(ROOT / 'target/release/mutations'), str(fixture), str(variants)], check=True)
        for variant in sorted(variants.glob('*.aaf')):
            outputs = []
            for name, binary in [('c', ROOT / 'tooling/reference-build/bin/aaftool'), ('rust', ROOT / 'target/release/aaftool')]:
                log = work / f'{name}.txt'
                log.unlink(missing_ok=True)
                result = subprocess.run([str(binary), '--aaf-summary', '--aaf-essences', '--aaf-clips', '--show-automation', '--show-metadata', '--pos-format', 'samples', '--relative-path', '--no-color', '--verb', '0', '--log-file', str(log), str(variant)], capture_output=True)
                outputs.append((result.returncode, log.read_bytes() if log.exists() else b''))
            if outputs[0] != outputs[1]:
                result_dir = ROOT / 'tooling/results/graph-differences'
                result_dir.mkdir(parents=True, exist_ok=True)
                for name, (_, data) in zip(('c', 'rust'), outputs):
                    (result_dir / f'{fixture.stem}-{variant.stem}-{name}.txt').write_bytes(data)
                raise SystemExit(f'Graph mismatch: {fixture.name} {variant.name}')
            if variant.name.startswith('large-wave-header'):
                for fmt in ('original', 'wav', 'bwav'):
                    for offset, length in ((0, 0), (1, 17)):
                        with tempfile.TemporaryDirectory(dir=work) as extract_tmp:
                            extracted = []
                            for name, binary in [('c', ROOT / 'tooling/reference-build/extraction'), ('rust', ROOT / 'target/release/extraction')]:
                                destination = Path(extract_tmp) / name
                                destination.mkdir()
                                result = subprocess.run([str(binary), str(variant), fmt, str(destination), str(offset), str(length)], capture_output=True, check=True)
                                files = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in destination.iterdir()}
                                extracted.append((result.stdout, files))
                            assert extracted[0] == extracted[1], (fixture.name, variant.name, fmt, offset, length, extracted)
            passed += 1
assert passed > 0
print(f'{passed} generated timeline-property comparisons passed')
