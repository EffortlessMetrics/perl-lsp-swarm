"""Policy adapter execution controls; mocks are not canonical domain proof."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ImportVerificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.receipt = self.base / "rust-small-proof.json"
        self.receipt.write_text(json.dumps({"result": "success"}))
        cargo = self.base / "cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json,os,sys
assert not any(key in os.environ for key in ('GH_TOKEN','GITHUB_TOKEN','GH_ENTERPRISE_TOKEN','GITHUB_ENTERPRISE_TOKEN'))
assert os.environ['RUSTUP_TOOLCHAIN']=='1.95.0'
assert os.environ['RUSTUP_AUTO_INSTALL']=='0'
assert os.environ['CARGO_BUILD_JOBS']=='2'
assert os.environ['CARGO_TARGET_DIR']==os.environ['RUNNER_TEMP']+'/perl-receipt-validator-target'
if sys.argv[1:]==['--version']:
    print('cargo 1.95.0')
    sys.exit(0)
with open(os.environ['VERIFY_CAPTURE'],'w') as stream: json.dump(sys.argv[1:],stream)
sys.exit(int(os.environ.get('VERIFY_EXIT','0')))
""")
        cargo.chmod(0o700)
        self.env = dict(os.environ, PATH=str(self.base) + os.pathsep + os.environ["PATH"],
                        EM_CI_ARTIFACT_DIR=str(self.base), RUNNER_TEMP=str(self.base),
                        VERIFY_CAPTURE=str(self.base / "argv.json"),
                        GH_TOKEN="test-only-secret", GITHUB_TOKEN="test-only-secret",
                        GH_ENTERPRISE_TOKEN="test-only-secret", GITHUB_ENTERPRISE_TOKEN="test-only-secret")

    def run_adapter(self, **changes):
        return subprocess.run(["bash", str(ROOT / "scripts/ci/verify_rust_small_import.sh")],
                              env={**self.env, **changes}, capture_output=True, text=True)

    def test_owner_command_is_exact_and_cargo_receives_no_metadata_credentials(self):
        result = self.run_adapter()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.base / "argv.json").read_bytes()),
                         ["run", "-p", "xtask", "--locked", "--", "rust-small-proof",
                          "--verify-receipt", str(self.receipt)])

    def test_owner_refusal_blocks_policy(self):
        self.assertEqual(self.run_adapter(VERIFY_EXIT="23").returncode, 23)

    def test_coherent_diagnostic_receipt_cannot_make_policy_green(self):
        self.receipt.write_text(json.dumps({"result": "product_failure"}))
        self.assertNotEqual(self.run_adapter().returncode, 0)

    def test_missing_receipt_prevents_cargo(self):
        self.receipt.unlink()
        self.assertNotEqual(self.run_adapter().returncode, 0)
        self.assertFalse((self.base / "argv.json").exists())

    def test_linked_receipt_prevents_cargo(self):
        original = self.receipt.with_name("original.json")
        self.receipt.rename(original)
        self.receipt.symlink_to(original)
        self.assertNotEqual(self.run_adapter().returncode, 0)
        self.assertFalse((self.base / "argv.json").exists())


if __name__ == "__main__":
    unittest.main()
