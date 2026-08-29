"""Bind the worked-lane corpus to its category ledger.

`docs/agents/examples/` is the worked-lane calibration corpus requested by
issue #5247. That issue names eight required example categories, and its
2026-08-10 research ruling established the rule this test exists to enforce:

    a PR appearing in the pilot matrix is not by itself sufficient to satisfy
    one of #5247's eight example categories. The example should name the
    defining transition and cite durable evidence that actually demonstrates
    it.

Without a machine-checked ledger that rule holds only for as long as someone
remembers it. The corpus landed one control-plane document in #6450, and a
later read of that single document attributed four of the eight categories to
it -- exactly the over-attribution the ruling forbids. Prose alone could not
contradict that read, because nothing recorded which categories the document
carries.

So the ledger in `docs/agents/examples/README.md` is the artifact, and this is
its oracle. The checks below are chosen so that the cheap ways of faking
coverage fail:

* a category may not be silently dropped, duplicated, or renamed;
* `COVERED` must name a corpus file that exists, cite at least one durable
  receipt, and name a defining transition long enough to be a claim rather
  than a label;
* every receipt a `COVERED` row cites must actually appear in the lane
  document it points at, so the ledger cannot borrow a receipt the document
  never used;
* `ABSENT` must name nothing at all -- no lane, no receipt, no ruling. This is
  the negative control. Stretching one pilot across an unmet category requires
  writing a file name into an `ABSENT` row, and that fails here;
* an example file that no `COVERED` row claims is an orphan: it is in the
  corpus while the ledger says the corpus does not cover it.

The ledger is calibration material, not authority, and it has to keep saying
so; `test_ledger_disclaims_runtime_authority` holds that line.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "docs" / "agents" / "examples"
LEDGER = EXAMPLES / "README.md"
AGENTS_README = ROOT / "docs" / "agents" / "README.md"
WORKFLOW = ROOT / ".github" / "workflows" / "worked-lane-corpus.yml"
TEST_PATH = "tests/test_worked_lane_corpus.py"
WORKFLOW_PATH = ".github/workflows/worked-lane-corpus.yml"

# The eight categories issue #5247 requires, in the order that issue lists
# them. Order is checked as well as membership: the ledger is read top to
# bottom by a human calibrating against the method, and a silently reordered
# ledger is a different document.
REQUIRED_CATEGORIES = (
    "fresh-semantic-change",
    "existing-pr-midstream",
    "docs-or-metadata-no-proof",
    "proof-invalidates-plan",
    "feedback-repair-and-focused-rereview",
    "clean-formal-review",
    "ci-instrument-failure",
    "multi-pr-goal",
)

# Two states only. A third, softer state ("partial", "weak", "implied") is the
# vocabulary that lets one pilot drift across categories, which is the defect
# this ledger exists to prevent. A lane either demonstrates the defining
# transition on cited evidence or the category is open.
COVERED = "COVERED"
ABSENT = "ABSENT"
ALLOWED_STATUSES = {COVERED, ABSENT}

NONE = "none"

REQUIRED_FIELDS = (
    "Status",
    "Worked lane",
    "Source receipts",
    "Terminal ruling",
    "Defining transition",
    "What remains unproved",
)

# A defining transition has to name the routing decision the evidence shows.
# A bare category label restated as a sentence fragment does not, and the
# cheapest way to keep that honest is a floor on how much was actually said.
MIN_TRANSITION_CHARS = 60
MIN_UNPROVED_CHARS = 30

CATEGORY_HEADING = re.compile(r"^### ([a-z0-9-]+)\s*$")
FIELD_LINE = re.compile(r"^- \*\*([^:*]+):\*\* (.+)$")
LANE_FILE = re.compile(r"^`([A-Za-z0-9_.-]+\.md)`$")
RECEIPT = re.compile(r"#(\d+)")

LEDGER_SECTION = "## Category ledger"

DISCLAIMER = "not runtime authority"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_ledger() -> list[tuple[str, dict[str, str]]]:
    """Return the ledger's category rows in document order.

    Parsing is deliberately strict. A field line that does not match
    `FIELD_LINE` is invisible to every check below, so a typo in a bold label
    would silently drop an oracle rather than fail one. The required-field
    check in `test_every_category_declares_every_field` is what converts that
    silence into a failure.
    """
    lines = _read(LEDGER).splitlines()
    try:
        start = lines.index(LEDGER_SECTION)
    except ValueError as error:  # pragma: no cover - guarded by its own test
        raise AssertionError(
            f"{LEDGER.relative_to(ROOT)}: missing '{LEDGER_SECTION}' section"
        ) from error

    rows: list[tuple[str, dict[str, str]]] = []
    current: str | None = None
    fields: dict[str, str] = {}
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        heading = CATEGORY_HEADING.match(line)
        if heading:
            if current is not None:
                rows.append((current, fields))
            current = heading.group(1)
            fields = {}
            continue
        field = FIELD_LINE.match(line)
        if field and current is not None:
            fields[field.group(1).strip()] = field.group(2).strip()
    if current is not None:
        rows.append((current, fields))
    return rows


def example_documents() -> set[str]:
    """Every worked-lane document in the corpus, excluding the ledger itself."""
    return {
        path.name
        for path in EXAMPLES.glob("*.md")
        if path.name != LEDGER.name
    }


def workflow_paths(event: str) -> set[str]:
    lines = _read(WORKFLOW).splitlines()
    marker = f"  {event}:"
    try:
        start = lines.index(marker)
    except ValueError as error:
        raise AssertionError(f"missing on.{event} event block") from error

    paths: set[str] = set()
    in_paths = False
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent <= 2 and stripped.endswith(":"):
            break
        if stripped == "paths:":
            in_paths = True
            continue
        if in_paths and stripped.startswith("- '") and stripped.endswith("'"):
            paths.add(stripped[3:-1])
        elif in_paths and indent <= 4:
            in_paths = False
    return paths


class WorkedLaneLedgerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.assertTrue(
            LEDGER.is_file(),
            f"{LEDGER.relative_to(ROOT)} must exist: the corpus is accounted for "
            f"by its ledger, not by the presence of example files",
        )
        self.rows = parse_ledger()
        self.by_category = dict(self.rows)

    def test_ledger_covers_every_required_category_exactly_once(self) -> None:
        listed = [category for category, _ in self.rows]
        self.assertEqual(
            listed,
            list(REQUIRED_CATEGORIES),
            "the ledger must list issue #5247's eight required categories, each "
            "exactly once, in the order that issue states them; an unmet "
            "category is recorded as ABSENT rather than omitted, because an "
            "omitted row and a met category are indistinguishable to a reader",
        )

    def test_every_category_declares_every_field(self) -> None:
        for category, fields in self.rows:
            missing = [name for name in REQUIRED_FIELDS if name not in fields]
            self.assertEqual(
                missing,
                [],
                f"{category}: missing ledger field(s) {missing}; every row "
                f"carries the same five-field accounting so that rows can be "
                f"compared rather than read as free prose",
            )

    def test_status_vocabulary_is_closed(self) -> None:
        for category, fields in self.rows:
            self.assertIn(
                fields.get("Status"),
                ALLOWED_STATUSES,
                f"{category}: status {fields.get('Status')!r} is outside the "
                f"closed vocabulary {sorted(ALLOWED_STATUSES)}; a softer third "
                f"state is how one pilot starts covering several categories",
            )

    def test_covered_rows_name_an_existing_lane_document(self) -> None:
        present = example_documents()
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            lane = fields.get("Worked lane", "")
            match = LANE_FILE.match(lane)
            self.assertIsNotNone(
                match,
                f"{category}: COVERED requires a backticked worked-lane file "
                f"name, got {lane!r}",
            )
            assert match is not None  # narrowed by the assertion above
            self.assertIn(
                match.group(1),
                present,
                f"{category}: names worked lane {match.group(1)!r}, which is not "
                f"in {EXAMPLES.relative_to(ROOT)}; the ledger claims coverage "
                f"from a document that does not exist",
            )

    def test_covered_rows_cite_durable_receipts_and_a_ruling(self) -> None:
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            receipts = fields.get("Source receipts", "")
            self.assertTrue(
                RECEIPT.search(receipts),
                f"{category}: COVERED must cite at least one durable issue or "
                f"PR reference, got {receipts!r}",
            )
            ruling = fields.get("Terminal ruling", "")
            self.assertNotEqual(
                ruling.lower(),
                NONE,
                f"{category}: COVERED must record the terminal ruling the "
                f"coverage rests on, or say why none applies",
            )

    def test_covered_rows_state_a_defining_transition_and_a_boundary(self) -> None:
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            transition = fields.get("Defining transition", "")
            self.assertGreaterEqual(
                len(transition),
                MIN_TRANSITION_CHARS,
                f"{category}: the defining transition ({transition!r}) is too "
                f"short to be a claim; #5247 requires the example to name the "
                f"transition its evidence demonstrates, not restate the "
                f"category name",
            )
            unproved = fields.get("What remains unproved", "")
            self.assertGreaterEqual(
                len(unproved),
                MIN_UNPROVED_CHARS,
                f"{category}: COVERED must still record what the lane does not "
                f"establish; a coverage claim with no boundary is the shape "
                f"#5247 exists to prevent",
            )

    def test_covered_rows_state_distinct_defining_transitions(self) -> None:
        """One document may carry two categories; it may not carry them twice.

        A worked lane can legitimately demonstrate more than one category when
        it holds separate evidence for each -- #5247 lists the categories as
        distinct transitions, not as distinct PRs. What it cannot do is inherit
        a second category from the first, and the visible symptom of that is a
        defining transition copied between rows.
        """
        seen: dict[str, str] = {}
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            transition = " ".join(fields.get("Defining transition", "").split())
            previous = seen.get(transition)
            self.assertIsNone(
                previous,
                f"{category} and {previous} declare the same defining "
                f"transition; a category is covered by the transition its own "
                f"evidence demonstrates, not by sharing another row's",
            )
            seen[transition] = category

    def test_covered_receipts_appear_in_the_lane_document(self) -> None:
        """A cited receipt must be one the lane document actually uses.

        Nothing else stops the ledger from attaching a strong, unrelated PR to
        a weak row: the file exists, the reference is well-formed, and the
        prose reads plausibly. Requiring the document to mention the same
        reference keeps the receipt bound to the lane it is claimed for.
        """
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            match = LANE_FILE.match(fields.get("Worked lane", ""))
            if match is None:
                continue  # reported by test_covered_rows_name_an_existing_lane_document
            document = EXAMPLES / match.group(1)
            if not document.is_file():
                continue  # reported by test_covered_rows_name_an_existing_lane_document
            body = _read(document)
            cited = set(RECEIPT.findall(fields.get("Source receipts", "")))
            used = set(RECEIPT.findall(body))
            borrowed = sorted(cited - used, key=int)
            self.assertEqual(
                borrowed,
                [],
                f"{category}: cites receipt(s) {borrowed} that "
                f"{match.group(1)} never references; a ledger row may only "
                f"claim the evidence its worked lane actually used",
            )

    def test_absent_rows_name_nothing(self) -> None:
        """The negative control.

        An unmet category is where over-attribution happens: a nearby pilot is
        close enough that naming it feels like progress. `ABSENT` therefore
        carries no lane, no receipt, and no ruling, so filling one in is a
        deliberate promotion to `COVERED` with every `COVERED` oracle attached,
        not a quiet edit to a row nobody checks.
        """
        for category, fields in self.rows:
            if fields.get("Status") != ABSENT:
                continue
            for name in ("Worked lane", "Source receipts", "Terminal ruling"):
                self.assertEqual(
                    fields.get(name, "").lower(),
                    NONE,
                    f"{category}: is ABSENT but its {name!r} field says "
                    f"{fields.get(name)!r}; an unmet category must not borrow a "
                    f"lane, a receipt, or a ruling from a pilot that "
                    f"demonstrates something else",
                )
            unproved = fields.get("What remains unproved", "")
            self.assertGreaterEqual(
                len(unproved),
                MIN_UNPROVED_CHARS,
                f"{category}: ABSENT must say what a lane would have to "
                f"demonstrate, so the gap stays actionable",
            )

    def test_no_example_document_is_unaccounted_for(self) -> None:
        claimed: set[str] = set()
        for _, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            match = LANE_FILE.match(fields.get("Worked lane", ""))
            if match is not None:
                claimed.add(match.group(1))
        orphans = sorted(example_documents() - claimed)
        self.assertEqual(
            orphans,
            [],
            f"worked-lane document(s) {orphans} are in "
            f"{EXAMPLES.relative_to(ROOT)} but no COVERED ledger row claims "
            f"them; a document in the corpus that the accounting does not see "
            f"is how the corpus starts overstating itself",
        )

    def test_ledger_disclaims_runtime_authority(self) -> None:
        self.assertIn(
            DISCLAIMER,
            _read(LEDGER),
            f"{LEDGER.relative_to(ROOT)} must keep stating that the corpus is "
            f"{DISCLAIMER!r}; #5247 requires the examples to remain optional "
            f"just-in-time references rather than a second method contract",
        )

    def test_agents_readme_points_at_the_ledger(self) -> None:
        self.assertIn(
            "examples/README.md",
            _read(AGENTS_README),
            "docs/agents/README.md must link the worked-lane ledger; the "
            "corpus front door is where a reader learns what is and is not "
            "covered",
        )


class WorkedLaneWorkflowTests(unittest.TestCase):
    """The ledger only holds if the check runs on the files it constrains."""

    REQUIRED_PATHS = {
        WORKFLOW_PATH,
        TEST_PATH,
        "docs/agents/README.md",
        "docs/agents/examples/**",
    }

    def test_workflow_exists(self) -> None:
        self.assertTrue(
            WORKFLOW.is_file(),
            f"{WORKFLOW_PATH} must exist, otherwise this contract is a local "
            f"convention rather than a check",
        )

    def test_workflow_triggers_on_every_constrained_path(self) -> None:
        for event in ("pull_request", "push"):
            missing = sorted(self.REQUIRED_PATHS - workflow_paths(event))
            self.assertEqual(
                missing,
                [],
                f"on.{event}.paths is missing {missing}; a change that adds, "
                f"renames, or deletes a worked lane has to run this check, or "
                f"the ledger goes stale on main with no gate having run",
            )


if __name__ == "__main__":  # pragma: no cover
    unittest.main()
