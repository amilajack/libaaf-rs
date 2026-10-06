#!/usr/bin/env python3
"""Paired Mac measurements, separate parse and extraction, equivalent optimized work."""
import argparse,subprocess,pathlib,json,re,statistics,math,tempfile,datetime,platform,hashlib
ROOT=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--pairs',type=int,default=21);p.add_argument('--iterations',type=int,default=100);p.add_argument('--large-file',type=pathlib.Path);p.add_argument('--large-iterations',type=int,default=10);a=p.parse_args()
if platform.system()!='Darwin':raise SystemExit('Use the designated Mac acceptance machine; shared CI timings are informational.')
if a.pairs<5 or a.iterations<1 or a.large_iterations<1:raise SystemExit('Use at least five pairs and positive iterations.')
workloads=[('small','MC_Empty.aaf'),('large','PT_Fades.aaf'),('multichannel','DR_Multichannel_7.1_single_source.aaf'),('embedded','PT_PCM_Internal.aaf')]
if a.large_file:
 if not a.large_file.is_file():raise SystemExit('Large workload does not exist')
 workloads.append(('large-local',None))
def cmd(*args):return subprocess.check_output(args,text=True).strip()
def measure(exe,mode,file,out):
 r=subprocess.run(['/usr/bin/time','-l',str(exe),mode,str(file),str(iterations),str(out)],capture_output=True,text=True,check=True)
 return {'seconds':int(r.stdout.strip())/1e9,'peak_rss':int(re.search(r'(\d+)\s+maximum resident set size',r.stderr)[1]),'peak_footprint':int(re.search(r'(\d+)\s+peak memory footprint',r.stderr)[1])}
results=[];bins={'c':ROOT/'tooling/reference-build/benchmark','rust':ROOT/'target/release/libaaf-acceptance'}
with tempfile.TemporaryDirectory(prefix='libaaf-bench-')as tmp:
 output=pathlib.Path(tmp)
 for name,fixture in workloads:
  for mode in ['parse','extract']:
   if mode=='extract'and name!='embedded':continue
   file=a.large_file if fixture is None else ROOT/'tooling/reference/test/aaf'/fixture
   iterations=a.large_iterations if fixture is None else a.iterations
   for _ in range(3):
    for exe in bins.values():measure(exe,mode,file,output)
   pairs=[]
   for i in range(a.pairs):
    pair={}
    for lang in (['c','rust']if i%2==0 else['rust','c']):pair[lang]=measure(bins[lang],mode,file,output)
    pairs.append(pair)
   ratios=[math.log(v['rust']['seconds']/v['c']['seconds'])for v in pairs];mean=statistics.mean(ratios);spread=1.96*statistics.stdev(ratios)/math.sqrt(len(ratios));ratio=math.exp(mean);memory=max(v['rust']['peak_rss']for v in pairs)/max(v['c']['peak_rss']for v in pairs)
   result={'workload':name,'fixture':fixture or '<private local AAF>','input_bytes':file.stat().st_size,'input_sha256':hashlib.sha256(file.read_bytes()).hexdigest(),'mode':mode,'iterations':iterations,'pairs':pairs,'c_median_seconds':statistics.median(v['c']['seconds']for v in pairs),'rust_median_seconds':statistics.median(v['rust']['seconds']for v in pairs),'paired_geomean_ratio':ratio,'ratio_95_percent_ci':[math.exp(mean-spread),math.exp(mean+spread)],'peak_rss_ratio':memory,'runtime_pass':math.exp(mean+spread)<=1.05,'memory_pass':memory<=1.10}
   results.append(result);print(f"{name}/{mode}: runtime {ratio:.3f}x C, peak RSS {memory:.3f}x C",flush=True)
report={'measured_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'environment':{'os':cmd('sw_vers'),'cpu':cmd('sysctl','-n','machdep.cpu.brand_string'),'memory_bytes':cmd('sysctl','-n','hw.memsize'),'rust':cmd('rustc','--version'),'cc':cmd('cc','--version'),'reference_commit':cmd('git','-C',str(ROOT/'tooling/reference'),'rev-parse','HEAD'),'storage':'same temporary directory on internal disk; overwritten WAV output; page cache warm','concurrency':'sequential alternating paired runs; normal user desktop activity','build':'Rust release fat LTO, panic abort; C reference CMake Release and harness -O3 -DNDEBUG'},'results':results,'acceptance_pass':all(r['runtime_pass']and r['memory_pass']for r in results)}
path=ROOT/'tooling/results/benchmarks.json';path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(report,indent=2)+'\n');print('Acceptance:',report['acceptance_pass'])
