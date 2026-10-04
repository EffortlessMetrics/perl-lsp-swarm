#!/usr/bin/env python3
"""Execute the real governed entry point with recording tools and real Git refs.

These are adapter controls, not execution of RIPR or its domain validators.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PROOF = ROOT / ".ci/ripr-proof.sh"
GIT_LOCAL_ENV = subprocess.check_output(
    ["git", "rev-parse", "--local-env-vars"], text=True).split()
BASE_ENV = {k: v for k, v in os.environ.items() if k not in GIT_LOCAL_ENV}
BASE_ENV.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
TOOL = r'''#!/usr/bin/env python3
import json, os, pathlib, signal, sys
args = sys.argv[1:]
if pathlib.Path(sys.argv[0]).name == 'ripr':
    print(os.environ.get('TEST_RIPR_VERSION', 'ripr 0.10.1'))
    raise SystemExit(0)
with open(os.environ['TEST_COMMANDS'], 'a') as stream:
    stream.write(json.dumps(args) + '\n')
command = args[1]
check = '--check' in args
def write(path, text='current fixture\n'):
    p = pathlib.Path(path); p.parent.mkdir(parents=True, exist_ok=True); p.write_text(text)
if not check:
    if command == 'ripr-pr':
        if os.environ.get('TEST_CANCEL_BEFORE_FRESH'):
            os.kill(os.getppid(), signal.SIGTERM)
            raise SystemExit(0)
        freshness = os.environ.get('TEST_FRESHNESS', 'current')
        if freshness != 'missing':
            token = os.environ['RIPR_FRESHNESS_TOKEN'] if freshness == 'current' else 'older-run/1/1'
            write(os.environ['RIPR_FRESHNESS_HANDOFF'] + '/clear-succeeded', token + '\n')
        if not os.environ.get('TEST_MISSING_RAW'):
            write('target/ripr/pr/raw-check.json')
        write('target/ripr/pr/repo-exposure.json')
        if os.environ.get('TEST_HIDDEN'):
            write('target/ripr/pr/.private-diagnostic', 'excluded hidden fixture')
            write('target/ripr/stdout-staging/raw-check.partial-fixture', 'unpublished partial')
        if os.environ.get('TEST_LINK'):
            pathlib.Path('target/ripr/pr/unsafe-link').symlink_to('/etc/passwd')
    elif command == 'ripr-plus':
        write('target/receipts/quality/ripr-plus.json')
    elif command == 'ripr-review-comments':
        write('target/ripr/review/comments.json')
        if os.environ.get('TEST_CANCEL'):
            os.kill(os.getppid(), signal.SIGTERM)
        if os.environ.get('TEST_DESTINATION_ANCESTOR'):
            pathlib.Path('.ci/artifacts/ripr/target').symlink_to(
                os.environ['TEST_OUTSIDE'], target_is_directory=True)
        if os.environ.get('TEST_SOURCE_ANCESTOR'):
            old = pathlib.Path('target/ripr')
            old.rename('target/ripr-current')
            old.symlink_to(os.environ['TEST_OUTSIDE'], target_is_directory=True)
        if os.environ.get('TEST_UNREADABLE'):
            write('target/ripr/pr/unreadable/diagnostic.json')
    elif command == 'impacted-evidence':
        if not os.environ.get('TEST_MISSING_IMPACTED'):
            write('target/xtask/impacted-evidence/latest.json')
            write('target/xtask/impacted-evidence/latest.md')
    elif command == 'quality-gate':
        write('target/receipts/quality/quality-gate-ripr.json')
        write('target/receipts/quality/quality-gate-ripr.md')
failure = os.environ.get('TEST_FAIL_COMMAND')
fail_phase = os.environ.get('TEST_FAIL_PHASE', 'generate')
raise SystemExit(1 if command == failure and check == (fail_phase == 'check') else 0)
'''


class GovernedRiprProof(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        self.repo.mkdir()
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.commit("base.txt", "base")
        self.git("checkout", "-b", "feature")
        self.head = self.commit("feature.txt", "feature")
        self.git("checkout", "main")
        self.base = self.commit("main.txt", "updated base")
        self.git("merge", "--no-ff", "feature", "-m", "evaluated merge")
        self.sha = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        self.assertNotEqual(self.head, self.sha)
        (self.repo / ".ci").mkdir()
        shutil.copyfile(PROOF, self.repo / ".ci/ripr-proof.sh")
        self.bin = Path(self.temp.name) / "bin"
        self.bin.mkdir()
        for name in ("cargo", "ripr"):
            tool = self.bin / name
            tool.write_text(TOOL)
            tool.chmod(0o755)
        self.commands = Path(self.temp.name) / "commands.jsonl"
        self.event = Path(self.temp.name) / "event.json"
        self.payload = {"repository": {"default_branch": "main"}, "pull_request": {
            "base": {"sha": self.base, "repo": {"full_name": "EffortlessMetrics/perl-lsp-swarm"}}, "head": {"sha": self.head, "repo": {
                "full_name": "EffortlessMetrics/perl-lsp-swarm"}},
            "labels": [{"name": "needs review $(false)"}]}}
        self.env = dict(BASE_ENV, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        GITHUB_WORKSPACE=str(self.repo), GITHUB_SHA=self.sha,
                        GITHUB_REPOSITORY="EffortlessMetrics/perl-lsp-swarm",
                        GITHUB_EVENT_NAME="pull_request", GITHUB_EVENT_PATH=str(self.event),
                        GITHUB_RUN_ID="987654", GITHUB_RUN_ATTEMPT="4",
                        RUNNER_TEMP=self.temp.name, TEST_COMMANDS=str(self.commands))

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args],
                                       text=True, stderr=subprocess.DEVNULL,
                                       env=BASE_ENV).strip()

    def commit(self, path, text):
        (self.repo / path).write_text(text)
        self.git("add", path)
        self.git("commit", "-m", text)
        return self.git("rev-parse", "HEAD")

    def run_proof(self, **env):
        self.event.write_text(json.dumps(self.payload))
        return subprocess.run(["bash", ".ci/ripr-proof.sh"], cwd=self.repo,
                              env=dict(self.env, **env), text=True, capture_output=True,
                              timeout=20)

    def calls(self):
        return [json.loads(line) for line in self.commands.read_text().splitlines()]

    def artifact(self, path=""):
        return self.repo / ".ci/artifacts/ripr" / path

    def test_complete_pipeline_keeps_separate_event_and_pr_identity(self):
        result = self.run_proof()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        for command in ("ripr-pr", "ripr-review-comments"):
            matching = [call for call in calls if call[1] == command]
            self.assertEqual(len(matching), 2)
            for call in matching:
                self.assertEqual(call[call.index("--base") + 1], self.base)
                self.assertEqual(call[call.index("--pr-head") + 1], self.head)
        gates = [call for call in calls if call[1] == "quality-gate"]
        self.assertEqual(len(gates), 2)
        self.assertIn("--check", gates[1])
        self.assertEqual(gates[0][gates[0].index("--mode") + 1], "enforce-new-ripr")
        impacted = [call for call in calls if call[1] == "impacted-evidence"]
        self.assertEqual(impacted[0][-1], "--labels-csv=needs review $(false)")
        self.assertTrue(self.artifact("target/ripr/pr/raw-check.json").is_file())
        badge = json.loads(self.artifact("target/receipts/quality/ripr-badge-producer.json").read_text())
        self.assertEqual(badge["head"], self.sha)
        self.assertEqual(badge["ripr_version"], "0.10.1")

    def test_wrong_evaluated_sha_never_enters_generation(self):
        result = self.run_proof(GITHUB_SHA=self.head)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evaluated event SHA", result.stderr)
        self.assertFalse(self.commands.exists())

    def test_wrong_image_tool_is_not_reinstalled(self):
        result = self.run_proof(TEST_RIPR_VERSION="ripr 0.10.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("image-owned RIPR", result.stderr)
        self.assertFalse(self.commands.exists())

    def test_foreign_pr_subject_is_refused(self):
        self.payload["pull_request"]["head"]["repo"]["full_name"] = "foreign/repo"
        result = self.run_proof()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.commands.exists())

    def test_foreign_base_repository_is_refused_before_generation(self):
        self.payload['pull_request']['base']['repo']['full_name'] = 'foreign/repository'
        result = self.run_proof()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.commands.exists())

    def test_event_revision_outside_evaluated_tree_cannot_become_fresh(self):
        self.git('checkout', '-b', 'unmerged', self.base)
        other = self.commit('unmerged.txt', 'unmerged source')
        self.git('checkout', '--detach', self.sha)
        for side in ('base', 'head'):
            with self.subTest(side=side):
                previous = self.payload['pull_request'][side]['sha']
                self.payload['pull_request'][side]['sha'] = other
                result = self.run_proof()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.commands.exists())
                self.payload['pull_request'][side]['sha'] = previous

    def test_inherited_git_overrides_cannot_redirect_the_checkout(self):
        other = Path(self.temp.name) / 'other'
        other.mkdir()
        subprocess.run(['git', '-C', str(other), 'init', '--initial-branch=main'], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        index = Path(self.temp.name) / 'outside-index'
        index.write_bytes(b'preserve caller data')
        result = self.run_proof(GIT_DIR=str(other / '.git'), GIT_WORK_TREE=str(other),
                                GIT_INDEX_FILE=str(index), GIT_CONFIG_COUNT='1',
                                GIT_CONFIG_KEY_0='core.bare', GIT_CONFIG_VALUE_0='true')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(index.read_bytes(), b'preserve caller data')
        self.assertFalse((other / 'target').exists())

    def test_missing_revision_cannot_become_fresh(self):
        self.payload["pull_request"]["base"]["sha"] = "f" * 40
        self.assertNotEqual(self.run_proof().returncode, 0)
        self.assertFalse(self.commands.exists())

    def test_missing_and_previous_attempt_freshness_suppress_old_files(self):
        for freshness in ("missing", "previous"):
            with self.subTest(freshness=freshness):
                result = self.run_proof(TEST_FRESHNESS=freshness)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.artifact("target").exists())
                self.assertFalse(any("--check" in call for call in self.calls()))
                self.commands.unlink()

    def test_generation_failure_stays_red_with_current_partial_diagnostics(self):
        result = self.run_proof(TEST_FAIL_COMMAND="ripr-plus")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.artifact("target/ripr/pr/raw-check.json").is_file())
        self.assertEqual(len([call for call in self.calls() if call[1] == "quality-gate"]), 2)

    def test_validator_failure_and_genuine_gap_are_not_promoted(self):
        for command, phase in (("ripr-review-comments", "check"), ("quality-gate", "generate")):
            with self.subTest(command=command):
                result = self.run_proof(TEST_FAIL_COMMAND=command, TEST_FAIL_PHASE=phase)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(self.artifact("target/ripr/pr/raw-check.json").is_file())
                shutil.rmtree(self.artifact())

    def test_missing_raw_receipt_stays_blocking(self):
        result = self.run_proof(TEST_MISSING_RAW="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing RIPR proof file", result.stderr)

    def test_missing_impacted_receipts_are_not_covered_by_other_success(self):
        result = self.run_proof(TEST_MISSING_IMPACTED="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("impacted-evidence/latest.json", result.stderr)

    def test_hidden_and_noncanonical_staging_are_excluded_without_dropping_raw(self):
        result = self.run_proof(TEST_HIDDEN="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.artifact("target/ripr/pr/raw-check.json").is_file())
        self.assertFalse(self.artifact("target/ripr/pr/.private-diagnostic").exists())
        self.assertFalse(self.artifact("target/ripr/stdout-staging").exists())

    def test_generated_symlink_cannot_export_private_bytes_or_pass(self):
        result = self.run_proof(TEST_LINK="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.artifact("target/ripr/pr/unsafe-link").exists())

    def outside(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        (outside / "private.json").write_text("private sentinel")
        return outside

    def test_source_ancestors_are_guarded_before_producer_invalidation(self):
        outside = self.outside()
        for ancestor in ("target", "target/ripr", "target/receipts", "target/xtask"):
            with self.subTest(ancestor=ancestor):
                path = self.repo / ancestor
                path.parent.mkdir(parents=True, exist_ok=True)
                path.symlink_to(outside, target_is_directory=True)
                result = self.run_proof()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("RIPR ancestor", result.stderr)
                self.assertFalse(self.commands.exists())
                self.assertEqual([p.name for p in outside.iterdir()], ["private.json"])
                path.unlink()
                shutil.rmtree(self.artifact())

    def test_changed_source_ancestor_cannot_export_outside_bytes(self):
        outside = self.outside()
        (outside / "pr").mkdir()
        (outside / "pr/private-diag.json").write_text("private")
        result = self.run_proof(TEST_SOURCE_ANCESTOR="1", TEST_OUTSIDE=str(outside))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.artifact("target/ripr/pr/private-diag.json").exists())
        self.assertEqual((outside / "private.json").read_text(), "private sentinel")

    def test_destination_ancestor_cannot_write_outside_reserved_root(self):
        outside = self.outside()
        result = self.run_proof(TEST_DESTINATION_ANCESTOR="1", TEST_OUTSIDE=str(outside))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([p.name for p in outside.iterdir()], ["private.json"])

    def test_traversal_error_is_red_instead_of_silently_omitting_diagnostics(self):
        # Inject only the directory scan failure; execute the real Bash copier.
        hook = Path(self.temp.name) / "hook"
        hook.mkdir()
        (hook / "sitecustomize.py").write_text(
            "import os\noriginal = os.scandir\n"
            "def controlled(path):\n"
            "    if str(path).endswith('/pr/unreadable'):\n"
            "        raise PermissionError('controlled scan failure')\n"
            "    return original(path)\n"
            "os.scandir = controlled\n")
        result = self.run_proof(TEST_UNREADABLE="1", PYTHONPATH=str(hook))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("controlled scan failure", result.stderr)
        self.assertFalse(self.artifact("target/ripr/pr/unreadable/diagnostic.json").exists())

    def test_preseeded_artifact_root_is_not_reused_or_deleted(self):
        self.artifact().mkdir(parents=True)
        prior = self.artifact("prior-attempt.json")
        prior.write_text("prior")
        result = self.run_proof()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(prior.read_text(), "prior")
        self.assertFalse(self.commands.exists())

    def test_symlink_artifact_root_is_refused(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        (self.repo / ".ci/artifacts").symlink_to(outside, target_is_directory=True)
        self.assertNotEqual(self.run_proof().returncode, 0)
        self.assertEqual(list(outside.iterdir()), [])

    def test_cancellation_stays_red_and_does_not_enter_validation(self):
        result = self.run_proof(TEST_CANCEL="1")
        self.assertEqual(result.returncode, 143, result.stderr)
        self.assertFalse(any("--check" in call for call in self.calls()))

    def test_cancellation_before_invalidation_preserves_signal_and_suppresses_files(self):
        result = self.run_proof(TEST_CANCEL_BEFORE_FRESH="1")
        self.assertEqual(result.returncode, 143, result.stderr)
        self.assertFalse(self.artifact("target").exists())
        self.assertEqual(len(self.calls()), 1)

    def test_leading_hyphen_label_is_bound_as_data_in_both_phases(self):
        self.payload["pull_request"]["labels"] = [{"name": "--help"}]
        result = self.run_proof()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [call for call in self.calls() if call[1] == "impacted-evidence"]
        self.assertEqual(len(calls), 2)
        for call in calls:
            self.assertIn("--labels-csv=--help", call)
            self.assertNotIn("--help", call)

    def test_merge_group_has_no_single_pr_head(self):
        self.payload["merge_group"] = {"base_sha": self.base, "head_sha": self.sha}
        result = self.run_proof(GITHUB_EVENT_NAME="merge_group")
        self.assertEqual(result.returncode, 0, result.stderr)
        call = self.calls()[0]
        self.assertEqual(call[call.index("--pr-head") + 1], "")

    def test_wrong_merge_group_head_is_refused_before_generation(self):
        self.payload['merge_group'] = {'base_sha': self.base, 'head_sha': self.head}
        result = self.run_proof(GITHUB_EVENT_NAME='merge_group')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.commands.exists())

    def test_non_pr_events_resolve_base_from_default_branch(self):
        for event in ("push", "workflow_dispatch", "schedule"):
            with self.subTest(event=event):
                result = self.run_proof(GITHUB_EVENT_NAME=event)
                self.assertEqual(result.returncode, 0, result.stderr)
                call = self.calls()[0]
                self.assertEqual(call[call.index("--base") + 1], self.base)
                self.assertEqual(call[call.index("--pr-head") + 1], "")
                self.commands.unlink()
                shutil.rmtree(self.artifact())
                shutil.rmtree(self.repo / "target", ignore_errors=True)

    def test_push_punctuation_branch_name_resolves(self):
        branch = "release+hotfix"
        # Point the punctuation ref at the feature commit, NOT at self.base:
        # origin/main already resolves to self.base, so aliasing the two would
        # let a validate-then-hardcode-origin/main mutant pass this test.
        self.assertNotEqual(self.head, self.base)
        self.git("update-ref", "refs/remotes/origin/" + branch, self.head)
        self.payload["repository"]["default_branch"] = branch
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertEqual(result.returncode, 0, result.stderr)
        call = self.calls()[0]
        self.assertEqual(call[call.index("--base") + 1], self.head)

    def test_push_invalid_default_branch_is_refused(self):
        self.payload["repository"]["default_branch"] = "main..x"
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid repository default branch", result.stderr)
        self.assertFalse(self.commands.exists())

    def test_push_missing_default_branch_is_refused(self):
        del self.payload["repository"]["default_branch"]
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid repository default branch", result.stderr)
        self.assertFalse(self.commands.exists())

    def test_push_unreachable_default_ref_is_refused_without_traceback(self):
        self.git("update-ref", "-d", "refs/remotes/origin/main")
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not reachable", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertFalse(self.commands.exists())

    def test_push_base_equal_to_head_declares_empty_range(self):
        self.git("update-ref", "refs/remotes/origin/main", self.sha)
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("covers an empty range", result.stderr)
        call = self.calls()[0]
        self.assertEqual(call[call.index("--base") + 1], self.sha)

    def test_push_base_ahead_of_checkout_declares_empty_range(self):
        # Main-moved race: the default-branch ref resolves to a descendant
        # of the evaluated checkout, so the three-dot base...HEAD scope is
        # empty even though the revisions differ. Build the descendant on a
        # side branch so HEAD stays at the evaluated SHA.
        self.git("checkout", "-b", "ahead")
        newer = self.commit("ahead.txt", "ahead")
        self.git("update-ref", "refs/remotes/origin/main", newer)
        self.git("checkout", "main")
        result = self.run_proof(GITHUB_EVENT_NAME="push")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("base is ahead of the evaluated checkout", result.stderr)
        self.assertIn("covers an empty range", result.stderr)
        call = self.calls()[0]
        self.assertEqual(call[call.index("--base") + 1], newer)


if __name__ == "__main__":
    unittest.main()
