from __future__ import annotations

import hashlib
import re
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "docs" / "agents" / "authority_status.toml"
MIGRATOR = ROOT / "scripts" / "migrate-legacy-authority-banners.py"
WORKFLOW = ROOT / ".github" / "workflows" / "legacy-authority-banners.yml"
MARKER = "<!-- authority-status:v1 -->"
TRUSTED_HISTORICAL_COMMIT = "4dc745fd3513d1a345cd1d6258bb96a13e284ae2"
EXPECTED = {
    "docs/reference/ORCHESTRATION_DOCTRINE.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/reference/PIPELINE_GATES.md": ("superseded", "DEVELOPMENT_METHOD.md"),
    "docs/reference/OCTOPUS_CLUSTER.md": ("historical", "DEVELOPMENT_METHOD.md"),
    "docs/reference/GLOSSARY.md": ("superseded", "AUTHORITY_STATUS.md"),
    "docs/reference/LIVE_SIGNALS_VS_LABELS.md": (
        "historical",
        "GITHUB_SURFACES.md",
    ),
    "docs/adr/0044-octopus-cluster-orchestration.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/articles/PIPELINE_STATE_MACHINE.md": (
        "historical",
        "GITHUB_SURFACES.md",
    ),
    "docs/handoff/SWARM_DESIGN.md": ("historical", "DEVELOPMENT_METHOD.md"),
    ".spec/3988-merge-readiness/spec.md": (
        "historical",
        "REVIEW_CURRENTNESS.md",
    ),
    "docs/development/RUST_1_95_ROLLOUT.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/development/RUST_1_95_PROACTIVE_GUARDS.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
    "docs/ci/perl-lsp-rust-1.95-rollout.md": (
        "superseded",
        "CLIPPY_POLICY.md",
    ),
    "docs/project/RAILS_INDEX.md": (
        "superseded",
        "DEVELOPMENT_METHOD.md",
    ),
}
ROLLOUT_REDIRECTS: dict[str, dict[str, str]] = {
    "docs/development/RUST_1_95_ROLLOUT.md": {
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md": {
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/development/RUST_1_95_PROACTIVE_GUARDS.md": {
        "successor": "docs/agents/DEVELOPMENT_METHOD.md",
    },
    "docs/ci/perl-lsp-rust-1.95-rollout.md": {
        "successor": "docs/CLIPPY_POLICY.md",
    },
    "docs/project/RAILS_INDEX.md": {
        "successor": "docs/agents/DEVELOPMENT_METHOD.md",
    },
}
RETIRED_ROLLOUT_PATHS = tuple(ROLLOUT_REDIRECTS)
RETIRED_REFERENCE_METADATA = {
    *ROLLOUT_REDIRECTS,
    "docs/agents/AUTHORITY_STATUS.md",
    "docs/agents/authority_status.toml",
    "docs/policy/NON_RUST_INVENTORY.md",
    ".github/workflows/legacy-authority-banners.yml",
    ".github/workflows/agent-authority-status.yml",
    "tests/test_legacy_authority_banners.py",
}
HISTORICAL_MARKER = re.compile(r"(?i)\b(?:historical|superseded|immutable)\b")
EXECUTABLE_MARKER = re.compile(
    r"(?i)\b(?:active|authoritative|authority|canonical|current|execute|"
    r"implementation|owns|run|select|source of truth|use|follow)\b"
)
HISTORICAL_BLOB_URL = re.compile(
    r"https://github\.com/EffortlessMetrics/perl-lsp-swarm/blob/"
    r"(?P<commit>[0-9a-f]{40})/(?P<path>[^?#]+)"
)


def unqualified_retired_references(document: str, body: str) -> list[str]:
    """Return retired-path references that could still direct current work."""
    if document in RETIRED_REFERENCE_METADATA:
        return []

    findings: list[str] = []
    for line_number, line in enumerate(body.splitlines(), start=1):
        for path in RETIRED_ROLLOUT_PATHS:
            start = line.find(path)
            if start < 0:
                continue
            context = line
            if not HISTORICAL_MARKER.search(context) or EXECUTABLE_MARKER.search(
                context
            ):
                findings.append(f"{document}:{line_number}: {line}")
                break
    return findings


def current_source_documents() -> list[tuple[str, str]]:
    """Read tracked UTF-8 source files, excluding inventory and test fixtures."""
    listing = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    documents: list[tuple[str, str]] = []
    for raw_path in listing.stdout.split(b"\0"):
        if not raw_path:
            continue
        relative = raw_path.decode("utf-8")
        if relative in RETIRED_REFERENCE_METADATA:
            continue
        path = ROOT / relative
        if not path.is_file():
            continue
        try:
            body = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if "\x00" not in body:
            documents.append((relative, body))
    return documents


def registry_rows() -> dict[str, dict[str, Any]]:
    registry = tomllib.loads(REGISTRY.read_text(encoding="utf-8"))
    return {row["path"]: row for row in registry["documents"]}


def banner_link_targets(document: str, head: str) -> dict[str, Path]:
    """Resolve every local Markdown link in a banner to a repository path."""
    targets: dict[str, Path] = {}
    for text, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", head):
        if href.startswith(("http://", "https://", "#")):
            continue
        target = href.split("#", 1)[0]
        if not target:
            continue
        targets[text] = ((ROOT / document).parent / target).resolve()
    return targets


def historical_links(head: str) -> list[tuple[str, str, str]]:
    links: list[tuple[str, str, str]] = []
    for _text, href in re.findall(r"\[([^\]]+)\]\(([^)]+)\)", head):
        match = HISTORICAL_BLOB_URL.fullmatch(href)
        if match:
            links.append((href, match.group("commit"), match.group("path")))
    return links


def historical_blob_sha1(commit: str, path: str) -> str:
    result = subprocess.run(
        ["git", "cat-file", "blob", f"{commit}:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    payload = result.stdout
    return hashlib.sha1(f"blob {len(payload)}\0".encode() + payload).hexdigest()


def trusted_historical_commit() -> str:
    return TRUSTED_HISTORICAL_COMMIT


def current_main_trusted_history_state(
    trusted_commit: str, repo: Path = ROOT
) -> tuple[str, str]:
    """Classify whether the checked-out main line retains the pinned commit.

    Returns ``(state, detail)`` where ``state`` is one of:

    - ``"present"``: the commit is an ancestor of ``origin/main``;
    - ``"absent"``: the full-history graph genuinely excludes the commit — a
      real finding;
    - ``"shallow"``: this clone is shallow/grafted, the one named condition
      under which the check cannot run and a skip is legitimate;
    - ``"error"``: the ancestry command itself failed unexpectedly. This
      proves nothing about ``main`` — and it is not the named shallow
      condition, so it must fail the suite rather than read as a skip
      (#15480).
    """
    shallow = subprocess.run(
        ["git", "rev-parse", "--is-shallow-repository"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    if shallow.stdout.strip() == "true":
        return (
            "shallow",
            "clone is shallow, so its grafted history cannot speak for main",
        )
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", trusted_commit, "origin/main"],
        cwd=repo,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return "present", ""
    if result.returncode == 1:
        return "absent", ""
    return (
        "error",
        "git merge-base --is-ancestor exited "
        f"{result.returncode}: {result.stderr.strip()}",
    )


def historical_identity_findings(
    path: str,
    row: dict[str, Any],
    head: str,
    trusted_commit: str,
) -> list[str]:
    links = historical_links(head)
    if len(links) != 1:
        return [f"{path}: expected one historical blob link, found {len(links)}"]

    _href, linked_commit, linked_path = links[0]
    trusted_blob = historical_blob_sha1(trusted_commit, path)
    findings: list[str] = []
    if linked_path != path:
        findings.append(f"{path}: historical link points at {linked_path}")
    if linked_commit != trusted_commit:
        findings.append(f"{path}: historical link is not on {trusted_commit}")
    if row.get("historical_source_commit") != trusted_commit:
        findings.append(f"{path}: registry commit is not {trusted_commit}")
    if row.get("historical_source_blob_sha1") != trusted_blob:
        findings.append(f"{path}: registry blob is not {trusted_blob}")
    return findings


def workflow_event_paths(event: str) -> set[str]:
    lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
    event_indent = None
    paths_indent = None
    collected: set[str] = set()
    in_event = False
    in_paths = False

    for line in lines:
        stripped = line.strip()
        indent = len(line) - len(line.lstrip())
        if stripped == f"{event}:" and indent == 2:
            event_indent = indent
            in_event = True
            in_paths = False
            continue
        if in_event and event_indent is not None and stripped and indent <= event_indent:
            break
        if in_event and stripped == "paths:":
            paths_indent = indent
            in_paths = True
            continue
        if in_paths and paths_indent is not None:
            if stripped and indent <= paths_indent:
                in_paths = False
                continue
            match = re.match(r"- ['\"](.+)['\"]$", stripped)
            if match:
                collected.add(match.group(1))

    return collected


class LegacyAuthorityBannerTests(unittest.TestCase):
    def test_current_sources_do_not_depend_on_retired_rollout_paths(self) -> None:
        findings: list[str] = []
        for relative, body in current_source_documents():
            findings.extend(unqualified_retired_references(relative, body))

        self.assertEqual(
            findings,
            [],
            "current docs must point at maintained authorities; retained mentions "
            "must say historical, superseded, or immutable",
        )

    def test_unqualified_retired_reference_is_rejected(self) -> None:
        self.assertEqual(
            unqualified_retired_references(
                "docs/current.md",
                "See docs/development/RUST_1_95_ROLLOUT.md for the current plan.",
            ),
            [
                "docs/current.md:1: See docs/development/RUST_1_95_ROLLOUT.md for the current plan."
            ],
        )
        self.assertEqual(
            unqualified_retired_references(
                "docs/forensics.md",
                "Historical reference: docs/development/RUST_1_95_ROLLOUT.md",
            ),
            [],
        )
        for marker in ("historical", "superseded", "immutable"):
            with self.subTest(marker=marker):
                line = (
                    f"Use the {marker} docs/development/RUST_1_95_ROLLOUT.md "
                    "as the current implementation plan."
                )
                self.assertEqual(
                    unqualified_retired_references("docs/current.md", line),
                    [f"docs/current.md:1: {line}"],
                )

    def test_no_panic_policy_line11_retired_path_follows_registry_status(self) -> None:
        """The exact main finding named by #14870.

        `docs/NO_PANIC_POLICY.md:11` failed because the retired rollout path sat
        on a line ending `, but`, with any historical qualifier on a later line.
        The registry classifies that path as superseded with successor
        `docs/CLIPPY_POLICY.md`. The checker is line-local, so the path's own
        line must carry `historical`/`superseded`/`immutable` and must not carry
        executable current-work verbs.

        This is a docs-phrasing lock. Aligning the workflow trigger with the
        test file it already executes is in `test_agent_authority_status.py`;
        it is not the #14837 scan-all/trigger-registry redesign.
        """
        original_main_line = (
            "[`docs/ci/perl-lsp-rust-1.95-rollout.md`]"
            "(ci/perl-lsp-rust-1.95-rollout.md), but"
        )
        self.assertEqual(
            unqualified_retired_references(
                "docs/NO_PANIC_POLICY.md",
                original_main_line
                + "\ncurrent source and policy files are authoritative "
                "for current-state claims.",
            ),
            [f"docs/NO_PANIC_POLICY.md:1: {original_main_line}"],
        )

        qualified_line = (
            "[`docs/ci/perl-lsp-rust-1.95-rollout.md`]"
            "(ci/perl-lsp-rust-1.95-rollout.md) is superseded"
        )
        self.assertEqual(
            unqualified_retired_references("docs/NO_PANIC_POLICY.md", qualified_line),
            [],
        )

        body = (ROOT / "docs" / "NO_PANIC_POLICY.md").read_text(encoding="utf-8")
        self.assertEqual(
            unqualified_retired_references("docs/NO_PANIC_POLICY.md", body),
            [],
        )
        self.assertIn(qualified_line, body)
        self.assertIn(
            "by [`docs/CLIPPY_POLICY.md`](CLIPPY_POLICY.md) and retained only as a record.",
            body,
        )

    def test_local_banners_match_registry(self) -> None:
        by_path = registry_rows()

        for path, (status, successor) in EXPECTED.items():
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                self.assertIn(MARKER, head)
                self.assertIn(f"Status: {status}.", head)
                self.assertIn("AUTHORITY_STATUS.md", head)
                self.assertIn(successor, head)
                self.assertEqual(by_path[path]["status"], status)

    def test_banner_links_resolve_to_real_files(self) -> None:
        """Every local banner link must resolve inside the repository."""
        for path, (_status, successor) in EXPECTED.items():
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                targets = banner_link_targets(path, head)
                self.assertTrue(
                    targets, f"{path}: banner has no resolvable Markdown link"
                )
                for text, target in targets.items():
                    self.assertTrue(
                        target.is_relative_to(ROOT),
                        f"{path}: banner link [{text}] escapes the repository: {target}",
                    )
                    self.assertTrue(
                        target.is_file(),
                        f"{path}: banner link [{text}] points at missing {target}",
                    )

                linked = set(targets.values())
                registry_successor = registry_rows()[path]["successor"]
                self.assertIn(
                    (ROOT / "docs/agents/AUTHORITY_STATUS.md").resolve(),
                    linked,
                    f"{path}: banner does not link the canonical authority index",
                )
                self.assertIn(
                    (ROOT / registry_successor).resolve(),
                    linked,
                    f"{path}: banner does not link the registry successor "
                    f"{registry_successor}",
                )
                self.assertEqual(Path(registry_successor).name, Path(successor).name)

    def test_each_document_carries_exactly_one_banner(self) -> None:
        for path in EXPECTED:
            with self.subTest(path=path):
                body = (ROOT / path).read_text(encoding="utf-8")
                self.assertEqual(
                    body.count(MARKER), 1, f"{path}: expected exactly one banner marker"
                )

    def test_explicit_old_statuses_are_not_still_current(self) -> None:
        pipeline = "\n".join(
            (ROOT / "docs/reference/PIPELINE_GATES.md")
            .read_text(encoding="utf-8")
            .splitlines()[:24]
        )
        adr = "\n".join(
            (ROOT / "docs/adr/0044-octopus-cluster-orchestration.md")
            .read_text(encoding="utf-8")
            .splitlines()[:24]
        )

        self.assertNotIn("**Status**: Active doctrine", pipeline)
        self.assertIn("**Status**: Superseded", pipeline)
        self.assertNotIn("**Status**: Accepted", adr)
        self.assertIn("**Status**: Superseded", adr)

    def test_banner_set_matches_every_non_archive_legacy_markdown_row(self) -> None:
        legacy_markdown = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"}
            and path.endswith(".md")
            and not path.startswith("archive/")
        }

        self.assertEqual(set(EXPECTED), legacy_markdown)
        self.assertEqual(len(EXPECTED), 14)

    def test_retired_legacy_scripts_say_so(self) -> None:
        legacy_scripts = {
            path
            for path, row in registry_rows().items()
            if row["status"] in {"historical", "superseded"}
            and not path.endswith(".md")
        }
        self.assertTrue(legacy_scripts, "expected retired commands in the registry")

        for path in sorted(legacy_scripts):
            with self.subTest(path=path):
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:40]
                )
                self.assertRegex(
                    head,
                    r"RETIRED|[Rr]etired|no longer has",
                    f"{path}: retired command does not declare its own status",
                )

    def test_rollout_redirects_bind_exact_historical_subject(self) -> None:
        rows = registry_rows()
        trusted_commit = trusted_historical_commit()
        state, detail = current_main_trusted_history_state(trusted_commit)
        if state == "shallow":
            self.skipTest(
                "cannot evaluate the pinned historical authority commit from "
                f"this clone (truncated instrument): {detail}"
            )
        if state == "error":
            self.fail(
                "the ancestry instrument failed unexpectedly, so the pinned "
                f"historical authority commit cannot be evaluated; this is "
                f"NOT_PROVEN, not a skip: {detail}"
            )
        self.assertEqual(
            state,
            "present",
            "current main must retain the pinned historical authority commit",
        )
        for path, expected in ROLLOUT_REDIRECTS.items():
            with self.subTest(path=path):
                row = rows[path]
                head = "\n".join(
                    (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
                )
                self.assertEqual(row["status"], "superseded")
                self.assertEqual(row["successor"], expected["successor"])
                self.assertEqual(
                    historical_identity_findings(path, row, head, trusted_commit), []
                )
                self.assertIn("stable redirect", head)
                self.assertIn("non-executable", head)

        path = next(iter(ROLLOUT_REDIRECTS))
        row = dict(rows[path])
        candidate_commit = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        row["historical_source_commit"] = candidate_commit
        row["historical_source_blob_sha1"] = historical_blob_sha1(candidate_commit, path)
        head = "\n".join(
            (ROOT / path).read_text(encoding="utf-8").splitlines()[:24]
        )
        self.assertNotEqual(
            historical_identity_findings(path, row, head, trusted_commit),
            [],
            "changing both registry identity fields to a matching candidate blob "
            "must remain rejected",
        )

        self.assertNotEqual(
            historical_identity_findings(path, rows[path], head, candidate_commit),
            [],
            "a moving main tip must not redefine the pinned historical authority",
        )

    def test_rollout_redirects_do_not_retain_executable_queue_prose(self) -> None:
        forbidden = (
            "Remaining implementation ladder",
            "Each row is one PR",
            "Coworker agents",
            "pick from rails",
            "Umbrella: **#8590**",
            "Canonical post-landing source of truth",
        )
        for path in ROLLOUT_REDIRECTS:
            with self.subTest(path=path):
                body = (ROOT / path).read_text(encoding="utf-8")
                self.assertLessEqual(len(body.splitlines()), 24)
                for phrase in forbidden:
                    self.assertNotIn(phrase, body)

    def test_strong_clippy_redirect_rejects_the_8590_collision(self) -> None:
        body = (
            ROOT / "docs/development/STRONG_CLIPPY_LINTS_ROLLOUT.md"
        ).read_text(encoding="utf-8")

        self.assertIn("Current #8590 owns CPANTS/kwalitee oracle work", body)
        for issue in ("#9850", "#11335", "#11337", "#11404"):
            self.assertIn(issue, body)
        self.assertNotIn("Umbrella: **#8590**", body)

    def test_historical_provenance_is_unique_to_rollout_redirects(self) -> None:
        rows = registry_rows()
        marked = {
            path
            for path, row in rows.items()
            if "historical_source_commit" in row or "historical_source_blob_sha1" in row
        }
        self.assertEqual(marked, set(ROLLOUT_REDIRECTS))

    def test_legacy_authority_workflow_filter_covers_the_banner_set_exactly(self) -> None:
        required = set(EXPECTED) | {
            "docs/agents/authority_status.toml",
            "scripts/migrate-legacy-authority-banners.py",
            "tests/test_legacy_authority_banners.py",
            ".github/workflows/legacy-authority-banners.yml",
        }
        for event in ("pull_request", "push"):
            with self.subTest(event=event):
                self.assertEqual(workflow_event_paths(event), required)

    def test_one_shot_migrator_is_inert(self) -> None:
        source = MIGRATOR.read_text(encoding="utf-8")

        self.assertIn("RETIRED", source)
        self.assertIn("tests/test_legacy_authority_banners.py", source)
        self.assertNotIn("write_text", source)
        self.assertNotIn("def migrate", source)
        self.assertIn("raise SystemExit(2)", source)


class CurrentMainTrustedHistoryStateTests(unittest.TestCase):
    """The ancestry check must separate real findings from instrument failures.

    ``git merge-base --is-ancestor`` distinguishes three outcomes and the
    pinned-commit check must preserve that distinction: exit 0 is presence,
    exit 1 is a genuine finding, and anything a truncated clone produces
    (exit 128, or a shallow/grafted graph) proves nothing about main.
    """

    def setUp(self) -> None:
        # Git marks objects read-only, which Windows cannot delete while open;
        # leaked temp dirs are acceptable, wrong classifications are not.
        self._tmp = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        self.addCleanup(self._tmp.cleanup)
        self.remote = Path(self._tmp.name) / "remote"
        self._init_repo(self.remote)
        self.root_commit = self._commit(self.remote, "root")
        self._commit(self.remote, "tip")
        self._git(self.remote, "branch", "origin/main", "main")

    @staticmethod
    def _init_repo(repo: Path) -> None:
        subprocess.run(
            ["git", "init", "-b", "main", str(repo)],
            check=True,
            capture_output=True,
        )
        CurrentMainTrustedHistoryStateTests._git(
            repo,
            "config",
            "user.email",
            "authority-test@example.com",
        )
        CurrentMainTrustedHistoryStateTests._git(
            repo, "config", "user.name", "Authority Test"
        )

    @staticmethod
    def _git(repo: Path, *args: str) -> str:
        result = subprocess.run(
            ["git", "-C", str(repo), *args],
            check=True,
            capture_output=True,
            text=True,
        )
        return result.stdout.strip()

    def _commit(self, repo: Path, message: str) -> str:
        # Command-local override: a host with inherited commit.gpgsign=true
        # and no usable signing agent would fail fixture commits in setUp.
        self._git(repo, "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", message)
        return self._git(repo, "rev-parse", "HEAD")

    def _shallow_clone(self) -> Path:
        shallow = Path(self._tmp.name) / "shallow"
        subprocess.run(
            ["git", "clone", "--depth", "1", self.remote.as_uri(), str(shallow)],
            check=True,
            capture_output=True,
        )
        return shallow

    def test_pinned_commit_on_main_is_present(self) -> None:
        state, detail = current_main_trusted_history_state(
            self.root_commit, self.remote
        )
        self.assertEqual((state, detail), ("present", ""))

    def test_full_graph_excluding_the_commit_is_a_real_finding(self) -> None:
        self._git(self.remote, "checkout", "--orphan", "stray")
        stray_commit = self._commit(self.remote, "unrelated root")
        state, detail = current_main_trusted_history_state(stray_commit, self.remote)
        self.assertEqual((state, detail), ("absent", ""))

    def test_missing_object_in_a_full_clone_is_an_instrument_failure(self) -> None:
        other = Path(self._tmp.name) / "other"
        self._init_repo(other)
        stranded_commit = self._commit(other, "history this clone never fetched")

        state, detail = current_main_trusted_history_state(stranded_commit, self.remote)

        self.assertEqual(state, "error")
        self.assertIn("exited 128", detail)

    def test_shallow_clone_without_the_object_is_an_instrument_failure(self) -> None:
        shallow = self._shallow_clone()

        state, detail = current_main_trusted_history_state(
            self.root_commit, shallow
        )

        self.assertEqual(state, "shallow")
        self.assertIn("shallow", detail)

    def test_shallow_clone_with_the_object_fetched_still_cannot_answer(self) -> None:
        shallow = self._shallow_clone()
        # The issue's second trap: after a depth-1 fetch the object exists but
        # the graph stays grafted, so the exit code alone still proves nothing.
        self._git(shallow, "fetch", "--deepen=1", "origin", "main")

        state, detail = current_main_trusted_history_state(
            self.root_commit, shallow
        )

        self.assertEqual(state, "shallow")
        self.assertIn("shallow", detail)


if __name__ == "__main__":
    unittest.main()
