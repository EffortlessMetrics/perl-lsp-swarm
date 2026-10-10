import json,os,stat,hashlib,subprocess,gzip
from pathlib import Path
base=Path('.spec/17479-nested-admission');raw=gzip.decompress((base/'helper-composed-metadata.json.gz').read_bytes());m=json.loads(raw);packages={p['id']:p for p in m['packages']};nodes={n['id']:n for n in m['resolve']['nodes']};members=set(m['workspace_members']);roots=[p['id'] for p in m['packages'] if p['name'] in ('perllsp','xtask','perl-ci-hygiene') and p['id'] in members];testroots={k for k in roots if packages[k]['name']!='perllsp'}
seen=set();todo=roots[:]
while todo:
 key=todo.pop()
 if key in seen:continue
 seen.add(key)
 for dep in nodes[key]['deps']:
  if any(kind['kind']!='dev' or key in testroots for kind in dep['dep_kinds']):todo.append(dep['pkg'])
closure=[{'id':key,'name':packages[key]['name'],'version':packages[key]['version'],'source':packages[key]['source'],'manifest_path':packages[key]['manifest_path'],'features':nodes[key]['features'],'repository_package':key in members} for key in sorted(seen)]
current={}
for p in m['packages']:
 if p['id'] in members and p['name'] in ('xtask','perl-ci-hygiene'):
  current[p['name']]=[{'name':t['name'],'kind':t['kind'],'src_path':str(Path(t['src_path']).relative_to(Path.cwd())),'features_required':t.get('required-features',[])} for t in p['targets'] if t['test'] and t['kind'] in (['lib'],['bin'],['test'])]
old=json.loads((base/'helper-source-map.json').read_text())['declared_targets'];assert current==old,(current.keys(),old.keys())
plan=json.loads((base/'helper-resource-plan.json').read_text());resources=plan['resources'];identities=set();stats={};allfiles=[]
for name,value in resources.items():
 root=Path(value);unique=set();files=[];apparent=allocated=0
 if root.exists():
  for path in root.rglob('*'):
   st=path.lstat()
   if not stat.S_ISREG(st.st_mode):continue
   identity=(st.st_dev,st.st_ino)
   if identity in unique:continue
   unique.add(identity);apparent+=st.st_size;allocated+=st.st_blocks*512
   files.append((str(path),st.st_size,st.st_blocks*512,identity,st.st_mode))
 identities.update(unique);allfiles.extend(files)
 stats[name]={'root':value,'exists':root.exists(),'regular_unique_inodes':len(unique),'apparent_bytes':apparent,'allocated_bytes':allocated}
combined={row[3]:row for row in allfiles};allocated=sum(row[2] for row in combined.values());apparent=sum(row[1] for row in combined.values())
executables=[]
for path,size,blocks,inode,mode in allfiles:
 if not mode & 0o111 or not '/debug/' in path or '/registry/' in path:continue
 try:
  with open(path,'rb') as f:magic=f.read(4)
 except OSError:continue
 if magic==b'\x7fELF':executables.append({'path':path,'apparent_bytes':size,'allocated_bytes':blocks})
maptargets=[]
for pkg,targets in current.items():
 for t in targets:
  for test in ([True,False] if t['kind']==['bin'] else [True]):
   directory=Path(resources['build'])/'debug/deps' if test else Path(resources['target'])/'debug'
   glob=t['name'].replace('-','_')+'-*' if test else t['name']
   candidates=[str(p) for p in directory.glob(glob) if p.is_file() and os.access(p,os.X_OK)]
   maptargets.append({**t,'package':pkg,'test_profile':test,'required_directory':str(directory),'retained_candidates':candidates,'current_artifact_receipt':None,'qualification':'MISSING_CURRENT_COMPILE_HANDOFF'})
assert len(maptargets)==309
fs=os.statvfs(resources['target']);free=fs.f_bavail*fs.f_frsize;total=fs.f_blocks*fs.f_frsize;maxexe=max((r['apparent_bytes'] for r in executables),default=0)
# Sensitivity, deliberately not a budget: existing executables are stale and
# unknown artifacts can exceed the largest measured executable.
scenario_missing=sum(not r['retained_candidates'] for r in maptargets)
scenario_all=309*maxexe;reserve=40*1024**3
out={'schema_version':1,'source_head_before_new_fix':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'metadata_sha256':hashlib.sha256(raw).hexdigest(),'metadata_scope':'offline locked entire-workspace feature-union graph: conservative selected normal/build closure and selected-test-root dev closure, platform-inclusive; not exact Cargo unit graph','closure_packages':closure,'closure_count':len(closure),'closure_repository_count':sum(p['repository_package'] for p in closure),'fixture_closure':'field std only; parking_lot0.12.5+arc_lock fixture closure and serde1.0.229/serde_json1.0.151 RPC generator closure are also source-mapped; cannot adopt old generated lock under fresh owner','resources':stats,'deduplicated_across_resource_roots':{'apparent_bytes':apparent,'allocated_bytes':allocated,'regular_unique_inodes':len(combined)},'filesystem':{'capacity_bytes':total,'free_bytes':free,'default_required_free_bytes':reserve,'default_shortfall_bytes':max(0,reserve-free)},'declared_runtime_harness_count':277,'normal_helper_bin_count':32,'helper_executable_inventory':maptargets,'all_309_current_artifact_receipts_missing':True,'retained_candidate_missing_count':scenario_missing,'largest_measured_default_debug_executable_bytes':maxexe,'retained_executables':executables,'uncertified_relink_sensitivity':{'all309_at_largest_measured_bytes':scenario_all,'plus_unchanged40GiB_reserve_bytes':scenario_all+reserve,'shortfall_vs_free_bytes':max(0,scenario_all+reserve-free),'not_a_growth_bound':'largest observed artifact need not bound unbuilt binaries; no reuse credited, no dependency growth/link-overlap/copy/temp allowance proven'},'aggregate_budget_file':None,'disposition':'BLOCKED: default reserve exceeds free capacity; all current bridge receipts absent; full growth and concurrent-link/temp/copy/memory basis unqualified. No native full route launch.'}
(base/'helper-composed-resource-census.json').write_text(json.dumps(out,indent=2)+'\n');(base/'helper-composed-target-inventory.json').write_text(json.dumps({'runtime_targets':current,'artifacts':maptargets},indent=2)+'\n');print(json.dumps({key:out[key] for key in ['closure_count','closure_repository_count','filesystem','retained_candidate_missing_count','largest_measured_default_debug_executable_bytes','uncertified_relink_sensitivity']}))
