#!/usr/bin/env python3
"""Execute the privileged retry workflow's Bash with data-only API fixtures."""

import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from urllib.parse import quote
import zipfile


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = Path(os.environ.get("RIPR_RETRY_WORKFLOW", ROOT / ".github/workflows/ripr-infra-retry.yml"))
SOURCE_SHA = "1" * 40
NEW_SHA = "2" * 40
MERGE_SHA = "3" * 40
TARGET = "example/target"
LANE = "ripr+ on GitHub Hosted"


def production_script():
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    starts = [i for i, line in enumerate(lines) if line == "        run: |"]
    if len(starts) != 1:
        raise AssertionError("expected one exact production Bash step")
    script = []
    for line in lines[starts[0] + 1:]:
        if line.strip() and not line.startswith("          "):
            break
        script.append(line[10:] if line.strip() else "")
    return "\n".join(script) + "\n"


FAKE_GH = r'''#!/usr/bin/env bash
set -euo pipefail
request="$*"
printf '%s\n' "$request" >> "$CALLS"
endpoint=""
for arg in "$@"; do
  if [[ "$arg" == repos/* ]]; then endpoint="$arg"; break; fi
done
selector="" method=GET
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --jq) selector="$2"; shift 2 ;;
    -X) method="$2"; shift 2 ;;
    *) shift ;;
  esac
done
case "$endpoint" in
  "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/artifacts?per_page=100")
    printf '%s' "$ARTIFACT_ID" ;;
  "repos/$GITHUB_REPOSITORY/actions/artifacts/123/zip")
    cat "$CLASSIFICATION_ZIP" ;;
  "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/attempts/$RUN_ATTEMPT/jobs?per_page=100")
    printf '%s\t%s\n' "$LIVE_JOB_ID" "$LIVE_JOB_NAME" ;;
  "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID")
    printf '%s\n' "$LIVE_STATE" ;;
  "repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/rerun-failed-jobs")
    [[ "$method" == POST ]] || exit 91
    printf 'POST\n' >> "$POSTS" ;;
  *)
    if [[ "$endpoint" == "$EXPECTED_SOURCE_REF" ]]; then
      [[ "$method" == GET ]] || exit 91
      printf '%s' "$SOURCE_REF_JSON" | jq -r "$selector"
      [[ "$REF_FAILURE" == 0 ]] || exit "$REF_FAILURE"
    elif [[ "$endpoint" == "$TARGET_REF" ]]; then
      [[ "$method" == GET ]] || exit 91
      printf '%s' "$TARGET_REF_JSON" | jq -r "$selector"
    else
      printf 'unexpected fixture endpoint\n' >&2
      exit 92
    fi ;;
esac
'''


class RetryWorkflowTests(unittest.TestCase):
    def test_production_environment_uses_event_source_identity(self):
        lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
        run = lines.index("        run: |")
        env = max(i for i, line in enumerate(lines[:run]) if line == "        env:")
        bindings = dict(line.strip().split(": ", 1) for line in lines[env + 1:run])
        for name, field in (("HEAD_SHA", "head_sha"), ("SOURCE_EVENT", "event"),
                            ("SOURCE_REPOSITORY", "head_repository.full_name"),
                            ("SOURCE_BRANCH", "head_branch")):
            with self.subTest(name=name):
                self.assertEqual(bindings[name], "${{ github.event.workflow_run." + field + " }}")

    def exercise(self, *, retry, source_event="pull_request", source_repo=TARGET,
                 source_branch="fix/source", live_sha=SOURCE_SHA, ref_failure=0,
                 attempt="1", classification="infra-no-proof", gate_run="987",
                 gate_attempt="1", gate_head=MERGE_SHA, job_id="456", job_name=LANE,
                 live_state="1 completed", artifact_id="123", missing_file=False,
                 event_head=SOURCE_SHA, gate_job_id="456", gate_lane=LANE):
        with tempfile.TemporaryDirectory(prefix="ripr-retry-proof-") as directory:
            work = Path(directory)
            bindir = work / "bin"
            bindir.mkdir()
            gh = bindir / "gh"
            gh.write_text(FAKE_GH, encoding="utf-8", newline="\n")
            gh.chmod(0o755)
            script = work / "retry.sh"
            script.write_text(production_script(), encoding="utf-8", newline="\n")
            archive = work / "classification.zip"
            fields = (f"classification={classification}\nhead_sha={gate_head}\n"
                      f"run_id={gate_run}\nrun_attempt={gate_attempt}\n"
                      f"lane_name={gate_lane}\nlane_job_id={gate_job_id}\n")
            with zipfile.ZipFile(archive, "w") as fixture:
                fixture.writestr("missing.env" if missing_file else "ripr-gate-classification.env", fields)
            summary = work / "summary"
            calls = work / "calls"
            posts = work / "posts"
            encoded = quote("heads/" + source_branch, safe="")
            env = os.environ.copy()
            env.update({
                "PATH": str(bindir) + os.pathsep + env.get("PATH", ""),
                "GITHUB_REPOSITORY": TARGET, "GH_TOKEN": "fixture-only",
                "GITHUB_STEP_SUMMARY": str(summary), "RUN_ID": "987",
                "RUN_ATTEMPT": attempt, "HEAD_SHA": event_head,
                "SOURCE_EVENT": source_event, "SOURCE_REPOSITORY": source_repo,
                "SOURCE_BRANCH": source_branch, "LIVE_SOURCE_SHA": live_sha,
                "REF_FAILURE": str(ref_failure), "ARTIFACT_ID": artifact_id,
                "LIVE_JOB_ID": job_id, "LIVE_JOB_NAME": job_name,
                "LIVE_STATE": live_state, "CLASSIFICATION_ZIP": str(archive),
                "CALLS": str(calls), "POSTS": str(posts),
                "EXPECTED_SOURCE_REF": f"repos/{source_repo}/git/ref/{encoded}",
                "TARGET_REF": f"repos/{TARGET}/git/ref/{encoded}",
                "SOURCE_REF_JSON": json.dumps({"object": {"sha": live_sha}}),
                "TARGET_REF_JSON": json.dumps({"object": {"sha": event_head}}),
                "TMPDIR": str(work),
            })
            bash = (r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt"
                    else shutil.which("bash"))
            self.assertTrue(bash, "Bash is required for the production seam")
            result = subprocess.run([bash, "--noprofile", "--norc", str(script)],
                                    env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            observed = posts.read_text().splitlines() if posts.exists() else []
            self.assertEqual(observed, ["POST"] if retry else [], result.stdout + result.stderr)
            return (calls.read_text() if calls.exists() else "",
                    summary.read_text() if summary.exists() else "", result.stdout)

    def test_superseded_source_does_not_retry(self):
        calls, summary, _ = self.exercise(retry=False, live_sha=NEW_SHA)
        self.assertIn("git/ref/heads%2Ffix%2Fsource", calls)
        self.assertIn("superseded", summary)

    def test_current_source_retries_despite_distinct_evaluated_merge(self):
        self.exercise(retry=True)

    def test_fork_uses_source_repository_not_same_named_target_branch(self):
        self.exercise(retry=False, source_repo="contributor/fork", live_sha=NEW_SHA)
        self.exercise(retry=True, source_repo="contributor/fork")

    def test_branch_ref_is_encoded_once_and_not_executed(self):
        branch = "fix/a#b%2Fc;$(printf-injected)"
        calls, _, _ = self.exercise(retry=True, source_branch=branch)
        self.assertIn("heads%2Ffix%2Fa%23b%252Fc%3B%24%28printf-injected%29", calls)

    def test_missing_source_identity_does_not_retry(self):
        for kwargs in ({"source_event": ""}, {"source_repo": ""},
                       {"source_repo": "invalid"}, {"source_branch": ""},
                       {"event_head": ""}, {"event_head": "invalid"}):
            with self.subTest(kwargs=kwargs):
                self.exercise(retry=False, **kwargs)

    def test_unproven_source_lookup_does_not_retry(self):
        for kwargs in ({"ref_failure": 1}, {"ref_failure": 4},
                       {"ref_failure": 1, "live_sha": NEW_SHA},
                       {"ref_failure": 4, "live_sha": NEW_SHA}, {"live_sha": ""},
                       {"live_sha": "null"}, {"live_sha": "not-a-sha"}):
            with self.subTest(kwargs=kwargs):
                self.exercise(retry=False, **kwargs)

    def test_push_manual_and_history_recovery_do_not_query_pr_source(self):
        for event in ("push", "workflow_dispatch", "schedule"):
            with self.subTest(event=event):
                calls, _, _ = self.exercise(retry=True, source_event=event,
                                            source_repo="", source_branch="", live_sha=NEW_SHA)
                self.assertNotIn("/git/ref/", calls)

    def test_retry_bound_and_duplicate_delivery_remain(self):
        for kwargs in ({"attempt": "2"}, {"live_state": "2 completed"},
                       {"live_state": "1 in_progress"}):
            with self.subTest(kwargs=kwargs):
                self.exercise(retry=False, **kwargs)

    def test_classifier_and_receipt_identity_remain_authoritative(self):
        for kwargs in ({"classification": "ripr-failure"}, {"gate_run": "986"},
                       {"gate_attempt": "2"}, {"gate_head": "invalid"},
                       {"job_id": "457"}, {"job_name": "another lane"},
                       {"artifact_id": ""}, {"missing_file": True}):
            with self.subTest(kwargs=kwargs):
                self.exercise(retry=False, **kwargs)

    def test_current_second_attempt_infra_exhaustion_is_evidence_bound(self):
        calls, summary, output = self.exercise(retry=False, attempt="2", gate_attempt="2")
        self.assertIn("/attempts/2/jobs", calls)
        self.assertIn("not-proven-infra-retry-exhausted", summary)
        self.assertIn("validated attempt 2 is classified infra-no-proof", output)
        self.assertIn("single automatic same-head retry is exhausted", output)
        self.assertIn("Verify the latest run state before a manual", output)
        self.assertNotIn("still infra-evicted", output)
        self.assertNotIn("No proof exists for this SHA", output)
        self.assertNotIn("/git/ref/", calls)

    def test_second_attempt_without_current_infra_evidence_has_no_exhaustion_claim(self):
        for kwargs in ({"classification": "ripr-failure"},
                       {"classification": "configured-timeout-no-proof"},
                       {"gate_run": "986"}, {"gate_attempt": "1"},
                       {"gate_attempt": ""}, {"gate_head": "invalid"},
                       {"gate_job_id": ""}, {"gate_lane": ""},
                       {"job_id": "457"}, {"job_name": "another lane"},
                       {"artifact_id": ""}, {"missing_file": True}):
            with self.subTest(kwargs=kwargs):
                options = {"gate_attempt": "2", **kwargs}
                _, summary, output = self.exercise(retry=False, attempt="2", **options)
                self.assertNotIn("not-proven-infra-retry-exhausted", summary + output)
                self.assertNotIn("still infra-evicted", output)


if __name__ == "__main__":
    unittest.main()
