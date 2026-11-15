"""Decision-logic tests only: fake compilers are never Rust qualification."""
from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import shlex
import os
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

_SPEC = importlib.util.spec_from_file_location(
    "error_closure", Path(__file__).with_name("check_lsp_error_boundary.py")
)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError("cannot load error closure probe")
probe = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(probe)
VERSIONS = {"serde": "1.0.229", "serde_json": "1.0.151", "tracing": "0.1.44"}
BASELINE = {"version": 4, "package": [
    {"name": name, "version": version, "source": probe.REGISTRY, "checksum": "a" * 64}
    for name, version in VERSIONS.items()
]}
LOCK = {"version": 4, "package": [{"name": probe.ROOT_NAME, "version": "0.0.0",
    "dependencies": list(probe.DIRECT)}, *BASELINE["package"]]}


def lock_text(lock):
    lines = [f'version = {lock["version"]}']
    for package in lock["package"]:
        lines.append("\n[[package]]")
        for key, value in package.items():
            lines.append(f"{key} = {json.dumps(value)}")
    return "\n".join(lines) + "\n"


def completed(code=0, stdout="", stderr=""):
    return subprocess.CompletedProcess(["compiler-test-double"], code, stdout, stderr)


def failure(code="E0432", text="unresolved import `perl_parser_core`", returncode=101):
    message = {"reason": "compiler-message", "message": {
        "level": "error", "code": {"code": code}, "message": text}}
    return completed(returncode, json.dumps(message) + "\n")


def metadata(directory):
    packages = [{"id": "probe-root", "name": probe.ROOT_NAME, "version": "0.0.0", "source": None,
                 "manifest_path": str(directory / "Cargo.toml")}]
    packages.extend({"id": name, "name": name, "version": version, "source": probe.REGISTRY}
                    for name, version in VERSIONS.items())
    nodes = [{"id": "probe-root", "dependencies": list(VERSIONS),
              "deps": [{"name": name, "pkg": name} for name in VERSIONS]}]
    nodes.extend({"id": name, "dependencies": [], "deps": []} for name in VERSIONS)
    return {"packages": packages, "workspace_members": ["probe-root"], "workspace_root": str(directory),
            "resolve": {"root": "probe-root", "nodes": nodes}}


class GraphTests(unittest.TestCase):
    def test_checked_registry_graph_passes(self):
        probe.validate_lock(LOCK, BASELINE, VERSIONS)
        probe.validate_metadata(metadata(Path("/probe")), Path("/probe"), LOCK)

    def test_each_direct_dependency_is_mandatory(self):
        for name in probe.DIRECT:
            with self.subTest(name=name):
                altered = copy.deepcopy(LOCK)
                altered["package"][0]["dependencies"].remove(name)
                with self.assertRaises(probe.Rejected):
                    probe.validate_lock(altered, BASELINE, VERSIONS)

    def test_omitted_lock_row_is_rejected(self):
        altered = copy.deepcopy(LOCK)
        altered["package"].pop()
        with self.assertRaises(probe.Rejected):
            probe.validate_lock(altered, BASELINE, VERSIONS)

    def test_root_or_duplicate_lock_row_cannot_disappear(self):
        for entries in [LOCK["package"][1:], LOCK["package"] + [LOCK["package"][0]]]:
            with self.assertRaises(probe.Rejected):
                probe.validate_lock({"version": 4, "package": entries}, BASELINE, VERSIONS)

    def test_path_git_or_alternative_registry_cannot_hide_a_dependency(self):
        for source in [None, "git+https://example.invalid/pkg", "registry+https://example.invalid"]:
            altered = copy.deepcopy(LOCK)
            altered["package"][1]["source"] = source
            with self.assertRaises(probe.Rejected):
                probe.validate_lock(altered, BASELINE, VERSIONS)

    def test_known_registry_name_with_changed_bytes_or_version_is_rejected(self):
        for field, value in [("checksum", "b" * 64), ("version", "9.9.9")]:
            altered = copy.deepcopy(LOCK)
            altered["package"][1][field] = value
            with self.assertRaises(probe.Rejected):
                probe.validate_lock(altered, BASELINE, VERSIONS)

    def test_perl_transitive_dependency_is_rejected_even_when_baseline_contains_it(self):
        for name in ["perl-parser-core", "perl-tdd-support", "perllsp"]:
            entry = {"name": name, "version": "0.1.0", "source": probe.REGISTRY, "checksum": "a" * 64}
            baseline, lock = copy.deepcopy(BASELINE), copy.deepcopy(LOCK)
            baseline["package"].append(entry)
            lock["package"].append(entry)
            with self.assertRaises(probe.Rejected):
                probe.validate_lock(lock, baseline, VERSIONS)

    def test_metadata_cannot_resolve_into_root_workspace(self):
        altered = metadata(Path("/probe"))
        altered["workspace_root"] = "/checkout"
        with self.assertRaises(probe.Rejected):
            probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_metadata_cannot_use_different_manifest(self):
        altered = metadata(Path("/probe"))
        altered["packages"][0]["manifest_path"] = "/checkout/Cargo.toml"
        with self.assertRaises(probe.Rejected):
            probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_metadata_cannot_omit_or_substitute_a_package(self):
        for edit in ("omit", "source"):
            altered = metadata(Path("/probe"))
            if edit == "omit":
                altered["packages"].pop()
            else:
                altered["packages"][1]["source"] = None
            with self.assertRaises(probe.Rejected):
                probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_inactive_locked_dependency_is_not_confused_with_a_missing_resolved_node(self):
        lock = copy.deepcopy(LOCK)
        lock["package"].append({"name": "inactive-optional", "version": "1.0.0",
                                "source": probe.REGISTRY, "checksum": "a" * 64})
        probe.validate_metadata(metadata(Path("/probe")), Path("/probe"), lock)

    def test_duplicate_package_cannot_hide_a_missing_package(self):
        altered = metadata(Path("/probe"))
        altered["packages"][-1] = dict(altered["packages"][1])
        with self.assertRaises(probe.Rejected):
            probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_missing_edge_graph_is_not_effective_dependency_proof(self):
        altered = metadata(Path("/probe"))
        del altered["resolve"]
        with self.assertRaises(probe.Rejected):
            probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_resolved_edges_cannot_hide_a_missing_package(self):
        altered = metadata(Path("/probe"))
        altered["resolve"]["nodes"][0]["dependencies"].append("hidden-parser")
        with self.assertRaises(probe.Rejected):
            probe.validate_metadata(altered, Path("/probe"), LOCK)

    def test_ambiguous_direct_version_is_not_guessed(self):
        baseline = copy.deepcopy(BASELINE)
        entry = dict(baseline["package"][0], version="1.0.228")
        baseline["package"].append(entry)
        with self.assertRaises(probe.NotProven):
            probe.selected_versions(baseline)

    def test_manifest_has_own_workspace_and_no_product_or_dev_dependencies(self):
        parsed = tomllib.loads(probe.manifest(Path("/checkout"), VERSIONS))
        self.assertIn("workspace", parsed)
        self.assertFalse(parsed["package"]["publish"])
        self.assertEqual(set(parsed["dependencies"]), set(probe.DIRECT))
        self.assertNotIn("dev-dependencies", parsed)
        self.assertNotIn("patch", parsed)
        self.assertEqual(parsed["test"][0]["path"], "/checkout/" + probe.CONTRACT)

    def test_driver_links_complete_real_transport_tree_not_reconstructed_errors(self):
        source = probe.driver(Path("/checkout"))
        for relative in [probe.SOURCES[0], probe.SOURCES[1], probe.SOURCES[6]]:
            self.assertIn('/checkout/' + relative, source)
        for declaration in ["enum ", "struct ", "type ", "include_str!"]:
            self.assertNotIn(declaration, source)
        self.assertEqual(source.count("#[path = "), 3)

    def test_denominator_includes_incoming_and_cancellation_not_just_easy_types(self):
        self.assertEqual(probe.ERROR_TYPES, (
            "protocol::JsonRpcError", "transport::FramingError", "transport::IncomingMessageError",
            "runtime::cancellation::CancellationError"))
        self.assertIn(probe.CORE + "transport/incoming_error.rs", probe.SOURCES)
        self.assertEqual(len(probe.SOURCES), len(set(probe.SOURCES)))

    def test_typed_message_decoder_is_in_the_exact_source_denominator(self):
        self.assertEqual(len(probe.SOURCES), 8)
        self.assertIn(probe.CORE + "transport/incoming_message.rs", probe.SOURCES)
        self.assertEqual(probe.SOURCES[6], probe.CORE + "runtime/cancellation/mod.rs")

    def test_every_type_gets_both_direct_and_facade_control(self):
        for name in probe.ERROR_TYPES:
            for alias in (False, True):
                source = probe.mutation(name, alias)
                self.assertIn(f" for {name} ", source)
                self.assertIn("perl_parser_core", source)
                self.assertEqual("mod taxonomy" in source, alias)
        with self.assertRaises(probe.Rejected):
            probe.mutation("ignored::OtherError", False)

    def test_path_quoting_preserves_spaces_unicode_and_backslashes(self):
        path = 'C:\\workspace with spaces\\"quote"\\λ.rs'
        self.assertEqual(json.loads(probe.quoted(path)), path)
        for control in ("\n", "\t", "\r", "\x00"):
            with self.assertRaises(probe.NotProven):
                probe.quoted(path + control)


class DiagnosticTests(unittest.TestCase):
    def test_intended_direct_and_alias_errors_pass(self):
        for code in ("E0432", "E0433"):
            self.assertTrue(probe.forbidden_dependency_failure(failure(code)))

    def test_success_crash_and_dependency_acquisition_failure_never_pass(self):
        for code in (0, 1, 2, 127, -11):
            self.assertFalse(probe.forbidden_dependency_failure(failure(returncode=code)))
        self.assertFalse(probe.forbidden_dependency_failure(completed(101, "", "offline dependency missing")))

    def test_unrelated_diagnostic_never_counts(self):
        for result in (failure("E0308"), failure(text="unresolved import `another_crate`")):
            self.assertFalse(probe.forbidden_dependency_failure(result))

    def test_intended_error_cannot_hide_an_unrelated_compiler_error(self):
        result = completed(101, failure().stdout + failure("E0308").stdout)
        self.assertFalse(probe.forbidden_dependency_failure(result))

    def test_malformed_cargo_output_is_not_proof(self):
        for text in ("", "null", "[]", "{broken", '{"reason":"compiler-message","message":null}'):
            self.assertFalse(probe.forbidden_dependency_failure(completed(101, text)))


class CompilerReadSetTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="error-read-set-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.root, self.probe_dir = self.base / "source tree λ", self.base / "probe tree"
        self.paths = [self.probe_dir / "src/lib.rs", *(self.root / p for p in probe.SOURCES)]
        for path in self.paths:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("// routing test double\n")
        self.depfile = self.probe_dir / "neutral_lsp_error_probe-test.d"

    def write_dependencies(self, paths):
        self.depfile.write_text(str(self.depfile) + ": " + " ".join(shlex.quote(str(p)) for p in paths))

    def test_complete_exact_read_set_with_spaces_and_unicode(self):
        self.write_dependencies(self.paths)
        self.assertEqual(len(probe.verify_compiled_sources(self.probe_dir, self.root)), 9)

    def test_missing_dependency_evidence_is_not_proof(self):
        with self.assertRaises(probe.NotProven):
            probe.verify_compiled_sources(self.probe_dir, self.root)

    def test_omitted_source_is_rejected(self):
        self.write_dependencies(self.paths[:-1])
        with self.assertRaises(probe.Rejected):
            probe.verify_compiled_sources(self.probe_dir, self.root)

    def test_copied_source_outside_checkout_cannot_substitute_for_production(self):
        copied = self.base / "copied-source.rs"
        copied.write_bytes(self.paths[-1].read_bytes())
        self.write_dependencies([*self.paths[:-1], copied])
        with self.assertRaises(probe.Rejected):
            probe.verify_compiled_sources(self.probe_dir, self.root)

    def test_extra_hidden_source_is_rejected(self):
        extra = self.base / "hidden.rs"
        extra.write_text("// ungoverned fixture source\n")
        self.write_dependencies([*self.paths, extra])
        with self.assertRaises(probe.Rejected):
            probe.verify_compiled_sources(self.probe_dir, self.root)

    def test_make_escaped_space_round_trips(self):
        self.depfile.write_text(str(self.depfile) + ": " + " ".join(str(p).replace(" ", "\\ ") for p in self.paths))
        self.assertEqual(len(probe.verify_compiled_sources(self.probe_dir, self.root)), 9)


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="error-closure-unit-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for relative in (*probe.SOURCES, probe.CONTRACT):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("// test-double source for routing tests; not Rust proof\n")
        (self.root / "Cargo.lock").write_text(lock_text(BASELINE))
        (self.root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "stable"\n')
        self.overrides = {}
        self.calls = []

    def fake_compiler(self, command, **kwargs):
        self.calls.append((command, kwargs))
        directory = kwargs["cwd"]
        self.assertEqual(kwargs["env"]["RUSTUP_AUTO_INSTALL"], "0")
        self.assertEqual(kwargs["env"]["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(kwargs["env"]["CARGO_BUILD_JOBS"], "2")
        self.assertNotEqual(directory, self.root)
        self.assertEqual(Path(kwargs["env"]["CARGO_TARGET_DIR"]), directory / "target")
        self.assertEqual(Path(kwargs["env"]["CARGO_BUILD_BUILD_DIR"]), directory / "build")
        if "--version" in command:
            result = completed(stdout="test-double tool 1.0")
            key = "version"
        elif "metadata" in command and "--locked" not in command:
            (directory / "Cargo.lock").write_text(lock_text(LOCK))
            result, key = completed(), "lock"
        elif "metadata" in command:
            result, key = completed(stdout=json.dumps(metadata(directory))), "metadata"
        elif "test" in command:
            result, key = completed(stdout=(
                "test result: ok. 90 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
                "test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n")), "tests"
        else:
            source = (directory / "src/lib.rs").read_text()
            key = "negative" if "impl " in source else "neutral"
            result = failure() if key == "negative" else completed()
            if key == "neutral":
                depfile = directory / "build/neutral_lsp_error_probe-test.d"
                depfile.parent.mkdir(exist_ok=True)
                paths = [directory / "src/lib.rs", *(self.root / p for p in probe.SOURCES)]
                depfile.write_text(str(depfile) + ": " + " ".join(shlex.quote(str(p)) for p in paths))
        override = self.overrides.get(key)
        if callable(override):
            return override(directory, result)
        return override if override is not None else result

    def run_probe(self):
        with patch.object(probe.shutil, "which", side_effect=lambda name: "/tools/" + name), \
             patch.object(probe, "run_process", side_effect=self.fake_compiler):
            return probe.execute(self.root)

    def test_full_routing_requires_baseline_tests_eight_controls_and_restoration(self):
        code, report = self.run_probe()
        self.assertEqual(code, 0, report)
        names = [step["name"] for step in report["steps"]]
        self.assertEqual(len(names), 15)
        self.assertEqual(sum(n.startswith(("alias:", "direct:")) for n in names), 8)
        self.assertEqual(names[-1], "restored-neutral")
        self.assertEqual(report["denominator"], list(probe.ERROR_TYPES))
        self.assertFalse(self.calls[0][1]["cwd"].exists(), "temporary build must be removed")

    def test_missing_tools_are_not_proof(self):
        with patch.object(probe.shutil, "which", return_value=None):
            code, report = probe.execute(self.root)
        self.assertEqual(code, 2)
        self.assertEqual(report["reason"], "cargo_or_rustc_not_found")
        self.assertEqual(report["steps"], [])

    def test_missing_production_source_is_not_replaced(self):
        (self.root / probe.SOURCES[-1]).unlink()
        code, report = self.run_probe()
        self.assertEqual(code, 2)
        self.assertEqual(report["steps"], [])

    def test_offline_resolution_failure_is_not_a_passing_negative_control(self):
        self.overrides["lock"] = completed(101, stderr="missing offline package")
        code, report = self.run_probe()
        self.assertEqual(code, 2)
        self.assertEqual(report["reason"], "isolated_offline_dependencies_unavailable")

    def test_positive_compile_failure_stops_before_controls(self):
        self.overrides["neutral"] = failure()
        code, report = self.run_probe()
        self.assertEqual(code, 1)
        self.assertFalse(any(step["name"].startswith("direct:") for step in report["steps"]))

    def test_negative_success_rejects_candidate(self):
        self.overrides["negative"] = completed()
        code, report = self.run_probe()
        self.assertEqual(code, 1)
        self.assertIn("taxonomy_control", report["reason"])

    def test_zero_filtered_ignored_or_missing_contract_suite_cannot_pass(self):
        for summary in (
            "test result: ok. 0 passed; 0 failed; 0 ignored;",
            "test result: ok. 9 passed; 0 failed; 1 ignored;",
            "test result: ok. 8 passed; 0 failed; 0 ignored;",
            "",
        ):
            self.overrides["tests"] = completed(stdout="test result: ok. 90 passed; 0 failed; 0 ignored;\n" + summary)
            code, report = self.run_probe()
            self.assertEqual(code, 1)
            self.assertEqual(report["reason"], "production_and_contract_tests_not_proven")

    def test_changed_source_is_not_credited_with_old_compiler_result(self):
        def mutate_source(directory, result):
            (self.root / probe.SOURCES[0]).write_text("changed after compile\n")
            return result
        self.overrides["neutral"] = mutate_source
        code, report = self.run_probe()
        self.assertEqual(code, 2)
        self.assertEqual(report["reason"], "selected_sources_changed_during_probe")

    def test_changed_lock_is_not_accepted(self):
        def mutate_lock(directory, result):
            path = directory / "Cargo.lock"
            path.write_text(path.read_text() + "# changed\n")
            return result
        self.overrides["negative"] = mutate_lock
        code, report = self.run_probe()
        self.assertEqual(code, 2)
        self.assertEqual(report["reason"], "lock_changed_during_probe")

    def test_timeout_is_not_proof(self):
        def timeout(directory, result):
            raise subprocess.TimeoutExpired("cargo", 240)
        self.overrides["neutral"] = timeout
        code, report = self.run_probe()
        self.assertEqual(code, 2)
        self.assertEqual(report["outcome"], "not_proven")


@unittest.skipUnless(os.name == "posix", "POSIX process-group execution only")
class ProcessSettlementTests(unittest.TestCase):
    def test_real_process_result_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix="probe-process-") as temporary:
            result = probe.run_process(
                [sys.executable, "-c", "print('executed'); raise SystemExit(7)"],
                cwd=Path(temporary), env=dict(os.environ), timeout=5)
        self.assertEqual(result.returncode, 7)
        self.assertEqual(result.stdout.strip(), "executed")

    def test_real_timeout_kills_process_group_before_return(self):
        with tempfile.TemporaryDirectory(prefix="probe-timeout-") as temporary:
            with self.assertRaises(subprocess.TimeoutExpired):
                probe.run_process([sys.executable, "-c", "import time; time.sleep(60)"],
                                  cwd=Path(temporary), env=dict(os.environ), timeout=0.05)


if __name__ == "__main__":
    unittest.main()
