#!/usr/bin/env python3
"""Classify Workflow Policy proof presence in the exact candidate tree (#13014).

The Workflow Policy job deliberately checks out the exact pull-request head, but
GitHub composes the *effective* workflow from the merge commit. When the default
branch lands a new proof and its workflow step together, an older candidate head
receives the newer step list while its tree still lacks the proof. The step then
dies before any assertion runs::

    error: no test target named `perl_version_matrix_cache_contract` in `xtask` package

That is a source-revision skew between workflow and tree, not a candidate defect,
yet it is reported exactly like a failing contract.

This helper gives every absent proof a typed, visible disposition so the workflow
can skip *only* the stale-head case while every other state stays red:

``present``
    the proof exists as a regular file in the candidate tree; the step runs and
    its own exit status decides the result. Nothing here can turn a failing or
    unloadable present proof green.
``stale_head_absent``
    the proof file is absent *and* the candidate's own copy of the workflow never
    mentions it. The candidate predates both the proof and its step, so the step
    is an authorized scoped no-op.
``declared_but_missing``
    the proof file is absent while the candidate's own workflow still declares
    it. That is a candidate that removed a proof it continues to claim, so it
    fails closed rather than silently retiring a gate.
``not_regular_file``
    the proof path exists but is a directory or a symlink. It fails closed; the
    guard never resolves outside a plain tracked file.

The declaration marker is the proof path itself, which this repository already
requires in the workflow's ``on.paths`` filters. Classification therefore reads
only the candidate tree: no base ref is fetched and no base-owned code is
executed inside the candidate.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

PRESENT = "present"
STALE_HEAD_ABSENT = "stale_head_absent"
DECLARED_BUT_MISSING = "declared_but_missing"
NOT_REGULAR_FILE = "not_regular_file"

#: Dispositions that must keep the job red.
FAILING_DISPOSITIONS = frozenset({DECLARED_BUT_MISSING, NOT_REGULAR_FILE})

DISPOSITION_REASON = {
    PRESENT: "present in the candidate tree; the proof runs and decides its own result",
    STALE_HEAD_ABSENT: (
        "absent from the candidate tree and undeclared by the candidate workflow; "
        "authorized stale-head scoped no-op"
    ),
    DECLARED_BUT_MISSING: (
        "absent from the candidate tree but still declared by the candidate "
        "workflow; the candidate removed a proof it continues to claim"
    ),
    NOT_REGULAR_FILE: "present but not a regular file; the guard refuses to resolve it",
}

OUTPUT_NAME_ALLOWED = set(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_"
)


def parse_proof(spec: str) -> tuple[str, str]:
    """Split one ``id=path`` proof spec.

    The id becomes a GitHub step output name, so it is restricted to characters
    that survive ``$GITHUB_OUTPUT`` without quoting. The path is repository
    relative and must stay inside the tree.
    """
    identifier, separator, path = spec.partition("=")
    if not separator:
        raise ValueError(f"proof spec must be id=path, got {spec!r}")
    identifier = identifier.strip()
    path = path.strip()
    if not identifier:
        raise ValueError(f"proof spec has an empty id: {spec!r}")
    if not path:
        raise ValueError(f"proof spec has an empty path: {spec!r}")
    if not set(identifier) <= OUTPUT_NAME_ALLOWED:
        raise ValueError(
            f"proof id {identifier!r} must use only letters, digits, and underscores"
        )
    if path.startswith("/") or ".." in Path(path).parts:
        raise ValueError(f"proof path {path!r} must be repository relative")
    return identifier, path


def classify(root: Path, workflow_text: str, proof_path: str) -> str:
    """Return the disposition of one proof path within the candidate tree."""
    target = root / proof_path
    if target.is_symlink():
        return NOT_REGULAR_FILE
    if target.exists() and not target.is_file():
        return NOT_REGULAR_FILE
    if target.is_file():
        return PRESENT
    if proof_path in workflow_text:
        return DECLARED_BUT_MISSING
    return STALE_HEAD_ABSENT


def classify_all(
    root: Path, workflow_text: str, proofs: list[tuple[str, str]]
) -> list[tuple[str, str, str]]:
    """Classify every proof, returning ``(id, path, disposition)`` rows."""
    return [
        (identifier, path, classify(root, workflow_text, path))
        for identifier, path in proofs
    ]


def render_report(rows: list[tuple[str, str, str]]) -> str:
    """Render the human-readable disposition table."""
    lines = [
        "| proof | path | disposition | meaning |",
        "| --- | --- | --- | --- |",
    ]
    lines.extend(
        f"| `{identifier}` | `{path}` | `{disposition}` | "
        f"{DISPOSITION_REASON[disposition]} |"
        for identifier, path, disposition in rows
    )
    return "\n".join(lines)


def write_outputs(path: str, rows: list[tuple[str, str, str]]) -> None:
    """Append ``id=disposition`` pairs to a ``$GITHUB_OUTPUT`` file."""
    with open(path, "a", encoding="utf-8") as handle:
        for identifier, _path, disposition in rows:
            handle.write(f"{identifier}={disposition}\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Classify whether each Workflow Policy proof exists in the exact "
            "candidate tree so stale heads skip only absent proofs (#13014)."
        )
    )
    parser.add_argument(
        "--workflow",
        required=True,
        help=(
            "the candidate's own copy of the workflow, used only to decide "
            "whether the candidate still declares an absent proof"
        ),
    )
    parser.add_argument(
        "--proof",
        action="append",
        default=[],
        metavar="ID=PATH",
        required=True,
        help="a proof to classify; repeat once per guarded step",
    )
    parser.add_argument(
        "--root",
        default=".",
        help="repository root to resolve proof paths against (default: .)",
    )
    parser.add_argument(
        "--github-output",
        default=os.environ.get("GITHUB_OUTPUT"),
        help="file receiving id=disposition step outputs (default: $GITHUB_OUTPUT)",
    )
    parser.add_argument(
        "--summary",
        default=os.environ.get("GITHUB_STEP_SUMMARY"),
        help="file receiving the markdown report (default: $GITHUB_STEP_SUMMARY)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    try:
        proofs = [parse_proof(spec) for spec in args.proof]
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    identifiers = [identifier for identifier, _ in proofs]
    if len(identifiers) != len(set(identifiers)):
        print(
            "error: duplicate proof ids resolve to ambiguous step outputs",
            file=sys.stderr,
        )
        return 2

    root = Path(args.root)
    workflow = root / args.workflow
    if not workflow.is_file():
        # Without the candidate workflow the declared/undeclared split cannot be
        # made, so absence cannot be classified. Refuse instead of guessing.
        print(
            f"error: candidate workflow {args.workflow} is not a readable file; "
            "proof presence cannot be classified",
            file=sys.stderr,
        )
        return 2
    workflow_text = workflow.read_text(encoding="utf-8", errors="replace")

    rows = classify_all(root, workflow_text, proofs)
    report = render_report(rows)
    print(report)

    if args.github_output:
        write_outputs(args.github_output, rows)
    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as handle:
            handle.write("### Workflow Policy candidate proof presence\n\n")
            handle.write(f"{report}\n\n")

    failures = [row for row in rows if row[2] in FAILING_DISPOSITIONS]
    if failures:
        for identifier, path, disposition in failures:
            print(
                f"error: proof {identifier} ({path}) is {disposition}: "
                f"{DISPOSITION_REASON[disposition]}",
                file=sys.stderr,
            )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
