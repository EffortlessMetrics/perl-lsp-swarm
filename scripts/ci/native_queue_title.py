#!/usr/bin/env python3
"""Read-only actual-G title subject proof; no enqueue, check or label publisher.

The existing native queue transport acquires a complete current prefix. Each H
must retain the Hermes App-authenticated expectedHeadOid intake acknowledgement.
The isolated Git proof matches Hermes's ordered tree composition, including squash
G. Candidate JSON, PR association, ref-name numbers and tree equality alone do not
authenticate queue membership. An unsupported native join remains NOT_PROVEN.
"""
from __future__ import annotations

import argparse
from contextlib import AbstractContextManager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import quote, urlencode

import native_queue as queue

ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / ".ci/policies/native-queue-title.json"
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,79}")
HEX256 = re.compile(r"[0-9a-f]{64}")
INTAKE_KEYS = {"schema_id", "schema_version", "repository", "pull_number", "head_sha",
               "entry_id", "admission_request_id", "enqueue_request_id", "base_sha",
               "expires_at", "authority_sha256", "producer_closure", "producer_generation_sha256"}
require, Refused = queue.require, queue.Refused


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def decode(raw):
    require(isinstance(raw, (str, bytes)) and len(raw) <= queue.MAX_BYTES,
            "native JSON exceeds the observation bound")
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate native JSON field")
            result[key] = value
        return result
    def constant(_):
        raise Refused("nonfinite native JSON")
    return json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)


def positive(value):
    return type(value) is int and value > 0


def policy(value):
    require(isinstance(value, dict) and type(value.get("schema_version")) is int and value["schema_version"] == 1
            and value.get("enabled") is True, "native title publisher binding is not activated")
    require(value.get("repository") == "EffortlessMetrics/perl-lsp-swarm"
            and value.get("base_branch") == "main"
            and positive(value.get("publisher_app_id")) and value["publisher_app_id"] != 15368
            and isinstance(value.get("publisher_check_name"), str)
            and 0 < len(value["publisher_check_name"]) <= 100
            and not any(ord(c) < 32 for c in value["publisher_check_name"])
            and value.get("workflow_path") == ".github/workflows/pr-title-check.yml",
            "reviewed title publisher policy is incomplete")
    expected = {".github": "tree", ".ci/policies/native-queue-title.json": "blob",
                "scripts/ci/native_queue.py": "blob", "scripts/ci/native_queue_title.py": "blob",
                "scripts/ci/pr_title_validator.js": "blob"}
    require(value.get("producer_closure") == expected,
            "reviewed transitive title producer closure is incomplete")
    return value


def producer_closure(value, configured):
    require(isinstance(value, dict) and set(configured["producer_closure"]) <= set(value),
            "admitted producer closure omits a required title source")
    for path, entry in value.items():
        require(isinstance(path, str) and re.fullmatch(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*", path)
                and not any(part in {".", "..", ".git"} for part in path.split("/"))
                and isinstance(entry, dict) and set(entry) == {"kind", "sha", "mode"}
                and queue.oid(entry["sha"])
                and isinstance(entry["kind"], str) and isinstance(entry["mode"], str)
                and ((entry["kind"] == "tree" and entry["mode"] == "040000")
                     or (entry["kind"] == "blob" and entry["mode"] in {"100644", "100755"}))
                and (path not in configured["producer_closure"]
                     or entry["kind"] == configured["producer_closure"][path]),
                "admitted producer closure has an invalid path, kind, object or mode")
    return value


def admitted_generation(members, configured):
    require(isinstance(members, list) and members, "admitted producer generation has no members")
    expected = None
    for member in members:
        data = member["admission"]["intake"]
        digest = data.get("producer_generation_sha256")
        require(isinstance(digest, str) and HEX256.fullmatch(digest),
                "admitted producer generation digest is missing")
        current = (digest, producer_closure(data.get("producer_closure"), configured))
        require(expected is None or current == expected,
                "native prefix has mixed admitted producer generations")
        expected = current
    return expected


def event_subject(event, env, configured):
    require(env.get("GITHUB_EVENT_NAME") == "merge_group"
            and event.get("action") == "checks_requested", "actual native merge_group event required")
    repo, group = event.get("repository", {}), event.get("merge_group", {})
    require(repo.get("full_name") == configured["repository"]
            and env.get("GITHUB_REPOSITORY") == configured["repository"]
            and positive(repo.get("id")) and queue.node(repo.get("node_id")),
            "native repository identity is incomplete")
    base, head, ref = group.get("base_sha"), group.get("head_sha"), group.get("head_ref")
    prefix = "refs/heads/gh-readonly-queue/" + configured["base_branch"] + "/"
    require(queue.oid(base) and queue.oid(head) and base != head
            and head == env.get("GITHUB_SHA") and ref == env.get("GITHUB_REF")
            and group.get("base_ref") == "refs/heads/" + configured["base_branch"]
            and isinstance(ref, str) and ref.startswith(prefix) and len(ref) > len(prefix)
            and re.fullmatch(r"[A-Za-z0-9_./-]{1,250}", ref) is not None
            and ".." not in ref and "//" not in ref,
            "native B/G/ref subject mismatch")
    numbers = [env.get("GITHUB_RUN_ID", ""), env.get("GITHUB_RUN_ATTEMPT", "")]
    require(all(re.fullmatch(r"[1-9][0-9]*", number) for number in numbers)
            and env.get("GITHUB_JOB") == "validate-title", "native run/attempt/job identity required")
    return {"repository": configured["repository"], "repository_id": repo["id"],
            "repository_node_id": repo["node_id"], "base_sha": base, "head_sha": head,
            "head_ref": ref, "run_id": int(numbers[0]), "run_attempt": int(numbers[1])}


class GitHub(queue.GitHub):
    def get(self, repository, suffix):
        require(repository == "EffortlessMetrics/perl-lsp-swarm" and (suffix == "" or suffix.startswith("/")),
                "read scope differs from configured repository")
        try:
            result = subprocess.run(
                ["gh", "api", "--hostname", "github.com", "--method", "GET",
                 "/repos/" + repository + suffix], capture_output=True, timeout=30, check=False)
            require(result.returncode == 0, "native read unavailable")
            return decode(result.stdout)
        except (OSError, subprocess.TimeoutExpired, ValueError, UnicodeError):
            raise Refused("native read unavailable; no automatic mutation or fallback") from None


def trusted_evaluator(api, subject, configured, expected_sha):
    """Revalidate independently selected protected main, never event B or G.

    GitHub's protected branch and exact main ref must still name the immutable
    bootstrap generation. Missing observations or concurrent movement refuse;
    this is not evidence that the disabled App policy has been installed.
    """
    require(queue.oid(expected_sha), "immutable trusted evaluator SHA is missing")
    repository = api.get(configured["repository"], "")
    branch = api.get(configured["repository"], "/branches/main")
    ref = api.get(configured["repository"], "/git/ref/heads/main")
    require(isinstance(repository, dict)
            and repository.get("full_name") == configured["repository"] == subject["repository"]
            and repository.get("id") == subject["repository_id"]
            and repository.get("node_id") == subject["repository_node_id"]
            and repository.get("default_branch") == configured["base_branch"] == "main"
            and isinstance(branch, dict) and branch.get("name") == "main"
            and branch.get("protected") is True
            and branch.get("commit", {}).get("sha") == expected_sha
            and isinstance(ref, dict) and ref.get("ref") == "refs/heads/main"
            and ref.get("object", {}).get("type") == "commit"
            and ref.get("object", {}).get("sha") == expected_sha,
            "protected main evaluator generation changed or is not proven")
    return {"repository": configured["repository"], "repository_id": subject["repository_id"],
            "ref": "refs/heads/main", "sha": expected_sha, "protected": True}


def checkout_generation(expected_sha):
    require(queue.oid(expected_sha), "immutable trusted evaluator SHA is missing")
    custody_env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    custody_env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_CONFIG_SYSTEM=os.devnull, GIT_NO_REPLACE_OBJECTS="1")
    process = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "HEAD"], env=custody_env,
                             capture_output=True, text=True, encoding="utf-8", timeout=30)
    require(process.returncode == 0 and process.stdout.strip() == expected_sha,
            "title evaluator checkout differs from independently selected immutable generation")


def native_run(api, subject, configured):
    rid, attempt = subject["run_id"], subject["run_attempt"]
    rows = [api.get(subject["repository"], f"/actions/runs/{rid}"),
            api.get(subject["repository"], f"/actions/runs/{rid}/attempts/{attempt}")]
    for run in rows:
        require(isinstance(run, dict) and run.get("id") == rid and run.get("run_attempt") == attempt
                and run.get("head_sha") == subject["head_sha"]
                and run.get("head_branch") == subject["head_ref"].removeprefix("refs/heads/")
                and run.get("event") == "merge_group" and run.get("status") == "in_progress"
                and positive(run.get("workflow_id")) and positive(run.get("check_suite_id"))
                and run.get("path", "").split("@", 1)[0] == configured["workflow_path"]
                and run.get("repository", {}).get("id") == subject["repository_id"]
                and run.get("repository", {}).get("full_name") == subject["repository"]
                and run.get("head_repository", {}).get("id") == subject["repository_id"],
                "current native G workflow run/attempt is not proven")
    require(all(rows[0][key] == rows[1][key] for key in ("workflow_id", "check_suite_id")),
            "native run generation differs from selected attempt")
    suite = api.get(subject["repository"], "/check-suites/" + str(rows[0]["check_suite_id"]))
    require(isinstance(suite, dict) and suite.get("app", {}).get("id") == 15368
            and suite.get("head_sha") == subject["head_sha"], "native title leaf is not Actions-bound")
    ref = api.get(subject["repository"], "/git/ref/" + quote(subject["head_ref"].removeprefix("refs/"), safe=""))
    require(isinstance(ref, dict) and ref.get("object", {}).get("sha") == subject["head_sha"],
            "native queue ref no longer names G")
    return {"workflow_id": rows[0]["workflow_id"], "check_suite_id": rows[0]["check_suite_id"]}


def complete_connection(value, bound, label):
    require(isinstance(value, dict) and type(value.get("totalCount")) is int
            and isinstance(value.get("nodes"), list)
            and value["totalCount"] == len(value["nodes"]) <= bound
            and value.get("pageInfo", {}).get("hasNextPage") is False,
            "complete " + label + " is not proven")
    return value["nodes"]


def group_members(api, subject, configured):
    observed = api.observe_group(subject["repository"], configured["base_branch"])
    repo = observed.get("repository", {})
    require(repo.get("id") == subject["repository_node_id"]
            and repo.get("nameWithOwner") == subject["repository"], "native queue repository changed")
    native = repo.get("mergeQueue")
    require(isinstance(native, dict) and queue.node(native.get("id")), "native queue is unavailable")
    nodes = complete_connection(native.get("entries"), 100, "native queue membership")
    require(nodes and all(isinstance(n, dict) and positive(n.get("position"))
                         and queue.node(n.get("id")) and queue.oid((n.get("baseCommit") or {}).get("oid"))
                         and queue.oid((n.get("headCommit") or {}).get("oid"))
                         and positive((n.get("pullRequest") or {}).get("number")) for n in nodes),
            "native queue entry is malformed")
    ordered = sorted(nodes, key=lambda n: n["position"])
    require([n["position"] for n in ordered] == list(range(1, len(nodes) + 1))
            and len({n["id"] for n in nodes}) == len(nodes)
            and len({n["pullRequest"]["number"] for n in nodes}) == len(nodes),
            "native queue positions or identities are incomplete")
    anchors = [n for n in ordered if n["baseCommit"]["oid"] == subject["base_sha"]
               and n["headCommit"]["oid"] == subject["head_sha"]]
    require(len(anchors) == 1, "native queue cannot uniquely join the complete prefix to actual B/G")
    members = []
    for entry in ordered[:anchors[0]["position"]]:
        pr = entry["pullRequest"]
        current = pr.get("mergeQueueEntry")
        require(queue.node(pr.get("id")) and queue.oid(pr.get("headRefOid"))
                and pr["headRefOid"] != subject["head_sha"] and pr.get("state") == "OPEN"
                and pr.get("isDraft") is False and pr.get("baseRefName") == configured["base_branch"]
                and pr.get("headRepository", {}).get("id") == subject["repository_node_id"]
                and pr.get("headRepository", {}).get("nameWithOwner") == subject["repository"]
                and isinstance(current, dict) and current.get("mergeQueue", {}).get("id") == native["id"]
                and all(current.get(key) == entry.get(key) for key in ("id", "position", "baseCommit", "headCommit"))
                and isinstance(pr.get("title"), str) and queue.node(pr.get("author", {}).get("login")),
                "current native PR/head/queue entry differs from the complete prefix")
        labels = complete_connection(pr.get("labels"), 100, "native title labels")
        require(all(isinstance(item, dict) and isinstance(item.get("name"), str) for item in labels)
                and len({item["name"] for item in labels}) == len(labels), "ambiguous title labels")
        members.append({"pull_number": pr["number"], "head_sha": pr["headRefOid"],
                        "entry_id": entry["id"], "position": entry["position"],
                        "native_entry": {"base_sha": entry["baseCommit"]["oid"],
                                         "head_sha": entry["headCommit"]["oid"]},
                        "pull": {"number": pr["number"], "title": pr["title"],
                                 "user": {"login": pr["author"]["login"]}, "labels": labels}})
    return {"queue_id": native["id"], "members": members}


def intake(api, subject, member, configured, now):
    suffix = "/commits/" + member["head_sha"] + "/check-runs?" + urlencode({
        "check_name": configured["publisher_check_name"], "filter": "all", "per_page": 100})
    result = api.get(subject["repository"], suffix)
    require(isinstance(result, dict) and type(result.get("total_count")) is int
            and isinstance(result.get("check_runs"), list)
            and result["total_count"] == len(result["check_runs"]) <= 100,
            "complete H check observation is unavailable")
    matches = []
    for check in result["check_runs"]:
        if (check.get("name") != configured["publisher_check_name"]
                or check.get("app", {}).get("id") != configured["publisher_app_id"]):
            continue
        raw = check.get("output", {}).get("text")
        if not isinstance(raw, str):
            continue  # Admission without acknowledged native intake is insufficient.
        data = decode(raw)
        if not isinstance(data, dict) or data.get("entry_id") != member["entry_id"]:
            continue
        require(set(data) == INTAKE_KEYS and data["schema_id"] == "hermes.queue-intake.v2"
                and type(data["schema_version"]) is int and data["schema_version"] == 2
                and data["repository"] == subject["repository"]
                and type(data["pull_number"]) is int and data["pull_number"] == member["pull_number"]
                and data["head_sha"] == member["head_sha"] and queue.oid(data["base_sha"])
                and all(isinstance(data[key], str) and NAME.fullmatch(data[key])
                        for key in ("admission_request_id", "enqueue_request_id"))
                and isinstance(data["authority_sha256"], str) and HEX256.fullmatch(data["authority_sha256"])
                and isinstance(data["producer_generation_sha256"], str)
                and HEX256.fullmatch(data["producer_generation_sha256"])
                and positive(check.get("id")) and check.get("head_sha") == member["head_sha"]
                and check.get("external_id") == "hermes-queue:" + data["admission_request_id"] + ":H"
                and check.get("status") == "completed" and check.get("conclusion") == "success",
                "App-authenticated exact H-to-entry intake is invalid")
        producer_closure(data["producer_closure"], configured)
        require(isinstance(data["expires_at"], str), "H admission expiry is missing")
        expiry = datetime.fromisoformat(data["expires_at"].replace("Z", "+00:00"))
        require(expiry.tzinfo is not None and expiry > now, "H admission expired")
        matches.append({"check_run_id": check["id"], "app_id": check["app"]["id"],
                        "check_name": check["name"], "external_id": check["external_id"], "intake": data})
    require(len(matches) == 1, "unique current App-authenticated native intake is not proven")
    return matches[0]


class GitProof(AbstractContextManager):
    """Private bare-store equivalent of Hermes Git.prove_queue_tree; no checkout."""
    def __init__(self, repository):
        self.repository = repository
        self.temporary = tempfile.TemporaryDirectory(prefix="native-title-proof-")
        self.home = Path(self.temporary.name)
        self.store = self.home / "objects.git"
        (self.home / "empty-hooks").mkdir()
        self.run(["init", "--bare", "--template=", str(self.store)], bare=False)

    def __exit__(self, *_):
        self.temporary.cleanup()

    def origin(self):
        return "https://github.com/" + self.repository + ".git"

    def environment(self):
        return {"PATH": os.defpath, "HOME": str(self.home), "XDG_CONFIG_HOME": str(self.home),
                "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_CONFIG_SYSTEM": os.devnull, "GIT_ATTR_NOSYSTEM": "1",
                "GIT_NO_REPLACE_OBJECTS": "1", "GIT_TERMINAL_PROMPT": "0",
                "GIT_ALLOW_PROTOCOL": "https", "LANG": "C", "LC_ALL": "C"}

    def run(self, args, *, bare=True):
        command = ["git", "-c", "core.hooksPath=" + str(self.home / "empty-hooks"),
                   "-c", "core.fsmonitor=false", "-c", "core.attributesFile=" + os.devnull,
                   "-c", "gc.auto=0", "-c", "maintenance.auto=false",
                   "-c", "protocol.ext.allow=never", "-c", "fetch.recurseSubmodules=false"]
        if bare:
            command += ["--git-dir=" + str(self.store)]
        try:
            result = subprocess.run(command + args, env=self.environment(), cwd=self.home,
                                    capture_output=True, text=True, encoding="utf-8", timeout=180)
        except (OSError, subprocess.TimeoutExpired):
            raise Refused("isolated Git composition unavailable") from None
        require(result.returncode == 0, "isolated Git composition conflicted or unavailable")
        return result.stdout.strip()

    def prove(self, subject, members, configured):
        base, group = subject["base_sha"], subject["head_sha"]
        heads = [m["head_sha"] for m in members]
        generation, closure = admitted_generation(members, configured)
        self.run(["fetch", "--no-tags", "--no-write-fetch-head", self.origin(),
                  *dict.fromkeys([base, *heads, group])])
        self.run(["merge-base", "--is-ancestor", base, group])
        # B/G are composition subjects, not executable evaluator authority. G
        # may contain a reviewed next generation; its authority comes from every
        # current H intake, not equality with B or candidate-provided policy.
        for path, expected in closure.items():
            actual = self.run(["rev-parse", "--verify", group + ":" + path])
            group_entry = self.run(["ls-tree", group, "--", path])
            expected_entry = f'{expected["mode"]} {expected["kind"]} {expected["sha"]}\t{path}'
            require(actual == expected["sha"] and group_entry == expected_entry
                    and self.run(["cat-file", "-t", actual]) == expected["kind"],
                    "G differs from the App-admitted producer closure")
        current, expected_tree = base, None
        for head in heads:
            expected_tree = self.run(["merge-tree", "--write-tree", "--no-messages", current, head]).splitlines()[0]
            require(queue.oid(expected_tree), "ordered composition tree is unavailable")
            current = self.run(["-c", "user.name=Native queue title verifier", "-c",
                                "user.email=queue-proof@example.invalid", "commit-tree", expected_tree,
                                "-p", current, "-p", head, "-m", "Private composition; never published"])
            require(queue.oid(current), "private composition commit is unavailable")
        actual_tree = self.run(["rev-parse", "--verify", group + "^{tree}"])
        require(expected_tree is not None and actual_tree == expected_tree,
                "actual G differs from complete ordered B/H tree composition")
        return {"tree_sha": actual_tree, "composition": "ordered_source_tree_equivalence",
                "producer_generation_sha256": generation,
                "producer_closure": closure}


def snapshot(api, subject, configured, now, evaluator_sha):
    evaluator = trusted_evaluator(api, subject, configured, evaluator_sha)
    run = native_run(api, subject, configured)
    observed = group_members(api, subject, configured)
    for member in observed["members"]:
        member["admission"] = intake(api, subject, member, configured, now)
    admitted_generation(observed["members"], configured)
    return {"subject": subject, "trusted_evaluator": evaluator, "run": run, **observed}


def proposition(value):
    # needs-issue-link and bot comments are owning-session housekeeping; only
    # skip-title-check changes the validator's label exemption. No G code writes.
    result = json.loads(canonical(value))
    for member in result["members"]:
        member["pull"]["labels"] = [label for label in member["pull"]["labels"]
                                    if label["name"] == "skip-title-check"]
    return result


def observe(api, subject, configured, evaluator_sha, proof_factory=GitProof, clock=lambda: datetime.now(timezone.utc)):
    before = snapshot(api, subject, configured, clock(), evaluator_sha)
    with proof_factory(subject["repository"]) as proof:
        composition = proof.prove(subject, before["members"], configured)
    after = snapshot(api, subject, configured, clock(), evaluator_sha)
    require(proposition(before) == proposition(after), "native G subject changed during composition proof")
    return {"schema_id": "perl.native-queue-title.v1", "schema_version": 1,
            **before, **composition, "claim_boundary": "title subject only; no protected landing claim"}


def recheck(api, receipt, subject, configured, evaluator_sha, clock=lambda: datetime.now(timezone.utc)):
    require(isinstance(receipt, dict) and receipt.get("schema_id") == "perl.native-queue-title.v1"
            and receipt.get("schema_version") == 1 and receipt.get("subject") == subject,
            "title receipt belongs to another native subject")
    before = {key: receipt[key] for key in ("subject", "trusted_evaluator", "run", "queue_id", "members")}
    current = snapshot(api, subject, configured, clock(), evaluator_sha)
    require(proposition(before) == proposition(current),
            "native membership, H intake or current title metadata changed before success")
    return hashlib.sha256(canonical(receipt).encode()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("observe", "recheck"))
    parser.add_argument("--receipt", required=True)
    args = parser.parse_args(argv)
    try:
        configured = policy(decode(POLICY.read_text()))
        event = decode(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        subject = event_subject(event, os.environ, configured)
        # Bootstrap pinned a SHA selected by independent protected-main reads.
        # Checking HEAD against event B would authenticate a speculative subject.
        evaluator_sha = os.environ.get("TRUSTED_TITLE_SHA")
        checkout_generation(evaluator_sha)
        api, target = GitHub(), Path(args.receipt)
        if args.operation == "observe":
            receipt = observe(api, subject, configured, evaluator_sha)
            target.write_text(canonical(receipt) + "\n")
            print("Proved actual G title subject for " + ", ".join("#" + str(m["pull_number"])
                                                                    for m in receipt["members"]))
        else:
            digest = recheck(api, decode(target.read_text()), subject, configured, evaluator_sha)
            print("Current native title subject: " + digest)
        return 0
    except (Refused, queue.Uncertain) as error:
        print("NOT_PROVEN: " + str(error))
        return 1
    except (OSError, ValueError, KeyError, TypeError,
            AttributeError, subprocess.TimeoutExpired):
        print("NOT_PROVEN: native title subject/publisher/currentness proof is incomplete")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
