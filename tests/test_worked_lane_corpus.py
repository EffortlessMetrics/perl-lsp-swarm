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
#
# These are length-and-vocabulary floors only. They reject a row that restates
# its own category name and nothing else; they do not and cannot judge whether
# the narrative is true of the lane. That judgment is review's -- it is what
# demoted `feedback-repair-and-focused-rereview` to `ABSENT` after every
# structural check on that row had passed. Do not try to grow these constants
# into a semantic check; grow the review instead.
MIN_TRANSITION_CHARS = 60
MIN_TRANSITION_TERMS = 12
MIN_UNPROVED_CHARS = 30

CATEGORY_HEADING = re.compile(r"^### ([a-z0-9-]+)\s*$")
FIELD_LINE = re.compile(r"^- \*\*([^:*]+):\*\* (.+)$")
# Ledger paths are relative to the corpus directory and use POSIX separators,
# including when a future lane is kept in a subdirectory. Every segment must
# begin with a character other than `.`, which is what actually rejects `..`
# and keeps the path a child of `EXAMPLES` once it is joined below --
# `test_lane_paths_cannot_escape_the_corpus` is the control. A permissive
# `[A-Za-z0-9_.-]+` segment reads as if it excluded `..`, but the dot is in the
# class, so `../x.md` matched and the receipts check would then read a document
# outside the corpus.
LANE_SEGMENT = r"[A-Za-z0-9_-][A-Za-z0-9_.-]*"
LANE_FILE = re.compile(rf"^`({LANE_SEGMENT}(?:/{LANE_SEGMENT})*\.md)`$")

# A receipt is an issue/PR reference or a commit. Both are checked, because a
# ledger that verified only `#NNNN` while a row also listed a squash SHA would
# let the unverified half of the citation say anything -- the row would look
# more precise than it had been checked to be.
ISSUE_REF = re.compile(r"#(\d+)")
COMMIT_REF = re.compile(r"\b([0-9a-f]{7,40})\b")
# Git abbreviates: a ledger row may carry the full squash SHA while the lane
# document carries a short head. Prefix matching in either direction is the
# comparison git itself uses.
MIN_ABBREV = 7

# The front door states how many categories are open. A hard-coded count there
# goes stale silently the moment a row is promoted, and it is the first thing a
# reader sees, so it is derived and checked rather than trusted.
README_ABSENT_COUNT = re.compile(r"(\d+) of (\d+) categories are currently `ABSENT`")

LEDGER_SECTION = "## Category ledger"

DISCLAIMER = "not runtime authority"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_ledger(text: str | None = None) -> list[tuple[str, dict[str, str]]]:
    """Return the ledger's category rows in document order.

    `text` overrides the committed ledger so the parser's own rules can be
    driven by a test directly; production callers read the real file.

    The parser is line-oriented and has no Markdown model -- no awareness of
    fenced code blocks, HTML comments, or nested headings. That is affordable
    only because nothing inside the ledger section is discarded: a line that
    matches neither a category heading nor a field label is folded into the
    field it follows, so every character between one field label and the next
    is part of some value that the checks below read.

    Silently dropping such a line was the original shape, and it was wrong. A
    field value soft-wrapped onto a second physical line renders identically
    and reviews identically, but the continuation matched no pattern and
    vanished -- so a row could carry a fabricated receipt or lose half its
    defining transition while the suite stayed green. Folding removes the
    hiding place; a line before a row's first field, which has no field to
    fold into, is a hard failure instead. A line before the section's first
    category is the same failure one level up, and a worse one: with no row to
    belong to it reads as a statement about the whole ledger, so "all eight
    categories are carried by the landed lane" could sit above rows saying the
    opposite and no check would see it.
    """
    lines = (_read(LEDGER) if text is None else text).splitlines()
    try:
        start = lines.index(LEDGER_SECTION)
    except ValueError as error:  # pragma: no cover - guarded by its own test
        raise AssertionError(
            f"{LEDGER.relative_to(ROOT)}: missing '{LEDGER_SECTION}' section"
        ) from error

    rows: list[tuple[str, dict[str, str]]] = []
    current: str | None = None
    fields: dict[str, str] = {}
    field_name: str | None = None
    for offset, line in enumerate(lines[start + 1 :], start=start + 2):
        if line.startswith("## "):
            break
        heading = CATEGORY_HEADING.match(line)
        if heading:
            if current is not None:
                rows.append((current, fields))
            current = heading.group(1)
            fields = {}
            field_name = None
            continue
        field = FIELD_LINE.match(line)
        if field and current is not None:
            field_name = field.group(1).strip()
            if field_name not in REQUIRED_FIELDS:
                # The field vocabulary is closed for the same reason the status
                # vocabulary is. An unrecognized label parses, renders, and is
                # read by nobody -- a row could carry "Coverage note: actually
                # this category IS covered" beside its own `Status: ABSENT` and
                # every check would still pass.
                raise AssertionError(
                    f"{LEDGER.relative_to(ROOT)}:{offset}: category "
                    f"{current!r} declares the unknown field {field_name!r}; "
                    f"a row states {list(REQUIRED_FIELDS)} and nothing else, "
                    f"or it carries a claim no check reads"
                )
            if field_name in fields:
                # Last-write-wins would let a row state a claim twice and have
                # only the second one checked, while Markdown shows a reader
                # both. A fabricated receipt placed above the real one is the
                # concrete case: every check reads the survivor and passes.
                raise AssertionError(
                    f"{LEDGER.relative_to(ROOT)}:{offset}: category "
                    f"{current!r} repeats the field {field_name!r}; a row "
                    f"states each field once, or the checks below read one "
                    f"value while the page shows another"
                )
            fields[field_name] = field.group(2).strip()
            continue
        if not line.strip():
            continue
        if current is None:
            raise AssertionError(
                f"{LEDGER.relative_to(ROOT)}:{offset}: text in "
                f"'{LEDGER_SECTION}' before its first category heading; the "
                f"section holds category rows and nothing else, because a "
                f"line with no row to belong to reads as a claim about every "
                f"row and no check below can contradict it"
            )
        if field_name is None:
            raise AssertionError(
                f"{LEDGER.relative_to(ROOT)}:{offset}: text inside category "
                f"{current!r} before its first field label; every line in a "
                f"category row must belong to a labelled field, or it is "
                f"content no check reads"
            )
        fields[field_name] = f"{fields[field_name]} {line.strip()}".strip()
    if current is not None:
        rows.append((current, fields))
    return rows


def receipt_tokens(text: str) -> tuple[set[str], set[str]]:
    """Split receipt references into issue/PR numbers and commit ids."""
    issues = set(ISSUE_REF.findall(text))
    # An issue reference is not also a commit: strip them before matching hex
    # so `#5717` cannot masquerade as a short SHA, and ignore anything that is
    # all digits for the same reason.
    without_issues = ISSUE_REF.sub(" ", text)
    commits = {
        token
        for token in COMMIT_REF.findall(without_issues)
        if not token.isdigit()
    }
    return issues, commits


def commit_is_bound(cited: str, used: set[str]) -> bool:
    """Is a cited commit the same commit as one the lane document names?

    Git abbreviates, so the ledger may hold a full squash SHA where the lane
    holds a short head. Either being a prefix of the other is the same
    comparison git makes.
    """
    return any(
        (cited.startswith(seen) or seen.startswith(cited))
        and min(len(cited), len(seen)) >= MIN_ABBREV
        for seen in used
    )


def example_documents() -> set[str]:
    """Every worked-lane document in the corpus, excluding the ledger itself.

    Return POSIX paths relative to the corpus so the result has the same shape
    as a ledger path on every host platform.
    """
    return {
        path.relative_to(EXAMPLES).as_posix()
        for path in EXAMPLES.rglob("*.md")
        if path != LEDGER
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
        if in_paths and stripped.startswith("- "):
            # Quote style is the author's choice and does not change what the
            # workflow matches, so accept single, double, and bare items. A
            # style-only reformat should not produce a drift failure that
            # looks like a real one.
            paths.add(stripped[2:].strip().strip("'\""))
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
        """A `COVERED` row needs an issue/PR anchor, not just a commit.

        `receipt_tokens` extracts commits too, and
        `test_covered_receipts_appear_in_the_lane_document` binds them -- but
        commits are additive evidence here, never a substitute for this floor.
        Deliberately so: a commit identifies a change, while the claim a row
        makes is that a *deliberated transition* was reached and ruled on. That
        deliberation lives on an issue or PR, which is also where the row's
        required `Terminal ruling` comes from -- `ci-instrument-failure` cites
        PR #5717 and rests on #4192's `PROMOTE`.

        So a row citing only a SHA is rejected on purpose. Relaxing this to
        `cited_issues or cited_commits` would let a bare hash carry a coverage
        claim whose ruling no reader can find, which lowers the evidentiary bar
        this ledger exists to raise.
        """
        for category, fields in self.rows:
            if fields.get("Status") != COVERED:
                continue
            receipts = fields.get("Source receipts", "")
            self.assertTrue(
                ISSUE_REF.search(receipts),
                f"{category}: COVERED must cite at least one durable issue or "
                f"PR reference as `#NNNN` (a full GitHub link may accompany it, "
                f"but the bare number is what is matched), got {receipts!r}",
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
            # A length floor alone is satisfied by padding the category name
            # out to 60 characters. Requiring vocabulary the category name
            # does not already supply costs nothing and rejects the laziest
            # form of that, without pretending to judge the narrative.
            own_words = set(category.split("-"))
            fresh = {
                word
                for word in re.findall(r"[a-z]+", transition.lower())
                if word not in own_words and len(word) > 2
            }
            self.assertGreaterEqual(
                len(fresh),
                MIN_TRANSITION_TERMS,
                f"{category}: the defining transition uses only "
                f"{len(fresh)} word(s) the category name does not already "
                f"supply; it restates the label instead of naming the "
                f"transition the evidence demonstrates",
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
            cited_issues, cited_commits = receipt_tokens(
                fields.get("Source receipts", "")
            )
            used_issues, used_commits = receipt_tokens(body)
            borrowed = sorted(cited_issues - used_issues, key=int)
            borrowed += sorted(
                commit
                for commit in cited_commits
                if not commit_is_bound(commit, used_commits)
            )
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

    def test_a_repeated_field_label_is_rejected(self) -> None:
        """Parsing is the oracle's only view of the ledger; it must not lose a line.

        A row that states a field twice renders both lines to a reader while a
        last-write-wins parser checks only the survivor -- a fabricated receipt
        placed above the real one passed every other check. The committed
        ledger cannot exercise that, so this drives the rule on a synthetic
        body, and the accepted case below keeps the rule from rejecting a
        well-formed row.
        """
        def row(*extra: str) -> str:
            lines = [
                f"- **{name}:** a sufficiently long placeholder value"
                for name in REQUIRED_FIELDS
            ]
            return "\n".join([LEDGER_SECTION, "", "### example", "", *extra, *lines, ""])

        for duplicate in ("Status", "Source receipts", "Defining transition"):
            with self.subTest(field=duplicate):
                with self.assertRaises(AssertionError) as caught:
                    parse_ledger(
                        row(f"- **{duplicate}:** an earlier, contradictory value")
                    )
                self.assertIn(
                    duplicate,
                    str(caught.exception),
                    "the failure must name the repeated field",
                )

        parsed = parse_ledger(row())
        self.assertEqual(
            [category for category, _ in parsed],
            ["example"],
            "a row that states each field once must still parse",
        )

    def test_an_unknown_field_label_is_rejected(self) -> None:
        """The field vocabulary is closed, like the status vocabulary.

        An unrecognized label parses and renders but is read by no check, so a
        row could carry a note contradicting its own `Status` and stay green.
        That is the same shape as the dropped continuation and the overwritten
        duplicate: a claim shown to a reader that the oracle never sees.
        """
        def row(*extra: str) -> str:
            lines = [
                f"- **{name}:** a sufficiently long placeholder value"
                for name in REQUIRED_FIELDS
            ]
            return "\n".join([LEDGER_SECTION, "", "### example", "", *extra, *lines, ""])

        with self.assertRaises(AssertionError) as caught:
            parse_ledger(
                row("- **Coverage note:** actually this category IS covered")
            )
        self.assertIn(
            "Coverage note",
            str(caught.exception),
            "the failure must name the unknown field",
        )

    def test_a_ledger_preamble_is_rejected(self) -> None:
        """A line above the first category is a claim about every category.

        The rows are individually checked, so the way to state something the
        oracle cannot contradict is to state it where no row owns it. A
        sentence between the section heading and the first `###` renders as an
        introduction to the whole table -- "all eight categories are carried by
        the landed lane" reads as current and survives every per-row check,
        because the parser had no row to attach it to and dropped it.

        Field labels above the first heading are the same hole: they parse as
        a row nobody validates. Both are rejected; blank lines and the prose
        sections outside the ledger are unaffected.
        """
        def ledger(*preamble: str) -> str:
            lines = [
                f"- **{name}:** a sufficiently long placeholder value"
                for name in REQUIRED_FIELDS
            ]
            return "\n".join(
                [LEDGER_SECTION, "", *preamble, "### example", "", *lines, ""]
            )

        preambles = {
            "prose": "All eight categories are carried by the landed lane.",
            "field label": "- **Status:** COVERED",
            "list item": "- every row below is superseded",
        }
        for label, line in preambles.items():
            with self.subTest(preamble=label):
                with self.assertRaises(AssertionError) as caught:
                    parse_ledger(ledger(line, ""))
                self.assertIn(
                    LEDGER_SECTION,
                    str(caught.exception),
                    "the failure must name the section the text sits in",
                )

        parsed = parse_ledger(ledger())
        self.assertEqual(
            [category for category, _ in parsed],
            ["example"],
            "blank lines between the heading and the first category are fine",
        )

    def test_lane_paths_cannot_escape_the_corpus(self) -> None:
        """A ledger path is a child of the corpus, and the pattern proves it.

        `test_covered_receipts_appear_in_the_lane_document` joins the matched
        path onto `EXAMPLES` and reads whatever it finds, so a `..` segment
        would bind a row's receipts to a document outside the corpus. The
        membership check would still fail such a row, but the two checks are
        independent, and the one that reads the file should not depend on the
        other having run.
        """
        for escape in ("../outside.md", "../../outside.md", "sub/../../outside.md"):
            self.assertIsNone(
                LANE_FILE.match(f"`{escape}`"),
                f"{escape!r} escapes {EXAMPLES.relative_to(ROOT)} but the lane "
                f"path pattern accepted it",
            )
        for allowed in ("lane.md", "product/lane.md", "a/b/lane.md"):
            self.assertIsNotNone(
                LANE_FILE.match(f"`{allowed}`"),
                f"{allowed!r} is a legitimate corpus-relative lane path but the "
                f"pattern rejected it",
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

    def test_agents_readme_absent_count_matches_the_ledger(self) -> None:
        """The front door's headline number is derived, not trusted.

        It is the one sentence most readers see, and promoting a row does not
        otherwise touch `docs/agents/README.md` -- so without this the summary
        would keep announcing an old count while the ledger said otherwise.
        """
        # Normalized, because the sentence is soft-wrapped prose and where the
        # line break falls is not part of the claim.
        match = README_ABSENT_COUNT.search(" ".join(_read(AGENTS_README).split()))
        self.assertIsNotNone(
            match,
            "docs/agents/README.md must state the open-category count in the "
            "form 'N of M categories are currently `ABSENT`' so it can be "
            "checked against the ledger",
        )
        assert match is not None  # narrowed by the assertion above
        absent = sum(1 for _, fields in self.rows if fields.get("Status") == ABSENT)
        self.assertEqual(
            (int(match.group(1)), int(match.group(2))),
            (absent, len(self.rows)),
            f"docs/agents/README.md says {match.group(1)} of {match.group(2)} "
            f"categories are ABSENT; the ledger has {absent} of "
            f"{len(self.rows)}",
        )


class WorkedLaneWorkflowTests(unittest.TestCase):
    """The ledger only holds if the check runs on the files it constrains."""

    # Derived from the files the oracle actually reads, not typed a second
    # time. A hand-maintained duplicate of the workflow's own list would only
    # prove the two lists agree with each other; deriving it from the paths
    # `parse_ledger`, `example_documents`, and the checks above depend on
    # means a new dependency cannot be added without appearing here.
    REQUIRED_PATHS = {
        WORKFLOW_PATH,
        TEST_PATH,
        AGENTS_README.relative_to(ROOT).as_posix(),
        f"{EXAMPLES.relative_to(ROOT).as_posix()}/**",
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
