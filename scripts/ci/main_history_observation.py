"""Assess a saved Main History Event run, classifier job and run-artifacts API response.

Supply the three GitHub REST JSON files in that order. Exit 0 for an observed or
inapplicable event, 1 for missing observation, 2 for unusable evidence. This
checks publication and enforcement, not the receipt's ancestry verdict.
"""
import argparse
import json
from pathlib import Path
import re


def assess(run, job, inventory):
    for field in ("path", "event", "head_branch"):
        if not isinstance(run.get(field), str) or not run[field]:
            raise ValueError(f"missing run {field}")
    if (run["path"], run["event"], run["head_branch"]) != (
        ".github/workflows/main-history-event.yml", "push", "main"
    ):
        return 0, "inapplicable", "outside the main push history workflow"

    if (type(run["id"]) is not int or type(run["run_attempt"]) is not int
            or not re.fullmatch("[0-9a-f]{40}", run["head_sha"])
            or job["run_id"] != run["id"] or job["run_attempt"] != run["run_attempt"]
            or job["name"] != "Classify main push movement"):
        raise ValueError("run and classifier job identities must match")
    if job["status"] != "completed":
        raise ValueError("classifier job has not completed")
    if job["conclusion"] not in ("success", "failure"):
        return 1, "missing_observation", f"classifier job concluded {job['conclusion']}"

    steps = {step["name"]: step["conclusion"] for step in job["steps"]}
    for name in ("Classify the delivered push event", "Publish the history-event receipt"):
        if steps.get(name) != "success":
            return 1, "missing_observation", f"{name} did not succeed"
    if steps.get("Enforce the detector verdict") not in ("success", "failure"):
        return 1, "missing_observation", "detector verdict was not enforced"

    name = f"main-history-event-receipt-{run['head_sha']}-{run['id']}"
    artifacts = inventory["artifacts"]
    if not isinstance(artifacts, list):
        raise ValueError("expected run artifacts array")
    for artifact in artifacts:
        if (artifact["name"] == name and artifact["expired"] is False
                and artifact["workflow_run"]["id"] == run["id"]
                and artifact["workflow_run"]["head_sha"] == run["head_sha"]):
            return 0, "observed", "classifier, publication, enforcement and receipt confirmed"
    if inventory.get("total_count", len(artifacts)) > len(artifacts):
        raise ValueError("artifact inventory is incomplete")
    return 1, "missing_observation", "no available receipt for this run and commit"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("run", "job", "artifacts"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    try:
        code, verdict, reason = assess(*(json.loads(path.read_text()) for path in
                                        (args.run, args.job, args.artifacts)))
    except (OSError, KeyError, ValueError, TypeError, AttributeError) as error:
        code, verdict, reason = 2, "invalid_evidence", str(error)
    print(json.dumps({"verdict": verdict, "reason": reason}))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
