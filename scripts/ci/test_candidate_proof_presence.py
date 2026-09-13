#!/usr/bin/env python3
"""Falsifiers for scripts/ci/candidate_proof_presence.py (#13014).

The repair is only worth having if it skips exactly one case and nothing else.
These are the propositions it rests on:

- a proof present in the candidate tree is never skipped, so a failing or
  unloadable present proof keeps deciding its own result;
- a proof absent from a candidate that never declared it is an authorized
  stale-head scoped no-op, and is visible rather than silent;
- a proof absent from a candidate that still declares it fails closed, so the
  repair cannot be used to retire a gate by deleting its file;
- a proof path that is a directory or symlink fails closed;
- the real repository tree classifies every guarded proof as ``present``, so the
  guard cannot degrade into a blanket skip on current main;
- the real workflow guards every classified proof and watches every proof path,
  so the classifier and the steps cannot drift apart.
"""

from __future__ import annotations

import importlib.util
import os
import re
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

SCRIPT = Path(__file__).with_name("candidate_proof_presence.py")
SPEC = importlib.util.spec_from_file_location("candidate_proof_presence", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
presence = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(presence)

REPO_ROOT = Path(os.environ.get("A3_REPO_ROOT", Path(__file__).resolve().parents[2]))
WORKFLOW_PATH = ".github/workflows/workflow-policy.yml"

PROOF_ID = "ci_nightly_cache_contract"
PROOF_PATH = "xtask/tests/ci_nightly_cache_contract.rs"

DECLARING_WORKFLOW = f"""\
on:
  pull_request:
    paths:
      - '{PROOF_PATH}'
jobs:
  workflow-policy-lint:
    steps:
      - run: cargo test -p xtask --test {PROOF_ID} --locked
"""

STALE_WORKFLOW = """\
on:
  pull_request:
    paths:
      - 'xtask/tests/ux_regression_gate_workflow_policy.rs'
jobs:
  workflow-policy-lint:
    steps:
      - run: cargo test -p xtask --test ux_regression_gate_workflow_policy --locked
"""


class ClassifyTests(unittest.TestCase):
    """The four dispositions, each established against a real filesystem."""

    def test_present_proof_is_never_skipped(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            (root / "xtask" / "tests").mkdir(parents=True)
            (root / PROOF_PATH).write_text("fn main() {}\n", encoding="utf-8")
            self.assertEqual(
                presence.classify(root, DECLARING_WORKFLOW, PROOF_PATH),
                presence.PRESENT,
            )

    def test_absent_and_undeclared_is_a_stale_head_no_op(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            self.assertEqual(
                presence.classify(root, STALE_WORKFLOW, PROOF_PATH),
                presence.STALE_HEAD_ABSENT,
            )

    def test_absent_but_declared_fails_closed(self) -> None:
        """Deleting a proof the candidate still declares must not become a skip."""
        with TemporaryDirectory() as name:
            root = Path(name)
            self.assertEqual(
                presence.classify(root, DECLARING_WORKFLOW, PROOF_PATH),
                presence.DECLARED_BUT_MISSING,
            )

    def test_directory_in_place_of_proof_fails_closed(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            (root / PROOF_PATH).mkdir(parents=True)
            self.assertEqual(
                presence.classify(root, STALE_WORKFLOW, PROOF_PATH),
                presence.NOT_REGULAR_FILE,
            )

    def test_symlinked_proof_fails_closed(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            (root / "xtask" / "tests").mkdir(parents=True)
            real = root / "xtask" / "tests" / "real.rs"
            real.write_text("fn main() {}\n", encoding="utf-8")
            (root / PROOF_PATH).symlink_to(real)
            self.assertEqual(
                presence.classify(root, STALE_WORKFLOW, PROOF_PATH),
                presence.NOT_REGULAR_FILE,
            )

    def test_a_longer_path_does_not_declare_a_shorter_one(self) -> None:
        """A prefix collision must not turn a stale head into a false red.

        Declaring ``xtask/tests/foo.rs`` because the workflow mentions
        ``xtask/tests/foo.rs.golden`` would report the absent proof as deleted
        and block the candidate for the wrong reason.
        """
        workflow = "      - 'xtask/tests/ci_nightly_cache_contract.rs.golden'\n"
        with TemporaryDirectory() as name:
            self.assertEqual(
                presence.classify(Path(name), workflow, PROOF_PATH),
                presence.STALE_HEAD_ABSENT,
            )
        self.assertFalse(presence.declares(workflow, PROOF_PATH))
        self.assertTrue(presence.declares(DECLARING_WORKFLOW, PROOF_PATH))

    def test_broken_symlink_fails_closed(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            (root / "xtask" / "tests").mkdir(parents=True)
            (root / PROOF_PATH).symlink_to(root / "xtask" / "tests" / "gone.rs")
            self.assertEqual(
                presence.classify(root, STALE_WORKFLOW, PROOF_PATH),
                presence.NOT_REGULAR_FILE,
            )


class MainExitCodeTests(unittest.TestCase):
    """Exit status is the contract the workflow step actually consumes."""

    def _run(self, root: Path, workflow_text: str, proofs: list[str]) -> tuple[int, Path, Path]:
        workflow = root / WORKFLOW_PATH
        workflow.parent.mkdir(parents=True, exist_ok=True)
        workflow.write_text(workflow_text, encoding="utf-8")
        output = root / "outputs.txt"
        summary = root / "summary.md"
        code = presence.main(
            [
                "--root",
                str(root),
                "--workflow",
                WORKFLOW_PATH,
                "--github-output",
                str(output),
                "--summary",
                str(summary),
                *[arg for proof in proofs for arg in ("--proof", proof)],
            ]
        )
        return code, output, summary

    def test_stale_head_is_green_and_reported(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            code, output, summary = self._run(
                root, STALE_WORKFLOW, [f"{PROOF_ID}={PROOF_PATH}"]
            )
            self.assertEqual(code, 0)
            self.assertEqual(
                output.read_text(encoding="utf-8").strip(),
                f"{PROOF_ID}={presence.STALE_HEAD_ABSENT}",
            )
            # The no-op must be visible, not silent.
            self.assertIn(presence.STALE_HEAD_ABSENT, summary.read_text(encoding="utf-8"))

    def test_present_proof_reports_present(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            (root / "xtask" / "tests").mkdir(parents=True)
            (root / PROOF_PATH).write_text("fn main() {}\n", encoding="utf-8")
            code, output, _summary = self._run(
                root, DECLARING_WORKFLOW, [f"{PROOF_ID}={PROOF_PATH}"]
            )
            self.assertEqual(code, 0)
            self.assertEqual(
                output.read_text(encoding="utf-8").strip(),
                f"{PROOF_ID}={presence.PRESENT}",
            )

    def test_declared_but_missing_exits_nonzero(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            code, _output, _summary = self._run(
                root, DECLARING_WORKFLOW, [f"{PROOF_ID}={PROOF_PATH}"]
            )
            self.assertEqual(code, 1)

    def test_one_failing_proof_fails_the_whole_step(self) -> None:
        """A green stale-head row must not mask a red row beside it."""
        with TemporaryDirectory() as name:
            root = Path(name)
            code, output, _summary = self._run(
                root,
                DECLARING_WORKFLOW,
                [
                    f"{PROOF_ID}={PROOF_PATH}",
                    "ux_regression_gate_workflow_policy="
                    "xtask/tests/ux_regression_gate_workflow_policy.rs",
                ],
            )
            self.assertEqual(code, 1)
            written = output.read_text(encoding="utf-8")
            self.assertIn(f"{PROOF_ID}={presence.DECLARED_BUT_MISSING}", written)

    def test_missing_candidate_workflow_is_an_instrument_failure(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            code = presence.main(
                [
                    "--root",
                    str(root),
                    "--workflow",
                    WORKFLOW_PATH,
                    "--proof",
                    f"{PROOF_ID}={PROOF_PATH}",
                ]
            )
            self.assertEqual(code, 2)

    def test_malformed_and_unsafe_specs_are_rejected(self) -> None:
        for spec in (
            "missing-separator",
            "=only-path",
            "only-id=",
            "bad id=xtask/tests/x.rs",
            "id=/absolute/path.rs",
            "id=../outside.rs",
            # PosixPath does not split on backslashes, so this traversal has to
            # be rejected explicitly rather than by Path(path).parts.
            "id=xtask\\..\\..\\etc\\passwd",
        ):
            with self.subTest(spec=spec):
                with self.assertRaises(ValueError):
                    presence.parse_proof(spec)

    def test_duplicate_ids_are_rejected(self) -> None:
        with TemporaryDirectory() as name:
            root = Path(name)
            code, _output, _summary = self._run(
                root,
                STALE_WORKFLOW,
                [f"{PROOF_ID}={PROOF_PATH}", f"{PROOF_ID}={PROOF_PATH}"],
            )
            self.assertEqual(code, 2)


class RepositoryContractTests(unittest.TestCase):
    """Bind the guard to the real workflow so the two cannot drift apart."""

    def setUp(self) -> None:
        self.workflow = (REPO_ROOT / WORKFLOW_PATH).read_text(encoding="utf-8")
        self.proofs = re.findall(
            r"--proof\s+([A-Za-z0-9_]+)=(\S+)", self.workflow
        )

    def test_workflow_declares_proofs(self) -> None:
        self.assertGreaterEqual(len(self.proofs), 7, self.proofs)

    def test_the_guard_comes_from_the_effective_workflow_revision(self) -> None:
        """The stale heads this repair targets carry no copy of the guard.

        Running the candidate's own copy would leave exactly those candidates
        unclassified, so every proof would run unguarded and an absent one would
        still die in cargo target selection. The guard is therefore taken from
        the revision that composed the effective workflow.
        """
        self.assertIn("ref: ${{ github.sha }}", self.workflow)
        self.assertIn("path: .effective-workflow", self.workflow)
        self.assertIn(
            "sparse-checkout: scripts/ci/candidate_proof_presence.py", self.workflow
        )
        self.assertIn(
            "python3 .effective-workflow/scripts/ci/candidate_proof_presence.py",
            self.workflow,
        )
        # The candidate's own copy must never be the executed instrument.
        self.assertNotIn("python3 scripts/ci/candidate_proof_presence.py", self.workflow)

    def test_the_classified_subject_is_the_candidate_tree(self) -> None:
        """Only the instrument is borrowed; the subject stays the candidate."""
        self.assertIn("--root .", self.workflow)
        self.assertIn("--workflow .github/workflows/workflow-policy.yml", self.workflow)

    def test_current_main_classifies_every_proof_as_present(self) -> None:
        """The guard must not degrade into a blanket skip on a healthy tree."""
        for identifier, path in self.proofs:
            with self.subTest(proof=identifier):
                self.assertEqual(
                    presence.classify(REPO_ROOT, self.workflow, path),
                    presence.PRESENT,
                )
                # Negative control: on its own this assertion also passes
                # against a classify() hardcoded to PRESENT, because every real
                # file exists. Pin that the verdict depends on the tree.
                with TemporaryDirectory() as empty:
                    self.assertNotEqual(
                        presence.classify(Path(empty), self.workflow, path),
                        presence.PRESENT,
                    )

    def test_every_classified_proof_guards_a_step(self) -> None:
        step_id = "candidate-proofs"
        for identifier, _path in self.proofs:
            with self.subTest(proof=identifier):
                self.assertIn(
                    f"if: steps.{step_id}.outputs.{identifier} != '"
                    f"{presence.STALE_HEAD_ABSENT}'",
                    self.workflow,
                )

    def test_guards_skip_only_on_an_explicit_stale_head_classification(self) -> None:
        """Fail open: an unset or unrecognized output must still run the proof.

        A head predating the guard script produces no outputs at all. The
        condition must therefore skip only on the positive stale-head value; a
        ``== 'present'`` condition would silently skip every proof instead.
        """
        conditions = re.findall(
            r"if: steps\.candidate-proofs\.outputs\.(\w+) (\S+) '([a-z_]+)'",
            self.workflow,
        )
        for _identifier, operator, value in conditions:
            self.assertEqual((operator, value), ("!=", presence.STALE_HEAD_ABSENT))

        # Identity, not just arity: a guard for an id that is never classified
        # would otherwise be masked by an unrelated guard keeping the count equal.
        guarded = [identifier for identifier, _operator, _value in conditions]
        classified = [identifier for identifier, _path in self.proofs]
        self.assertEqual(sorted(guarded), sorted(classified))
        self.assertEqual(len(guarded), len(set(guarded)), guarded)

    def test_the_classify_invocation_is_itself_a_declaration(self) -> None:
        """Removing only the ``on.paths`` entry must not buy a skip.

        The ``--proof`` arguments live in the same workflow file the classifier
        reads, so a candidate that still wires a proof into the classify step
        still declares it. Reaching ``stale_head_absent`` therefore requires a
        candidate whose workflow never mentions the proof at all, which is the
        stale head. This is what keeps a deleted proof from retiring a gate.
        """
        for identifier, path in self.proofs:
            with self.subTest(proof=identifier):
                without_watch = "\n".join(
                    line
                    for line in self.workflow.splitlines()
                    if line.strip() != f"- '{path}'"
                )
                self.assertIn(f"--proof {identifier}={path}", without_watch)
                with TemporaryDirectory() as name:
                    self.assertEqual(
                        presence.classify(Path(name), without_watch, path),
                        presence.DECLARED_BUT_MISSING,
                    )

    def test_every_proof_path_is_watched_on_pull_request_and_push(self) -> None:
        for _identifier, path in self.proofs:
            with self.subTest(path=path):
                self.assertGreaterEqual(
                    self.workflow.count(f"- '{path}'"),
                    2,
                    f"{path} must be watched on pull_request and push",
                )


if __name__ == "__main__":
    unittest.main()
