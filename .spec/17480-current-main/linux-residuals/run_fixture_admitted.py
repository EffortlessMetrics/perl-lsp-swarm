import json,subprocess,sys,time,shutil
from pathlib import Path
proof=Path('/workspace/perl-helper-current-proof'); label=sys.argv[1]; args=sys.argv[2:]; wrapper='./scripts/cargo-admitted'
pre=subprocess.run([wrapper,'--preflight',*args],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
(proof/(label+'-preflight.log')).write_text(pre.stderr)
line=next(x for x in pre.stderr.splitlines() if x.startswith('cargo-admitted preflight: '));report=json.loads(line.split(': ',1)[1]);doc={'schema_version':1,'scope':report['scope'],'reserve_bytes':8*1024**3,'expected_growth_bytes':2*1024**3,'basis':str(proof/'fixture-test-budget-basis.txt')};budget=proof/(label+'-budget.json');budget.write_text(json.dumps(doc,indent=2)+'\n')
pre2=subprocess.run([wrapper,'--preflight','--budget-file',str(budget),*args],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
(proof/(label+'-admitted-preflight.log')).write_text(pre2.stderr)
if pre2.returncode:print(pre2.stderr);raise SystemExit(pre2.returncode)
start=time.monotonic();before=shutil.disk_usage('/workspace').free
with (proof/(label+'.stdout')).open('w') as out,(proof/(label+'.log')).open('w') as err:r=subprocess.run([wrapper,'--budget-file',str(budget),*args],stdout=out,stderr=err)
measurement={'exit':r.returncode,'elapsed_seconds':time.monotonic()-start,'free_before':before,'free_after':shutil.disk_usage('/workspace').free,'resources':report['scope']['resources']}
(proof/(label+'-measurement.json')).write_text(json.dumps(measurement,indent=2)+'\n');print(label,measurement,flush=True)
raise SystemExit(r.returncode)
