import json,subprocess,time,shutil
from pathlib import Path
p=Path('/workspace/perl-helper-current-proof')
pre=json.loads((p/'selection71-dev-preflight.json').read_text())
args=json.loads((p/'selection71-dev-source-inputs.json').read_text())['argv']
basis=p/'selection71-dev-budget-basis.txt'
basis.write_text('Exact71 default cargo-test configuration; root profiles/config/xtask manifest unchanged. Existing owned default dev324library outputs and37executables remain present. Cargo validates reuse. Hygiene adds existingworkspace serde/sha2 edges; feature unification may invalidate units. Previous finite devtestbuild added2426773504bytes including ten selectedtest artifacts and30normalbins. This narrower request selects3testharnesses but maycompileadditionalnormalbins;4GiB additional growth is conservative warm estimate, not proven upper bound. Reserve8GiB, jobs2; admission retains current capacity guard and native descendant closure. No profile copying/relabeling.\n')
budget={'schema_version':1,'scope':pre['scope'],'reserve_bytes':8*1024**3,'expected_growth_bytes':4*1024**3,'basis':str(basis)}
f=p/'selection71-dev-budget.json';f.write_text(json.dumps(budget,indent=2)+'\n')
r=subprocess.run(['./scripts/cargo-admitted','--preflight','--budget-file',str(f),*args],capture_output=True,text=True)
(p/'selection71-dev-admitted-preflight.log').write_text(r.stderr)
assert r.returncode==0,r.stderr
before=shutil.disk_usage('/workspace').free;start=time.monotonic()
with (p/'selection71-dev-no-run.stdout').open('w') as out,(p/'selection71-dev-no-run.log').open('w') as err:
 r=subprocess.run(['./scripts/cargo-admitted','--budget-file',str(f),*args],stdout=out,stderr=err)
(p/'selection71-dev-no-run-measurement.json').write_text(json.dumps({'exit':r.returncode,'seconds':time.monotonic()-start,'free_before':before,'free_after':shutil.disk_usage('/workspace').free,'source':'71a69a477665e98dd11d1be163a4d08cea51ef64','profile':'default cargo-test/dev','resources':pre['scope']['resources']},indent=2)+'\n')
print('Build completed',r.returncode,flush=True)
raise SystemExit(r.returncode)
