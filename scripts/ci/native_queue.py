#!/usr/bin/env python3
"""Immediate native enqueue/readback transport for #17900; not an eligibility engine.

The CLI only observes. A trusted in-process caller may call enqueue(authorize=...)
when the existing owners have verified eligibility, conditional merge authority,
all required H producers, and the installed queue/source/protection generation.
No flag, environment variable or candidate JSON installs that authority here.
Queue custody is not G qualification, complete membership, or verified landing.
"""
from __future__ import annotations

import argparse
import copy
from dataclasses import asdict, dataclass
import json
import re
import subprocess
from typing import Callable

MAX_BYTES = 1024 * 1024
OID = re.compile(r"[0-9a-f]{40}")
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
ENTRY_STATES = {"QUEUED", "AWAITING_CHECKS", "LOCKED", "MERGEABLE", "UNMERGEABLE"}
OBSERVE = """query NativeQueue($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    id nameWithOwner
    pullRequest(number: $number) {
      id number state isDraft headRefOid baseRefName title body reviewDecision
      headRepository { id } stack { id }
      isInMergeQueue isMergeQueueEnabled mergeQueue { id }
      autoMergeRequest { enabledAt }
      mergeCommit { oid }
      mergeQueueEntry {
        id state headCommit { oid } baseCommit { oid }
        mergeQueue { id }
        pullRequest { id headRefOid }
      }
    }
  }
}"""
OBSERVE_GROUP = """query NativeQueueGroup($owner: String!, $name: String!, $branch: String!) {
  repository(owner: $owner, name: $name) {
    id nameWithOwner
    mergeQueue(branch: $branch) {
      id entries(first: 100) {
        totalCount pageInfo { hasNextPage }
        nodes {
          id position state baseCommit { oid } headCommit { oid }
          pullRequest {
            id number state isDraft headRefOid baseRefName title
            author { login } headRepository { id nameWithOwner }
            labels(first: 100) { totalCount pageInfo { hasNextPage } nodes { name } }
            mergeQueueEntry {
              id position baseCommit { oid } headCommit { oid } mergeQueue { id }
            }
          }
        }
      }
    }
  }
}"""
ENQUEUE = """mutation NativeEnqueue($input: EnqueuePullRequestInput!) {
  enqueuePullRequest(input: $input) { mergeQueueEntry { id } }
}"""


class Refused(ValueError):
    """No new mutation is admissible from the available observation."""


class Uncertain(RuntimeError):
    """Transport failed; a submitted mutation must not be blindly repeated."""


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise Refused(reason)


def node(value) -> bool:
    return isinstance(value, str) and 0 < len(value) <= 512 and not any(c.isspace() for c in value)


def oid(value) -> bool:
    return isinstance(value, str) and OID.fullmatch(value) is not None


def object_id(value):
    require(value is None or isinstance(value, dict) and oid(value.get("oid")),
            "invalid native commit observation")
    return None if value is None else value["oid"]


@dataclass(frozen=True)
class Subject:
    repository: str
    repository_node_id: str
    pr: int
    expected_head: str
    base_branch: str

    def __post_init__(self):
        require(isinstance(self.repository, str) and REPOSITORY.fullmatch(self.repository)
                and node(self.repository_node_id) and type(self.pr) is int and self.pr > 0
                and oid(self.expected_head) and isinstance(self.base_branch, str)
                and 0 < len(self.base_branch) <= 1024
                and not any(ord(c) < 33 for c in self.base_branch), "invalid queue subject")


class GitHub:
    """Use existing gh auth; only a fixed read query and immediate enqueue exist."""

    def _graphql(self, query: str, variables: dict) -> dict:
        require(query in (OBSERVE, OBSERVE_GROUP, ENQUEUE), "operation outside native queue transport")
        unavailable = ("native enqueue outcome unconfirmed; use read-only recovery"
                       if query == ENQUEUE else "native queue observation unavailable")
        try:
            process = subprocess.run(
                ["gh", "api", "graphql", "--hostname", "github.com", "--input", "-"],
                input=json.dumps({"query": query, "variables": variables}).encode(),
                capture_output=True, timeout=30, check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            raise Uncertain(unavailable) from None
        if process.returncode:
            # Never include credential-bearing gh stderr or API error bodies.
            raise Uncertain(unavailable)
        try:
            require(len(process.stdout) <= MAX_BYTES, "native response exceeds bound")
            def pairs(items):
                result = {}
                for key, value in items:
                    require(key not in result, "duplicate native JSON field")
                    result[key] = value
                return result
            def constant(_):
                raise Refused("nonfinite native JSON")
            result = json.loads(process.stdout, object_pairs_hook=pairs, parse_constant=constant)
            require(isinstance(result, dict)
                    and ("errors" not in result or type(result["errors"]) is list and not result["errors"])
                    and isinstance(result.get("data"), dict), "incomplete GraphQL response")
            return result["data"]
        except (Refused, ValueError, UnicodeError, TypeError):
            raise Uncertain(unavailable) from None

    def observe(self, subject: Subject) -> dict:
        owner, name = subject.repository.split("/")
        return self._graphql(OBSERVE, {"owner": owner, "name": name, "number": subject.pr})

    def observe_group(self, repository: str, branch: str) -> dict:
        """Read the complete bounded native queue; no PR/ref-number inference."""
        require(REPOSITORY.fullmatch(repository) is not None and node(branch),
                "invalid queue observation scope")
        owner, name = repository.split("/")
        return self._graphql(OBSERVE_GROUP, {"owner": owner, "name": name, "branch": branch})

    def enqueue(self, pr_id: str, head: str) -> str:
        require(node(pr_id) and oid(head), "invalid immediate enqueue subject")
        result = self._graphql(ENQUEUE, {"input": {
            "pullRequestId": pr_id, "expectedHeadOid": head, "jump": False,
        }})
        try:
            entry_id = result["enqueuePullRequest"]["mergeQueueEntry"]["id"]
            require(node(entry_id), "missing enqueue acknowledgment")
            return entry_id
        except (KeyError, TypeError, Refused):
            raise Uncertain("native enqueue acknowledgment unconfirmed; recover before any retry") from None


def observation(subject: Subject, data: dict) -> dict:
    """Bind one PR's live entry; never infer the complete G constituent set."""
    try:
        repo = data["repository"]
        require(repo["id"] == subject.repository_node_id
                and repo["nameWithOwner"] == subject.repository, "repository identity changed")
        pr = repo["pullRequest"]
        require(node(pr["id"]) and type(pr["number"]) is int and pr["number"] == subject.pr,
                "pull-request identity mismatch")
        require(pr["headRefOid"] == subject.expected_head, "candidate head moved")
        require(pr["baseRefName"] == subject.base_branch, "target branch changed")
        require(pr["headRepository"]["id"] == subject.repository_node_id,
                "cross-repository replay is not this admission route")
        require(pr["state"] in {"OPEN", "CLOSED", "MERGED"}
                and all(type(pr[k]) is bool for k in ("isDraft", "isInMergeQueue", "isMergeQueueEnabled"))
                and all(isinstance(pr[k], str) for k in ("title", "body"))
                and pr["reviewDecision"] in {None, "APPROVED", "CHANGES_REQUESTED", "REVIEW_REQUIRED"},
                "invalid PR observation")
        queue = pr["mergeQueue"]
        require(queue is None or isinstance(queue, dict) and node(queue.get("id")), "invalid queue identity")
        queue_id = None if queue is None else queue["id"]
        require(not pr["isMergeQueueEnabled"] or queue_id is not None, "enabled queue identity missing")
        auto = pr["autoMergeRequest"]
        require(auto is None or isinstance(auto, dict) and isinstance(auto.get("enabledAt"), str),
                "invalid deferred auto-merge observation")
        stack = pr["stack"]
        require(stack is None or isinstance(stack, dict) and node(stack.get("id")), "invalid stack observation")
        stack_id = None if stack is None else stack["id"]
        entry = pr["mergeQueueEntry"]
        require(pr["isInMergeQueue"] == (entry is not None), "queue custody observations disagree")
        bound_entry = None
        if entry is not None:
            require(pr["state"] == "OPEN" and not pr["isDraft"] and pr["isMergeQueueEnabled"]
                    and node(entry["id"]) and entry["state"] in ENTRY_STATES
                    and entry["mergeQueue"]["id"] == queue_id
                    and entry["pullRequest"]["id"] == pr["id"]
                    and entry["pullRequest"]["headRefOid"] == subject.expected_head,
                    "native queue entry differs from selected PR/queue")
            bound_entry = {"id": entry["id"], "state": entry["state"], "queue_id": queue_id,
                           "head_oid": object_id(entry["headCommit"]),
                           "base_oid": object_id(entry["baseCommit"])}
        merge_oid = object_id(pr["mergeCommit"])
        require(pr["state"] != "MERGED" or merge_oid is not None, "merged commit unavailable")
        status = ("QUEUE_ENTRY_OBSERVED" if entry is not None else
                  "MERGED_OBSERVED" if pr["state"] == "MERGED" else
                  "CLOSED" if pr["state"] == "CLOSED" else
                  "AUTO_MERGE_ARMED" if auto is not None else "NOT_QUEUED")
        return {"status": status, "subject": asdict(subject), "pr_id": pr["id"],
                "queue_id": queue_id, "queue_enabled": pr["isMergeQueueEnabled"], "stack_id": stack_id,
                "draft": pr["isDraft"], "title": pr["title"], "body": pr["body"],
                "review_decision": pr["reviewDecision"], "entry": bound_entry,
                "merge_oid": merge_oid, "qualification_result": "NOT_PROVEN",
                "landing_result": "NOT_PROVEN", "mutation_attempted": False}
    except (KeyError, TypeError, AttributeError):
        raise Refused("incomplete native queue observation") from None


class Adapter:
    def __init__(self, subject: Subject, api=None):
        self.subject = subject
        self.api = GitHub() if api is None else api
        self.attempted = False

    def observe(self) -> dict:
        return observation(self.subject, self.api.observe(self.subject))

    def recover(self) -> dict:
        """Read once. An absent entry does not prove a prior request never ran."""
        return self.observe()

    def enqueue(self, *, authorize: Callable[[Subject, dict], bool] | None = None) -> dict:
        """At most one mutation; authorization is supplied by trusted control code.

        The owner must freshly join local eligibility/current findings, conditional
        merge authority, and qualified source/protection/publisher observations.
        This transport cannot authenticate a callback or prevent all same-H metadata
        races. Installed invalidation/withdrawal belongs to that owner, not a CLI flag.
        """
        require(not self.attempted, "enqueue already attempted; use read-only recovery")
        before = self.observe()
        if before["status"] != "NOT_QUEUED":
            return before  # Existing custody/deferred/terminal state is not a new request.
        require(before["queue_enabled"] and not before["draft"], "queue disabled or candidate draft")
        require(before["stack_id"] is None, "stack requires separately qualified async admission")
        require(callable(authorize) and authorize(self.subject, copy.deepcopy(before)) is True,
                "trusted native admission/conditional authority not established")
        current = self.observe()  # Reacquire after the potentially long owner judgment.
        if current["status"] != "NOT_QUEUED":
            return current
        require(current == before, "native identity or metadata changed during admission")
        self.attempted = True  # No ambiguous mutation is automatically repeated.
        acknowledged_id = None
        try:
            acknowledged_id = self.api.enqueue(current["pr_id"], self.subject.expected_head)
        except Uncertain:
            pass
        try:
            after = self.recover()
        except (Refused, Uncertain):
            after = None
        stable = (after is not None and all(after[key] == current[key] for key in
                  ("subject", "pr_id", "queue_id", "stack_id", "title", "body", "review_decision")))
        confirmed = (after is not None and (after["status"] == "MERGED_OBSERVED" or
                     after["status"] == "QUEUE_ENTRY_OBSERVED" and stable
                     and (acknowledged_id is None or after["entry"]["id"] == acknowledged_id)))
        if confirmed:
            return {**after, "mutation_attempted": True, "acknowledged_entry_id": acknowledged_id}
        return {"status": "ENQUEUE_NOT_CONFIRMED", "subject": asdict(self.subject),
                "mutation_attempted": True, "mutation_may_have_occurred": True,
                "acknowledged_entry_id": acknowledged_id, "observation": after,
                "qualification_result": "NOT_PROVEN", "landing_result": "NOT_PROVEN",
                "next_action": ("entry exists but admission changed; owner must withdraw or disposition"
                                if after is not None and after["entry"] is not None and not stable
                                else "read-only recovery; do not automatically resubmit")}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["observe", "recover"])
    parser.add_argument("--repository", required=True)
    parser.add_argument("--repository-node-id", required=True)
    parser.add_argument("--pr", type=int, required=True)
    parser.add_argument("--expected-head", required=True)
    parser.add_argument("--base-branch", required=True)
    args = parser.parse_args(argv)
    try:
        subject = Subject(args.repository, args.repository_node_id, args.pr, args.expected_head, args.base_branch)
        result = Adapter(subject).recover()
    except (Refused, Uncertain) as error:
        print(json.dumps({"status": "NOT_PROVEN", "reason": str(error)}))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0  # An observation completed, not product qualification or merge acceptance.


if __name__ == "__main__":
    raise SystemExit(main())
