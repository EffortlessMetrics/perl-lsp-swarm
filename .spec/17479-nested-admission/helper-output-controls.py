import hashlib,os,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path.cwd();TEMP=Path('.spec/17479-nested-admission/helper-resource-plan.json');import json
PRIVATE=Path(json.loads(TEMP.read_text())['resources']['temp'])

def snapshot(root):
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()} if root.exists() else {}

class Outputs(unittest.TestCase):
 def test_actual_support_projection_uses_explicit_private_outputs_and_preserves_checkout(self):
  before=snapshot(ROOT/'target')
  with tempfile.TemporaryDirectory(dir=PRIVATE) as directory:
   destination=Path(directory);policy=destination/'support.toml';docs=destination/'support.md'
   r=subprocess.run(['python3','-B','scripts/zed_dap_asset_receipts.py','project-dap-support','--policy-output',str(policy),'--docs-output',str(docs)],capture_output=True,text=True,timeout=30)
   self.assertEqual(r.returncode,0,r.stderr)
   self.assertEqual(policy.read_bytes(),(ROOT/'policy/zed-dap-support.toml').read_bytes())
   self.assertEqual(docs.read_bytes(),(ROOT/'docs/EDITORS/ZED_DAP_SUPPORT.md').read_bytes())
  self.assertEqual(snapshot(ROOT/'target'),before)
 def test_explicit_py_compile_emits_private_cache_and_preserves_source_caches(self):
  caches=[*ROOT.glob('scripts/__pycache__'),*ROOT.glob('scripts/zed_host/__pycache__')];before={str(p):snapshot(p) for p in caches}
  with tempfile.TemporaryDirectory(dir=PRIVATE) as directory:
   env={**os.environ,'PYTHONPYCACHEPREFIX':directory}
   r=subprocess.run(['python3','-B','-m','py_compile','scripts/zed_exact_source_prepare.py','scripts/zed_exact_source_launch.py','scripts/zed_exact_source_finalize.py','scripts/zed_host/common.py','scripts/zed_host/prepare.py','scripts/zed_host/process.py','scripts/zed_host/finalize.py'],env=env,capture_output=True,text=True,timeout=30)
   self.assertEqual(r.returncode,0,r.stderr);self.assertGreaterEqual(len(list(Path(directory).rglob('*.pyc'))),7)
  self.assertEqual({str(p):snapshot(p) for p in caches},before)

unittest.main()
