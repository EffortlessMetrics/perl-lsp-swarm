"""Pure admission laws and credential-free execution of primary RC dispatch.

The #17628 tests execute reviewed literal shell through the existing graph
reader. They are not a sandbox, full Actions interpreter, authenticated channel
proof, or installed acceptance. Bash and grep are required; no Rust is built.
"""
import copy
import itertools
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest

import release_publication_admission as leaf
import check_release_prerequisite_graph as graph


def require(value, message):
    if not value:
        raise RuntimeError(message)


def rejects(call):
    try:
        call()
    except leaf.AdmissionError:
        return
    raise RuntimeError("invalid input was accepted")


def policy():
    return {
        "schema_version": "perl_lsp.release_publication_policy.v1",
        "profiles": {"public-beta-rc": {
            "required": ["github_release"], "conditional": ["open_vsx"],
            "forbidden": ["crates_io", "vscode_marketplace", "docker", "homebrew"],
            "requires_prerelease": True,
        }},
    }


def packet():
    return {
        "schema_version": "perl_lsp.release_publication_admission.v1",
        "subject": {"source_sha": "a" * 40, "prepared_sha": "b" * 40,
                    "tag": "v0.18.0-rc.1", "version": "0.18.0-rc.1",
                    "prerelease": True, "topology_sha256": "c" * 64},
        "policy": {"repository": "EffortlessMetrics/perl-lsp-swarm",
                   "workflow_revision": "d" * 40, "sha256": "e" * 64,
                   "profile_id": "public-beta-rc"},
        "producer": {"repository": "EffortlessMetrics/perl-lsp-swarm",
                     "workflow_id": 12, "ref": "refs/heads/release-candidate",
                     "head_sha": "b" * 40, "run_id": 34, "run_attempt": 2},
        "artifact": {"id": 56, "sha256": "f" * 64},
        "input_packet_sha256": "1" * 64, "terminal_manifest_sha256": "2" * 64,
        "channels": ["github_release"],
    }


def selection():
    return leaf.derive_selection(policy(), "public-beta-rc", True, ["github_release"])


class AdmissionTests(unittest.TestCase):
    def test_rc_law_is_nonqualifying_and_deterministic(self):
        selected = selection()
        require(selected == ("github_release",), "wrong derived permission")
        p = packet()
        result = leaf.validate_subject(p, copy.deepcopy(p), selected)
        require(result.qualification == "not_proven", "pure identity granted eligibility")
        require("complete_vsix" in result.missing_adapters, "missing VSIX proof hidden")
        require("producer_authentication" in result.missing_adapters, "provenance hidden")
        require(result == leaf.validate_subject(dict(reversed(list(p.items()))), p, selected),
                "map ordering changed result")

    def test_every_exact_subject_join_is_load_bearing(self):
        for section in ("subject", "policy", "producer", "artifact"):
            for key, value in packet()[section].items():
                changed = packet()
                changed[section][key] = (value + 1 if type(value) is int else
                                        not value if type(value) is bool else "9" + value[1:])
                rejects(lambda: leaf.validate_subject(changed, packet(), selection()))
        for key in ("input_packet_sha256", "terminal_manifest_sha256"):
            changed = packet()
            changed[key] = "9" * 64
            rejects(lambda: leaf.validate_subject(changed, packet(), selection()))

    def test_no_unknown_or_forbidden_publisher_alias(self):
        for channel in ("crates_io", "vscode_marketplace", "docker", "homebrew",
                        "Scoop", "Chocolatey", "WinGet", "github", "latest", "stable"):
            rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True,
                                                  ["github_release", channel]))
        rejects(lambda: leaf.validate_subject(packet(), packet(), ("github_release", "docker")))

    def test_open_vsx_requires_real_missing_adapter(self):
        rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True,
                                              ["github_release", "open_vsx"]))
        p = packet()
        p["open_vsx_evidence"] = {"status": "pass"}
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_missing_required_channel_and_duplicates_refuse(self):
        for channels in ([], ["github_release", "github_release"]):
            rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True, channels))
        p = packet()
        p["channels"] = []
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_unknown_profile_and_prerelease_false_refuse(self):
        rejects(lambda: leaf.derive_selection(policy(), "stable", True, ["github_release"]))
        rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", False, ["github_release"]))
        p = packet()
        p["subject"]["prerelease"] = False
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_unknown_skip_fields_refuse_independently(self):
        p = packet()
        leaf.validate_subject(p, p, selection())
        p["skip_crates"] = p["skip_marketplace"] = p["skip_open_vsx"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_native_boolean_and_untrusted_policy_claim_do_not_grant(self):
        p = packet()
        p["native"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))
        p = packet()
        p["policy"]["trusted"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_closed_json_and_malformed_utf8(self):
        require(leaf.parse_object(b'{"a":1}') == {"a": 1}, "valid JSON refused")
        for raw in (b'{"a":1,"a":2}', b'{"a":{"b":1,"b":2}}', b'{"a":NaN}',
                    b'{"a":Infinity}', b'{"a":1e400}', b'\xff', b'[]', b'{} trailing'):
            rejects(lambda: leaf.parse_object(raw))
        for section in (None, "subject", "policy", "producer", "artifact"):
            p = packet()
            target = p if section is None else p[section]
            target["unknown"] = "x"
            rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_malformed_exact_identity_and_boolean_numeric_refuse(self):
        for section, key, value in (("producer", "run_id", True),
                                    ("producer", "run_attempt", 0),
                                    ("artifact", "id", -1),
                                    ("subject", "source_sha", "A" * 40),
                                    ("artifact", "sha256", "short")):
            p = packet()
            p[section][key] = value
            rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_policy_partition_and_checked_in_source(self):
        path = Path(__file__).resolve().parents[2] / "policy/release-publication-admission.json"
        require(leaf.parse_object(path.read_bytes()) == policy(), "checked policy drift")
        p = policy()
        p["profiles"]["public-beta-rc"]["conditional"].append("github_release")
        rejects(lambda: leaf.derive_selection(p, "public-beta-rc", True, ["github_release"]))
        p = policy()
        p["profiles"]["public-beta-rc"]["forbidden"].append("scoop")
        rejects(lambda: leaf.derive_selection(p, "public-beta-rc", True, ["github_release"]))


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/release.yml"
REPOSITORY = "fixture-owner/fixture-release"
SHA = "a" * 40
MANIFEST = "b" * 64
WORKFLOWS = ("publish-crates.yml", "publish-extension.yml", "docker-publish.yml")
SKIP_KEYS = ("skip_crates", "skip_extension", "skip_docker")
QUALIFICATION_GUARD = (
    "${{ inputs.no_publish == false && "
    "needs.publisher-eligibility.outputs.qualification == 'satisfied' }}"
)
FAKE_GH = r'''import json, os, sys
from pathlib import Path
log = Path(os.environ["FAKE_GH_LOG"])
with log.open("a", encoding="utf-8") as out:
    out.write(json.dumps(sys.argv[1:]) + "\n")
ordinal = len(log.read_text(encoding="utf-8").splitlines())
sys.exit(73 if ordinal == int(os.environ.get("FAIL_CALL", "0")) else 0)
'''


def only_step(workflow: str, job: str, name: str) -> dict:
    # Reuse the carrier's supported workflow grammar; do not add a YAML parser.
    block = graph._step(graph._job_block(workflow, job), name)
    graph._commands(block)  # Validate the literal run shape before stripping its indent.
    script = block.split("        run: |\n", 1)[1]
    env = graph._plain_fields(graph._field(block, "env", 8, mapping=True), 10)
    return {"env": env, "run": "\n".join(line[10:] if line.strip() else ""
                                          for line in script.splitlines()) + "\n"}


@unittest.skipUnless(sys.platform.startswith("linux"), "Linux release-shell scope; no native Windows proof")
class PrimaryRcContainmentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = WORKFLOW.read_text(encoding="utf-8")
        self.step = only_step(self.workflow, "dispatch-publishers", "Dispatch primary publishers")
        self.bash = shutil.which("bash")
        self.grep = shutil.which("grep")
        self.assertIsNotNone(self.bash, "Bash is required; no skipped proof")
        self.assertIsNotNone(self.grep, "grep is required; no skipped proof")
        self.temporary = tempfile.TemporaryDirectory(prefix="release-primary-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "gh.jsonl"
        # A minimal PATH has no real gh, package manager or network client.
        # The absolute interpreter avoids accidentally finding another Python.
        fake = self.bin / "gh"
        fake.write_text(f"#!{sys.executable} -S\n{FAKE_GH}", encoding="utf-8")
        fake.chmod(0o755)
        (self.bin / "grep").symlink_to(self.grep)
        self.cwd = self.root / "empty-cwd"
        self.cwd.mkdir()

    def run_primary(self, *, version="0.18.0-rc.987", prerelease="true",
                    skips=("false", "false", "false"), tag=None, overrides=None,
                    fail_call=0, step=None):
        selected = self.step if step is None else step
        context = {
            "github.token": "fixture-only-not-a-credential",
            "github.repository": REPOSITORY,
            "github.sha": SHA,
            "needs.release-metadata.outputs.version": version,
            "needs.release-metadata.outputs.tag": "v" + version if tag is None else tag,
            "needs.release-metadata.outputs.prerelease": prerelease,
            "needs.publisher-eligibility.outputs.candidate_run_id": "12345",
            "needs.publisher-eligibility.outputs.manifest_sha256": MANIFEST,
            **{f"github.event.inputs.{key}": value for key, value in zip(SKIP_KEYS, skips)},
        }
        context.update(overrides or {})
        env = {
            "PATH": str(self.bin), "HOME": str(self.cwd), "LC_ALL": "C",
            "FAKE_GH_LOG": str(self.log), "FAIL_CALL": str(fail_call),
        }
        # Resolve only actual YAML env bindings. Inventing PRERELEASE here
        # would hide a missing production binding or use of the raw input.
        for key, value in selected.get("env", {}).items():
            expression = re.fullmatch(r"\$\{\{\s*(.*?)\s*\}\}", str(value))
            env[key] = str(context[expression[1]]) if expression else str(value)
        self.log.unlink(missing_ok=True)
        source = selected["run"]
        self.assertNotIn("${{", source, "unsupported inline Actions expression")
        self.assertEqual(list(self.cwd.iterdir()), [])
        result = subprocess.run(
            [self.bash, "--noprofile", "--norc", "-c", source],
            cwd=self.cwd, env=env, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(list(self.cwd.iterdir()), [])
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return result, calls

    def test_rc_all_skip_combinations_cannot_dispatch_primary_publishers(self):
        for skips in itertools.product(("true", "false", ""), repeat=3):
            with self.subTest(skips=skips):
                result, calls = self.run_primary(skips=skips)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls, [], "RC skip flags authorized a publisher")
                self.assertIn("Open VSX", result.stdout)
                self.assertIn("deferred", result.stdout)

    def test_hyphenated_version_cannot_be_forced_stable_at_primary_boundary(self):
        result, calls = self.run_primary(prerelease="false")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [], "hyphenated RC reached a stable primary workflow")

    def test_explicit_prerelease_of_final_spelling_is_also_contained(self):
        result, calls = self.run_primary(version="0.18.0", prerelease="true")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [])

    def test_final_dispatch_keeps_exact_selected_workflows_and_subject(self):
        for skips in itertools.product(("true", "false", ""), repeat=3):
            with self.subTest(skips=skips):
                result, calls = self.run_primary(version="0.18.0", prerelease="false", skips=skips)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = [w for w, skip in zip(WORKFLOWS, skips) if skip != "true"]
                self.assertEqual([call[3] for call in calls], [
                    f"/repos/{REPOSITORY}/actions/workflows/{w}/dispatches" for w in expected])
                for workflow, call in zip(expected, calls):
                    self.assertEqual(call[:3], ["api", "-X", "POST"])
                    self.assertIn("ref=v0.18.0", call)
                    self.assertIn("inputs[version]=0.18.0", call)
                    self.assertIn("inputs[expected_sha]=" + SHA, call)
                    self.assertIn("inputs[candidate_run_id]=12345", call)
                    self.assertIn("inputs[manifest_sha256]=" + MANIFEST, call)
                    if workflow == "publish-crates.yml":
                        self.assertIn("inputs[dry_run]=false", call)
                    if workflow == "docker-publish.yml":
                        self.assertIn("inputs[push]=true", call)

    def test_failed_primary_dispatch_does_not_continue_to_later_publishers(self):
        for fail_call in (1, 2, 3):
            with self.subTest(fail_call=fail_call):
                result, calls = self.run_primary(version="0.18.0", prerelease="false", fail_call=fail_call)
                self.assertEqual(result.returncode, 73, result.stderr)
                self.assertEqual(len(calls), fail_call)

    def test_missing_or_malformed_prerelease_is_refused_before_any_dispatch(self):
        for value in ("", "TRUE", "0", "null", "false\ntrue"):
            with self.subTest(value=value):
                result, calls = self.run_primary(version="0.18.0", prerelease=value)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("prerelease", result.stdout + result.stderr)
                self.assertEqual(calls, [])

    def test_mismatched_tag_version_is_refused_before_any_dispatch(self):
        for version, tag in (("0.18.0", "v0.18.0-rc.987"), ("0.18.0-rc.987", "v0.18.0"), ("", "v")):
            with self.subTest(version=version, tag=tag):
                result, calls = self.run_primary(version=version, tag=tag, prerelease="false")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("tag/version", result.stdout + result.stderr)
                self.assertEqual(calls, [])

    def test_existing_subject_validation_still_precedes_any_dispatch(self):
        for key, bad in (("github.sha", "bad"),
                         ("needs.publisher-eligibility.outputs.candidate_run_id", "NaN"),
                         ("needs.publisher-eligibility.outputs.manifest_sha256", "short")):
            with self.subTest(key=key):
                result, calls = self.run_primary(version="0.18.0", prerelease="false", overrides={key: bad})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])

    def test_prerelease_binding_is_derived_metadata_not_raw_operator_input(self):
        self.assertEqual(self.step["env"].get("PRERELEASE"),
                         "${{ needs.release-metadata.outputs.prerelease }}")
        step = copy.deepcopy(self.step)
        del step["env"]["PRERELEASE"]
        result, calls = self.run_primary(version="0.18.0", prerelease="false", step=step)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_private_qualification_guards_and_not_proven_producer_are_preserved(self):
        for name in ("publish-release", "dispatch-publishers"):
            self.assertEqual(graph._field(graph._job_block(self.workflow, name), "if", 4), QUALIFICATION_GUARD)
        binding = only_step(self.workflow, "publisher-eligibility", "Bind and seal terminal candidate authority")
        self.assertIn("printf 'qualification=not_proven\\n'", binding["run"])
        self.assertNotIn("qualification=satisfied", binding["run"])

    def test_guard_removal_is_detected_with_actual_recorded_dispatches(self):
        # Mutation of actual parsed workflow code, not a hand-written replica.
        source = self.step["run"]
        start = source.index("# RC_PRIMARY_CONTAINMENT_BEGIN")
        end = source.index("# RC_PRIMARY_CONTAINMENT_END", start)
        end = source.index("\n", end) + 1
        mutant = copy.deepcopy(self.step)
        mutant["run"] = source[:start] + source[end:]
        result, calls = self.run_primary(step=mutant)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(calls), 3, "mutation must really execute the forbidden requests")
        self.assertNotIn("deferred", result.stdout)

    def test_metadata_overrides_false_raw_input_for_an_rc_tag(self):
        step = only_step(self.workflow, "release-metadata", "Resolve and validate tag")
        output = self.root / "metadata-output"
        context = {"github.event.inputs.tag": "v0.18.0-rc.987", "github.event.inputs.expected_sha": SHA,
                   "github.ref_type": "branch", "github.ref_name": "main",
                   "github.event.repository.default_branch": "main", "github.event.inputs.prerelease": "false"}
        env = {"PATH": str(self.bin), "HOME": str(self.cwd), "GITHUB_SHA": SHA,
               "GITHUB_OUTPUT": str(output), "LC_ALL": "C"}
        for key, value in step["env"].items():
            match = re.fullmatch(r"\$\{\{\s*(.*?)\s*\}\}", value)
            self.assertIsNotNone(match)
            env[key] = context[match[1]]
        result = subprocess.run([self.bash, "--noprofile", "--norc", "-c", step["run"]],
                                cwd=self.cwd, env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        actual = dict(line.split("=", 1) for line in output.read_text().splitlines())
        self.assertEqual(actual, {"tag": "v0.18.0-rc.987", "version": "0.18.0-rc.987", "prerelease": "true"})
        dispatched, calls = self.run_primary(version=actual["version"], tag=actual["tag"], prerelease=actual["prerelease"])
        self.assertEqual(dispatched.returncode, 0, dispatched.stderr)
        self.assertEqual(calls, [])


if __name__ == "__main__":
    unittest.main()
