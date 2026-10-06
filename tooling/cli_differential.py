#!/usr/bin/env python3
"""Help, required options, validation precedence, file selection, and quiet errors."""
import pathlib, re, subprocess
root=pathlib.Path(__file__).resolve().parents[1]
fixture=str(root/'tooling/reference/test/aaf/MC_Empty.aaf')
cases=[[],['--help'],['-h'],['--no-color'],['--aaf-summary'],[fixture],['--dump-meta',fixture],['--extract-clips',fixture],['--extract-essences',fixture],['--extract-clips','--extract-essences','--extract-path','.',fixture],['--aaf-summary','--verb','4',fixture],['--aaf-summary','--pos-format','bad',fixture],['--aaf-summary','--extract-format','bad',fixture],['--aaf-summary','--verb','0','missing.aaf'],['--aaf-summary','--no-color','--verb','0',fixture,'missing.aaf']]
for args in cases:
    outputs=[]
    for binary in ['tooling/reference-build/bin/aaftool','target/release/aaftool']:
        r=subprocess.run([str(root/binary),*args],capture_output=True)
        # The sole text normalization is the explicitly permitted version banner.
        stdout=re.sub(rb'(?m)^libaaf(?:-rs)? .*\n',b'<version>\n',r.stdout)
        outputs.append((r.returncode,stdout,r.stderr))
    if outputs[0]!=outputs[1]:
        raise SystemExit(f'CLI mismatch: {args}\nC: {outputs[0]!r}\nRust: {outputs[1]!r}')
print(f'{len(cases)}/{len(cases)} CLI help/validation cases passed')
