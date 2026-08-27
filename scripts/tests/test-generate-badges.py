#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "generate-badges.py"


class GenerateBadgesTests(unittest.TestCase):
    def run_generator(self, payload, *, check=False, exit_code=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "badges").mkdir()
            (root / "badges/ripr-plus.json").write_text(json.dumps({"schemaVersion": 1, "label": "ripr+", "message": "0", "color": "brightgreen"}, indent=2) + "\n")
            local_script = root / "scripts/generate-badges.py"
            local_script.parent.mkdir()
            local_script.write_bytes(SCRIPT.read_bytes())
            fake = root / "ripr"
            fake.write_text("#!/usr/bin/env python3\nimport json\n" f"print(json.dumps({json.dumps(payload)}))\n" f"raise SystemExit({exit_code})\n")
            fake.chmod(0o755)
            command = ["python3", str(local_script)] + (["--check"] if check else [])
            result = subprocess.run(command, env={**os.environ, "RIPR_BIN": str(fake)}, capture_output=True, text=True)
            output = (root / "badges/ripr-plus.json").read_text() if (root / "badges/ripr-plus.json").exists() else None
            return result, output

    def test_nonzero_counts_are_yellow(self):
        result, output = self.run_generator({"counts": {"unsuppressed_exposure_gaps": 3, "unsuppressed_test_efficiency_findings": 2}})
        self.assertEqual(result.returncode, 0, result.stderr)
        badge = json.loads(output)
        self.assertEqual((badge["message"], badge["color"]), ("5", "yellow"))

    def test_missing_counts_are_zero(self):
        result, output = self.run_generator({})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(output)["message"], "0")

    def test_invalid_payload_and_process_failure_fail_closed(self):
        self.assertNotEqual(self.run_generator([])[0].returncode, 0)
        self.assertNotEqual(self.run_generator({"counts": {}}, exit_code=7)[0].returncode, 0)

    def test_check_detects_drift(self):
        self.assertNotEqual(self.run_generator({"counts": {"unsuppressed_exposure_gaps": 1}}, check=True)[0].returncode, 0)


if __name__ == "__main__":
    unittest.main()
