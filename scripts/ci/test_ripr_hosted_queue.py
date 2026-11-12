#!/usr/bin/env python3
"""Offline production-shell controls, not RIPR domain or hosted-capacity proof."""
import os
import copy
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from bash_binary import bash_binary, bash_path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ripr.yml"


def job(name):
    return re.search(r"^  " + re.escape(name) + r":\n(.*?)(?=^  [\w-]+:|\Z)",
                     WORKFLOW.read_text(encoding="utf-8"), re.M | re.S).group(1)


def body(name, step):
    block = job(name).split("      - name: " + step + "\n", 1)[1]
    block = block.split("\n      - ", 1)[0]
    return "\n".join(line[10:] for line in block.split("        run: |\n", 1)[1].splitlines()
                     if line.startswith("          ") or not line.strip())


class HostedQueue(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.env = dict(os.environ)
        # Never inherit the caller's repository identity into fixture Git.
        names = subprocess.check_output(["git", "rev-parse", "--local-env-vars"], text=True, encoding="utf-8").split()
        for key in names:
            self.env.pop(key, None)
        self.env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        self.git("init", "--initial-branch=main")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "--allow-empty", "-m", "trusted-control")
        self.trusted = self.git("rev-parse", "HEAD")
        self.bin = self.repo / "bin"
        self.bin.mkdir()
        # An API recorder supplies only fixture data, with no subprocess/network.
        gh = self.bin / "gh"
        fake = self.repo / "fake_gh.py"
        fake.write_text("""#!/usr/bin/env python3
import json, os, sys
with open(os.environ['API_CALLS'], 'a', encoding='utf-8') as f:
    f.write(json.dumps(sys.argv[1:]) + '\\n')
args = ' '.join(sys.argv[1:])
if '/logs' in args:
    print(os.environ.get('LANE_LOG', 'ordinary compilation failure'))
elif '/annotations' in args:
    print('[]')
elif '{conclusion:' in args:
    print(json.dumps({'conclusion': os.environ['GITHUB_RESULT'], 'steps': []}))
else:
    print('123')
""", encoding="utf-8", newline="\n")
        gh.write_text('#!/usr/bin/env bash\nexec "$FAKE_PYTHON" "$FAKE_GH" "$@"\n',
                      encoding="utf-8", newline="\n")
        gh.chmod(0o755)
        scripts = self.repo / "scripts/ci"
        scripts.mkdir(parents=True)
        for name in ("classify-ripr-lane-termination", "preserve-lane-log-tail.sh"):
            shutil.copyfile(ROOT / "scripts/ci" / name, scripts / name)
        self.env.update(
            FAKE_BIN=bash_path(self.bin), FAKE_PYTHON=bash_path(Path(sys.executable)),
            FAKE_GH=bash_path(fake), PYTHONUTF8="1",
            EVENT_NAME="merge_group", GITHUB_SHA="a" * 40, MERGE_GROUP_SHA="a" * 40,
            PROOF_SHA="a" * 40, GITHUB_RESULT="success",
            GITHUB_RUN_ID="1234", GITHUB_RUN_ATTEMPT="2",
            PROOF_RUN_ID="1234", PROOF_RUN_ATTEMPT="2",
            RIPR_DECISION_SHA=self.trusted,
            RIPR_GATE_DEADLINE_EPOCH=str(int(time.time()) + 300),
            GITHUB_STEP_SUMMARY=bash_path(self.repo / "summary"),
            GITHUB_OUTPUT=bash_path(self.repo / "output"),
            GITHUB_REPOSITORY="fixture/repo", API_CALLS=bash_path(self.repo / "api"),
        )

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, env=self.env,
                                       stderr=subprocess.DEVNULL, text=True, encoding="utf-8").strip()

    def execute(self, script=None, **env):
        return subprocess.run([bash_binary(), "-euo", "pipefail", "-c",
                               'export PATH="$(cd "$FAKE_BIN" && pwd):$PATH"\n' +
                               (script or body("ripr", "Evaluate hosted result"))],
                              cwd=self.repo, env=self.env | env,
                              text=True, encoding="utf-8", capture_output=True, timeout=10)

    def test_exact_current_attempt_succeeds_without_api(self):
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("RIPR_GATE_EVALUATION=success", result.stdout)
        self.assertFalse((self.repo / "api").exists())

    def test_success_requires_g_run_attempt_and_control_identity(self):
        for change in (
            {"EVENT_NAME": "pull_request"}, {"EVENT_NAME": "workflow_dispatch"},
            {"MERGE_GROUP_SHA": ""}, {"MERGE_GROUP_SHA": "b" * 40},
            {"PROOF_SHA": ""}, {"PROOF_SHA": "b" * 40},
            {"PROOF_RUN_ID": "1233"}, {"PROOF_RUN_ATTEMPT": "1"},
            {"PROOF_RUN_ATTEMPT": ""}, {"GITHUB_RUN_ID": "bad"},
            {"GITHUB_RUN_ATTEMPT": "0"}, {"RIPR_DECISION_SHA": ""},
            {"RIPR_DECISION_SHA": "c" * 40},
            {"RIPR_GATE_DEADLINE_EPOCH": "0"},
        ):
            with self.subTest(change=change):
                result = self.execute(**change)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("RIPR_GATE_VERDICT=success", result.stdout)

    def test_failure_skipped_missing_and_cancelled_never_green(self):
        for outcome in ("failure", "skipped", "", "cancelled"):
            with self.subTest(outcome=outcome):
                result = self.execute(GITHUB_RESULT=outcome)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertNotIn("RIPR_GATE_VERDICT=success", result.stdout)
                self.assertIn("RIPR_GATE_VERDICT=", result.stdout)
        calls = (self.repo / "api").read_text(encoding="utf-8")
        self.assertIn("/runs/1234/attempts/2/jobs", calls)

    def test_positive_eviction_and_real_gap_both_block_with_distinct_class(self):
        for log, verdict in (
            ("The runner has received a shutdown signal", "infra-no-proof"),
            ("quality gate failed; see receipt x\nThe runner has received a shutdown signal", "ripr-failure"),
        ):
            with self.subTest(verdict=verdict):
                result = self.execute(GITHUB_RESULT="failure", LANE_LOG=log)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("RIPR_GATE_VERDICT=" + verdict, result.stdout)
                receipt = (self.repo / "ripr-gate-classification.env").read_text(encoding="utf-8")
                for field in ("run_id=1234", "run_attempt=2", "head_sha=" + "a"*40,
                              "decision_sha=" + self.trusted):
                    self.assertIn(field, receipt)

    def test_result_only_rerun_negative_control_discriminates_weakened_body(self):
        script = body("ripr", "Evaluate hosted result")
        # Plausible bug: check G and run, but accept prior producer attempt.
        weakened = script.replace(
            '[ "$GITHUB_RUN_ATTEMPT" != "${PROOF_RUN_ATTEMPT:-}" ]', "false", 1)
        self.assertNotEqual(script, weakened)
        self.assertNotEqual(self.execute(PROOF_RUN_ATTEMPT="1").returncode, 0)
        self.assertEqual(self.execute(weakened, PROOF_RUN_ATTEMPT="1").returncode, 0)

    def test_producer_checks_real_checkout_and_emits_identity(self):
        script = body("ripr-github", "Verify merge-group source")
        result = self.execute(script, GITHUB_SHA=self.trusted, MERGE_GROUP_SHA=self.trusted)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.repo / "output").read_text(encoding="utf-8"),
                         f"subject_sha={self.trusted}\nrun_id=1234\nrun_attempt=2\n")
        self.assertNotEqual(self.execute(script).returncode, 0)

    def test_single_hosted_producer_and_trusted_control_separation(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertEqual(re.findall(r"^  ([\w-]+):$", text.split("jobs:\n", 1)[1], re.M),
                         ["ripr-github", "seed-cache", "ripr"])
        for retired in ("EM_RUNNER_READ_TOKEN", "orgs/$ORG/actions/runners",
                        "route-ripr", "ripr-selfhosted", "ripr-fallback", "actions: write"):
            self.assertNotIn(retired, text)
        producer, finalizer = job("ripr-github"), job("ripr")
        self.assertIn("if: github.event_name == 'merge_group'", producer)
        self.assertIn("ref: ${{ github.event.merge_group.head_sha }}", producer)
        self.assertIn("RIPR_BASE_SHA: ${{ github.event.merge_group.base_sha }}", producer)
        self.assertIn("needs: [ripr-github]", finalizer)
        self.assertIn("if: always() && github.event_name == 'merge_group'", finalizer)
        self.assertIn("ref: ${{ steps.evaluator.outputs.sha }}", finalizer)
        self.assertNotIn("ref: refs/heads/main", finalizer)
        self.assertNotIn("ref: ${{ github.event.merge_group.base_sha }}", finalizer)
        self.assertNotIn("ref: ${{ github.event.merge_group.head_sha }}", finalizer)
        self.assertIn("timeout-minutes: 135", producer)
        for field in ("subject_sha", "run_id", "run_attempt"):
            self.assertIn(f"{field}: ${{{{ steps.subject.outputs.{field} }}}}", producer)
            self.assertIn(f"needs.ripr-github.outputs.{field}", finalizer)

    def authority_fixture(self):
        repository = {"full_name": "EffortlessMetrics/perl-lsp-swarm", "id": 42,
                      "node_id": "R_fixture", "default_branch": "main"}
        return {
            "repository": repository,
            "branch": {"name": "main", "protected": True, "commit": {"sha": self.trusted}},
            "ref": {"ref": "refs/heads/main", "object": {"type": "commit", "sha": self.trusted}},
            "context": {"eventName": "merge_group",
                        "repo": {"owner": "EffortlessMetrics", "repo": "perl-lsp-swarm"},
                        "payload": {"repository": copy.deepcopy(repository), "action": "checks_requested",
                                    "merge_group": {"base_ref": "refs/heads/main",
                                                    "base_sha": "b" * 40, "head_sha": "a" * 40}}},
        }

    def execute_authority(self, phase, fixture=None, **overrides):
        block = job("ripr").split("      - name: " + phase + "\n", 1)[1].split("\n      - ", 1)[0]
        script = "\n".join(line[12:] for line in block.split("          script: |\n", 1)[1].splitlines()
                           if line.startswith("            ") or not line.strip())
        harness = "const f=" + json.dumps(fixture or self.authority_fixture()) + ";" + """
const context=f.context, calls=[];
const read=key=>async args=>{
  if(args.owner!=='EffortlessMetrics'||args.repo!=='perl-lsp-swarm') throw Error('wrong API scope');
  if(key==='branch'&&args.branch!=='main') throw Error('wrong branch');
  if(key==='ref'&&args.ref!=='heads/main') throw Error('wrong ref');
  calls.push(key); return {data:f[key]};
};
const github={rest:{repos:{get:read('repository'),getBranch:read('branch')},git:{getRef:read('ref')}}};
const core={setOutput:(key,value)=>console.log(key+'='+value)};
(async()=>{
""" + script + """
if(calls.join(',')!=='repository,branch,ref') throw Error('incomplete independent reads');
})().catch(e=>{console.error(e.message);process.exitCode=1});
"""
        return subprocess.run(["node", "-e", harness], cwd=self.repo,
                              env=self.env | overrides, text=True, encoding="utf-8",
                              capture_output=True, timeout=10)

    def test_protected_t_distinct_from_b_and_g_and_current_on_both_sides(self):
        self.assertNotIn(self.trusted, ("a" * 40, "b" * 40))
        for phase in ("Resolve protected main evaluator generation",
                      "Revalidate protected decision authority before evaluation",
                      "Revalidate protected decision authority after evaluation"):
            result = self.execute_authority(phase)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = self.execute(body("ripr", "Record decision authority"),
                              SELECTED_DECISION_SHA=self.trusted, GITHUB_ENV=bash_path(self.repo / "env"))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_authority_rejects_wrong_malformed_unprotected_or_inconsistent_reads(self):
        cases = (
            (("repository", "id"), 43),
            (("repository", "node_id"), "R_other"),
            (("repository", "full_name"), "other/repo"),
            (("repository", "default_branch"), "speculative"),
            (("branch", "protected"), False),
            (("branch", "name"), "speculative"),
            (("branch", "commit", "sha"), "bad"),
            (("branch", "commit", "sha"), "b" * 40),
            (("ref", "ref"), "refs/heads/speculative"),
            (("ref", "object", "type"), "tag"),
            (("ref", "object", "sha"), "a" * 40),
            (("context", "repo", "owner"), "other"),
            (("context", "payload", "repository", "id"), -1),
            (("context", "payload", "repository", "node_id"), ""),
            (("context", "eventName"), "pull_request"),
            (("context", "payload", "action"), "destroyed"),
            (("context", "payload", "merge_group", "base_ref"), "refs/heads/speculative"),
        )
        for phase in ("Resolve protected main evaluator generation",
                      "Revalidate protected decision authority before evaluation",
                      "Revalidate protected decision authority after evaluation"):
            for path, value in cases:
                with self.subTest(phase=phase, path=path):
                    fixture = self.authority_fixture()
                    target = fixture
                    for key in path[:-1]:
                        target = target[key]
                    target[path[-1]] = value
                    result = self.execute_authority(phase, fixture)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("NOT_PROVEN", result.stderr)

    def test_stale_t_and_checkout_custody_fail_before_and_after_evaluation(self):
        for phase in ("Revalidate protected decision authority before evaluation",
                      "Revalidate protected decision authority after evaluation"):
            for expected in ("", "bad", "b" * 40, "a" * 40):
                with self.subTest(phase=phase, expected=expected):
                    result = self.execute_authority(phase, RIPR_DECISION_SHA=expected)
                    self.assertNotEqual(result.returncode, 0)
            moved = self.authority_fixture()
            moved["branch"]["commit"]["sha"] = "c" * 40
            moved["ref"]["object"]["sha"] = "c" * 40
            self.assertNotEqual(self.execute_authority(phase, moved).returncode, 0)
        source = self.repo / "decision-script"
        source.write_text("trusted\n", encoding="utf-8")
        self.git("add", "decision-script")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-m", "new trusted generation")
        # Independent API/selected T still points at the old commit.
        phase = "Revalidate protected decision authority after evaluation"
        self.assertIn("custody", self.execute_authority(phase).stderr)
        self.trusted = self.git("rev-parse", "HEAD")
        self.env["RIPR_DECISION_SHA"] = self.trusted
        self.assertEqual(self.execute_authority(phase).returncode, 0)
        source.write_text("modified\n", encoding="utf-8")
        self.assertIn("custody", self.execute_authority(phase).stderr)
        result = self.execute(body("ripr", "Record decision authority"),
                              SELECTED_DECISION_SHA="b" * 40, GITHUB_ENV=bash_path(self.repo / "env"))
        self.assertNotEqual(result.returncode, 0)

    def test_success_publication_waits_for_post_evaluation_authority_check(self):
        finalizer = job("ripr")
        names = re.findall(r"^      - name: (.*)$", finalizer, re.M)
        phases = ["Resolve protected main evaluator generation", "Checkout decision scripts",
                  "Record decision authority", "Revalidate protected decision authority before evaluation",
                  "Evaluate hosted result", "Revalidate protected decision authority after evaluation",
                  "Finalize verified hosted result"]
        self.assertEqual(sorted(phases, key=names.index), phases)
        for phase in phases:
            block = finalizer.split("      - name: " + phase + "\n", 1)[1].split("\n      - ", 1)[0]
            self.assertNotIn("continue-on-error:", block)
            # Native default success() skips final publication after any failure.
            self.assertNotIn("        if:", block)
        self.assertNotIn("RIPR_GATE_VERDICT=success", body("ripr", "Evaluate hosted result"))
        self.assertIn("RIPR_GATE_VERDICT=success", body("ripr", "Finalize verified hosted result"))

    def test_native_finalizer_stays_required_and_actions_bound(self):
        policy = tomllib.loads((ROOT / ".ci/policies/required-checks.toml").read_text(encoding="utf-8"))
        row = next(row for row in policy["checks"] if row["name"] == "ripr+ New Gap Gate")
        self.assertEqual(row["applicability"], "conditional")
        self.assertTrue(row["required"])
        self.assertEqual(row["ruleset_integration_id"], 15368)
        self.assertIn("every actual native merge_group/checks_requested G", row["reason"])
        self.assertIn("cache-maintenance", row["reason"])

    def test_mandatory_producer_steps_and_fail_closed_checks_survive(self):
        producer = job("ripr-github")
        for name in ("Run ripr doctor", "Generate PR evidence", "Build xtask",
                     "Generate repo-wide RIPR+ baseline receipt", "Record exact RIPR badge producer",
                     "Generate review guidance", "Generate impacted evidence", "Generate PR evidence summary",
                     "Generate warning annotations", "Audit suppression lifecycle",
                     "Validate PR evidence contracts", "Enforce new RIPR gap quality gate",
                     "Append PR evidence summary", "Record RIPR freshness handoff result",
                     "Upload ripr PR evidence", "Upload xtask build observations",
                     "Save memoized ripr-plus receipt"):
            self.assertIn("- name: " + name, producer)
        validate = body("ripr-github", "Validate PR evidence contracts")
        for command in ("ripr-pr", "ripr-plus", "ripr-review-comments",
                        "impacted-evidence", "ripr-pr-summary", "ripr-annotations"):
            self.assertRegex(validate, r"cargo xtask " + command + r"[^\n]* --check")
        self.assertIn('!= "$RIPR_FRESHNESS_TOKEN"', validate)
        self.assertIn('!= "$RIPR_FRESHNESS_TOKEN"',
                      body("ripr-github", "Enforce new RIPR gap quality gate"))


if __name__ == "__main__":
    unittest.main()
