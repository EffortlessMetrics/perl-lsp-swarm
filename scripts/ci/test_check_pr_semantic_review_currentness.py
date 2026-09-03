#!/usr/bin/env python3
"""Focused falsifiers for check-pr-semantic-review-currentness.py."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check-pr-semantic-review-currentness.py")
SPEC = importlib.util.spec_from_file_location("semantic_currentness", SCRIPT)
assert SPEC and SPEC.loader
module = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = module
SPEC.loader.exec_module(module)


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def commit(root: Path, message: str) -> str:
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", message)
    return git(root, "rev-parse", "HEAD")


def setup_repo() -> tuple[tempfile.TemporaryDirectory[str], Path, str, str]:
    tmp = tempfile.TemporaryDirectory()
    root = Path(tmp.name)
    git(root, "init", "-q")
    git(root, "config", "user.name", "Test")
    git(root, "config", "user.email", "test@example.invalid")
    (root / "docs").mkdir()
    (root / "docs/route.md").write_text("route = stable\n", encoding="utf-8")
    base = commit(root, "base")
    (root / "docs/route.md").write_text("route = candidate\n", encoding="utf-8")
    reviewed = commit(root, "candidate")
    return tmp, root, base, reviewed


# Every section `parse_marker` requires, with the REVIEW_CURRENT conclusion, and no
# marker. Tests append exactly one marker so the emitter and the checker share a body.
REVIEW_SECTIONS = """## Review scope
- cumulative candidate

## Evidence and falsifiers
- focused proof

## No material findings

## What this establishes
- claim supported

## Residual risk / not proved
- external state

## Substantive review result
- REVIEW_CURRENT
"""


def body(
    pr: int,
    root: Path,
    base: str,
    head: str,
    *,
    digest: str | None = None,
) -> str:
    digest = digest or module.subject_digest(root, base, head)
    marker = {
        "head": head,
        "merge_base": base,
        "pr": pr,
        "result": "REVIEW_CURRENT",
        "subject_sha256": digest,
    }
    encoded = json.dumps(marker, sort_keys=True, separators=(",", ":"))
    return f"{REVIEW_SECTIONS}\n<!-- semantic-review:v1 {encoded} -->\n"


def review(pr: int, root: Path, base: str, head: str, **kwargs):
    return module.Review(
        login="reviewer",
        user_type=kwargs.get("user_type", "User"),
        state=kwargs.get("state", "COMMENTED"),
        body=body(pr, root, base, head, digest=kwargs.get("digest")),
        commit_oid=head,
        submitted_at=kwargs.get("submitted_at", "2026-08-12T00:00:00Z"),
    )


# A locale whose preferred encoding is ASCII. This is the portable stand-in for the
# Windows cp1252 host that produced the reported `'charmap' codec can't decode byte
# 0x8f`: both decode subprocess text output through a non-UTF-8 locale codec, so both
# fail on the first non-ASCII byte. `PYTHONUTF8`/`PYTHONCOERCECLOCALE` are pinned off
# because either one would silently restore UTF-8 and make the control vacuous.
NON_UTF8_ENV = {
    "LC_ALL": "C",
    "LANG": "C",
    "PYTHONUTF8": "0",
    "PYTHONCOERCECLOCALE": "0",
}

# café — arrow → : ordinary non-ASCII prose of the kind review bodies carry.
NON_ASCII_BYTES = b"caf\xc3\xa9 \xe2\x80\x94 arrow \xe2\x86\x92"
NON_ASCII_TEXT = NON_ASCII_BYTES.decode("utf-8")


def run_in_child(snippet: str, *, env_overrides: dict[str, str], cwd: Path):
    """Execute `snippet` against the script module in a separate interpreter.

    Locale is fixed when the interpreter starts, so a decode contract that depends on
    it cannot be exercised by mutating this process; it needs a real child.
    """
    env = {**os.environ, **env_overrides}
    driver = (
        "import importlib.util, sys\n"
        f"spec = importlib.util.spec_from_file_location('m', {str(SCRIPT)!r})\n"
        "m = importlib.util.module_from_spec(spec)\n"
        "sys.modules['m'] = m\n"
        "spec.loader.exec_module(m)\n"
    ) + snippet
    return subprocess.run(
        [sys.executable, "-c", driver],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


class HostLocaleDecodingTests(unittest.TestCase):
    """`_run` must decode child output as UTF-8 whatever the host locale is.

    Text mode without an explicit encoding uses `locale.getpreferredencoding(False)`.
    Git emits UTF-8 paths and `gh` emits UTF-8 JSON, so on a cp1252 or C-locale host
    the marker script used to fail on the first non-ASCII byte of a review body and
    degrade the whole run to `NOT_PROVEN / instrument_failure`.
    """

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_control_child_locale_is_really_not_utf8(self) -> None:
        """Negative control: without this, the decode tests could pass vacuously."""
        probe = run_in_child(
            "import locale\nprint(locale.getpreferredencoding(False))\n",
            env_overrides=NON_UTF8_ENV,
            cwd=self.root,
        )
        self.assertEqual(0, probe.returncode, probe.stderr)
        self.assertNotIn("utf", probe.stdout.strip().lower())

    def test_run_decodes_non_ascii_child_output_under_non_utf8_locale(self) -> None:
        snippet = (
            "import sys, pathlib\n"
            f"payload = {NON_ASCII_BYTES!r}\n"
            "child = 'import sys;sys.stdout.buffer.write(%r)' % payload\n"
            "out = m._run([sys.executable, '-c', child], cwd=pathlib.Path('.')).stdout\n"
            "sys.stdout.buffer.write(out.encode('utf-8'))\n"
        )
        result = run_in_child(snippet, env_overrides=NON_UTF8_ENV, cwd=self.root)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(NON_ASCII_TEXT, result.stdout)

    @unittest.skipIf(os.name == "nt", "the gh shim needs a POSIX shell")
    def test_fetch_pr_reads_non_ascii_review_bodies_under_non_utf8_locale(self) -> None:
        """The production surface: `gh` returns UTF-8 JSON, review prose is non-ASCII.

        The sibling `_run` test carries this contract on Windows, where the host
        codec is cp1252 and an unfixed read silently mojibakes instead of raising.
        """
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        payload = json.dumps(
            [
                {
                    "user": {"login": "reviewer", "type": "User"},
                    "state": "COMMENTED",
                    "body": NON_ASCII_TEXT,
                    "commit_id": "a" * 40,
                    "submitted_at": "2026-08-12T00:00:00Z",
                }
            ],
            ensure_ascii=False,
        )
        metadata = json.dumps({"headRefOid": "b" * 40, "baseRefOid": "c" * 40})
        # GitHub returns UTF-8 JSON carrying literal non-ASCII, not \\u escapes, so the
        # fixtures are written as bytes and the shim streams them back verbatim.
        (bin_dir / "meta.json").write_bytes(metadata.encode("utf-8"))
        (bin_dir / "reviews.json").write_bytes(payload.encode("utf-8"))
        shim = bin_dir / "gh"
        shim.write_text(
            '#!/bin/sh\nif [ "$1" = pr ]; then cat "$(dirname "$0")/meta.json"\n'
            'else cat "$(dirname "$0")/reviews.json"\nfi\n',
            encoding="utf-8",
        )
        shim.chmod(shim.stat().st_mode | stat.S_IEXEC)
        snippet = (
            "import sys, pathlib\n"
            "head, base, reviews = m.fetch_pr('o/r', 7, pathlib.Path('.'))\n"
            "sys.stdout.buffer.write(reviews[0].body.encode('utf-8'))\n"
        )
        result = run_in_child(
            snippet,
            env_overrides={**NON_UTF8_ENV, "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"},
            cwd=self.root,
        )
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(NON_ASCII_TEXT, result.stdout)

    def test_binary_subject_digest_is_unchanged_by_the_text_contract(self) -> None:
        """The digest reads raw bytes; the encoding fix must not touch it."""
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/route.md").write_text(
            f"route = {NON_ASCII_TEXT}\n", encoding="utf-8"
        )
        head = commit(root, "non-ascii content")
        self.assertRegex(module.subject_digest(root, base, head), r"^[0-9a-f]{64}$")


class MarkerResultTests(unittest.TestCase):
    """A marker asserts the review reached REVIEW_CURRENT, so nothing else may mint one."""

    def setUp(self) -> None:
        self.tmp, self.root, self.base, self.head = setup_repo()
        self.addCleanup(self.tmp.cleanup)
        merge_base = self.base

        def fake_fetch_pr(repo: str, pr: int, root: Path):
            return self.head, merge_base, []

        self._real_fetch_pr = module.fetch_pr
        module.fetch_pr = fake_fetch_pr
        self.addCleanup(lambda: setattr(module, "fetch_pr", self._real_fetch_pr))

    def test_explicit_review_current_emits_a_valid_marker(self) -> None:
        emitted = module.emit_marker(self.root, "o/r", 42, "REVIEW_CURRENT")
        payload = json.loads(module.MARKER_RE.findall(emitted)[0])
        self.assertEqual("REVIEW_CURRENT", payload["result"])
        self.assertEqual(42, payload["pr"])
        self.assertEqual(self.head, payload["head"])

    def test_legacy_bare_emit_marker_cannot_mint_a_marker(self) -> None:
        """The published pre-#14653 invocation must not still mint REVIEW_CURRENT.

        A `--result` default would have preserved exactly the defect this flag
        closes: the caller never states a conclusion and gets one anyway. Every
        published invocation now names the result, so the bare form must fail.
        """
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as raised:
                module.main(["42", "o/r", "--root", str(self.root), "--emit-marker"])
        self.assertNotEqual(0, raised.exception.code)
        self.assertNotIn("semantic-review:v1", stdout.getvalue())
        self.assertIn("--result", stderr.getvalue())

    def test_emit_marker_requires_an_explicit_result_argument(self) -> None:
        """The API carries no silent default either, not just the CLI."""
        with self.assertRaises(TypeError):
            module.emit_marker(self.root, "o/r", 42)  # type: ignore[call-arg]

    def test_emitted_marker_is_accepted_by_the_verifier(self) -> None:
        """Round-trip: the emitter and the checker must agree on one subject.

        Guards the `--result` plumbing against emitting a marker the checker rejects.
        """
        emitted = module.emit_marker(self.root, "o/r", 42, "REVIEW_CURRENT")
        review_body = REVIEW_SECTIONS + "\n" + emitted + "\n"
        marker = module.parse_marker(review_body, 42, self.head)
        self.assertIsNotNone(marker)
        self.assertEqual("REVIEW_CURRENT", marker.result)
        self.assertEqual(self.head, marker.head)

    def test_non_review_current_results_are_refused(self) -> None:
        for result in module.SUBSTANTIVE_REVIEW_RESULTS:
            if result == module.MARKER_RESULT:
                continue
            with self.subTest(result=result):
                with self.assertRaises(module.MarkerRefused):
                    module.emit_marker(self.root, "o/r", 42, result)

    def test_unknown_result_is_an_instrument_error_not_a_refusal(self) -> None:
        with self.assertRaises(module.CurrentnessError):
            module.emit_marker(self.root, "o/r", 42, "LGTM")

    def test_cli_refusal_prints_no_marker_and_exits_distinctly(self) -> None:
        """The refusal must not be mistaken for a verdict or an instrument failure."""
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            code = module.main(
                [
                    "42",
                    "o/r",
                    "--root",
                    str(self.root),
                    "--emit-marker",
                    "--result",
                    "CHANGES_REQUIRED",
                ]
            )
        self.assertEqual(3, code)
        emitted = stdout.getvalue()
        self.assertNotIn("semantic-review:v1", emitted)
        payload = json.loads(emitted)
        self.assertEqual("MARKER_REFUSED", payload["classification"])
        self.assertEqual("CHANGES_REQUIRED", payload["result"])

    def test_cli_explicit_review_current_emits_the_marker(self) -> None:
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            code = module.main(
                [
                    "42",
                    "o/r",
                    "--root",
                    str(self.root),
                    "--emit-marker",
                    "--result",
                    "REVIEW_CURRENT",
                ]
            )
        self.assertEqual(0, code)
        self.assertIn("semantic-review:v1", stdout.getvalue())


class SemanticReviewCurrentnessTests(unittest.TestCase):
    def test_exact_subject_bound_review_is_current(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head)],
        )
        self.assertEqual("REVIEW_CURRENT", result["classification"])
        self.assertFalse(result["carried_forward"])

    def test_no_substantive_review_is_not_proven(self) -> None:
        tmp, root, _base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(root, pr=42, current_head=head, reviews=[])
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "no_substantive_review_currentness_marker",
            result["reason"],
        )

    def test_generic_human_comment_is_not_substantive(self) -> None:
        tmp, root, _base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        generic = module.Review(
            "reviewer",
            "User",
            "COMMENTED",
            "LGTM",
            head,
            "2026-08-12T00:00:00Z",
        )
        result = module.evaluate(root, pr=42, current_head=head, reviews=[generic])
        self.assertEqual("NOT_PROVEN", result["classification"])

    def test_prose_whitespace_only_followup_carries_review_forward(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/route.md").write_text(
            "route    =    candidate\n\n",
            encoding="utf-8",
        )
        current = commit(root, "format prose")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("REVIEW_CURRENT", result["classification"])
        self.assertTrue(result["carried_forward"])

    def test_material_route_change_requires_focused_review(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/route.md").write_text(
            "route = different-production-path\n",
            encoding="utf-8",
        )
        current = commit(root, "route change")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual("material_content_change_after_review", result["reason"])

    def test_added_path_is_not_neutral(self) -> None:
        tmp, root, base, reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/new.md").write_text("new route\n", encoding="utf-8")
        current = commit(root, "add path")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review(42, root, base, reviewed)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "path,_file-kind,_or_structural_change_after_review",
            result["reason"],
        )

    def test_code_indentation_change_is_not_neutral(self) -> None:
        tmp, root, base, _reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "src").mkdir()
        (root / "src/logic.py").write_text(
            "if True:\n    value = 1\n",
            encoding="utf-8",
        )
        reviewed = commit(root, "add python")
        review_row = review(42, root, base, reviewed)
        (root / "src/logic.py").write_text(
            "if True:\nvalue = 1\n",
            encoding="utf-8",
        )
        current = commit(root, "indentation change")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review_row],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "post-review_change_is_not_in_a_whitespace-insensitive_prose_file",
            result["reason"],
        )

    def test_fenced_command_respacing_in_prose_is_not_neutral(self) -> None:
        """A prose extension must not make an executable fence whitespace-insensitive.

        `.claude/skills/**/SKILL.md` publishes commands agents run. Respacing one is a
        pure whitespace edit in a `.md` file, so the extension and `--ignore-all-space`
        rules both accept it while the command it names changes target.
        """
        tmp, root, base, _reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/runbook.md").write_text(
            "# Runbook\n\nCleanup:\n\n```bash\nrm -rf ./build/tmp\n```\n",
            encoding="utf-8",
        )
        reviewed = commit(root, "add runbook")
        review_row = review(42, root, base, reviewed)
        (root / "docs/runbook.md").write_text(
            "# Runbook\n\nCleanup:\n\n```bash\nrm -rf . /build/tmp\n```\n",
            encoding="utf-8",
        )
        current = commit(root, "respace the fenced command")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review_row],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual(
            "post-review_change_alters_fenced_code_content",
            result["reason"],
        )

    def test_prose_reflow_outside_a_fence_still_carries_forward(self) -> None:
        """The fence rule must not collapse the neutral class it is narrowing."""
        tmp, root, base, _reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/runbook.md").write_text(
            "# Runbook\n\nCleanup:\n\n```bash\nrm -rf ./build/tmp\n```\n",
            encoding="utf-8",
        )
        reviewed = commit(root, "add runbook")
        review_row = review(42, root, base, reviewed)
        (root / "docs/runbook.md").write_text(
            "# Runbook\n\nCleanup:\n\n```bash\nrm -rf ./build/tmp\n```\n",
            encoding="utf-8",
        )
        (root / "docs/route.md").write_text(
            "route    =    candidate\n\n",
            encoding="utf-8",
        )
        current = commit(root, "reflow prose beside an untouched fence")
        result = module.evaluate(
            root,
            pr=42,
            current_head=current,
            reviews=[review_row],
        )
        self.assertEqual("REVIEW_CURRENT", result["classification"])
        self.assertTrue(result["carried_forward"])

    def test_wrong_subject_digest_is_not_proven(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head, digest="0" * 64)],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])
        self.assertEqual("review_subject_digest_mismatch", result["reason"])

    def test_bot_marker_is_not_substantive(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        result = module.evaluate(
            root,
            pr=42,
            current_head=head,
            reviews=[review(42, root, base, head, user_type="Bot")],
        )
        self.assertEqual("NOT_PROVEN", result["classification"])

    def test_contradictory_body_is_rejected_despite_a_valid_marker(self) -> None:
        """A REVIEW_CURRENT marker cannot carry a body that concluded otherwise.

        The emitter takes the caller's word for `--result` — it never sees the
        review body, which does not exist when the marker is generated. The
        verifier is what makes the pair honest, so the contradictory case is
        pinned here rather than left to the emitter's claim boundary.
        """
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        for contradiction in ("CHANGES_REQUIRED", "NOT_PROVEN", "BLOCKED_BY_PREREQUISITE"):
            with self.subTest(result=contradiction):
                contradicted = body(42, root, base, head).replace(
                    "## Substantive review result\n- REVIEW_CURRENT",
                    f"## Substantive review result\n- {contradiction}",
                )
                # The marker itself is untouched and still well-formed.
                self.assertIn("semantic-review:v1", contradicted)
                self.assertIsNone(module.parse_marker(contradicted, 42, head))
                result = module.evaluate(
                    root,
                    pr=42,
                    current_head=head,
                    reviews=[
                        module.Review(
                            "reviewer", "User", "COMMENTED", contradicted, head,
                            "2026-08-12T00:00:00Z",
                        )
                    ],
                )
                self.assertEqual("NOT_PROVEN", result["classification"])

    def test_ambiguous_result_declarations_are_rejected(self) -> None:
        """An ambiguous conclusion is not a current one.

        Searching for a `REVIEW_CURRENT` token anywhere after the heading accepts
        a body that also declares something else — two result sections, or one
        section listing several results, such as an unedited template. Each shape
        below carries a well-formed marker, so only the declaration check can
        reject it.
        """
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        marker = module.MARKER_RE.findall(body(42, root, base, head))[0]
        prefix = REVIEW_SECTIONS.split("## Substantive review result")[0]

        shapes = {
            "two sections, the first not current": (
                "## Substantive review result\n- CHANGES_REQUIRED\n\n"
                "## Substantive review result\n- REVIEW_CURRENT\n"
            ),
            "two sections, current first": (
                "## Substantive review result\n- REVIEW_CURRENT\n\n"
                "## Substantive review result\n- NOT_PROVEN\n"
            ),
            "unedited template listing every option": (
                "## Substantive review result\n- REVIEW_CURRENT\n- CHANGES_REQUIRED\n"
                "- NOT_PROVEN\n- BLOCKED_BY_PREREQUISITE\n- SUPERSEDED_OR_CLOSE\n"
            ),
            "one section, two results": (
                "## Substantive review result\n- REVIEW_CURRENT\n- CHANGES_REQUIRED\n"
            ),
        }
        for name, section in shapes.items():
            with self.subTest(shape=name):
                ambiguous = f"{prefix}{section}\n<!-- semantic-review:v1 {marker} -->\n"
                self.assertIn("semantic-review:v1", ambiguous)
                self.assertIsNone(module.declared_review_result(ambiguous))
                self.assertIsNone(module.parse_marker(ambiguous, 42, head))

    def test_fenced_examples_do_not_count_as_declarations(self) -> None:
        """Quoting the contract must not disqualify the review that quotes it.

        Reviews on this very checker paste the result section as an example. If a
        fenced copy counted as a real section the declaration count would exceed
        one and reject an honest conclusion — a false reject in the direction the
        narrowing is supposed to avoid.
        """
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        example = (
            "```\n## Substantive review result\n- CHANGES_REQUIRED\n"
            "- NOT_PROVEN\n```\n"
        )
        placements = {
            "before the declaration": lambda b: b.replace(
                "## Review scope", f"{example}\n## Review scope"
            ),
            "after the declaration": lambda b: b.replace(
                "<!-- semantic-review", f"{example}\n<!-- semantic-review"
            ),
            "tilde fence": lambda b: b.replace(
                "## Review scope",
                "~~~\n## Substantive review result\n- NOT_PROVEN\n~~~\n\n## Review scope",
            ),
        }
        for name, place in placements.items():
            with self.subTest(placement=name):
                quoted = place(body(42, root, base, head))
                self.assertEqual("REVIEW_CURRENT", module.declared_review_result(quoted))
                self.assertIsNotNone(module.parse_marker(quoted, 42, head))

    def test_a_real_second_declaration_outside_a_fence_is_still_caught(self) -> None:
        """The fence exemption must not become a smuggling route."""
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        smuggled = body(42, root, base, head).replace(
            "<!-- semantic-review",
            "```\nfenced noise\n```\n\n## Substantive review result\n"
            "- CHANGES_REQUIRED\n\n<!-- semantic-review",
        )
        self.assertIsNone(module.declared_review_result(smuggled))
        self.assertIsNone(module.parse_marker(smuggled, 42, head))

    def test_discussing_other_results_in_prose_still_carries_forward(self) -> None:
        """The narrowing must not reject an honest review that names alternatives.

        Reviews routinely explain why they are not NOT_PROVEN or CHANGES_REQUIRED.
        A mention is not a declaration, so only list items in the one result
        section count — otherwise this guard would fail closed on good bodies.
        """
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        discursive = body(42, root, base, head).replace(
            "## Residual risk / not proved\n- external state",
            "## Residual risk / not proved\n- external state\n"
            "- Considered CHANGES_REQUIRED and NOT_PROVEN; neither applies here.",
        )
        self.assertEqual(
            "REVIEW_CURRENT", module.declared_review_result(discursive)
        )
        self.assertIsNotNone(module.parse_marker(discursive, 42, head))

    def test_malformed_utf8_in_a_reviewed_prose_file_fails_closed(self) -> None:
        """Undecodable reviewed content is NOT_PROVEN, never a silent carry-forward.

        Replacement decoding would collapse distinct malformed sequences to the
        same U+FFFD, letting a real fenced-command change compare equal.
        """
        tmp, root, base, _reviewed = setup_repo()
        self.addCleanup(tmp.cleanup)
        (root / "docs/runbook.md").write_bytes(
            b"# Runbook\n\n```bash\nrm -rf ./build/\xff\xfe\n```\n"
        )
        reviewed = commit(root, "add runbook with malformed bytes")
        review_row = review(42, root, base, reviewed)
        (root / "docs/runbook.md").write_bytes(
            b"# Runbook\n\n```bash\nrm -rf ./build/\xfe\xff\n```\n"
        )
        current = commit(root, "different malformed bytes in the same fence")

        # Undecodable content is an instrument failure, not a verdict, so it
        # surfaces the module's typed error rather than a silent comparison.
        with self.assertRaises(module.CurrentnessError):
            module.evaluate(root, pr=42, current_head=current, reviews=[review_row])

        # Through the CLI that failure must land as NOT_PROVEN, never as a
        # carried-forward review. This is the user-visible contract.
        # TemporaryDirectory + addCleanup, matching this file's other tests:
        # TestCase.enterContext is 3.11+, and the focused command in this repo's
        # docs is run by hand on whatever interpreter the reviewer has.
        fixture_dir = tempfile.TemporaryDirectory()
        self.addCleanup(fixture_dir.cleanup)
        fixture = Path(fixture_dir.name) / "f.json"
        fixture.write_text(
            json.dumps({"head": current, "reviews": [review_row._asdict()]}),
            encoding="utf-8",
        )
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            code = module.main(
                ["42", "o/r", "--root", str(root), "--fixture", str(fixture)]
            )
        self.assertEqual(2, code)
        payload = json.loads(stdout.getvalue())
        self.assertEqual("NOT_PROVEN", payload["classification"])
        self.assertEqual("instrument_failure", payload["reason"])

    def test_marker_head_must_equal_review_commit(self) -> None:
        tmp, root, base, head = setup_repo()
        self.addCleanup(tmp.cleanup)
        other = module.Review(
            "reviewer",
            "User",
            "COMMENTED",
            body(42, root, base, head),
            base,
            "2026-08-12T00:00:00Z",
        )
        result = module.evaluate(root, pr=42, current_head=head, reviews=[other])
        self.assertEqual("NOT_PROVEN", result["classification"])


if __name__ == "__main__":
    unittest.main()
