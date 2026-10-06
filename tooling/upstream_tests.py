#!/usr/bin/env python3
"""Run pinned upstream comparisons and extraction hashes without Valgrind coupling."""
import ast,pathlib,subprocess,tempfile,hashlib,shlex,sys
ROOT=pathlib.Path(__file__).resolve().parents[1];ref=ROOT/'tooling/reference';exe=pathlib.Path(sys.argv[1]if len(sys.argv)>1 else ROOT/'target/release/aaftool').resolve()
values={'TEST_MEDIA_DIR':str(ref/'test/res'),'DIR_SEP':'/'}
def value(n):
 if isinstance(n,ast.Constant):return n.value
 if isinstance(n,ast.Name):return values[n.id]
 if isinstance(n,ast.BinOp)and isinstance(n.op,ast.Add):return value(n.left)+value(n.right)
 if isinstance(n,ast.List):return [value(v)for v in n.elts]
 raise ValueError(ast.dump(n))
passed=0
with tempfile.TemporaryDirectory()as tmp:
 out=pathlib.Path(tmp)
 for n in ast.parse((ref/'test/test.py').read_text()).body:
  if not(isinstance(n,ast.Expr)and isinstance(n.value,ast.Call)and isinstance(n.value.func,ast.Name)and n.value.func.id in ['test','extract']):continue
  call=n.value;args=[value(v)for v in call.args];file,extra=args[:2];options=['--aaf-summary','--aaf-essences','--aaf-clips','--pos-format','samples','--show-automation','--show-metadata',*shlex.split(extra),'--verb','0','--no-color','--relative-path']
  if call.func.id=='test':
   log=out/'analysis.txt';r=subprocess.run([str(exe),*options,'--log-file',str(log),str(ref/'test/aaf'/file)],capture_output=True,check=True)
   actual=log.read_text();expected=(ref/'test/expected'/(file+'.expected')).read_text();assert actual==expected,file
  else:
   subprocess.run([str(exe),*options,'--extract-path',str(out),str(ref/'test/aaf'/file)],capture_output=True,check=True)
   for digest,name in args[2]:assert hashlib.md5((out/name).read_bytes()).hexdigest()==digest,(file,name)
  passed+=1
print(f'{passed} pinned upstream cases passed (65 analysis comparisons + 6 extraction hashes)')
