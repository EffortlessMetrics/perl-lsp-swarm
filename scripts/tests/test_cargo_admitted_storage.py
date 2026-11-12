import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import shutil
import stat
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("cargo_admitted", ROOT / "scripts/cargo_admitted.py")
safe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safe)
NATIVE_TREE = safe.ClippyTree
NATIVE_LAUNCH = safe.call_clippy


def git_output(path):
    return os.fsencode(path) + b"\n"


def projected_environment(command):
    # Decode only the renderer's JSON-compatible assignments, not Cargo files.
    values = {}
    for index, argument in enumerate(command):
        if argument == "--":
            break
        if argument != "--config" or not command[index + 1].startswith("env."):
            continue
        key, encoded = command[index + 1].split("=", 1)
        fields, value = key.split("."), json.loads(encoded)
        if len(fields) == 2:
            values[fields[1]] = value
        else:
            values.setdefault(fields[1], {})[fields[2]] = value
    return values


class ChildEnvironmentProjectionTests(unittest.TestCase):
    def setUp(self):
        self.values = {name: "/admitted/" + name for name in (
            "TEMP", "TMP", "TMPDIR", "RUSTUP_AUTO_INSTALL", "CARGO_TARGET_DIR",
            "CARGO_BUILD_BUILD_DIR", "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS")}
        self.values.update(RUSTUP_AUTO_INSTALL="0", CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="3")
        self.prefix = ["cargo", "--config", "/fixture/config.local.toml"]
        self.tree = Mock()

    def project(self):
        return safe.child_environment_config(self.prefix, self.values, self.values, self.tree)

    def oracle(self, shapes, malformed=False):
        def probe(command, env, **bounds):
            self.assertEqual(command[:len(self.prefix)], self.prefix)
            self.assertEqual(command[-2:], ["locate-project", "--message-format=json"])
            self.assertGreater(bounds["timeout"], 0)
            self.assertLessEqual(bounds["timeout"], 15)
            self.assertLessEqual(bounds["output_limit"], 16384)
            projection = projected_environment(command)
            if malformed or any(name in shapes and type(value) is not shapes[name]
                                for name, value in projection.items()):
                raise subprocess.CalledProcessError(101, command, b"", b"fixture: incompatible config")
            return subprocess.CompletedProcess(command, 0, b'{"root":"/fixture/Cargo.toml"}', b"")
        return probe

    def test_absent_table_scalar_and_mixed_project_every_admitted_value(self):
        mixed = {name: str if i % 2 else dict for i, name in enumerate(self.values)}
        for label, shapes, calls in (
                ("absent", {}, 1), ("table", dict.fromkeys(self.values, dict), 1),
                ("scalar", dict.fromkeys(self.values, str), 2),
                ("mixed", mixed, 3 + len(self.values) + sum(shape is str for shape in mixed.values()))):
            with self.subTest(shape=label):
                self.tree.probe.reset_mock()
                self.tree.probe.side_effect = self.oracle(shapes)
                projection = projected_environment(self.project())
                expected = {name: value if shapes.get(name) is str else
                            {"value": value, "force": True, "relative": False}
                            for name, value in self.values.items()}
                self.assertEqual(projection, expected)
                self.assertEqual(self.tree.probe.call_count, calls)
                # The accepted combined vector is the last probe, and is reused.
                self.assertEqual(projected_environment(self.tree.probe.call_args.args[0]), projection)

    def test_malformed_config_refuses_after_first_key_has_no_supported_shape(self):
        self.tree.probe.side_effect = self.oracle({}, malformed=True)
        with self.assertRaisesRegex(safe.CargoConfigRefused, "neither supported configuration shape.*TEMP"):
            self.project()
        self.assertEqual(self.tree.probe.call_count, 4)

    def test_fixed_key_set_bounds_attempts_and_final_combined_validation(self):
        for final_success in (True, False):
            with self.subTest(final_success=final_success):
                self.tree.probe.reset_mock()
                def probe(command, env, **bounds):
                    projected = projected_environment(command)
                    if (self.tree.probe.call_count <= 2 or
                            any(isinstance(value, dict) for value in projected.values()) or
                            (len(projected) > 1 and not final_success)):
                        raise subprocess.CalledProcessError(101, command, b"", b"fixture: shape changed")
                    return subprocess.CompletedProcess(command, 0, b"{}", b"")
                self.tree.probe.side_effect = probe
                if final_success:
                    self.assertEqual(projected_environment(self.project()), self.values)
                else:
                    with self.assertRaisesRegex(safe.CargoConfigRefused, "combined configuration projection rejected"):
                        self.project()
                self.assertEqual(self.tree.probe.call_count, 2 * len(self.values) + 3)

    def test_unexpected_status_timeout_output_failure_and_cancellation_do_not_search_shapes(self):
        for error in (subprocess.CalledProcessError(7, ["fixture"], b"", b"unexpected"),
                      subprocess.CalledProcessError(-9, ["fixture"], b"", b"signal"),
                      subprocess.TimeoutExpired(["fixture"], 15),
                      safe.Denied("output observation bound"), KeyboardInterrupt()):
            with self.subTest(error=type(error).__name__, status=getattr(error, "returncode", None)):
                self.tree.probe.reset_mock()
                self.tree.probe.side_effect = error
                expected = KeyboardInterrupt if isinstance(error, KeyboardInterrupt) else safe.CargoConfigRefused
                with self.assertRaises(expected):
                    self.project()
                self.assertEqual(self.tree.probe.call_count, 1)

    def test_deadline_and_output_budget_cover_all_shape_probes(self):
        clock = [100.0]
        def slow(command, env, **bounds):
            clock[0] += 16
            return subprocess.CompletedProcess(command, 0, b"{}", b"")
        self.tree.probe.side_effect = slow
        with patch.object(safe.time, "monotonic", side_effect=lambda: clock[0]), \
             self.assertRaisesRegex(safe.CargoConfigRefused, "observation bound exhausted"):
            self.project()
        self.assertEqual(self.tree.probe.call_count, 1)

        self.tree.probe.reset_mock()
        def noisy(command, env, **bounds):
            if self.tree.probe.call_count <= 2:
                raise subprocess.CalledProcessError(101, command, b"", b"x" * 16384)
            return subprocess.CompletedProcess(command, 0, b"x" * 16384, b"")
        self.tree.probe.side_effect = noisy
        with self.assertRaisesRegex(safe.CargoConfigRefused, "observation bound exhausted"):
            self.project()
        self.assertEqual(self.tree.probe.call_count, 4)


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        # These fixtures mock the launch/owner; native discovery is tested separately.
        # Synthetic resource plans have no Git allocation subject. Real creation
        # provenance is exercised below through the production planner/entrypoint.
        self.enterContext(patch.object(safe, "PrivateAllocation", return_value=Mock(
            receipt={"status": "unknown", "reason": "synthetic_admission_fixture"})))
        self.discovery_fixture = patch.object(safe, "child_discovery_capability", return_value={})
        self.discovery_fixture.start()
        self.addCleanup(self.discovery_fixture.stop)
        self.temp = tempfile.TemporaryDirectory(prefix="cargo-admitted-proof-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.slot = self.root / "slot"
        self.paths = {"target": self.slot / "target", "build": self.slot / "build",
                      "cargo_home": self.root / "home", "temp": self.root / "temp"}
        self.env = {"MIN_FREE_GB": "0.001", "MAX_USED_PCT": "100"}
        # Storage policy fixtures substitute the process seam; actual Linux
        # ownership is independently exercised by test_cargo_admitted_builtin_tree.
        self.tree = Mock()
        self.tree.probe.return_value = subprocess.CompletedProcess([], 0, b"{}\n", b"")
        self.tree.finish.return_value = {"tree_settled": True, "cancelled": False}
        self.enterContext(patch.object(safe, "ClippyTree", return_value=self.tree))
        self.enterContext(patch.object(safe, "call_clippy", side_effect=
            lambda command, env, lock, tree, **kw: safe.subprocess.call(command, env=env)))

    def run_safe(self, args, call=None):
        with patch.dict(os.environ, self.env, clear=True), \
             patch.object(safe, "resource_plan", return_value=(self.root, self.slot, self.paths)), \
             patch.object(safe.subprocess, "run", return_value=
                          subprocess.CompletedProcess([], 0, b"", b"")), \
             patch.object(safe.subprocess, "call", side_effect=call or (lambda *a, **kw: 0)):
            return safe.main(args)

    def test_overrides_aliases_and_global_option_bypasses_refused_before_allocation(self):
        for args in [["+1.95", "build"], ["--config", "x", "build"], ["b"], ["nextest"],
                     ["clean"], ["clippy"], ["build", "--out-dir=x"], ["build", "--artifact-dir=x"],
                     ["build", "--build-dir=x"], ["test", "--manifest-path=x"], ["build", "--target-dir=x"],
                     ["build", "--config=x"], ["build", "-j8"], ["check", "-Zfoo"]]:
            self.assertEqual(self.run_safe(args), 75, args)
            self.assertFalse(self.slot.exists())

    def test_joined_short_tokens_refuse_before_allocation(self):
        for options in (["-vj8"], ["-qj8"], ["-vj", "8"], ["-v", "-j8"],
                        ["-vZunstable-options"], ["-vZ", "unstable-options"],
                        ["-vC/tmp"], ["-vC", "/tmp"], ["-pfoo"], ["-Ffoo"]):
            self.assertEqual(self.run_safe(["build", *options]), 75, options)
            self.assertFalse(self.slot.exists())

    def test_local_config_cli_args_injected_when_present(self):
        seen = []

        def cargo(command, env):
            seen.append(command)
            return 0

        local_path = self.root / ".cargo" / "config.local.toml"
        local_path.parent.mkdir(parents=True, exist_ok=True)
        local_path.write_text("[build]\njobs = 1\n", encoding="utf-8")
        self.assertEqual(self.run_safe(["check"], cargo), 0)
        self.assertIn(str(local_path), seen[0])
        self.assertLess(seen[0].index(str(local_path)), seen[0].index("unstable.unstable-options=false"))

    def test_forced_child_environment_wins_over_local_config(self):
        local = self.root / ".cargo" / "config.local.toml"
        expected = {name: str(self.paths["temp"]) for name in ("TEMP", "TMP", "TMPDIR")}
        expected["RUSTUP_AUTO_INSTALL"] = "0"
        expected.update(CARGO_TARGET_DIR=str(self.paths["target"]),
                        CARGO_BUILD_BUILD_DIR=str(self.paths["build"]),
                        CARGO_INCREMENTAL="0")
        sentinel = {"value": "retained local value", "force": True, "relative": False}

        for shape, jobs in (("absent", "2"), ("table", "3"), ("scalar", "3"), ("mixed", "3")):
            expected["CARGO_BUILD_JOBS"] = jobs
            self.env["CARGO_BUILD_JOBS"] = jobs
            with self.subTest(local_config=shape, jobs=jobs):
                values = {}
                if shape != "absent":
                    local.parent.mkdir(exist_ok=True)
                    values = {name: "foreign-" + name if shape == "scalar" or (shape == "mixed" and i % 2)
                              else {"value": "foreign-" + name, "force": True, "relative": True}
                              for i, name in enumerate(expected)}
                    values["LOCAL_CONFIG_SENTINEL"] = sentinel
                    entries = []
                    for name, value in values.items():
                        if isinstance(value, str):
                            entries.append("env." + name + "=" + json.dumps(value))
                        else:
                            entries.extend("env." + name + "." + field + "=" + json.dumps(setting)
                                           for field, setting in value.items())
                    local.write_text("\n".join(entries) + "\n")

                def probe(command, env, **bounds):
                    projection = projected_environment(command)
                    if any(name in values and type(value) is not type(values[name])
                           for name, value in projection.items()):
                        raise subprocess.CalledProcessError(101, command, b"", b"fixture: incompatible config")
                    return subprocess.CompletedProcess(command, 0, b"{}", b"")
                self.tree.probe.side_effect = probe

                def cargo(command, env):
                    # Decode only this fixture's dotted, JSON-compatible TOML
                    # scalars, in actual --config order. Native Cargo separately
                    # establishes the merge rule; this is not a Cargo interpreter.
                    child_config = {}
                    for index, argument in enumerate(command):
                        if argument == "--":
                            break
                        if argument != "--config":
                            continue
                        value = command[index + 1]
                        entries = local.read_text().splitlines() if value == str(local) else [value]
                        for entry in entries:
                            if entry.startswith("env."):
                                key, encoded = entry.split("=", 1)
                                fields = key.split(".")
                                if len(fields) == 2:
                                    if fields[1] in child_config:
                                        self.assertIsInstance(child_config[fields[1]], str)
                                    child_config[fields[1]] = json.loads(encoded)
                                else:
                                    _, name, field = fields
                                    self.assertIsInstance(child_config.setdefault(name, {}), dict)
                                    child_config[name][field] = json.loads(encoded)
                    self.assertEqual(
                        {name: child_config.get(name) for name in expected},
                        {name: value if isinstance(values.get(name), str) else
                         {"value": value, "force": True, "relative": False}
                         for name, value in expected.items()})
                    self.assertEqual({name: env[name] for name in expected}, expected)
                    self.assertEqual(child_config.get("LOCAL_CONFIG_SENTINEL"), sentinel if shape != "absent" else None)
                    self.assertNotIn("CARGO_HOME", child_config)  # Cargo rejects this key in [env].
                    return 0

                self.assertEqual(self.run_safe(["check"], cargo), 0)

    @unittest.skipUnless(
        sys.platform == "linux",
        "setup-failure settlement uses the Linux owned-tree path",
    )
    def test_setup_failure_never_launches_product_and_uses_original_owner_settlement(self):
        import contextlib
        import io
        for error, probes, status in (
                (subprocess.CalledProcessError(101, ["fixture"], b"", b"malformed"), 4, 75),
                (subprocess.CalledProcessError(7, ["fixture"], b"", b"unexpected"), 1, 75),
                (subprocess.TimeoutExpired(["fixture"], 15), 1, 75),
                (safe.Denied("output observation bound"), 1, 75),
                (KeyboardInterrupt(), 1, 130)):
            with self.subTest(error=type(error).__name__, status=getattr(error, "returncode", None)):
                self.tree.reset_mock()
                self.tree.probe.side_effect = error
                product = Mock()
                output = io.StringIO()
                with contextlib.redirect_stderr(output):
                    self.assertEqual(self.run_safe(["check"], product), status)
                product.assert_not_called()
                self.assertEqual(self.tree.probe.call_count, probes)
                self.tree.finish.assert_called_once_with(stop=True)
                self.assertFalse((self.slot / "cargo-active").exists())
                self.assertNotIn("Cargo product:", output.getvalue())

    def test_preflight_never_runs_configuration_or_product_probe(self):
        product = Mock()
        self.assertEqual(self.run_safe(["--preflight", "check"], product), 0)
        self.tree.probe.assert_not_called()
        self.tree.finish.assert_not_called()
        product.assert_not_called()
        self.assertFalse(self.slot.exists())

    def test_separate_short_values_and_program_arguments_remain_supported(self):
        seen = []
        def cargo(command, env):
            seen.append(command)
            return 0
        args = ["test", "-v", "-vv", "-vvv", "-p", "foo", "-F", "bar", "--", "-vj8", "--config=program-value"]
        self.assertEqual(self.run_safe(args, cargo), 0)
        self.assertEqual(seen[0][-len(args[1:]):], args[1:])

    def test_template_resource_paths_refuse_before_allocation(self):
        for component in ("cache-{workspace-path-hash}", "cache-}"):
            destination = self.root / component
            env = dict(self.env, DEVPLANE=str(destination), HOME=str(self.root), USERPROFILE=str(self.root))
            with patch.dict(os.environ, env, clear=True), \
                 patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo")), \
                 patch.object(safe.subprocess, "call") as cargo:
                self.assertEqual(safe.main(["check"]), 75)
                cargo.assert_not_called()
            self.assertFalse(destination.exists())
        for name in ("CARGO_HOME", "TMPDIR"):
            destination = self.root / (name + "-{workspace-root}")
            env = dict(self.env, DEVPLANE=str(self.slot), HOME=str(self.root), USERPROFILE=str(self.root), **{name: str(destination)})
            with patch.dict(os.environ, env, clear=True), \
                 patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo")), \
                 patch.object(safe.subprocess, "call") as cargo:
                self.assertEqual(safe.main(["check"]), 75)
                cargo.assert_not_called()
            self.assertFalse(destination.exists())
            self.assertFalse(self.slot.exists())

    def test_low_disk_on_each_effective_resource_prevents_any_allocation(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        # Distinct existing ancestor per effective volume; slot remains absent.
        volumes = [self.root / str(i) for i in range(5)]
        for path in volumes:
            path.mkdir()
        self.slot = volumes[0] / "slot"
        self.paths = {key: volumes[i + 1] / "new" for i, key in enumerate(self.paths)}
        for denied in volumes:
            seen = []
            def disk(path):
                seen.append(path)
                return Usage(1000000000, 1000000000, 0) if path == denied else Usage(1000000000, 0, 1000000000)
            with patch.object(safe.shutil, "disk_usage", side_effect=disk):
                self.assertEqual(self.run_safe(["build"]), 75)
            self.assertIn(denied, seen)
            self.assertFalse(self.slot.exists())
            self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_explicit_byte_budget_accepts_large_nearly_full_volume_without_double_count(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
               "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:aggregate-upper-bound"}
        # Five destinations on one 4-TiB volume, 80 GiB free (~98% used).
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4016*gib, 80*gib)):
            report = safe.check_capacity([self.slot, *self.paths.values()], env)
            self.assertEqual(report["policy"], "byte-budget")
            self.assertEqual(report["reserve_bytes"], 40*gib)
            self.assertEqual(report["expected_total_growth_bytes"], 20*gib)
            self.assertEqual(report["evidence_reference"], env["CARGO_STORAGE_BUDGET_EVIDENCE"])
            self.assertEqual(len(report["destinations"]), 5)
            self.assertTrue(all(row["required_free_bytes"] == 60*gib for row in report["destinations"]))
            self.assertTrue(all(row["headroom_bytes"] == 20*gib for row in report["destinations"]))
            # No opt-in: the inherited percentage policy still refuses.
            with self.assertRaises(safe.Denied):
                safe.check_capacity([self.slot], {})

    def test_byte_budget_exact_boundary_and_overflow(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
               "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:boundary"}
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4036*gib, 60*gib)):
            report = safe.check_capacity([self.slot], env)
            self.assertEqual(report["destinations"][0]["headroom_bytes"], 0)
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4036*gib+1, 60*gib-1)):
            with self.assertRaises(safe.Denied):
                safe.check_capacity([self.slot], env)
        # Each byte operand is finite; their sum overflows.
        self.env = dict(env, MIN_FREE_GB="1e299", CARGO_EXPECTED_GROWTH_GB="1e299")
        self.assertEqual(self.run_safe(["build"]), 75)
        self.assertFalse(self.slot.exists())

    def test_percent_ceiling_names_max_used_pct_not_zero_shortfall(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        # 356 GiB free is far above the 40 GiB floor, but 91.3% used >= 85%.
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 3740*gib, 356*gib)):
            with self.assertRaises(safe.Denied) as caught:
                safe.check_capacity([self.slot], {})
        message = str(caught.exception)
        self.assertIn("MAX_USED_PCT", message)
        self.assertIn("max_used_pct=85", message)
        self.assertNotIn("insufficient disk headroom", message)
        self.assertNotIn("shortfall_bytes=0", message)
        self.assertNotIn("shortfall_bytes", caught.exception.details)
        self.assertEqual(caught.exception.details["max_used_pct"], 85.0)
        self.assertGreater(caught.exception.details["pct_shortfall_bytes"], 0)
        self.assertEqual(caught.exception.details["additional_free_bytes_needed"],
                         caught.exception.details["pct_shortfall_bytes"])

    def test_percent_ceiling_exact_boundary_reports_one_byte_remedy(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        # Exactly 85% used: gate denies, and the remedy must be 1 byte, not 0.
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(1000*gib, 850*gib, 150*gib)):
            with self.assertRaises(safe.Denied) as caught:
                safe.check_capacity([self.slot], {})
        self.assertEqual(caught.exception.details["additional_free_bytes_needed"], 1)
        # Freeing the reported amount makes the same gate pass.
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(1000*gib, 850*gib - 1, 150*gib + 1)):
            report = safe.check_capacity([self.slot], {})
        self.assertIn("destinations", report)

    def test_absolute_floor_still_names_headroom_with_positive_shortfall(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        env = {"MIN_FREE_GB": "40", "MAX_USED_PCT": "100"}
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4086*gib, 10*gib)):
            with self.assertRaises(safe.Denied) as caught:
                safe.check_capacity([self.slot], env)
        message = str(caught.exception)
        self.assertIn("insufficient disk headroom", message)
        self.assertIn("shortfall_bytes=", message)
        self.assertNotIn("shortfall_bytes=0", message)
        self.assertEqual(caught.exception.details["shortfall_bytes"], 30*gib)

    def test_byte_budget_is_in_emitted_resource_descriptor(self):
        import contextlib
        import io
        import json
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        self.env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                    "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:evidence-reference-only"}
        output = io.StringIO()
        with patch.object(safe.shutil, "disk_usage", return_value=Usage(4096*gib, 4016*gib, 80*gib)), contextlib.redirect_stderr(output):
            self.assertEqual(self.run_safe(["check"]), 0)
        descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1].splitlines()[0])
        self.assertEqual(descriptor["admission"]["policy"], "byte-budget")
        self.assertEqual(descriptor["admission"]["evidence_reference"], self.env["CARGO_STORAGE_BUDGET_EVIDENCE"])
        self.assertTrue(all(row["headroom_bytes"] == 20*gib for row in descriptor["admission"]["destinations"]))

    def test_byte_budget_checks_every_destination_before_allocation(self):
        from collections import namedtuple
        Usage = namedtuple("Usage", "total used free")
        gib = 1024 ** 3
        self.env = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                    "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:all-destination-upper-bound"}
        volumes = [self.root / str(i) for i in range(5)]
        for path in volumes:
            path.mkdir()
        self.slot = volumes[0] / "slot"
        self.paths = {key: volumes[i + 1] / "new" for i, key in enumerate(self.paths)}
        for denied in volumes:
            seen = []
            def disk(path):
                seen.append(path)
                free = 60*gib - 1 if path == denied else 60*gib
                return Usage(4096*gib, 4096*gib-free, free)
            with patch.object(safe.shutil, "disk_usage", side_effect=disk):
                self.assertEqual(self.run_safe(["build"]), 75)
            self.assertIn(denied, seen)
            self.assertFalse(self.slot.exists())
            self.assertFalse(any(path.exists() for path in self.paths.values()))

    def test_byte_budget_invalid_or_missing_inputs_refuse_before_allocation(self):
        valid = {"CARGO_STORAGE_POLICY": "byte-budget", "CARGO_EXPECTED_GROWTH_GB": "20",
                 "CARGO_STORAGE_BUDGET_EVIDENCE": "fixture:bounded-growth"}
        cases = [{"CARGO_EXPECTED_GROWTH_GB": value} for value in ("", "NaN", "inf", "-inf", "0", "-1", "no")]
        cases += [{"CARGO_STORAGE_BUDGET_EVIDENCE": value} for value in ("", "  ")]
        cases += [{"MIN_FREE_GB": value} for value in ("39.99", "NaN", "inf", "0")]
        cases += [{"CARGO_STORAGE_POLICY": "unknown"}]
        for change in cases:
            self.env = dict(valid, **change)
            self.assertEqual(self.run_safe(["build"]), 75, change)
            self.assertFalse(self.slot.exists())
        for missing in ("CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE"):
            self.env = dict(valid)
            del self.env[missing]
            self.assertEqual(self.run_safe(["build"]), 75)
            self.assertFalse(self.slot.exists())

    def test_cancellation_retains_exclusive_resource_lease(self):
        def cancel(*args, **kw):
            raise KeyboardInterrupt()
        self.tree.finish.return_value = {"tree_settled": False}
        if sys.platform == "linux":
            self.assertEqual(self.run_safe(["run"], cancel), 75)
        else:
            with self.assertRaises(KeyboardInterrupt):
                self.run_safe(["run"], cancel)
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertEqual(self.run_safe(["test"]), 75)

    def test_serialized_reuse_and_fixed_both_paths(self):
        self.env["CARGO_UNSTABLE_UNSTABLE_OPTIONS"] = "true"
        seen = []
        def cargo(command, env):
            self.assertTrue((self.slot / "cargo-active").is_dir())
            self.assertEqual(self.run_safe(["check"]), 75)
            self.assertEqual(env["TEMP"], env["TMPDIR"])
            self.assertEqual(env["TMP"], env["TMPDIR"])
            self.assertEqual(env["CARGO_INCREMENTAL"], "0")
            self.assertIn("unstable.unstable-options=false", command)
            self.assertEqual(env["RUSTUP_AUTO_INSTALL"], "0")
            self.assertIn('env.RUSTUP_AUTO_INSTALL.value="0"', command)
            self.assertIn('env.RUSTUP_AUTO_INSTALL.force=true', command)
            self.assertIn('env.RUSTUP_AUTO_INSTALL.relative=false', command)
            self.assertTrue(any(arg.startswith("build.build-dir=") for arg in command))
            self.assertEqual(command[command.index("--target-dir") + 1], str(self.paths["target"]))
            seen.append(command)
            return 0
        for _ in range(2):
            self.assertEqual(self.run_safe(["test", "--", "--config=program-arg"], cargo), 0)
            self.assertFalse((self.slot / "cargo-active").exists())
        self.assertEqual(seen[0], seen[1])

    def test_cancel_or_failed_spawn_retains_lease_without_pid_stealing(self):
        def fail(*args, **kw):
            raise OSError("spawn failed")
        self.tree.finish.return_value = {"tree_settled": False}
        self.assertEqual(self.run_safe(["build"], fail), 75)
        self.assertTrue((self.slot / "cargo-active").exists())
        self.assertEqual(self.run_safe(["build"]), 75)

    def test_completed_failure_releases_lease_but_preserves_artifacts(self):
        def fail(*args, **kw):
            (self.paths["build"] / "evidence").write_text("retain")
            return 101
        self.assertEqual(self.run_safe(["build"], fail), 101)
        self.assertEqual((self.paths["build"] / "evidence").read_text(), "retain")
        self.assertFalse((self.slot / "cargo-active").exists())

    def test_abnormal_child_exit_retains_lease(self):
        self.tree.finish.return_value = {"tree_settled": False}
        for code in (-9, 3221225786, 9):
            # Fresh fixture slot for each termination status; never clear a
            # retained production lease just to make a retry pass.
            self.slot = self.root / ("terminated-" + str(code))
            self.paths = {name: self.slot / name for name in self.paths}
            self.assertEqual(self.run_safe(["test"], lambda *a, **kw: code),
                             75 if sys.platform == "linux" else code)
            self.assertTrue((self.slot / "cargo-active").exists())
            self.assertEqual(self.run_safe(["check"]), 75)

    def test_bounded_common_repository_identity_and_explicit_override_refusal(self):
        env = {"DEVPLANE": str(self.root), "CARGO_HOME": str(self.root / "cargo")}
        with patch.object(safe.subprocess, "check_output", return_value=git_output(self.root / "repo.git")):
            first = safe.resource_plan(env)
            self.assertEqual(first, safe.resource_plan(env))
            with self.assertRaises(safe.Denied):
                safe.resource_plan(dict(env, CARGO_TARGET_DIR=str(self.root / "arbitrary")))
        self.assertFalse(first[1].exists())

    def test_real_git_nested_main_and_linked_worktree_identity(self):
        import contextlib
        import io
        import json
        repo = self.root / "repo"
        linked = self.root / "linked"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1")
        def git(*args, cwd=self.root):
            subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True)
        git("init", "-b", "main", str(repo))
        git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--allow-empty", "-m", "fixture", cwd=repo)
        git("worktree", "add", "-b", "linked", str(linked), cwd=repo)
        env.update(self.env, DEVPLANE=str(self.root / "cache"),
                   CARGO_HOME=str(self.root / "cargo"), TMPDIR=str(self.root / "tmp"))
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(key, None)
        descriptors = []
        previous = Path.cwd()
        try:
            for worktree in (repo, linked):
                nested = worktree / "nested" / "directory"
                nested.mkdir(parents=True)
                os.chdir(nested)
                output = io.StringIO()
                with patch.dict(os.environ, env, clear=True), patch.object(safe.subprocess, "call", return_value=0), contextlib.redirect_stderr(output):
                    self.assertEqual(safe.main(["check"]), 0)
                descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1].splitlines()[0])
                self.assertEqual(Path(descriptor["worktree"]), worktree.resolve())
                self.assertNotEqual(Path(descriptor["worktree"]), nested)
                descriptors.append(descriptor)
        finally:
            os.chdir(previous)
        for name in ("target", "build"):
            self.assertNotEqual(descriptors[0]["resources"][name], descriptors[1]["resources"][name])
        self.assertEqual(descriptors[0]["lease"], descriptors[1]["lease"])
        self.assertEqual(descriptors[0]["resources"]["cargo_home"], descriptors[1]["resources"]["cargo_home"])

    @unittest.skipIf(os.name == "nt", "POSIX permits trailing whitespace in directory names")
    def test_real_git_whitespace_paths_keep_distinct_private_state(self):
        repo = self.root / "repo"
        # Git permits trailing space/tab in a separate common directory too.
        # Trailing newlines in gitdir files are not supported by Git itself.
        common = self.root / "common \t"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for key in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(key, None)
        def git(*args, cwd=self.root):
            return subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True).stdout
        git("init", "-b", "main", "--separate-git-dir", str(common), str(repo))
        git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--allow-empty", "-m", "fixture", cwd=repo)
        trees = [self.root / ("tree" + suffix) for suffix in ("", " ", "\t", "\n", "\r")]
        for index, tree in enumerate(trees):
            git("worktree", "add", "-b", "linked-" + str(index), str(tree), cwd=repo)
        plans = []
        previous = Path.cwd()
        try:
            for tree in trees:
                os.chdir(tree)
                # Check the real Git protocol, including its one terminal LF.
                self.assertEqual(git("rev-parse", "--show-toplevel", cwd=tree), os.fsencode(tree.resolve()) + b"\n")
                self.assertEqual(git("rev-parse", "--path-format=absolute", "--git-common-dir", cwd=tree), os.fsencode(common.resolve()) + b"\n")
                with patch.dict(os.environ, env, clear=True):
                    self.assertEqual(safe.git_path("--path-format=absolute", "--git-common-dir"), common.resolve())
                    plan = safe.resource_plan(env)
                self.assertEqual(plan[0], tree.resolve())
                plans.append(plan)
        finally:
            os.chdir(previous)
        self.assertEqual(len({plan[1] for plan in plans}), 1)
        for name in ("target", "build"):
            self.assertEqual(len({plan[2][name] for plan in plans}), len(trees))
        self.assertFalse(plans[0][1].exists())

    def test_git_path_requires_one_terminator_without_stripping_path_bytes(self):
        with patch.object(safe.subprocess, "check_output", return_value=os.fsencode(self.root)):
            with self.assertRaises(safe.Denied):
                safe.git_path("--show-toplevel")
        if os.name != "nt":
            # Two LF bytes are a valid trailing LF in the path plus Git's LF.
            path = self.root / "tree\n"
            with patch.object(safe.subprocess, "check_output", return_value=git_output(path)):
                self.assertEqual(safe.git_path("--show-toplevel"), path.resolve())

    def test_real_git_location_overrides_refuse_before_allocation(self):
        repo, other = self.root / "repo", self.root / "other"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for path in (repo, other):
            subprocess.run(["git", "init", "-q", str(path)], env=env, check=True, capture_output=True)
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(name, None)
        selectors = {"GIT_DIR": str(other / ".git"), "GIT_WORK_TREE": str(other),
                     "GIT_COMMON_DIR": str(other / ".git")}
        previous = Path.cwd()
        try:
            os.chdir(repo)
            redirected = dict(env, **selectors)
            # Real negative control: Git's view is no longer Cargo's CWD.
            result = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], env=redirected)
            if os.name == "nt":
                # Git spells native Windows separators with forward slashes.
                self.assertTrue(result.endswith(b"\n"))
                self.assertEqual(Path(os.fsdecode(result[:-1])).resolve(), other.resolve())
            else:
                self.assertEqual(result, git_output(other.resolve()))
            for selection in (selectors, *({name: selected} for name, value in selectors.items() for selected in (value, ""))):
                with patch.dict(os.environ, dict(env, **selection), clear=True), \
                     patch.object(safe, "check_capacity", return_value={"fixture": "identity-only"}) as capacity, \
                     patch.object(safe.subprocess, "call", return_value=0) as cargo:
                    self.assertEqual(safe.main(["check"]), 75)
                    capacity.assert_not_called()
                    cargo.assert_not_called()
                self.assertFalse((self.root / "plane").exists())
        finally:
            os.chdir(previous)

    @unittest.skipIf(os.name == "nt", "POSIX filename bytes may be non-UTF-8")
    def test_real_git_non_utf8_common_and_worktree_paths_are_lossless(self):
        repo = self.root / os.fsdecode(b"repo-\xff")
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1",
                   DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"))
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"):
            env.pop(name, None)
        subprocess.run(["git", "init", "-q", str(repo)], env=env, check=True, capture_output=True)
        previous = Path.cwd()
        try:
            os.chdir(repo)
            with patch.dict(os.environ, env, clear=True):
                first = safe.resource_plan(env)
                self.assertEqual(first, safe.resource_plan(env))
            self.assertEqual(os.fsencode(first[0]), os.fsencode(repo.resolve()))
            self.assertNotEqual(first[2]["target"], first[2]["build"])
            self.assertFalse(first[1].exists())
        finally:
            os.chdir(previous)

    def test_same_named_worktrees_and_stale_shared_overrides(self):
        env = {"DEVPLANE": str(self.root / "plane"), "CARGO_HOME": str(self.root / "cargo")}
        common = str(self.root / "repo.git")
        plans = []
        for worktree in (self.root / "one" / "same", self.root / "two" / "same"):
            with patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]):
                plans.append(safe.resource_plan(env))
        self.assertEqual(plans[0][1], plans[1][1])  # unchanged exclusion domain
        for name, variable in (("target", "CARGO_TARGET_DIR"), ("build", "CARGO_BUILD_BUILD_DIR")):
            self.assertNotEqual(plans[0][2][name], plans[1][2][name])
            with patch.object(safe.subprocess, "check_output", side_effect=[git_output(plans[0][0]), git_output(common)]):
                self.assertEqual(safe.resource_plan(dict(env, **{variable: str(plans[0][2][name])})), plans[0])
            for stale in (plans[1][2][name], plans[0][1] / name):
                with patch.object(safe.subprocess, "check_output", side_effect=[git_output(plans[0][0]), git_output(common)]):
                    with self.assertRaises(safe.Denied):
                        safe.resource_plan(dict(env, **{variable: str(stale)}))
        self.assertFalse(plans[0][1].exists())

    def test_legacy_resources_and_common_lease_are_preserved(self):
        worktree, common = str(self.root / "repo"), str(self.root / "repo.git")
        env = dict(self.env, DEVPLANE=str(self.root / "plane"), CARGO_HOME=str(self.root / "cargo"),
                   HOME=str(self.root), USERPROFILE=str(self.root))
        with patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]):
            _, slot, paths = safe.resource_plan(env)
        for name in ("target", "build"):
            legacy = slot / name
            legacy.mkdir(parents=True)
            (legacy / "evidence").write_bytes(b"retain exact legacy bytes")
        lease = slot / "cargo-active"
        lease.mkdir()
        with patch.dict(os.environ, env, clear=True), \
             patch.object(safe.subprocess, "check_output", side_effect=[git_output(worktree), git_output(common)]), \
             patch.object(safe.subprocess, "call") as cargo:
            self.assertEqual(safe.main(["check"]), 75)
            cargo.assert_not_called()
        self.assertTrue(lease.is_dir())
        for name in ("target", "build"):
            self.assertFalse(paths[name].exists())
            self.assertEqual((slot / name / "evidence").read_bytes(), b"retain exact legacy bytes")

    @unittest.skipUnless(os.environ.get("CARGO_ADMITTED_REAL_BUILD_TEST") == "1",
                         "set CARGO_ADMITTED_REAL_BUILD_TEST=1 for the offline Cargo A/B/A regression")
    def test_real_cargo_alternating_worktrees_preserve_dependency_behavior(self):
        """Exercise real Cargo; a shared intermediate directory fails this oracle.

        Disk policy is separately fixture-tested above. This test patches only its
        observation so a tiny offline build does not pretend to admit a host budget.
        It does not touch any existing repository, target, or user Cargo defaults.
        """
        import contextlib
        import io
        import json
        # This opt-in test launches a real owner, so do not bypass discovery admission.
        self.discovery_fixture.stop()
        repo, linked = self.root / "repo", self.root / "linked"
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        for name in ("CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            env.pop(name, None)
        env.update(DEVPLANE=str(self.root / "plane"), TMPDIR=str(self.root / "tmp"),
                   CARGO_NET_OFFLINE="true", CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2",
                   GIT_CONFIG_GLOBAL=str(self.root / "no-global"), GIT_CONFIG_NOSYSTEM="1")
        def run(command, cwd):
            result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout.strip()
        def write(path, text):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        run(["git", "init", "-b", "main", str(repo)], self.root)
        write(repo / "Cargo.toml", '[workspace]\nmembers=["app","core"]\nresolver="2"\n')
        write(repo / "core/Cargo.toml", '[package]\nname="identity-core"\nversion="0.1.0"\nedition="2021"\n')
        write(repo / "core/src/lib.rs", 'pub fn value() -> u32 { 1 }\n')
        write(repo / "app/Cargo.toml", '[package]\nname="identity-app"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nidentity-core={path="../core"}\n')
        write(repo / "app/src/main.rs", 'fn main() { println!("{}", identity_core::value()); }\n')
        run(["cargo", "generate-lockfile", "--offline"], repo)
        run(["git", "add", "."], repo)
        run(["git", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
             "commit", "-m", "old behavior"], repo)
        run(["git", "worktree", "add", "-b", "changed", str(linked)], repo)
        write(linked / "core/src/lib.rs", 'pub fn value() -> u32 { 2 }\n')
        # Both worktrees, including the changed library, exist before the first
        # build. No source touching/backdating is needed to expose the stale hit.
        descriptors = []
        previous = Path.cwd()
        try:
            for worktree, expected in ((repo, "1"), (linked, "2"), (repo, "1")):
                os.chdir(worktree)
                output = io.StringIO()
                with patch.dict(os.environ, env, clear=True), \
                     patch.object(safe, "check_capacity", return_value={"fixture": "identity-only"}), \
                     patch.object(safe, "ClippyTree", NATIVE_TREE), \
                     patch.object(safe, "call_clippy", NATIVE_LAUNCH), \
                     contextlib.redirect_stderr(output):
                    self.assertEqual(safe.main(["build", "-p", "identity-app", "--offline", "--locked"]), 0)
                descriptor = json.loads(output.getvalue().split("cargo-admitted resources: ", 1)[1].splitlines()[0])
                if sys.platform == "linux":
                    settlement = json.loads(output.getvalue().split("cargo-admitted Cargo settlement: ", 1)[1].splitlines()[0])
                    self.assertTrue(settlement["tree_settled"])
                    self.assertTrue(settlement["lease_released"])
                    self.assertEqual(settlement["proof"], "kernel ECHILD (__WALL)")
                descriptors.append(descriptor)
                executable = Path(descriptor["resources"]["target"]) / "debug" / ("identity-app.exe" if os.name == "nt" else "identity-app")
                # This one test owns the TemporaryDirectory and runs builds
                # sequentially: no next build starts before this assertion ends.
                # It proves source isolation, not concurrent/post-lease consumers.
                self.assertEqual(run([str(executable)], worktree), expected)
        finally:
            os.chdir(previous)
        for name in ("target", "build"):
            self.assertNotEqual(descriptors[0]["resources"][name], descriptors[1]["resources"][name])
            self.assertEqual(descriptors[0]["resources"][name], descriptors[2]["resources"][name])
        self.assertEqual(descriptors[0]["lease"], descriptors[1]["lease"])

    def test_nonfinite_policy_refused(self):
        for key in ("MIN_FREE_GB", "MAX_USED_PCT"):
            for value in ("NaN", "inf", "-inf", "0", "-1"):
                with self.assertRaises(safe.Denied):
                    safe.check_capacity([self.root], {key: value})

    def test_native_and_foreign_paths(self):
        self.assertEqual(safe.native_path(str(self.root)), self.root.resolve())
        with self.assertRaises(safe.Denied):
            safe.native_path("relative/path")
        if os.name == "nt":
            self.assertEqual(safe.native_path("/c/with space/cache"), Path("C:/with space/cache"))
            for value in ("/mnt/c/cache", "/tmp/cache", "//wsl$/Ubuntu/cache"):
                with self.assertRaises(safe.Denied):
                    safe.native_path(value)
        else:
            for value in ("C:/cache", "/mnt/c/cache"):
                with self.assertRaises(safe.Denied):
                    safe.native_path(value)

    def test_symlink_or_junction_rejected(self):
        link = self.root / "link"
        if os.name == "nt":
            result = subprocess.run(["cmd", "/c", "mklink", "/J", str(link), str(self.root)], capture_output=True)
            if result.returncode:
                self.skipTest("junction creation unavailable")
        else:
            link.symlink_to(self.root, target_is_directory=True)
        try:
            with self.assertRaises(safe.Denied):
                safe.native_path(str(link / "child"))
        finally:
            if os.name == "nt":
                link.rmdir()
            else:
                link.unlink()


class ResourceIdentityPreflightTests(unittest.TestCase):
    """Real Git and CLI proof; preflight never launches Cargo or acquires a lease."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="admission identity ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "source repo"
        self.foreign = self.root / "foreign repo"
        # Keep fixtures independent of user Git selectors and build settings.
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("GIT_", "CARGO_", "RUST"))}
        self.env.update(GIT_CONFIG_GLOBAL=str(self.root / "no global config"),
                        GIT_CONFIG_NOSYSTEM="1", DEVPLANE=str(self.root / "private plane"),
                        CARGO_HOME=str(self.root / "cargo home"),
                        TMPDIR=str(self.root / "scratch space"),
                        CARGO_BUILD_JOBS="2",
                        # Fixture-only disk policy; this is identity proof, not
                        # qualification of a real build's capacity envelope.
                        MIN_FREE_GB="0.001", MAX_USED_PCT="100")
        for repo in (self.repo, self.foreign):
            subprocess.run(["git", "init", "-q", str(repo)], env=self.env,
                           check=True, capture_output=True)

    def preflight(self, cwd, **overrides):
        return subprocess.run([sys.executable, str(ROOT / "scripts/cargo_admitted.py"),
                               "--preflight", "check"], cwd=cwd,
                              env=dict(self.env, **overrides), text=True,
                              capture_output=True, timeout=20)

    def accepted(self, cwd, **overrides):
        import json
        result = self.preflight(cwd, **overrides)
        self.assertEqual(result.returncode, 0, result.stderr)
        prefix = "cargo-admitted preflight: "
        lines = [line for line in result.stderr.splitlines() if line.startswith(prefix)]
        self.assertEqual(len(lines), 1, result.stderr)
        report = json.loads(lines[0][len(prefix):])
        self.assertEqual(report["verdict"], "PASS")
        self.assertFalse(report["lease_acquired"])
        self.assertFalse(Path(report["lease"]).exists())
        self.assertEqual(Path(report["scope"]["working_directory"]), cwd.resolve())
        return report

    def refused(self, cwd, reason, **overrides):
        result = self.preflight(cwd, **overrides)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertIn(reason, result.stderr)
        import json
        prefix = "cargo-admitted preflight: "
        lines = [line for line in result.stderr.splitlines() if line.startswith(prefix)]
        self.assertEqual(len(lines), 1, result.stderr)
        report = json.loads(lines[0][len(prefix):])
        self.assertEqual(report["verdict"], "DENY")
        self.assertFalse(report["lease_acquired"])
        self.assertIsNone(report["scope"])
        self.assertNotIn("cargo-admitted resources: ", result.stderr)

    def test_spaced_paths_and_foreign_cwd_bind_to_invoked_repository(self):
        nested = self.repo / "nested folder"
        nested.mkdir()
        first = self.accepted(self.repo)
        nested_report = self.accepted(nested)
        foreign = self.accepted(self.foreign)
        for report, repo in ((first, self.repo), (nested_report, self.repo),
                             (foreign, self.foreign)):
            self.assertEqual(Path(report["scope"]["worktree"]), repo)
            self.assertEqual(Path(report["scope"]["resources"]["cargo_home"]),
                             self.root / "cargo home")
            self.assertEqual(Path(report["scope"]["resources"]["temp"]),
                             self.root / "scratch space")
        self.assertEqual(first["scope"]["resources"], nested_report["scope"]["resources"])
        self.assertEqual(first["lease"], nested_report["lease"])
        self.assertNotEqual(first["lease"], foreign["lease"])
        for name in ("target", "build"):
            self.assertNotEqual(first["scope"]["resources"][name], foreign["scope"]["resources"][name])
        self.assertFalse(Path(self.env["DEVPLANE"]).exists())
        self.assertFalse(Path(self.env["CARGO_HOME"]).exists())
        self.assertFalse(Path(self.env["TMPDIR"]).exists())

    def test_relative_targets_refuse_even_when_they_resolve_to_owned_paths(self):
        for cwd in (self.repo, self.foreign):
            resources = self.accepted(cwd)["scope"]["resources"]
            for name, variable in (("target", "CARGO_TARGET_DIR"),
                                   ("build", "CARGO_BUILD_BUILD_DIR")):
                with self.subTest(cwd=cwd, variable=variable):
                    expected = Path(resources[name])
                    relative = os.path.relpath(expected, cwd)
                    self.assertFalse(Path(relative).is_absolute())
                    self.assertEqual((cwd / relative).resolve(), expected)
                    self.refused(cwd, "storage paths must be absolute", **{variable: relative})
                    # The identical owned resource is admitted when explicit.
                    self.accepted(cwd, **{variable: str(expected)})
        self.assertFalse(Path(self.env["DEVPLANE"]).exists())

    def test_explicit_targets_cannot_transfer_foreign_repository_ownership(self):
        source = self.accepted(self.repo)["scope"]["resources"]
        foreign = self.accepted(self.foreign)["scope"]["resources"]
        for name, variable in (("target", "CARGO_TARGET_DIR"),
                               ("build", "CARGO_BUILD_BUILD_DIR")):
            for cwd, own, other in ((self.repo, source, foreign),
                                    (self.foreign, foreign, source)):
                with self.subTest(cwd=cwd, variable=variable):
                    self.refused(cwd, variable + " override unsupported", **{variable: other[name]})
                    self.accepted(cwd, **{variable: own[name]})
        self.assertFalse(Path(self.env["DEVPLANE"]).exists())

    def check_linked_storage(self, junction=False):
        report = self.accepted(self.repo)
        resources = report["scope"]["resources"]
        private = Path(resources["target"]).parent
        private.mkdir(parents=True)
        sentinel = private / "owner evidence"
        sentinel.write_bytes(b"preserve original storage owner")
        alias = self.root / "storage alias"
        if junction:
            subprocess.run(["cmd", "/c", "mklink", "/J", str(alias), str(private)],
                           check=True, capture_output=True)
            self.addCleanup(alias.rmdir)
        else:
            try:
                alias.symlink_to(private, target_is_directory=True)
            except OSError as error:
                if os.name != "nt" or error.winerror != 1314:
                    raise
                self.skipTest("Windows symlink privilege unavailable; junction has separate coverage")
            self.addCleanup(alias.unlink)
        self.assertEqual(alias.resolve(), private)
        for name, variable in (("target", "CARGO_TARGET_DIR"),
                               ("build", "CARGO_BUILD_BUILD_DIR")):
            with self.subTest(variable=variable):
                selected = alias / name
                self.assertEqual(selected.resolve(), Path(resources[name]))
                self.refused(self.repo, "linked storage path is unsupported", **{variable: str(selected)})
                self.accepted(self.repo, **{variable: resources[name]})
                self.assertFalse(Path(resources[name]).exists())
        self.assertEqual(sentinel.read_bytes(), b"preserve original storage owner")
        self.assertFalse(Path(report["lease"]).exists())
        self.assertFalse(Path(resources["cargo_home"]).exists())
        self.assertFalse(Path(resources["temp"]).exists())

    def test_symlink_alias_cannot_adopt_owned_storage(self):
        self.check_linked_storage()

    @unittest.skipUnless(os.name == "nt", "Windows junction control")
    def test_junction_alias_cannot_adopt_owned_storage(self):
        self.check_linked_storage(junction=True)


@unittest.skipUnless(sys.platform == "linux", "creation provenance is qualified on native Linux")
class PrivateAllocationProvenanceTests(unittest.TestCase):
    """Real Git/filesystem entrypoint controls; capacity and Cargo are fixtures."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cargo-allocation-proof-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo, self.worktree = self.root / "repository", self.root / "source"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("GIT_", "CARGO_", "RUST"))}
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(self.root / "no-global"),
                        DEVPLANE=str(self.root / "devplane"), CARGO_HOME=str(self.root / "cargo-home"),
                        TMPDIR=str(self.root / "temp"))
        self.git("init", "-q")
        self.git("config", "user.name", "Allocation Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        (self.repo / "source.txt").write_text("original\n")
        self.git("add", "source.txt")
        self.git("commit", "-qm", "original")
        self.git("worktree", "add", "-qb", "original", str(self.worktree))

    def git(self, *arguments, cwd=None):
        return subprocess.check_output(["git", "-C", str(cwd or self.repo), *arguments],
                                       env=self.env, stderr=subprocess.PIPE)

    @contextlib.contextmanager
    def working_directory(self, path):
        previous = Path.cwd()
        os.chdir(path)
        try:
            yield
        finally:
            os.chdir(previous)

    def invoke(self, *, preflight=False, launch=None, probe=None):
        import io
        output = io.StringIO()
        tree = Mock()
        tree.probe.return_value = subprocess.CompletedProcess([], 0, b"{}", b"")
        tree.probe.side_effect = probe
        tree.finish.return_value = {"tree_settled": True, "cancelled": False}
        with self.working_directory(self.worktree), patch.dict(os.environ, self.env, clear=True), \
             patch.object(safe, "check_capacity", return_value={"fixture": "not host qualification"}), \
             patch.object(safe, "child_discovery_capability", return_value={}), \
             patch.object(safe, "ClippyTree", return_value=tree), \
             patch.object(safe, "call_clippy", side_effect=launch, return_value=0) as cargo, \
             contextlib.redirect_stderr(output):
            result = safe.main((["--preflight"] if preflight else []) + ["check", "--locked"])
        messages = [json.loads(line.split(": ", 1)[1]) for line in output.getvalue().splitlines()
                    if line.startswith(("cargo-admitted resources: ", "cargo-admitted preflight: "))]
        return result, messages, output.getvalue(), cargo.call_count

    def plan(self):
        with self.working_directory(self.worktree):
            return safe.resource_plan(self.env)

    def record_path(self):
        return self.plan()[2]["target"].parent / "allocation.json"

    def allocate(self):
        result, messages, output, calls = self.invoke()
        self.assertEqual((result, calls), (0, 1), output)
        record = self.record_path()
        self.assertTrue(record.is_file())
        return record, messages[0]

    def snapshot(self):
        return {str(path.relative_to(self.root)):
                (path.lstat().st_dev, path.lstat().st_ino, path.lstat().st_mode,
                 path.lstat().st_mtime_ns,
                 hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None)
                for path in self.root.rglob("*")}

    def new_worktree(self, name):
        self.worktree = self.root / name
        self.git("worktree", "add", "-qb", name, str(self.worktree))

    def assert_refused_without_launch(self, *, preflight=False):
        result, _, output, calls = self.invoke(preflight=preflight)
        self.assertEqual((result, calls), (75, 0), output)
        return output

    def test_record_binds_original_roots_source_and_its_own_inode(self):
        record, descriptor = self.allocate()
        document = json.loads(record.read_bytes())
        self.assertEqual(document["schema_version"], "cargo_private_allocation.v1")
        self.assertEqual(document["record_identity"], [record.stat().st_dev, record.stat().st_ino])
        self.assertEqual(document["creator_source_sha256"], hashlib.sha256((ROOT / "scripts/cargo_admitted.py").read_bytes()).hexdigest())
        self.assertEqual(len(document["allocation_id"]), 32)
        for role, path in (("worktree", self.worktree), ("common_dir", self.repo / ".git"),
                           ("target", Path(descriptor["resources"]["target"])),
                           ("build", Path(descriptor["resources"]["build"]))):
            self.assertEqual(document["subject"][role],
                             {"path": str(path), "identity": list(safe.directory_identity(path))})
        self.assertNotIn("temp", document["subject"])
        self.assertNotIn("cargo_home", document["subject"])
        self.assertLessEqual(record.stat().st_size, 16 * 1024)
        receipt = descriptor["private_allocation_provenance"]
        self.assertEqual(receipt["status"], "matching_creation_record")
        self.assertEqual(receipt["record_sha256"], hashlib.sha256(record.read_bytes()).hexdigest())
        self.assertEqual(receipt["consumer_custody"], "unknown")
        self.assertEqual(receipt["reclaim_eligibility"], "unknown")

    def test_preflight_does_not_create_or_modify_provenance(self):
        before = self.snapshot()
        result, messages, output, calls = self.invoke(preflight=True)
        self.assertEqual((result, calls), (0, 0), output)
        self.assertEqual(messages[0]["private_allocation_provenance"]["status"], "unknown")
        self.assertEqual(self.snapshot(), before)
        self.allocate()
        before = self.snapshot()
        result, messages, output, calls = self.invoke(preflight=True)
        self.assertEqual((result, calls), (0, 0), output)
        self.assertEqual(messages[0]["private_allocation_provenance"]["status"], "matching_creation_record")
        self.assertEqual(self.snapshot(), before)

    def test_preexisting_or_partly_preexisting_roots_are_never_adopted(self):
        for existing in (("target",), ("build",), ("target", "build")):
            with self.subTest(existing=existing):
                self.new_worktree("existing-" + "-".join(existing))
                _, _, paths = self.plan()
                for key in existing:
                    paths[key].mkdir(parents=True)
                    (paths[key] / "retain").write_bytes(b"unrecorded output")
                result, messages, output, calls = self.invoke()
                self.assertEqual((result, calls), (0, 1), output)
                receipt = messages[0]["private_allocation_provenance"]
                self.assertEqual(receipt["status"], "unknown")
                self.assertEqual(receipt["reason"], "preexisting_unrecorded_state")
                self.assertFalse(self.record_path().exists())
                for key in existing:
                    self.assertEqual((paths[key] / "retain").read_bytes(), b"unrecorded output")

    def test_copied_record_bytes_cannot_adopt_a_replacement_record_file(self):
        record, _ = self.allocate()
        raw = record.read_bytes()
        original = record.with_name("retained-original-record")
        record.rename(original)
        record.write_bytes(raw)
        self.assertNotEqual(record.stat().st_ino, original.stat().st_ino)
        self.assert_refused_without_launch(preflight=True)
        self.assert_refused_without_launch()
        self.assertEqual(original.read_bytes(), raw)
        self.assertEqual(record.read_bytes(), raw)

    def test_replaced_private_roots_and_git_admin_identity_refuse(self):
        for role in ("target", "build", "private_member", "git_dir", "common_dir"):
            with self.subTest(role=role):
                self.new_worktree("replaced-" + role)
                record, _ = self.allocate()
                document = json.loads(record.read_bytes())
                path = Path(document["subject"][role]["path"])
                retained = path.with_name(path.name + "-retained")
                path.rename(retained)
                shutil.copytree(retained, path)
                self.assert_refused_without_launch(preflight=True)
                self.assert_refused_without_launch()
                # Exact fixture-owned restoration keeps later subcases independent.
                shutil.rmtree(path)
                retained.rename(path)

    def test_record_links_and_special_files_refuse_without_hanging(self):
        for kind in ("symlink", "hardlink", "fifo", "directory"):
            with self.subTest(kind=kind):
                self.new_worktree("invalid-" + kind)
                record, _ = self.allocate()
                original = record.with_name("retained-original-record")
                record.rename(original)
                if kind == "symlink":
                    record.symlink_to(original)
                elif kind == "hardlink":
                    os.link(original, record)
                elif kind == "fifo":
                    os.mkfifo(record)
                else:
                    record.mkdir()
                self.assert_refused_without_launch(preflight=True)
                self.assertTrue(original.is_file())

    def test_malformed_oversized_and_cross_subject_records_refuse(self):
        record, _ = self.allocate()
        raw = record.read_bytes()
        document = json.loads(raw)
        wrong_host = json.loads(raw)
        wrong_host["subject"]["hostname"] = "other-host"
        wrong_worktree = json.loads(raw)
        wrong_worktree["subject"]["worktree"]["path"] = str(self.repo)
        wrong_identity = json.loads(raw)
        wrong_identity["record_identity"][0] = True
        payloads = [b"", b"{", b"[]", b"null", b"x" * (16 * 1024 + 1),
                    b'{"schema_version":"cargo_private_allocation.v1","schema_version":"cargo_private_allocation.v1"}',
                    json.dumps({**document, "extra": 1}).encode(),
                    json.dumps({**document, "schema_version": "future.v2"}).encode(),
                    json.dumps(wrong_host).encode(), json.dumps(wrong_worktree).encode(),
                    json.dumps(wrong_identity).encode()]
        for payload in payloads:
            with self.subTest(payload=payload[:80]):
                record.write_bytes(payload)
                self.assert_refused_without_launch(preflight=True)
                self.assertEqual(record.read_bytes(), payload)
        record.write_bytes(raw)
        result, _, output, calls = self.invoke(preflight=True)
        self.assertEqual((result, calls), (0, 0), output)

    def test_partial_write_and_fsync_failures_publish_no_completed_receipt(self):
        for failure in ("partial-write", "file-fsync", "directory-fsync"):
            with self.subTest(failure=failure):
                self.new_worktree(failure)
                write = os.write
                fsync = os.fsync
                writes = 0

                def fail_write(fd, data):
                    nonlocal writes
                    writes += 1
                    if writes == 1:
                        return write(fd, data[:7])
                    raise OSError("fixture partial write")

                def fail_sync(fd):
                    directory = stat.S_ISDIR(os.fstat(fd).st_mode)
                    if directory == (failure == "directory-fsync"):
                        raise OSError("fixture fsync failure")
                    return fsync(fd)

                with patch.object(safe.os, "write", side_effect=fail_write if failure == "partial-write" else write), \
                     patch.object(safe.os, "fsync", side_effect=fsync if failure == "partial-write" else fail_sync):
                    result, messages, output, calls = self.invoke()
                self.assertEqual((result, calls), (75, 0), output)
                self.assertEqual(messages, [])
                self.assertTrue(self.record_path().is_file())
                self.assertFalse((self.plan()[1] / "cargo-active").exists())
                if failure == "partial-write":
                    self.assertEqual(self.record_path().stat().st_size, 7)
                    self.assert_refused_without_launch()

    def test_new_root_partial_failure_cannot_create_or_later_adopt_record(self):
        _, _, paths = self.plan()
        mkdir = Path.mkdir

        def fail_build(path, *args, **kwargs):
            if path == paths["build"]:
                raise OSError("fixture build root creation failure")
            return mkdir(path, *args, **kwargs)

        with patch.object(Path, "mkdir", fail_build):
            self.assert_refused_without_launch()
        self.assertTrue(paths["target"].is_dir())
        self.assertFalse(paths["build"].exists())
        self.assertFalse(self.record_path().exists())
        result, messages, output, calls = self.invoke()
        self.assertEqual((result, calls), (0, 1), output)
        self.assertEqual(messages[0]["private_allocation_provenance"]["status"], "unknown")
        self.assertFalse(self.record_path().exists())

    def test_record_created_by_another_actor_is_never_overwritten(self):
        subject = safe.file_subject

        def insert(path):
            self.record_path().write_bytes(b"preserve foreign record")
            return subject(path)

        with patch.object(safe, "file_subject", side_effect=insert):
            self.assert_refused_without_launch()
        self.assertEqual(self.record_path().read_bytes(), b"preserve foreign record")

    def test_replaced_private_container_is_not_written(self):
        _, _, paths = self.plan()
        private = paths["target"].parent
        subject = safe.file_subject

        def replace(path):
            private.rename(private.with_name(private.name + "-retained"))
            private.mkdir()
            (private / "foreign-state").write_bytes(b"preserve replacement")
            return subject(path)

        with patch.object(safe, "file_subject", side_effect=replace):
            self.assert_refused_without_launch()
        self.assertEqual(sorted(path.name for path in private.iterdir()), ["foreign-state"])

    def test_only_one_root_created_by_this_attempt_stays_unknown(self):
        _, _, paths = self.plan()
        mkdir = Path.mkdir
        inserted = False

        def insert_build(path, *args, **kwargs):
            nonlocal inserted
            result = mkdir(path, *args, **kwargs)
            if path == paths["target"] and not inserted:
                inserted = True
                mkdir(paths["build"])
                (paths["build"] / "foreign-output").write_bytes(b"retain foreign creation")
            return result

        with patch.object(Path, "mkdir", insert_build):
            result, messages, output, calls = self.invoke()
        self.assertEqual((result, calls), (0, 1), output)
        self.assertTrue(inserted)
        self.assertEqual(messages[0]["private_allocation_provenance"]["status"], "unknown")
        self.assertFalse(self.record_path().exists())
        self.assertEqual((paths["build"] / "foreign-output").read_bytes(), b"retain foreign creation")

    def test_record_replaced_after_allocation_refuses_before_launch(self):
        def replace(_worktree):
            record = self.record_path()
            raw = record.read_bytes()
            record.rename(record.with_name("retained-before-launch"))
            record.write_bytes(raw)
            return []

        with patch.object(safe, "local_config_cli_args", side_effect=replace):
            self.assert_refused_without_launch()

    def test_recorded_source_or_root_replaced_during_projection_refuses_product(self):
        for role in ("source", "target"):
            with self.subTest(role=role):
                self.new_worktree("projection-" + role)
                record, descriptor = self.allocate()
                original_record = record.read_bytes()
                target = Path(descriptor["resources"]["target"])
                (target / "retained-output").write_bytes(b"preserve original output")
                retained = target.with_name("retained-target")
                probes = []

                def replace_during_projection(command, env, **bounds):
                    probes.append(command)
                    self.assertEqual(command[-2:], ["locate-project", "--message-format=json"])
                    if role == "source":
                        self.git("worktree", "move", str(self.worktree), str(self.root / "retained-projection-source"))
                        self.git("worktree", "add", "-qb", "replacement-projection", str(self.worktree))
                    else:
                        target.rename(retained)
                        target.mkdir()
                        (target / "foreign-output").write_bytes(b"preserve replacement output")
                    return subprocess.CompletedProcess(command, 0, b"{}", b"")

                result, _, output, calls = self.invoke(probe=replace_during_projection)
                self.assertEqual(len(probes), 1)
                self.assertEqual((result, calls), (75, 0), output)
                self.assertNotIn("cargo-admitted Cargo product:", output)
                self.assertEqual(record.read_bytes(), original_record)
                original_target = target if role == "source" else retained
                self.assertEqual((original_target / "retained-output").read_bytes(), b"preserve original output")
                if role == "target":
                    self.assertEqual((target / "foreign-output").read_bytes(), b"preserve replacement output")

    def test_schema_valid_same_inode_readback_must_match_written_bytes(self):
        read = safe.PrivateAllocation._read
        changed = False

        def change_allocation_id(allocation):
            nonlocal changed
            if not changed:
                changed = True
                record = allocation.path
                original = record.read_bytes()
                identity = (record.stat().st_dev, record.stat().st_ino)
                document = json.loads(original)
                old_id = document["allocation_id"]
                document["allocation_id"] = ("a" if old_id[0] != "a" else "b") + old_id[1:]
                record.write_bytes((json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode())
                self.assertEqual((record.stat().st_dev, record.stat().st_ino), identity)
                self.assertNotEqual(record.read_bytes(), original)
            return read(allocation)

        with patch.object(safe.PrivateAllocation, "_read", change_allocation_id):
            result, messages, output, calls = self.invoke()
        self.assertTrue(changed)
        self.assertEqual((result, calls), (75, 0), output)
        self.assertEqual(messages, [])
        self.assertIn("readback differs from written bytes", output)

    def test_record_changed_during_read_cannot_complete_preflight(self):
        record, _ = self.allocate()
        raw = record.read_bytes()
        altered = json.loads(raw)
        altered["creator_source_sha256"] = "a" * 64
        replacement = (json.dumps(altered, sort_keys=True, separators=(",", ":")) + "\n").encode()
        self.assertEqual(len(replacement), len(raw))
        read = os.read
        changed = False
        expected = (record.stat().st_dev, record.stat().st_ino)

        def alter_after_read(fd, length):
            nonlocal changed
            chunk = read(fd, length)
            opened = os.fstat(fd)
            if not changed and (opened.st_dev, opened.st_ino) == expected:
                changed = True
                record.write_bytes(replacement)
            return chunk

        with patch.object(safe.os, "read", side_effect=alter_after_read):
            self.assert_refused_without_launch(preflight=True)
        self.assertTrue(changed)
        self.assertEqual(record.read_bytes(), replacement)

    def test_creator_source_digest_is_historical_not_a_current_revision_gate(self):
        with patch.object(safe, "file_subject", return_value={"sha256": "a" * 64}):
            record, _ = self.allocate()
        self.assertEqual(json.loads(record.read_bytes())["creator_source_sha256"], "a" * 64)
        result, messages, output, calls = self.invoke(preflight=True)
        self.assertEqual((result, calls), (0, 0), output)
        self.assertEqual(messages[0]["private_allocation_provenance"]["status"], "matching_creation_record")

    def test_non_linux_keeps_unknown_without_reading_or_writing_record(self):
        worktree, slot, paths = self.plan()
        with patch.object(safe.sys, "platform", "darwin"), \
             patch.object(safe.os, "open", side_effect=AssertionError("record opened")):
            allocation = safe.PrivateAllocation(worktree, slot, paths, self.env)
            allocation.revalidate()
            allocation.record_created({}, None, None, None, None)
        self.assertEqual(allocation.receipt["status"], "unknown")
        self.assertEqual(allocation.receipt["reason"], "platform_not_qualified")
        self.assertFalse(slot.exists())

    def test_recreated_worktree_path_cannot_reuse_recorded_allocation(self):
        result, messages, output, calls = self.invoke()
        self.assertEqual((result, calls), (0, 1), output)
        resources = messages[0]["resources"]
        target = Path(resources["target"])
        payload = target / "old-output"
        payload.write_bytes(b"retain original allocation")
        original_source = safe.directory_identity(self.worktree)
        original_target = safe.directory_identity(target)
        self.git("worktree", "move", str(self.worktree), str(self.root / "retained-source"))
        (self.repo / "source.txt").write_text("replacement\n")
        self.git("add", "source.txt")
        self.git("commit", "-qm", "replacement")
        self.git("worktree", "add", "-qb", "replacement", str(self.worktree))
        self.assertNotEqual(safe.directory_identity(self.worktree), original_source)
        for preflight in (True, False):
            result, _, output, calls = self.invoke(preflight=preflight)
            self.assertEqual((result, calls), (75, 0), output)
            self.assertEqual(safe.directory_identity(target), original_target)
            self.assertEqual(payload.read_bytes(), b"retain original allocation")

    def test_source_switch_after_planning_refuses_before_allocating_old_slot(self):
        _, original_slot, _ = self.plan()
        foreign = self.root / "foreign-repository"
        foreign.mkdir()
        self.git("init", "-q", cwd=foreign)
        allocation = safe.PrivateAllocation

        def switch(worktree, slot, paths, env):
            (self.worktree / ".git").write_text("gitdir: " + str(foreign / ".git") + "\n")
            self.assertNotEqual(self.plan()[1], original_slot)
            return allocation(worktree, slot, paths, env)

        with patch.object(safe, "PrivateAllocation", side_effect=switch):
            self.assert_refused_without_launch()
        self.assertFalse(original_slot.exists())
        self.assertFalse(self.plan()[1].exists())

    def test_normal_edits_keep_existing_paths_and_directory_identities(self):
        first, messages, output, calls = self.invoke()
        self.assertEqual((first, calls), (0, 1), output)
        resources = messages[0]["resources"]
        originals = {key: safe.directory_identity(Path(resources[key])) for key in ("target", "build")}
        record = self.record_path()
        record_bytes = record.read_bytes()
        record_identity = (record.stat().st_dev, record.stat().st_ino)
        (self.worktree / "source.txt").write_text("normal edit\n")
        self.git("add", "source.txt", cwd=self.worktree)
        self.git("commit", "-qm", "normal edit", cwd=self.worktree)
        second, messages, output, calls = self.invoke()
        self.assertEqual((second, calls), (0, 1), output)
        self.assertEqual(messages[0]["resources"], resources)
        self.assertEqual({key: safe.directory_identity(Path(resources[key])) for key in originals}, originals)
        self.assertEqual(record.read_bytes(), record_bytes)
        self.assertEqual((record.stat().st_dev, record.stat().st_ino), record_identity)


if __name__ == "__main__":
    unittest.main()
