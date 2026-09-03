#!/usr/bin/env python3
"""Verify a durable substantive review against the cumulative PR subject.

A submitted review may remain current across a later branch push only when the later
range is mechanically whitespace-only in already-reviewed prose files. Any content,
structural, path, mode, code, or configuration change returns NOT_PROVEN until a focused
review publishes a new subject-bound marker. Movement of the PR base is not part of this
comparison.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Iterable, Mapping, NamedTuple, Optional

MARKER_RE = re.compile(r"<!--\s*semantic-review:v1\s+(\{.*?\})\s*-->", re.DOTALL)
OID_RE = re.compile(r"^[0-9a-f]{40}$")
FENCE_RE = re.compile(r"^[ \t]*(`{3,}|~{3,})")
REQUIRED_SECTIONS = (
    "## Review scope",
    "## Evidence and falsifiers",
    "## What this establishes",
    "## Residual risk / not proved",
    "## Substantive review result",
)
# The substantive review vocabulary owned by the `review-pr` skill. Only
# REVIEW_CURRENT carries a subject-bound marker; the marker asserts that a review
# reached that conclusion, so no other result may mint one.
SUBSTANTIVE_REVIEW_RESULTS = (
    "REVIEW_CURRENT",
    "CHANGES_REQUIRED",
    "NOT_PROVEN",
    "BLOCKED_BY_PREREQUISITE",
    "SUPERSEDED_OR_CLOSE",
)
MARKER_RESULT = "REVIEW_CURRENT"
# The conclusion is a declaration, not a mention: exactly one result section
# carrying exactly one result. `##` is matched at line start so a heading inside
# a fenced block or a deeper `###` cannot pose as the declaration.
RESULT_SECTION_RE = re.compile(
    r"^[ \t]*##[ \t]+Substantive review result[ \t]*$", re.MULTILINE
)
ANY_SECTION_RE = re.compile(r"^[ \t]*##[ \t]", re.MULTILINE)
RESULT_ITEM_RE = re.compile(r"^[ \t]*[-*][ \t]*([A-Z_]+)\b", re.MULTILINE)


class Review(NamedTuple):
    login: str
    user_type: str
    state: str
    body: str
    commit_oid: str
    submitted_at: str


class Marker(NamedTuple):
    pr: int
    head: str
    merge_base: str
    subject_sha256: str
    result: str


class CurrentnessError(RuntimeError):
    """Instrument or repository-object failure; not a semantic verdict."""


def _run(
    args: list[str],
    *,
    cwd: Path,
    check: bool = True,
    text: bool = True,
) -> subprocess.CompletedProcess[Any]:
    """Run a child process, decoding text output as UTF-8 regardless of host locale.

    Text mode without an explicit encoding decodes through
    `locale.getpreferredencoding(False)`, so the same command yields different results
    on different hosts. Git emits UTF-8 paths and `gh` emits UTF-8 JSON, and review
    bodies in this repository routinely carry non-ASCII prose, so the locale default
    is wrong for every text call site here — in two ways. Under the C locale the ASCII
    codec raises `UnicodeDecodeError` on the first non-ASCII byte. Under cp1252 a
    Windows reviewer usually gets something worse: most bytes map to the wrong
    characters silently, and only the five undefined ones (0x81, 0x8d, 0x8f, 0x90,
    0x9d) raise. Pinning UTF-8 removes both.

    `errors="strict"` keeps a genuine decode failure loud: it reaches `main` as an
    instrument failure rather than replacing bytes that feed a digest or a path
    comparison.
    """
    encoding = "utf-8" if text else None
    errors = "strict" if text else None
    return subprocess.run(
        args,
        cwd=cwd,
        check=check,
        capture_output=True,
        text=text,
        encoding=encoding,
        errors=errors,
    )


def _git_text(root: Path, *args: str, check: bool = True) -> str:
    return _run(["git", *args], cwd=root, check=check).stdout.strip()


def ensure_commit(root: Path, oid: str) -> None:
    if not OID_RE.fullmatch(oid):
        raise CurrentnessError(f"invalid commit oid: {oid!r}")
    probe = _run(
        ["git", "cat-file", "-e", f"{oid}^{{commit}}"],
        cwd=root,
        check=False,
    )
    if probe.returncode == 0:
        return
    fetched = _run(
        ["git", "fetch", "--no-tags", "origin", oid],
        cwd=root,
        check=False,
    )
    if fetched.returncode != 0:
        raise CurrentnessError(
            f"commit {oid} is unavailable; fetch failed: {fetched.stderr.strip()}"
        )
    verify = _run(
        ["git", "cat-file", "-e", f"{oid}^{{commit}}"],
        cwd=root,
        check=False,
    )
    if verify.returncode != 0:
        raise CurrentnessError(f"fetched object is not a commit: {oid}")


def subject_digest(root: Path, merge_base: str, head: str) -> str:
    ensure_commit(root, merge_base)
    ensure_commit(root, head)
    ancestry = _run(
        ["git", "merge-base", "--is-ancestor", merge_base, head],
        cwd=root,
        check=False,
    )
    if ancestry.returncode != 0:
        raise CurrentnessError(
            f"marker merge base {merge_base} is not an ancestor of reviewed head {head}"
        )
    diff = _run(
        [
            "git",
            "diff",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            merge_base,
            head,
            "--",
        ],
        cwd=root,
        text=False,
    ).stdout
    return hashlib.sha256(diff).hexdigest()


def declared_review_result(body: str) -> Optional[str]:
    """Return the one substantive result this body declares, else None.

    None means absent *or ambiguous*, and ambiguity is the interesting case: a
    body carrying two result sections, or one section listing several results —
    an unedited template, say — declares no single conclusion. Searching for a
    `REVIEW_CURRENT` token anywhere would accept both, letting a body whose real
    conclusion is `CHANGES_REQUIRED` carry a current marker past the merge guard.

    A result named in prose is a mention, not a declaration, so only list items
    inside the single result section count. That keeps a review free to discuss
    the other outcomes without disqualifying itself.
    """
    headings = RESULT_SECTION_RE.findall(body)
    if len(headings) != 1:
        return None
    match = RESULT_SECTION_RE.search(body)
    if match is None:
        return None
    following = ANY_SECTION_RE.search(body, match.end())
    section = body[match.end() : following.start()] if following else body[match.end() :]
    declared = [
        item
        for item in RESULT_ITEM_RE.findall(section)
        if item in SUBSTANTIVE_REVIEW_RESULTS
    ]
    if len(declared) != 1:
        return None
    return declared[0]


def parse_marker(body: str, expected_pr: int, review_commit: str) -> Optional[Marker]:
    if not all(section in body for section in REQUIRED_SECTIONS):
        return None
    if "## Findings" not in body and "## No material findings" not in body:
        return None
    if declared_review_result(body) != MARKER_RESULT:
        return None

    matches = MARKER_RE.findall(body)
    if len(matches) != 1:
        return None
    try:
        raw = json.loads(matches[0])
    except json.JSONDecodeError:
        return None
    if not isinstance(raw, dict) or set(raw) != {
        "head",
        "merge_base",
        "pr",
        "result",
        "subject_sha256",
    }:
        return None
    if raw.get("result") != "REVIEW_CURRENT" or raw.get("pr") != expected_pr:
        return None
    head = raw.get("head")
    merge_base = raw.get("merge_base")
    digest = raw.get("subject_sha256")
    if not all(isinstance(value, str) for value in (head, merge_base, digest)):
        return None
    if not OID_RE.fullmatch(head) or not OID_RE.fullmatch(merge_base):
        return None
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        return None
    if head != review_commit:
        return None
    return Marker(expected_pr, head, merge_base, digest, "REVIEW_CURRENT")


def latest_valid_review(
    reviews: Iterable[Review], expected_pr: int
) -> tuple[Review, Marker] | None:
    candidates: list[tuple[Review, Marker]] = []
    for review in reviews:
        if review.user_type.lower() == "bot" or review.state.upper() == "DISMISSED":
            continue
        marker = parse_marker(review.body, expected_pr, review.commit_oid)
        if marker is not None:
            candidates.append((review, marker))
    if not candidates:
        return None
    return max(candidates, key=lambda pair: pair[0].submitted_at)


def fenced_blocks(text: str) -> list[str]:
    """Return the exact content of every fenced block, in document order.

    Prose files in this repository carry executable procedure inside fences — skill
    trees, shared contracts, and runbooks all publish commands that way. Whitespace is
    load-bearing there even though the surrounding file is prose, so the neutral class
    has to see fence content byte-for-byte rather than through a whitespace-insensitive
    comparison.
    """
    blocks: list[str] = []
    body: list[str] | None = None
    opener = ""
    for line in text.splitlines():
        match = FENCE_RE.match(line)
        if body is None:
            if match:
                body = []
                opener = match.group(1)
            continue
        if match:
            closer = match.group(1)
            # A closing fence uses the opener's character and is at least as long.
            if closer[0] == opener[0] and len(closer) >= len(opener):
                blocks.append("\n".join(body))
                body = None
                continue
        body.append(line)
    if body is not None:
        blocks.append("\n".join(body))
    return blocks


def blob_text(root: Path, rev: str, path: str) -> str:
    """Decode one blob as UTF-8, failing closed on malformed bytes.

    The result feeds `fenced_blocks`, whose equality decides whether a review
    carries forward. Replacement decoding is unsafe for that comparison: two
    different malformed byte sequences both collapse to U+FFFD, so a real change
    inside an executable fence could compare equal and silently carry a stale
    review over it. Refusing to decode surfaces as NOT_PROVEN, which is the
    correct answer for evidence this instrument cannot read.
    """
    raw = _run(["git", "show", f"{rev}:{path}"], cwd=root, text=False).stdout
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise CurrentnessError(
            f"{path} at {rev} is not valid UTF-8, so its reviewed content "
            f"cannot be compared: {error}"
        ) from error


def neutral_followup(root: Path, reviewed_head: str, current_head: str) -> tuple[bool, str]:
    ensure_commit(root, reviewed_head)
    ensure_commit(root, current_head)
    ancestry = _run(
        ["git", "merge-base", "--is-ancestor", reviewed_head, current_head],
        cwd=root,
        check=False,
    )
    if ancestry.returncode != 0:
        return False, "reviewed head is not an ancestor of current head"

    names = _git_text(root, "diff", "--name-status", reviewed_head, current_head, "--")
    if not names:
        return True, "no candidate changes after review"
    rows = [line.split("\t") for line in names.splitlines() if line]
    if any(parts[0] != "M" or len(parts) != 2 for parts in rows):
        return False, "path, file-kind, or structural change after review"
    paths = [parts[1] for parts in rows]
    if any(Path(path).suffix.lower() not in {".md", ".txt"} for path in paths):
        return False, "post-review change is not in a whitespace-insensitive prose file"

    # A prose extension does not make the whole file whitespace-insensitive. Fenced
    # blocks in these files carry commands and configuration, where inserting or
    # removing a space changes what runs, so they are compared byte-for-byte.
    for path in paths:
        if fenced_blocks(blob_text(root, reviewed_head, path)) != fenced_blocks(
            blob_text(root, current_head, path)
        ):
            return False, "post-review change alters fenced code content"

    diff = _run(
        [
            "git",
            "diff",
            "--quiet",
            "--ignore-all-space",
            "--ignore-blank-lines",
            reviewed_head,
            current_head,
            "--",
        ],
        cwd=root,
        check=False,
    )
    if diff.returncode == 0:
        return True, "later candidate range is whitespace-only in reviewed prose files"
    if diff.returncode == 1:
        return False, "material content change after review"
    raise CurrentnessError(f"git diff failed: {diff.stderr.strip()}")


def evaluate(
    root: Path,
    *,
    pr: int,
    current_head: str,
    reviews: Iterable[Review],
) -> dict[str, Any]:
    ensure_commit(root, current_head)
    selected = latest_valid_review(reviews, pr)
    if selected is None:
        return {
            "classification": "NOT_PROVEN",
            "reason": "no_substantive_review_currentness_marker",
            "pr": pr,
            "current_head": current_head,
            "reviewed_head": None,
            "carried_forward": False,
        }
    review, marker = selected
    actual_digest = subject_digest(root, marker.merge_base, marker.head)
    if actual_digest != marker.subject_sha256:
        return {
            "classification": "NOT_PROVEN",
            "reason": "review_subject_digest_mismatch",
            "pr": pr,
            "current_head": current_head,
            "reviewed_head": marker.head,
            "reviewer": review.login,
            "carried_forward": False,
        }

    if marker.head == current_head:
        return {
            "classification": "REVIEW_CURRENT",
            "reason": "subject_bound_review_matches_current_head",
            "pr": pr,
            "current_head": current_head,
            "reviewed_head": marker.head,
            "merge_base": marker.merge_base,
            "subject_sha256": marker.subject_sha256,
            "reviewer": review.login,
            "carried_forward": False,
        }

    neutral, reason = neutral_followup(root, marker.head, current_head)
    if not neutral:
        return {
            "classification": "NOT_PROVEN",
            "reason": reason.replace(" ", "_"),
            "pr": pr,
            "current_head": current_head,
            "reviewed_head": marker.head,
            "merge_base": marker.merge_base,
            "subject_sha256": marker.subject_sha256,
            "reviewer": review.login,
            "carried_forward": False,
        }
    return {
        "classification": "REVIEW_CURRENT",
        "reason": reason.replace(" ", "_"),
        "pr": pr,
        "current_head": current_head,
        "reviewed_head": marker.head,
        "merge_base": marker.merge_base,
        "subject_sha256": marker.subject_sha256,
        "reviewer": review.login,
        "carried_forward": marker.head != current_head,
    }


def _flatten_pages(value: Any) -> list[Mapping[str, Any]]:
    if not isinstance(value, list):
        raise CurrentnessError("review API did not return an array")
    if value and all(isinstance(page, list) for page in value):
        return [item for page in value for item in page if isinstance(item, dict)]
    return [item for item in value if isinstance(item, dict)]


def fetch_pr(repo: str, pr: int, root: Path) -> tuple[str, str, list[Review]]:
    metadata_raw = _run(
        [
            "gh",
            "pr",
            "view",
            str(pr),
            "--repo",
            repo,
            "--json",
            "headRefOid,baseRefOid",
        ],
        cwd=root,
    ).stdout
    metadata = json.loads(metadata_raw)
    current_head = metadata.get("headRefOid")
    base_head = metadata.get("baseRefOid")
    if not isinstance(current_head, str) or not isinstance(base_head, str):
        raise CurrentnessError("PR metadata omitted head/base identity")

    reviews_raw = _run(
        [
            "gh",
            "api",
            "--paginate",
            "--slurp",
            f"repos/{repo}/pulls/{pr}/reviews?per_page=100",
        ],
        cwd=root,
    ).stdout
    rows = _flatten_pages(json.loads(reviews_raw))
    reviews = [
        Review(
            login=str((row.get("user") or {}).get("login") or ""),
            user_type=str((row.get("user") or {}).get("type") or ""),
            state=str(row.get("state") or ""),
            body=str(row.get("body") or ""),
            commit_oid=str(row.get("commit_id") or ""),
            submitted_at=str(row.get("submitted_at") or ""),
        )
        for row in rows
    ]
    return current_head, base_head, reviews


class MarkerRefused(RuntimeError):
    """The substantive review result does not carry a marker; not an instrument failure."""


def emit_marker(root: Path, repo: str, pr: int, result: str) -> str:
    if result not in SUBSTANTIVE_REVIEW_RESULTS:
        raise CurrentnessError(f"unknown substantive review result: {result!r}")
    if result != MARKER_RESULT:
        raise MarkerRefused(
            f"{result} does not carry a subject-bound marker; "
            f"only {MARKER_RESULT} does"
        )
    current_head, base_head, _ = fetch_pr(repo, pr, root)
    ensure_commit(root, current_head)
    ensure_commit(root, base_head)
    merge_base = _git_text(root, "merge-base", base_head, current_head)
    digest = subject_digest(root, merge_base, current_head)
    payload = {
        "head": current_head,
        "merge_base": merge_base,
        "pr": pr,
        "result": result,
        "subject_sha256": digest,
    }
    return "<!-- semantic-review:v1 " + json.dumps(
        payload, sort_keys=True, separators=(",", ":")
    ) + " -->"


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pr", type=int)
    parser.add_argument("repo")
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--emit-marker", action="store_true")
    parser.add_argument(
        "--result",
        choices=SUBSTANTIVE_REVIEW_RESULTS,
        default=None,
        help=(
            "the substantive review result this marker binds. Required with "
            f"--emit-marker, with no default: only {MARKER_RESULT} emits a marker, "
            "and every other result is refused. A default would let the legacy "
            "invocation keep minting a REVIEW_CURRENT marker without the caller "
            "ever stating their conclusion, which is the defect this flag closes."
        ),
    )
    args = parser.parse_args(argv)
    if args.emit_marker and args.result is None:
        parser.error("--emit-marker requires --result naming the substantive review result")
    root = args.root.resolve()
    fixture = args.fixture
    if fixture is None and os.environ.get("SEMANTIC_REVIEW_TEST_FIXTURE"):
        fixture = Path(os.environ["SEMANTIC_REVIEW_TEST_FIXTURE"])
    try:
        if args.emit_marker:
            print(emit_marker(root, args.repo, args.pr, args.result))
            return 0
        if fixture:
            raw = json.loads(fixture.read_text(encoding="utf-8"))
            current_head = str(raw["head"])
            reviews = [Review(**row) for row in raw.get("reviews", [])]
        else:
            current_head, _base, reviews = fetch_pr(args.repo, args.pr, root)
        result = evaluate(
            root,
            pr=args.pr,
            current_head=current_head,
            reviews=reviews,
        )
    except MarkerRefused as refusal:
        # A refusal is a correct outcome of a non-REVIEW_CURRENT review, not a broken
        # instrument, so it stays distinguishable from both verdicts and failures.
        print(
            json.dumps(
                {
                    "classification": "MARKER_REFUSED",
                    "reason": "result_does_not_carry_a_marker",
                    "detail": str(refusal),
                    "pr": args.pr,
                    "result": args.result,
                },
                sort_keys=True,
            )
        )
        return 3
    except (
        CurrentnessError,
        KeyError,
        OSError,
        subprocess.CalledProcessError,
        json.JSONDecodeError,
        TypeError,
        ValueError,
    ) as error:
        result = {
            "classification": "NOT_PROVEN",
            "reason": "instrument_failure",
            "detail": str(error),
            "pr": args.pr,
        }
        print(json.dumps(result, sort_keys=True))
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0 if result["classification"] == "REVIEW_CURRENT" else 1


if __name__ == "__main__":
    sys.exit(main())
