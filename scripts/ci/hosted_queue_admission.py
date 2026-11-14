#!/usr/bin/env python3
"""Explicit exact-H metadata admission, never a product-test receipt.

Runs only from independently verified protected-main source. No check writes,
enqueue, candidate checkout/execution, signer, or persistent service.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
from datetime import datetime

REPO = "EffortlessMetrics/perl-lsp-swarm"
WORKFLOW = ".github/workflows/queue-admission-request.yml"
OID = re.compile(r"[0-9a-f]{40}")
ROOT = Path(__file__).resolve().parents[2]

CLOSURE = {".github":"tree", ".ci/policies/native-queue-title.json":"blob",
           "scripts/ci/native_queue.py":"blob", "scripts/ci/native_queue_title.py":"blob",
           "scripts/ci/pr_title_validator.js":"blob", "scripts/ci/native_actions_intake.py":"blob",
           "scripts/ci/hosted_queue_admission.py":"blob", "scripts/ci/check-pr-semantic-review-currentness.py":"blob"}


def review_converged(reviews):
    """Latest decisive review state per author; a later COMMENT is not approval.

    Dismissed requests do not remain vetoes. Equal-time conflicting decisions
    cannot establish ordering. Complete API pagination is the caller's duty.
    """
    latest = {}
    for row in reviews:
        state = row.get("state")
        if state not in {"APPROVED", "CHANGES_REQUESTED"}:
            continue
        user = row.get("user", {})
        require(isinstance(user.get("login"), str) and user.get("type") in {"User", "Bot"}, "review author missing")
        try:
            stamp = datetime.fromisoformat(row["submitted_at"].replace("Z", "+00:00"))
        except (KeyError, ValueError, TypeError):
            raise RuntimeError("NOT_PROVEN: review ordering missing") from None
        require(stamp.tzinfo is not None, "review ordering timezone missing")
        key = user["login"].lower()
        prior = latest.get(key)
        require(prior is None or prior[0] != stamp or prior[1] == state, "ambiguous review ordering")
        if prior is None or stamp > prior[0]:
            latest[key] = (stamp, state)
    return all(state != "CHANGES_REQUESTED" for _, state in latest.values())


def require(ok, reason):
    if not ok:
        raise RuntimeError("NOT_PROVEN: " + reason)


class API:
    def get(self, path):
        require(path.startswith("/repos/" + REPO + "/"), "read scope")
        p = subprocess.run(["gh", "api", "--hostname", "github.com", "--method", "GET", path],
                           check=True, capture_output=True, text=True, timeout=30)
        return json.loads(p.stdout)

    def pages(self, path):
        rows = []
        for page in range(1, 101):
            batch = self.get(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
            require(isinstance(batch, list), "complete list unavailable")
            rows.extend(batch)
            if len(batch) < 100:
                return rows
        raise RuntimeError("NOT_PROVEN: pagination bound exceeded")

    def threads(self, number):
        query = """query($n:Int!,$cursor:String){repository(owner:"EffortlessMetrics",name:"perl-lsp-swarm"){pullRequest(number:$n){reviewThreads(first:100,after:$cursor){nodes{isResolved}pageInfo{hasNextPage endCursor}}}}}"""
        cursor = None
        rows = []
        for _ in range(100):
            args = ["gh", "api", "--hostname", "github.com", "graphql", "-f", "query=" + query, "-F", f"n={number}"]
            if cursor:
                args.extend(["-f", "cursor=" + cursor])
            p = subprocess.run(args, check=True, capture_output=True, text=True, timeout=30)
            value = json.loads(p.stdout)
            require(not value.get("errors"), "thread read error")
            data = value["data"]["repository"]["pullRequest"]["reviewThreads"]
            rows.extend(data["nodes"])
            info = data["pageInfo"]
            if info["hasNextPage"] is False:
                return rows
            require(isinstance(info["endCursor"], str) and info["endCursor"] and info["endCursor"] != cursor, "thread pagination")
            cursor = info["endCursor"]
        raise RuntimeError("NOT_PROVEN: thread pagination bound exceeded")


def canonical_review(root, number, head, reviews):
    spec = importlib.util.spec_from_file_location("semantic_review", root / "scripts/ci/check-pr-semantic-review-currentness.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    rows = [mod.Review(r["user"]["login"], r["user"]["type"], r["state"],
                       r.get("body") or "", r["commit_id"], r["submitted_at"]) for r in reviews]
    result = mod.evaluate(root, pr=number, current_head=head, reviews=rows)
    require(result.get("classification") == "REVIEW_CURRENT" and
            result.get("reviewed_head") == head and result.get("carried_forward") is False,
            "exact-H substantive review required")
    return result


def observe(api, event, env, root=ROOT, review=canonical_review):
    require(env.get("GITHUB_EVENT_NAME") == "pull_request_target" and event.get("action") == "labeled"
            and event.get("label", {}).get("name") == "queue-admission", "explicit admission event required")
    repository = event.get("repository", {})
    require(repository.get("full_name") == REPO and env.get("GITHUB_REPOSITORY") == REPO,
            "repository identity")
    pr = event["pull_request"]
    number, head = pr["number"], pr["head"]["sha"]
    require(type(number) is int and number > 0 and OID.fullmatch(head), "PR/H identity")
    source = env.get("GITHUB_WORKFLOW_SHA", "")
    require(OID.fullmatch(source) and env.get("GITHUB_WORKFLOW_REF") == REPO + "/" + WORKFLOW + "@refs/heads/main",
            "protected workflow source identity")
    prefix = "/repos/" + REPO
    def source_current():
        repo = api.get(prefix + "/")
        branch = api.get(prefix + "/branches/main")
        ref = api.get(prefix + "/git/ref/heads/main")
        require(repo.get("id") == repository.get("id") and type(repo.get("id")) is int and repo["id"] > 0
                and repo.get("node_id") == repository.get("node_id") and repo.get("default_branch") == "main"
                and branch.get("protected") is True and branch.get("name") == "main"
                and branch.get("commit", {}).get("sha") == source
                and ref.get("ref") == "refs/heads/main" and ref.get("object", {}).get("type") == "commit"
                and ref.get("object", {}).get("sha") == source,
                "protected evaluator moved or unavailable")
    def current():
        value = api.get(prefix + f"/pulls/{number}")
        require(value.get("number") == number and value.get("state") == "open" and value.get("draft") is False
                and value.get("head", {}).get("sha") == head
                and value.get("base", {}).get("ref") == "main"
                and value.get("base", {}).get("repo", {}).get("id") == repository.get("id"), "PR moved/draft/closed/wrong base")
        return value
    def permission(login):
        require(isinstance(login, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9-]{0,38}", login), "actor identity")
        value = api.get(prefix + "/collaborators/" + login + "/permission")
        require(value.get("user", {}).get("login", "").lower() == login.lower()
                and value.get("permission") in {"write", "maintain", "admin"}, "actor lacks current write authority")
    source_current()
    current()
    sender = event.get("sender", {})
    require(sender.get("type") == "User" and sender.get("login") == env.get("GITHUB_ACTOR")
            and env.get("GITHUB_TRIGGERING_ACTOR") == env.get("GITHUB_ACTOR"), "request/rerun actor mismatch")
    permission(sender.get("login"))
    require(pr.get("draft") is False, "draft request")
    reviews = api.pages(prefix + f"/pulls/{number}/reviews")
    require(all(t.get("isResolved") is True for t in api.threads(number)), "unresolved review threads")
    require(review_converged(reviews), "current change request remains")
    result = review(root, number, head, reviews)
    permission(result["reviewer"])
    # The canonical review process owns independent authorship and root publication.
    # A shared GitHub publishing login is neither proof nor disproof of that review.
    # Equal observations fail closed on any concurrent review edit/withdrawal.
    require(api.pages(prefix + f"/pulls/{number}/reviews") == reviews, "review changed during admission")
    require(all(t.get("isResolved") is True for t in api.threads(number)), "thread changed during admission")
    current()
    source_current()
    require(re.fullmatch(r"[1-9][0-9]*", env.get("GITHUB_RUN_ID", ""))
            and re.fullmatch(r"[1-9][0-9]*", env.get("GITHUB_RUN_ATTEMPT", "")), "run identity")
    return {"schema": "native-actions-admission.v1", "phase": "admission_only",
            "product_tests_executed": False, "repository": REPO, "pull_number": number,
            "head_sha": head, "event_action": "labeled", "base_ref": "main", "requester": sender["login"], "trusted_source_sha": env.get("ADMISSION_SOURCE_SHA"), "protected_wrapper_sha": source,
            "review": result, "run_id": int(env["GITHUB_RUN_ID"]), "run_attempt": int(env["GITHUB_RUN_ATTEMPT"])}


if __name__ == "__main__":
    # This checkout is T, never H. Fetching H for a binary diff does not execute it.
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    require(actual == os.environ.get("ADMISSION_SOURCE_SHA"), "evaluator checkout mismatch")
    require(OID.fullmatch(actual) and actual != "0"*40, "immutable admission source unavailable")
    receipt = observe(API(), json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text()), os.environ)
    # Freeze the same producer closure on every reviewed H, without checking out H.
    closure = {}
    for path, kind in CLOSURE.items():
        entry = subprocess.check_output(["git", "ls-tree", receipt["head_sha"], "--", path], cwd=ROOT, text=True).strip()
        metadata, actual_path = entry.split("\t", 1)
        mode, actual_kind, sha = metadata.split(" ")
        require(actual_path == path and actual_kind == kind and OID.fullmatch(sha), "producer closure unavailable")
        closure[path] = {"mode":mode,"kind":kind,"sha":sha}
    receipt["producer_closure"] = closure
    receipt["producer_generation_sha256"] = hashlib.sha256(json.dumps(closure,sort_keys=True,separators=(",", ":")).encode()).hexdigest()
    Path(os.environ["RUNNER_TEMP"], "queue-admission.json").write_text(json.dumps(receipt, sort_keys=True))
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as f:
        f.write("Admission-only exact-H review passed. No product tests executed. Native enqueue and actual-G qualification remain separate.\n")
