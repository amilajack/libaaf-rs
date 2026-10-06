#!/usr/bin/env python3
"""Local-only acceptance for recursively discovered AAF files; no fixtures uploaded."""
import argparse,pathlib,subprocess,tempfile,json,hashlib,difflib
ROOT=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('directory',type=pathlib.Path);p.add_argument('--rust',default=str(ROOT/'target/release/aaftool'));p.add_argument('--reference',default=str(ROOT/'tooling/reference-build/bin/aaftool'));a=p.parse_args();files=sorted(f for f in a.directory.rglob('*')if f.is_file()and f.suffix.lower()=='.aaf');out=ROOT/'tooling/results/downloads';out.mkdir(parents=True,exist_ok=True);results=[]
with tempfile.TemporaryDirectory()as tmp:
 for i,f in enumerate(files):
  logs=[];before=hashlib.sha256(f.read_bytes()).hexdigest()
  for name,exe in [('reference',a.reference),('rust',a.rust)]:
   log=pathlib.Path(tmp)/name;log.unlink(missing_ok=True)
   r=subprocess.run([exe,'--aaf-summary','--aaf-clips','--aaf-essences','--pos-format','samples','--show-metadata','--show-automation','--relative-path','--no-color','--verb','0','--log-file',str(log),str(f)],capture_output=True)
   logs.append((r.returncode,log.read_text()if log.exists()else r.stderr.decode(errors='replace')))
  results.append({'case':i+1,'name':f.name,'bytes':f.stat().st_size,'sha256':before,'referenceExit':logs[0][0],'rustExit':logs[1][0],'match':logs[0]==logs[1],'unchanged':before==hashlib.sha256(f.read_bytes()).hexdigest()})
  if logs[0]!=logs[1]:(out/f'{i+1}.diff').write_text(''.join(difflib.unified_diff(logs[0][1].splitlines(True),logs[1][1].splitlines(True),'reference','rust')))
(out/'report.json').write_text(json.dumps(results,indent=2)+'\n');print(f"{sum(r['match']for r in results)}/{len(results)} native Downloads comparisons passed; originals unchanged: {all(r['unchanged']for r in results)}")
raise SystemExit(not all(r['match']and r['unchanged']for r in results))
