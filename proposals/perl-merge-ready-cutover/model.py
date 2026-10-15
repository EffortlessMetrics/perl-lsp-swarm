"""Inactive cutover contract oracle. Not imported by any workflow or merge tool.

Models proposed event admission and evidence acceptance; does not emulate Actions.
"""
from dataclasses import dataclass
import re


LABEL = "merge-ready"
INVALIDATIONS = {"opened", "synchronize", "reopened", "ready_for_review",
                 "converted_to_draft"}


@dataclass(frozen=True)
class Event:
    action: str
    label: str = ""
    event: str = "pull_request"
    draft: bool = False
    base_ref: str = "main"
    base_changed: bool = False
    pr: int = 1
    run: int = 1
    attempt: int = 1
    state: str = "open"


def material(e):
    return e.event == "pull_request" and (
        e.action in INVALIDATIONS
        or (e.action in {"labeled", "unlabeled"} and e.label == LABEL)
        or (e.action == "edited" and e.base_changed))


def admit(e):
    # Membership in the PR label set is deliberately insufficient.
    return (material(e) and e.action == "labeled" and e.label == LABEL
            and not e.draft and e.base_ref in {"main", "master"}
            and e.attempt == 1 and e.state == "open")


def result_name(e, protected_name):
    # Skipped jobs in irrelevant label runs must NEVER reuse protected names.
    return protected_name if material(e) else "Ignored metadata / " + protected_name


def group(e, workflow):
    return f"{workflow}-pr-{e.pr}" if material(e) else f"{workflow}-ignored-{e.run}"


@dataclass(frozen=True)
class Subject:
    repository: str
    pr: int
    head: str
    base: str
    merge: str
    tree: str
    workflow: str
    run: int
    attempt: int


def accepts(e, expected, receipt, results, required_workers, *,
            merge_parents, checkout, run_cancelled=False):
    """Immutable proof validity, independently of budget or current PR state."""
    if not admit(e) or receipt != expected or run_cancelled:
        return False
    if (e.pr, e.run, e.attempt) != (expected.pr, expected.run, expected.attempt):
        return False
    if not all(re.fullmatch(r"[0-9a-f]{40}", value) for value in (
            expected.head, expected.base, expected.merge, expected.tree, expected.workflow)):
        return False
    if merge_parents != (expected.base, expected.head):
        return False
    if checkout != (expected.merge, expected.tree):
        return False
    return bool(required_workers) and all(
        results.get(worker) == "success" for worker in required_workers)


def readiness(proof_valid, expected, *, live_head, live_base, live_label,
              live_draft=False, live_state="open", live_base_ref="main"):
    """Current integration assessment; never erases completed subject evidence."""
    if not proof_valid:
        return "NOT_PROVEN"
    if (live_state != "open" or live_draft or not live_label
            or live_base_ref not in {"main", "master"}):
        return "NOT_READY"
    if live_head != expected.head:
        return "HEAD_CHANGED_REASSESS"
    if live_base != expected.base:
        return "BASE_ADVANCED_REASSESS"
    return "READY_FOR_REMAINING_LIVE_GATES"


def budget(elapsed_seconds):
    if elapsed_seconds < 0:
        return "NOT_MEASURED"
    return "WITHIN_BUDGET" if elapsed_seconds <= 1200 else "BUDGET_EXCEEDED"


def remaining_budget(elapsed_seconds):
    """Execution receives remaining deadline, never another automatic attempt."""
    return max(0, 1200 - elapsed_seconds)
