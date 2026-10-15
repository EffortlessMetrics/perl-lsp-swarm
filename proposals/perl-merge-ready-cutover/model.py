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


def material(e):
    return e.event == "pull_request" and (
        e.action in INVALIDATIONS
        or (e.action in {"labeled", "unlabeled"} and e.label == LABEL)
        or (e.action == "edited" and e.base_changed))


def admit(e):
    # Membership in the PR label set is deliberately insufficient.
    return (material(e) and e.action == "labeled" and e.label == LABEL
            and not e.draft and e.base_ref in {"main", "master"}
            and e.attempt == 1)


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
            live_head, live_base, live_label, merge_parents, checkout,
            elapsed_seconds, run_cancelled=False):
    """All values originate in one run; never borrow same-named green checks."""
    if not admit(e) or receipt != expected or run_cancelled:
        return False
    if (e.pr, e.run, e.attempt) != (expected.pr, expected.run, expected.attempt):
        return False
    if not all(re.fullmatch(r"[0-9a-f]{40}", value) for value in (
            expected.head, expected.base, expected.merge, expected.tree, expected.workflow)):
        return False
    if (live_head, live_base) != (expected.head, expected.base) or not live_label:
        return False
    if merge_parents != (expected.base, expected.head):
        return False
    if checkout != (expected.merge, expected.tree):
        return False
    if not 0 <= elapsed_seconds <= 1200:
        return False
    return bool(required_workers) and all(
        results.get(worker) == "success" for worker in required_workers)
