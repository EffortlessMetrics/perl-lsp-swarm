"""The selected Linux child-discovery profile must work before allocation."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("cargo_admitted_discovery", ROOT / "scripts/cargo_admitted.py")
owner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(owner)


class DiscoveryAdmissionTests(unittest.TestCase):
    def test_unavailable_profile_refuses_before_planning_for_execution_and_preflight(self):
        for mode in ("proc-children", "proc-parent-pidfd"):
            for prefix in ([], ["--preflight"]):
                with self.subTest(mode=mode, prefix=prefix), \
                     patch.dict(os.environ, {"CARGO_ADMITTED_CHILD_DISCOVERY": mode}, clear=True), \
                     patch.object(owner.sys, "platform", "linux"), \
                     patch.object(owner, "child_discovery_capability", side_effect=owner.Denied("missing discovery")) as probe, \
                     patch.object(owner, "resource_plan") as plan, \
                     patch.object(owner.subprocess, "call") as cargo, \
                     contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(owner.main(prefix + ["test", "-p", "leaf", "--lib"]), 75)
                    probe.assert_called_once_with(mode)
                    plan.assert_not_called()
                    cargo.assert_not_called()

    def test_supported_profile_preflight_reaches_capacity_and_reports_pass(self):
        for mode in ("proc-children", "proc-parent-pidfd"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                slot = root / "slot"
                paths = {name: slot / name for name in ("target", "build", "cargo_home", "temp")}
                stderr = io.StringIO()
                with patch.dict(os.environ, {"CARGO_ADMITTED_CHILD_DISCOVERY": mode}, clear=True), \
                     patch.object(owner.sys, "platform", "linux"), \
                     patch.object(owner, "child_discovery_capability", return_value={"mode": mode}) as probe, \
                     patch.object(owner, "resource_plan", return_value=(root, slot, paths)), \
                     patch.object(owner, "check_capacity", return_value={"policy": "fixture"}) as capacity, \
                     patch.object(owner.subprocess, "call") as cargo, \
                     contextlib.redirect_stderr(stderr):
                    self.assertEqual(owner.main(["--preflight", "test", "-p", "leaf", "--lib"]), 0)
                report = json.loads(stderr.getvalue().split("cargo-admitted preflight: ", 1)[1])
                self.assertEqual(report["verdict"], "PASS")
                self.assertFalse(report["lease_acquired"])
                probe.assert_called_once_with(mode)
                capacity.assert_called_once()
                cargo.assert_not_called()
                self.assertFalse(slot.exists())

    def test_default_builtin_on_non_linux_does_not_probe_linux_discovery(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(owner.sys, "platform", "win32"), \
             patch.object(owner, "child_discovery_capability") as probe, \
             patch.object(owner, "resource_plan", side_effect=owner.Denied("plan reached")) as plan, \
             contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(owner.main(["test", "-p", "leaf", "--lib"]), 75)
        probe.assert_not_called()
        plan.assert_called_once()


if __name__ == "__main__":
    unittest.main()
