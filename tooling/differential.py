#!/usr/bin/env python3
"""Compare native implementations without altering substantive output."""
import argparse, difflib, json, pathlib, subprocess, tempfile
ROOT=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--rust',default=str(ROOT/'target/release/aaftool'));p.add_argument('--reference',default=str(ROOT/'tooling/reference-build/bin/aaftool'));p.add_argument('--quick',action='store_true');args=p.parse_args()
results=[];out=ROOT/'tooling/results/differential';out.mkdir(parents=True,exist_ok=True)
variants=[['--aaf-summary','--aaf-essences','--aaf-clips','--pos-format','samples','--show-automation','--show-metadata','--relative-path']]
if not args.quick:
 variants += [['--aaf-clips','--aaf-essences','--pos-format',fmt] for fmt in ['tc','hms','raw']]
 variants += [['--aaf-clips','--pt-true-fades','--pt-remove-sae','--pos-format','samples'],['--cfb-header','--cfb-fat','--cfb-minifat','--cfb-difat','--cfb-nodes'],['--aaf-classes','--aaf-meta']]
with tempfile.TemporaryDirectory() as tmp:
 for f in sorted((ROOT/'tooling/reference/test/aaf').glob('*.aaf')):
  for i,variant in enumerate(variants):
   a=[]
   for lang,exe in [('c',args.reference),('rust',args.rust)]:
    log=pathlib.Path(tmp)/(lang+'.txt');r=subprocess.run([exe,*variant,'--verb','0','--no-color','--log-file',str(log),str(f)],capture_output=True)
    a.append((r.returncode,log.read_text() if log.exists() else r.stderr.decode(errors='replace')))
   ok=a[0]==a[1];results.append({'fixture':f.name,'options':variant,'pass':ok})
   if not ok:
    (out/f'{f.stem}-{i}.diff').write_text(''.join(difflib.unified_diff(a[0][1].splitlines(True),a[1][1].splitlines(True),'reference','rust')))
summary={'passed':sum(r['pass'] for r in results),'total':len(results),'cases':results};(out/'report.json').write_text(json.dumps(summary,indent=2)+'\n')
print(f"{summary['passed']}/{summary['total']} differential cases passed; diffs: {out}")
raise SystemExit(summary['passed']!=summary['total'])
