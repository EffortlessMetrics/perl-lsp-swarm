#!/usr/bin/env python3
"""Apply the provider/documentation half of PR #6625's semantic review contract."""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SKILLS = (
    ROOT / ".agents/skills/review-pr/SKILL.md",
    ROOT / ".claude/skills/review-pr/SKILL.md",
)
DOC = ROOT / "docs/agents/REVIEW_CURRENTNESS.md"
TEST = ROOT / "tests/test_semantic_review_currentness.py"

SKILL_ANCHOR = """   cannot be dispositioned per finding. `scripts/reviews/inline` never resolves
   anything; resolution stays in `scripts/reviews/disposition`.
"""

SKILL_INSERT = """

   A `COMMENTED` review is only a GitHub fact; it does not become substantive merely
   because a human submitted it. When the cumulative conclusion is `REVIEW_CURRENT`,
   append one subject-bound marker generated from the current PR diff:

   ```bash
   python3 scripts/ci/check-pr-semantic-review-currentness.py \
     <pr> <owner/repo> --emit-marker
   ```

   Put the emitted `<!-- semantic-review:v1 ... -->` marker in the same review body
   after the useful review record below. The marker binds the judgment to the PR's
   merge base, reviewed head, and cumulative binary diff; it is not an exact-head
   ceremony. The checker may carry it across a later push only when every later edit is
   whitespace-only in an already-reviewed `.md` or `.txt` file. Any code, configuration,
   path, mode, or material prose change requires focused review and a new marker.
"""

DOC_ANCHOR = """repair invalidates evidence and reliable replacement proof is missing.

## GitHub-native merge blockers
"""

DOC_INSERT = """repair invalidates evidence and reliable replacement proof is missing.

## Durable semantic review record

A GitHub `COMMENTED` review is evidence that somebody submitted text. It does not prove
that the cumulative claim received a substantive review, and zero unresolved threads
does not supply the missing judgment.

A `REVIEW_CURRENT` review therefore carries one machine-readable marker in the same
submitted review body:

```text
<!-- semantic-review:v1 {"head":"<reviewed-head>","merge_base":"<merge-base>","pr":<number>,"result":"REVIEW_CURRENT","subject_sha256":"<cumulative-diff-digest>"} -->
```

Generate it only after the useful review record is complete:

```bash
python3 scripts/ci/check-pr-semantic-review-currentness.py \
  <pr> <owner/repo> --emit-marker
```

The digest covers `git diff --binary --full-index <merge-base> <reviewed-head>`. The
marker is not a requirement to re-review merely because a SHA changed. It makes the
reviewed subject falsifiable and lets the pre-merge guard distinguish a real cumulative
judgment from an empty comment.

Automation carries the marker forward only through an intentionally narrow neutral
class: no candidate change, or whitespace-only edits in already-reviewed `.md`/`.txt`
files. It never treats whitespace changes in code or configuration as neutral because
indentation and spacing can be semantic. Any other later change yields `NOT_PROVEN`
until focused review publishes a new marker. Human review can determine that a broader
change preserves the conclusion; the mechanical checker does not invent that judgment.

## GitHub-native merge blockers
"""

TEST_ANCHOR = """def test_subject_bound_checker_requires_durable_review_record() -> None:
"""
TEST_INSERT = """def test_review_skills_publish_subject_bound_currentness_marker() -> None:
    for relative in (
        ".agents/skills/review-pr/SKILL.md",
        ".claude/skills/review-pr/SKILL.md",
    ):
        text = (ROOT / relative).read_text(encoding="utf-8")
        assert "check-pr-semantic-review-currentness.py" in text
        assert "--emit-marker" in text
        assert "semantic-review:v1" in text
        assert "COMMENTED` review is only a GitHub fact" in text


def test_shared_contract_defines_durable_semantic_review_record() -> None:
    text = " ".join(
        (ROOT / "docs/agents/REVIEW_CURRENTNESS.md")
        .read_text(encoding="utf-8")
        .split()
    )
    assert "## Durable semantic review record" in text
    assert "git diff --binary --full-index" in text
    assert "whitespace-only edits in already-reviewed `.md`/`.txt` files" in text
    assert "indentation and spacing can be semantic" in text


""" + TEST_ANCHOR


def insert_once(path: Path, anchor: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    if replacement.strip() in text:
        return
    count = text.count(anchor)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}")
    path.write_text(text.replace(anchor, anchor + replacement, 1), encoding="utf-8")


def main() -> None:
    for skill in SKILLS:
        insert_once(skill, SKILL_ANCHOR, SKILL_INSERT)

    text = DOC.read_text(encoding="utf-8")
    if "## Durable semantic review record" not in text:
        count = text.count(DOC_ANCHOR)
        if count != 1:
            raise SystemExit(f"{DOC}: expected one anchor, found {count}")
        DOC.write_text(text.replace(DOC_ANCHOR, DOC_INSERT, 1), encoding="utf-8")

    text = TEST.read_text(encoding="utf-8")
    if "test_review_skills_publish_subject_bound_currentness_marker" not in text:
        count = text.count(TEST_ANCHOR)
        if count != 1:
            raise SystemExit(f"{TEST}: expected one anchor, found {count}")
        TEST.write_text(text.replace(TEST_ANCHOR, TEST_INSERT, 1), encoding="utf-8")


if __name__ == "__main__":
    main()
