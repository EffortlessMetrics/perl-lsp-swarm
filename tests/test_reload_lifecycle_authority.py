"""Guard the single in-repo reload-lifecycle architecture authority.

`docs/specs/PLSP-SPEC-0037-reload-lifecycle-v1.md` is the durable contract for the
generation-bound reload and observation-route lifecycle (#10874). This suite exists so
the contract cannot rot in the two ways that would silently matter:

1. a second spec or ADR quietly becomes a competing reload-lifecycle authority;
2. the contract itself is edited into a shape that no longer decides anything --
   an identity row without an owner, a rule id deleted to make an implementation
   pass, a cross-reference pointing at a rule that no longer exists.

The checks read the contract as data. They deliberately do not restate its prose: a
test that asserts "this sentence is present" proves only that someone typed the
sentence. What is asserted here is structure, closure, and uniqueness.
"""

from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

SPEC = "docs/specs/PLSP-SPEC-0037-reload-lifecycle-v1.md"
SPEC_CATALOG = "docs/specs/README.md"
DOC_INDEX = "docs/INDEX.md"
WORKFLOW = ".github/workflows/reload-lifecycle-authority.yml"
SELF_TEST = "tests/test_reload_lifecycle_authority.py"
DAP_RELOAD_ADR = "docs/adr/0046-loaded-module-reload-semantics.md"

# A document claims reload-lifecycle authority by carrying this exact line. Inline
# mentions inside prose (backticked, mid-sentence) deliberately do not match.
AUTHORITY_MARKER = re.compile(r"^RELOAD-LIFECYCLE-AUTHORITY: v1$", re.MULTILINE)

# Identity rows: | RL-I01 <name> | <owner> | <today> | <realisation> |
IDENTITY_ROW = re.compile(
    r"^\|\s*(RL-I\d{2})\s+([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]*?)\s*\|\s*$",
    re.MULTILINE,
)
# Rule definitions: - **RL-R01.** <text>
RULE_DEF = re.compile(r"^- \*\*(RL-R\d{2})\.\*\*\s+(\S.*)$", re.MULTILINE)
# Forbidden rows: | RL-F01 | <design> |
FORBIDDEN_ROW = re.compile(r"^\|\s*(RL-F\d{2})\s*\|\s*([^|]*?)\s*\|\s*$", re.MULTILINE)
# Sequence headings: ### RL-S01 <title>
SEQUENCE_HEADING = re.compile(r"^### (RL-S\d{2})\s+(\S.*)$", re.MULTILINE)

# Any identifier reference anywhere in the document. `RL-R*` glob forms do not match.
ANY_ID = re.compile(r"RL-([IRFS])(\d{2})")

HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*$", re.MULTILINE)
MD_LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")

TODAY_VOCABULARY = {"implemented", "partial", "contracted"}
CLAIMS_A_TYPE = {"implemented", "partial"}

# A backticked CamelCase word in the realisation column is a claim that a Rust type
# by that name exists in `crates/`. Lowercase backticks (`perl-dap`, a crate path)
# are prose, not claims.
BACKTICKED = re.compile(r"`([^`]+)`")
RUST_TYPE_NAME = re.compile(r"\b([A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*)\b")

# Documents scanned for a competing authority claim.
DOC_ROOTS = ("docs",)


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def markdown_docs() -> list[Path]:
    found: list[Path] = []
    for root in DOC_ROOTS:
        found.extend(sorted((ROOT / root).rglob("*.md")))
    return found


def slugify(heading: str) -> str:
    """Approximate GitHub's heading-anchor slug."""
    text = heading.strip().lower()
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"[^\w\s-]", "", text)
    return re.sub(r"\s+", "-", text).strip("-")


def claimed_type_names(realisation: str) -> list[str]:
    """Rust type names a realisation cell claims exist."""
    names: list[str] = []
    for span in BACKTICKED.findall(realisation):
        names.extend(RUST_TYPE_NAME.findall(span))
    return names


def type_name_exists_in_crates(name: str) -> bool:
    result = subprocess.run(
        ["git", "grep", "-l", "-F", name, "--", "crates"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(f"git grep failed for {name!r}: {result.stderr.strip()}")
    return any(line.endswith(".rs") for line in result.stdout.splitlines())


def contiguous(ids: list[str], prefix: str) -> list[str]:
    """Return the problems with an id series: duplicates or a numbering gap."""
    problems: list[str] = []
    seen: set[str] = set()
    for value in ids:
        if value in seen:
            problems.append(f"{value} is defined more than once")
        seen.add(value)
    numbers = sorted(int(value[len(prefix):]) for value in seen)
    if not numbers:
        problems.append(f"no {prefix}NN identifiers are defined")
        return problems
    expected = list(range(1, len(numbers) + 1))
    if numbers != expected:
        missing = sorted(set(expected) - set(numbers))
        problems.append(
            f"{prefix} series is not contiguous from 01; missing {missing or 'nothing'}, "
            f"highest {numbers[-1]}, count {len(numbers)}"
        )
    return problems


class ReloadLifecycleAuthorityTest(unittest.TestCase):
    """The contract is discoverable, singular, and internally closed."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.spec = read(SPEC)

    def test_spec_exists_and_declares_current_status(self) -> None:
        self.assertTrue((ROOT / SPEC).is_file(), f"{SPEC} is missing")
        self.assertRegex(
            self.spec,
            re.compile(r"^Status: current$", re.MULTILINE),
            "the reload lifecycle contract must declare `Status: current`",
        )

    def test_spec_declares_no_runtime_status_impact(self) -> None:
        """The contract is architecture. It must not start claiming behavior."""
        match = re.search(r"^Status impact: (.*)$", self.spec, re.MULTILINE)
        self.assertIsNotNone(match, "the spec must carry a `Status impact:` line")
        assert match is not None
        self.assertIn(
            "none",
            match.group(1).lower(),
            "the reload lifecycle contract changes no runtime behavior; "
            "a PR that gives it runtime status impact needs a different document",
        )

    def test_exactly_one_document_claims_reload_lifecycle_authority(self) -> None:
        """A second spec or ADR cannot silently become a competing authority."""
        claimants = [
            path.relative_to(ROOT).as_posix()
            for path in markdown_docs()
            if AUTHORITY_MARKER.search(path.read_text(encoding="utf-8"))
        ]
        self.assertEqual(
            claimants,
            [SPEC],
            "exactly one document may carry the RELOAD-LIFECYCLE-AUTHORITY marker",
        )

    def test_dap_reload_adr_does_not_claim_workspace_reload_authority(self) -> None:
        """ADR-0046 is an adjacent DAP contract, not this lifecycle."""
        adr = read(DAP_RELOAD_ADR)
        self.assertIsNone(
            AUTHORITY_MARKER.search(adr),
            f"{DAP_RELOAD_ADR} must not claim workspace reload-lifecycle authority",
        )
        self.assertIn(
            Path(DAP_RELOAD_ADR).name,
            self.spec,
            "the spec must give the adjacent DAP reload ADR an explicit disposition",
        )

    def test_spec_is_indexed(self) -> None:
        """An unindexed architecture authority is not discoverable."""
        filename = Path(SPEC).name
        self.assertIn(
            filename,
            read(SPEC_CATALOG),
            f"{SPEC_CATALOG} must list the reload lifecycle contract",
        )
        self.assertIn(
            f"specs/{filename}",
            read(DOC_INDEX),
            f"{DOC_INDEX} must list the reload lifecycle contract",
        )

    def test_identity_rows_name_an_owner_and_a_today_status(self) -> None:
        rows = IDENTITY_ROW.findall(self.spec)
        self.assertGreaterEqual(len(rows), 20, "the identity table lost rows")
        problems: list[str] = []
        for identifier, name, owner, today, _realisation in rows:
            if not name.strip():
                problems.append(f"{identifier} has no identity name")
            if not owner.strip():
                problems.append(f"{identifier} has no canonical owner")
            marker = today.split("(")[0].strip().lower()
            if marker not in TODAY_VOCABULARY:
                problems.append(
                    f"{identifier} today-status {today!r} is outside "
                    f"{sorted(TODAY_VOCABULARY)}"
                )
        self.assertEqual(problems, [], "; ".join(problems))
        self.assertEqual(
            contiguous([row[0] for row in rows], "RL-I"), [], "identity id series"
        )

    def test_claimed_types_exist_and_absent_ones_are_not_claimed(self) -> None:
        """Over-claiming is the defect this table most needs not to have.

        A row marked `implemented` or `partial` names real Rust types, and those
        names must resolve in `crates/`. A row marked `contracted` asserts no type
        exists, so it must not name one -- that catches under-claiming too, which
        is how a row goes stale once its owner lands the work.
        """
        problems: list[str] = []
        for identifier, _name, _owner, today, realisation in IDENTITY_ROW.findall(
            self.spec
        ):
            marker = today.split("(")[0].strip().lower()
            names = claimed_type_names(realisation)
            if marker in CLAIMS_A_TYPE:
                for name in names:
                    if not type_name_exists_in_crates(name):
                        problems.append(
                            f"{identifier} is {marker} and names `{name}`, "
                            "which no Rust file under crates/ mentions"
                        )
            elif marker == "contracted" and names:
                problems.append(
                    f"{identifier} is contracted but names {names}; "
                    "either the type landed and the row is stale, or the name is wrong"
                )
        self.assertEqual(problems, [], "; ".join(problems))

    def test_every_implemented_row_actually_names_a_type(self) -> None:
        """`implemented` without a named type is an unfalsifiable claim."""
        nameless = [
            identifier
            for identifier, _name, _owner, today, realisation in IDENTITY_ROW.findall(
                self.spec
            )
            if today.split("(")[0].strip().lower() == "implemented"
            and not claimed_type_names(realisation)
        ]
        self.assertEqual(
            nameless,
            [],
            f"these rows claim `implemented` but name no type: {nameless}",
        )

    def test_rule_series_is_contiguous_and_each_rule_says_something(self) -> None:
        rules = RULE_DEF.findall(self.spec)
        self.assertEqual(contiguous([rule[0] for rule in rules], "RL-R"), [])
        short = [rule[0] for rule in rules if len(rule[1].strip()) < 20]
        self.assertEqual(short, [], f"these rules have no substantive text: {short}")

    def test_forbidden_series_is_contiguous_and_each_row_names_a_design(self) -> None:
        rows = FORBIDDEN_ROW.findall(self.spec)
        self.assertEqual(contiguous([row[0] for row in rows], "RL-F"), [])
        empty = [row[0] for row in rows if len(row[1].strip()) < 10]
        self.assertEqual(empty, [], f"these forbidden rows name no design: {empty}")

    def test_sequence_series_is_contiguous(self) -> None:
        headings = SEQUENCE_HEADING.findall(self.spec)
        self.assertEqual(contiguous([head[0] for head in headings], "RL-S"), [])

    def test_sequence_bodies_state_a_terminal_outcome(self) -> None:
        sections = re.split(r"^### (RL-S\d{2})\s", self.spec, flags=re.MULTILINE)[1:]
        pairs = list(zip(sections[0::2], sections[1::2]))
        self.assertTrue(pairs, "no worked sequences were found")
        missing = [
            identifier
            for identifier, body in pairs
            if "Terminal:" not in body.split("\n### ")[0]
        ]
        self.assertEqual(
            missing,
            [],
            f"these worked sequences state no terminal result: {missing}",
        )

    def test_no_reference_points_at_an_undefined_identifier(self) -> None:
        """Deleting a rule must not leave dangling citations behind."""
        defined = set()
        defined.update(row[0] for row in IDENTITY_ROW.findall(self.spec))
        defined.update(rule[0] for rule in RULE_DEF.findall(self.spec))
        defined.update(row[0] for row in FORBIDDEN_ROW.findall(self.spec))
        defined.update(head[0] for head in SEQUENCE_HEADING.findall(self.spec))
        referenced = {f"RL-{kind}{number}" for kind, number in ANY_ID.findall(self.spec)}
        dangling = sorted(referenced - defined)
        self.assertEqual(
            dangling, [], f"these identifiers are cited but never defined: {dangling}"
        )

    def test_internal_anchors_and_relative_links_resolve(self) -> None:
        anchors = {slugify(text) for _level, text in HEADING.findall(self.spec)}
        spec_dir = (ROOT / SPEC).parent
        problems: list[str] = []
        for target in MD_LINK.findall(self.spec):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            if target.startswith("#"):
                if target[1:] not in anchors:
                    problems.append(f"anchor {target} does not resolve")
                continue
            path_part = target.split("#", 1)[0]
            if path_part and not (spec_dir / path_part).exists():
                problems.append(f"link target {target} does not exist")
        self.assertEqual(problems, [], "; ".join(problems))


class ReloadLifecycleWorkflowWiringTest(unittest.TestCase):
    """The guard is wired: an orphan oracle proves nothing."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = read(WORKFLOW)

    def test_workflow_runs_this_suite(self) -> None:
        self.assertIn(
            f"python3 -m unittest {SELF_TEST}",
            self.workflow,
            "the workflow must invoke this suite as a contiguous command",
        )

    def test_workflow_triggers_on_every_surface_it_guards(self) -> None:
        for surface in (SPEC, SPEC_CATALOG, DOC_INDEX, DAP_RELOAD_ADR, SELF_TEST, WORKFLOW):
            self.assertIn(
                f"'{surface}'",
                self.workflow,
                f"{surface} can change without running this gate",
            )

    def test_workflow_guards_both_event_kinds(self) -> None:
        self.assertIn("pull_request:", self.workflow)
        self.assertIn("push:", self.workflow)
        self.assertEqual(
            self.workflow.count(f"- '{SPEC}'"),
            2,
            "the spec path must appear in both the pull_request and push filters",
        )


if __name__ == "__main__":
    unittest.main()
