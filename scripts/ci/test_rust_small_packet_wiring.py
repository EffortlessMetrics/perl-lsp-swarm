"""Actual proof script/exporter integration with mocked product execution."""
import json
import os
from pathlib import Path
import subprocess
import unittest

from scripts.ci import test_export_rust_small_receipt as receipt_fixture

ROOT = Path(__file__).resolve().parents[2]


class PacketWiringTests(unittest.TestCase):
    def setUp(self):
        self.fixture = receipt_fixture.ExportTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.base = self.fixture.base
        self.log = self.base / "cargo-argv"
        cargo = self.base / "cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json,os,pathlib,sys
with open(os.environ['PACKET_TEST_LOG'],'a') as stream: stream.write(' '.join(sys.argv[1:])+'\\n')
if sys.argv[1]=='fmt': sys.exit(int(os.environ.get('FMT_STATUS','0')))
receipt=pathlib.Path(os.environ['GITHUB_WORKSPACE'])/'target/receipts/rust-small-proof.json'
data=json.loads(receipt.read_bytes())
data['result']='success' if os.environ.get('PRODUCT_STATUS','0')=='0' else 'product_failure'
if os.environ.get('BAD_RECEIPT')=='true': data['subject']['git_sha']='b'*40
receipt.write_text(json.dumps(data))
sys.exit(int(os.environ.get('PRODUCT_STATUS','0')))
""")
        cargo.chmod(0o700)
        self.env = {**os.environ, **self.fixture.env, "EM_CI_LANE": "perl-rust-small",
                    "PATH": str(self.base) + os.pathsep + os.environ["PATH"],
                    "PACKET_TEST_LOG": str(self.log), "GIT_CONFIG_GLOBAL": str(self.base / "gitconfig")}

    def run_proof(self, **changes):
        return subprocess.run(["bash", ".ci/rust-standard-proof.sh"], cwd=ROOT,
                              env={**self.env, **changes}, capture_output=True, text=True)

    def test_success_exports_exact_current_product_receipt(self):
        result = self.run_proof()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.fixture.root / "rust-small-proof.json").read_bytes(),
                         self.fixture.source.read_bytes())
        self.assertEqual(self.log.read_text().splitlines(),
                         ["fmt --all -- --check", "run -p perl-rust-small-verifier --bin perl-rust-small-proof --locked"])

    def test_product_failure_exports_diagnostic_without_swallowing_status(self):
        result = self.run_proof(PRODUCT_STATUS="23")
        self.assertEqual(result.returncode, 23, result.stderr)
        manifest = json.loads((self.fixture.root / "perl-rust-small-export.json").read_bytes())
        self.assertEqual(manifest["result"], "product_failure")

    def test_export_failure_cannot_make_product_success_green(self):
        result = self.run_proof(BAD_RECEIPT="true")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_export_failure_preserves_original_product_failure(self):
        self.assertEqual(self.run_proof(PRODUCT_STATUS="23", BAD_RECEIPT="true").returncode, 23)

    def test_formatter_failure_never_exports_or_enters_product(self):
        self.assertEqual(self.run_proof(FMT_STATUS="17").returncode, 17)
        self.assertEqual(self.log.read_text().splitlines(), ["fmt --all -- --check"])
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_unrelated_lane_refuses_before_product(self):
        self.assertEqual(self.run_proof(EM_CI_LANE="ripr").returncode, 64)
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
