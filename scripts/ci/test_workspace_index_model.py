#!/usr/bin/env python3
"""Adapter controls; these do not substitute for native TLC or Rust results."""
from pathlib import Path
import json
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import workspace_index_model as model


def frame(code, body, severity=0):
    return f"@!@!@STARTMSG {code}:{severity} @!@!@\n{body}\n@!@!@ENDMSG {code} @!@!@\n"


class BindingControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.spec = model.load_spec(model.ROOT)
        paths = [model.SPEC, self.spec["authority_ledger"]]
        paths += [self.spec[key] for key in ["model", "config", "property", "assumptions", "toolchain"]]
        paths += [row["config"] for row in self.spec["negative_controls"]]
        paths += self.spec["implementation_sources"] + self.spec["adapter_sources"]
        for name in sorted(set(paths)):
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(model.ROOT / name, target)
        self.runtime = {"fixture_runtime": "not native execution"}

    def bound(self, kind="model_check"):
        return model.binding(self.root, self.spec, kind, self.runtime,
                             {"tree": "b" * 40, "commit": "a" * 40})

    def append(self, path):
        with (self.root / path).open("a", encoding="utf-8") as stream:
            stream.write("\nchanged-input-control\n")

    def update_json(self, path, key, value):
        artifact = self.root / path
        payload = json.loads(artifact.read_text())
        payload[key] = value
        artifact.write_text(json.dumps(payload), encoding="utf-8")

    def test_model_binding_never_claims_implementation_or_adapter_proof(self):
        bound = self.bound()
        self.assertNotIn("implementation_digest", bound)
        self.assertNotIn("adapter_digest", bound)
        for key, value in bound.items():
            if key.endswith("_digest"):
                self.assertRegex(value, r"^sha256:v1:[a-f0-9]{64}$")

    def test_property_only_change_invalidates_with_same_model(self):
        before = self.bound()
        self.update_json(self.spec["property"], "claim", "changed claim control")
        after = self.bound()
        self.assertEqual(before["model_digest"], after["model_digest"])
        self.assertNotEqual(before["property_digest"], after["property_digest"])
        with self.assertRaisesRegex(ValueError, "stale"):
            model.require_current_binding({"state_machine": before, "source_subject": {"tree": "same"}}, after, {"tree": "same"})

    def test_assumptions_and_bounds_are_independent_bindings(self):
        before = self.bound()
        self.append(self.spec["assumptions"])
        assumptions_changed = self.bound()
        self.assertNotEqual(before["assumptions_digest"], assumptions_changed["assumptions_digest"])
        self.assertEqual(before["scope_digest"], assumptions_changed["scope_digest"])
        self.append(self.spec["config"])
        self.assertNotEqual(before["scope_digest"], self.bound()["scope_digest"])

    def test_native_source_change_does_not_masquerade_as_model_change(self):
        model_before = self.bound()
        implementation_before = self.bound("implementation_conformance")
        self.append("crates/perl-workspace/src/workspace/workspace_index.rs")
        self.assertEqual(model_before, self.bound())
        self.assertNotEqual(implementation_before["implementation_digest"],
                            self.bound("implementation_conformance")["implementation_digest"])

    def test_test_adapter_change_invalidates_conformance(self):
        before = self.bound("implementation_conformance")
        self.append("crates/perl-workspace/tests/workspace_index_resource_model.rs")
        after = self.bound("implementation_conformance")
        self.assertEqual(before["implementation_digest"], after["implementation_digest"])
        self.assertNotEqual(before["adapter_digest"], after["adapter_digest"])

    def test_whole_tracked_tree_change_invalidates_implementation_closure(self):
        before = self.bound("implementation_conformance")
        after = model.binding(self.root, self.spec, "implementation_conformance", self.runtime,
                              {"tree": "c" * 40, "commit": "d" * 40})
        self.assertNotEqual(before["implementation_digest"], after["implementation_digest"])
        with self.assertRaisesRegex(ValueError, "tracked-source tree"):
            model.binding(self.root, self.spec, "implementation_conformance", self.runtime)

    def test_tool_pin_or_runtime_change_cannot_reuse_evidence(self):
        before = self.bound()
        self.runtime = {"fixture_runtime": "different"}
        self.assertNotEqual(before["toolchain_digest"], self.bound()["toolchain_digest"])
        self.update_json(self.spec["toolchain"], "sha256", "0" * 64)
        with self.assertRaisesRegex(ValueError, "pin"):
            self.bound()

    def test_missing_input_and_path_escape_refuse(self):
        with self.assertRaises(ValueError):
            model.source_file(self.root, "../escape")
        (self.root / self.spec["model"]).unlink()
        with self.assertRaises(FileNotFoundError):
            self.bound()

    def test_duplicate_json_keys_refuse(self):
        path = self.root / "duplicate.json"
        path.write_text('{"schema_id":1,"schema_id":2}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate"):
            model.read_json(path)

    def test_dropped_claim_or_negative_control_refuses(self):
        self.update_json(self.spec["property"], "invariants", list(model.INVARIANTS[:-1]))
        with self.assertRaisesRegex(ValueError, "invariant denominator"):
            model.load_spec(self.root)
        self.update_json(self.spec["property"], "invariants", list(model.INVARIANTS))
        self.update_json(model.SPEC, "negative_controls", self.spec["negative_controls"][:-1])
        with self.assertRaisesRegex(ValueError, "negative-control denominator"):
            model.load_spec(self.root)

    def test_shadow_authority_or_changed_production_case_refuses(self):
        self.update_json(model.SPEC, "authority_rows", ["WSI-COORD-002"])
        with self.assertRaisesRegex(ValueError, "existing WSI authority"):
            model.load_spec(self.root)
        self.update_json(model.SPEC, "authority_rows", list(model.AUTHORITY_ROWS))
        self.update_json(model.SPEC, "conformance_cases", ["unrelated_one", "unrelated_two"])
        with self.assertRaisesRegex(ValueError, "production denominator"):
            model.load_spec(self.root)

    def test_dropped_enabled_invariant_or_extra_constraint_refuses(self):
        path = self.root / self.spec["config"]
        original = path.read_text()
        for mutant in [original.replace("    SingleDrain\n", ""),
                       original + "\nCONSTRAINT FALSE\n",
                       original.replace('Rejected = "rejected"', 'Rejected = "kept"')]:
            path.write_text(mutant)
            with self.assertRaisesRegex(ValueError, "configuration"):
                model.load_spec(self.root)
        path.write_text(original)
        model.load_spec(self.root)

    def test_undeclared_import_or_instance_refuses(self):
        path = self.root / self.spec["model"]
        original = path.read_text()
        for mutant in [original.replace("EXTENDS Naturals, FiniteSets", "EXTENDS Naturals, FiniteSets, Other"),
                       original.replace("vars ==", "LOCAL Other == INSTANCE External\nvars ==")]:
            path.write_text(mutant)
            with self.assertRaisesRegex(ValueError, "imported-model"):
                self.bound()
        path.write_text(original)
        closure = model.model_closure(self.root, self.spec)
        self.assertEqual(closure["standard_library"]["sha256"], model.TLC_SHA256)

    def test_compiler_environment_selectors_and_external_config_refuse(self):
        cargo_home = self.root / "cargo-home"
        cargo_home.mkdir()
        environment = {"CARGO_HOME": str(cargo_home)}
        before = model.conformance_environment(self.root, environment)
        for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET",
                    "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            with self.assertRaisesRegex(ValueError, "selectors"):
                model.conformance_environment(self.root, {**environment, key: ""})
        changed = model.conformance_environment(self.root, {**environment, "CARGO_TERM_COLOR": "never"})
        self.assertNotEqual(before, changed)
        owned = model.conformance_environment(self.root, {**environment, "CARGO_INCREMENTAL": "0"})
        self.assertIn("CARGO_INCREMENTAL", owned["environment_digests"])
        with self.assertRaisesRegex(ValueError, "selectors"):
            model.conformance_environment(self.root, {**environment, "CARGO_INCREMENTAL": "1"})
        credentials = {"CARGO_REGISTRIES_PRIVATE_TOKEN": "not-a-real-token",
                       "CARGO_REGISTRY_PASSWORD": "not-a-real-password"}
        self.assertEqual(before, model.conformance_environment(self.root, {**environment, **credentials}))
        with self.assertRaisesRegex(ValueError, "credential-bearing"):
            model.conformance_environment(self.root, {**environment, "CARGO_REGISTRY_INDEX": "https://user:pass@host/index"})
        (cargo_home / "config.toml").write_text("[build]\nrustflags=[]\n")
        with self.assertRaisesRegex(ValueError, "configuration"):
            model.conformance_environment(self.root, environment)

    def test_root_cargo_config_is_tracked_and_content_bound(self):
        cargo_home = self.root / "cargo-home"
        cargo_home.mkdir()
        environment = {"CARGO_HOME": str(cargo_home)}
        config = self.root / ".cargo/config.toml"
        config.parent.mkdir()
        config.write_text("[build]\njobs = 1\n", encoding="utf-8")
        subject = {"tree": "b" * 40, "commit": "a" * 40}
        before_environment = model.conformance_environment(self.root, environment)
        before = model.binding(self.root, self.spec, "implementation_conformance",
                               {"build_environment": before_environment}, subject)
        calls = []
        def capture(command, *args, **kwargs):
            calls.append(command)
            return {}
        with patch.object(model, "run_owned", side_effect=capture), \
                patch.object(model, "require_run", return_value="a" * 40):
            model.source_subject(self.root, self.root, "fixture")
        tracked = next(command for command in calls if "ls-files" in command)
        self.assertIn("--error-unmatch", tracked)
        self.assertIn(".cargo/config.toml", tracked)
        config.write_text("[build]\njobs = 2\n", encoding="utf-8")
        after_environment = model.conformance_environment(self.root, environment)
        after = model.binding(self.root, self.spec, "implementation_conformance",
                              {"build_environment": after_environment}, subject)
        self.assertNotEqual(before_environment["root_cargo_configs"],
                            after_environment["root_cargo_configs"])
        self.assertEqual(before["implementation_digest"], after["implementation_digest"])
        self.assertNotEqual(before["toolchain_digest"], after["toolchain_digest"])
        with self.assertRaisesRegex(ValueError, "stale"):
            model.require_current_binding({"state_machine": before, "source_subject": subject},
                                          after, subject)


class NativeOutputControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def result(self, text, code=0):
        path = self.root / "native.stdout"
        path.write_text(text, encoding="utf-8")
        return {"stdout": {"path": path.name}, "exit_code": code, "error": None,
                "settlement": {"tree_settled": True, "cancelled": False}}

    def success(self, distinct=17, frontier=0):
        return (frame(2185, "Starting...")
                + frame(2193, "Model checking completed. No error has been found.")
                + frame(2199, f"42 states generated, {distinct} distinct states found, {frontier} states left on queue.")
                + frame(2186, "Finished."))

    def negative(self):
        return (frame(2185, "Starting...")
                + frame(2110, "Invariant NoFullQueryAfterRejection is violated.", 1)
                + frame(2121, "The behavior up to this point is:", 1)
                + frame(2217, "State 1: <Initial predicate>", 4)
                + frame(2217, "State 2: <Query>", 4)
                + frame(2186, "Finished."))

    def test_native_completion_requires_nonempty_finished_search(self):
        result = model.classify_tlc(self.result(self.success()), self.root)
        self.assertEqual(result["distinct_states"], 17)
        for text in [self.success(distinct=0), self.success(distinct=1), self.success(frontier=1),
                     self.success().replace(frame(2186, "Finished."), ""),
                     self.success() + frame(2186, "Finished.")]:
            with self.assertRaises(ValueError):
                model.classify_tlc(self.result(text), self.root)

    def test_unrelated_failure_is_not_a_negative_control_pass(self):
        text = self.negative()
        self.assertEqual(model.classify_tlc(self.result(text, 12), self.root,
                                           "NoFullQueryAfterRejection")["result"],
                         "expected_counterexample")
        for exit_code, expected in [(1, "NoFullQueryAfterRejection"), (12, "SingleDrain")]:
            with self.assertRaises(ValueError):
                model.classify_tlc(self.result(text, exit_code), self.root, expected)
        for mutant in [text.replace(frame(2217, "State 2: <Query>", 4), ""),
                       text + frame(2193, "Success"),
                       text + frame(9999, "another error", 1),
                       text + frame(9999, "internal bug", 2)]:
            with self.assertRaises(ValueError):
                model.classify_tlc(self.result(mutant, 12), self.root, "NoFullQueryAfterRejection")

    def test_malformed_truncated_nested_or_contradictory_frames_refuse(self):
        for suffix in ["@!@!@STARTMSG 9000:0 @!@!@\ntruncated",
                       "@!@!@STARTMSG 9000:0 @!@!@\n" + frame(2193, "nested"),
                       "@!@!@ENDMSG 9000 @!@!@\n",
                       frame(9000, "mismatch").replace("ENDMSG 9000", "ENDMSG 9001"),
                       "@!@!@STARTMSG broken\n", frame(9999, "error", 1),
                       frame(9999, "bug", 2)]:
            with self.assertRaises(ValueError):
                model.classify_tlc(self.result(self.success() + suffix), self.root)

    def test_timeout_and_unsettled_tree_never_become_passes(self):
        result = self.result(self.success())
        result["error"] = "deadline"
        with self.assertRaises(ValueError):
            model.classify_tlc(result, self.root)
        result["error"] = None
        result["settlement"]["tree_settled"] = False
        with self.assertRaises(ValueError):
            model.classify_tlc(result, self.root)

    def test_missing_child_discovery_refuses_before_launch(self):
        with patch.object(model, "child_discovery_capability", side_effect=model.Denied("missing")), \
                patch.object(model.subprocess, "Popen") as launch:
            result = model.run_owned([sys.executable, "-c", "print('must not run')"],
                                     self.root, self.root, "refusal")
        launch.assert_not_called()
        self.assertIn("missing", result["error"])

    def require_native_scope(self):
        try:
            model.child_discovery_capability()
        except model.Denied as error:
            self.skipTest("native process qualification unavailable: " + str(error))

    def test_native_output_limit_uses_existing_child_owner(self):
        self.require_native_scope()
        result = model.run_owned([sys.executable, "-c", "print('x' * 8192)"],
                                 self.root, self.root, "overflow", 5, 64)
        self.assertIn("capture bound", result["error"])
        self.assertLessEqual(result["stdout"]["bytes"], 64)
        self.assertTrue(result["settlement"]["tree_settled"])

    def test_native_timeout_reaps_detached_descendant(self):
        self.require_native_scope()
        code = (
            "import subprocess,sys,time; "
            "p=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)'],start_new_session=True); "
            "print(p.pid,flush=True); time.sleep(30)"
        )
        result = model.run_owned([sys.executable, "-c", code],
                                 self.root, self.root, "timeout", 0.3, 4096)
        self.assertIn("deadline", result["error"])
        self.assertTrue(result["settlement"]["tree_settled"])
        child = int((self.root / "timeout.stdout").read_text().strip())
        self.assertFalse(Path("/proc", str(child)).exists(), "detached child must be reaped before return")

    def test_native_java_environment_injection_is_removed(self):
        self.require_native_scope()
        code = "import os; print(','.join(k for k in " + repr(model.JAVA_INJECTION) + " if k in os.environ))"
        with patch.dict(model.os.environ, {key: "injected" for key in model.JAVA_INJECTION}):
            result = model.run_owned([sys.executable, "-c", code],
                                     self.root, self.root, "environment", 5, clean_java=True)
        self.assertEqual(model.require_run(result, self.root).strip(), "")


class SelectionControls(unittest.TestCase):
    def test_unknown_base_runs_instead_of_silently_skipping(self):
        self.assertTrue(model.select_needed(Path("/repo"), "", Path("/tmp"))[0])

    def test_native_scope_instrument_failure_runs(self):
        with patch.object(model, "run_owned", side_effect=ValueError("missing graph")):
            self.assertTrue(model.select_needed(Path("/repo"), "a" * 40, Path("/tmp"))[0])

    def test_resolved_graph_includes_local_test_dependencies(self):
        root = Path("/repo")
        metadata = {
            "packages": [
                {"id": "workspace", "manifest_path": "/repo/crates/perl-workspace/Cargo.toml", "source": None},
                {"id": "helper", "manifest_path": "/repo/crates/helper/Cargo.toml", "source": None},
                {"id": "other", "manifest_path": "/repo/crates/other/Cargo.toml", "source": None},
            ],
            "resolve": {"nodes": [
                {"id": "workspace", "deps": [{"pkg": "helper"}]},
                {"id": "helper", "deps": []}, {"id": "other", "deps": []},
            ]},
        }
        self.assertEqual(model.resolved_local_closure(metadata, root),
                         {"crates/perl-workspace/", "crates/helper/"})
        del metadata["resolve"]["nodes"][1]
        with self.assertRaises(KeyError):
            model.resolved_local_closure(metadata, root)

    def test_ci_keeps_model_and_real_api_execution_called(self):
        workflow = (model.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("python3 scripts/ci/test_workspace_index_model.py", workflow)
        self.assertIn("python3 scripts/ci/workspace_index_model.py model", workflow)
        self.assertIn("python3 scripts/ci/workspace_index_model.py conformance", workflow)
        self.assertIn("steps.workspace-index-model-scope.outputs.needed == 'true'", workflow)


if __name__ == "__main__":
    unittest.main(verbosity=2)
