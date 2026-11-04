from __future__ import annotations

from copy import deepcopy
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from git_environment import isolated_git_env

from check_release_tag_provenance import (  # noqa: E402
    load_manifest,
    validate_manifest,
    verify_git_refs,
)


SHA_A = "1" * 40
SHA_B = "2" * 40


def valid_manifest() -> dict:
    return {
        "schema_version": 1,
        "repository": "example/project",
        "audited_at": "2026-07-12",
        "tag": [
            {
                "name": "v0.1.0",
                "current_sha": SHA_A,
                "record_status": "match",
                "recorded_sha": SHA_A[:8],
                "recorded_reachable": True,
                "lineage": "root",
            },
            {
                "name": "v0.2.0",
                "current_sha": SHA_B,
                "record_status": "pending",
                "predecessor": "v0.1.0",
                "lineage": "linear",
            },
        ],
        "missing_tag": [
            {
                "version": "0.1.1",
                "status": "never-cut",
                "note": "fixture",
            }
        ],
    }


class ManifestValidationTests(unittest.TestCase):
    def test_valid_manifest(self) -> None:
        self.assertEqual([], validate_manifest(valid_manifest()))

    def test_duplicate_tag_is_rejected(self) -> None:
        manifest = valid_manifest()
        manifest["tag"].append(deepcopy(manifest["tag"][0]))
        errors = validate_manifest(manifest)
        self.assertTrue(any("duplicate tag record" in error for error in errors))

    def test_malformed_sha_is_rejected(self) -> None:
        manifest = valid_manifest()
        manifest["tag"][0]["current_sha"] = "abc123"
        errors = validate_manifest(manifest)
        self.assertTrue(any("40-hex" in error for error in errors))

    def test_unknown_predecessor_is_rejected(self) -> None:
        manifest = valid_manifest()
        manifest["tag"][1]["predecessor"] = "v9.9.9"
        errors = validate_manifest(manifest)
        self.assertTrue(any("unknown predecessor" in error for error in errors))

    def test_record_status_must_match_sha_relationship(self) -> None:
        manifest = valid_manifest()
        manifest["tag"][0]["record_status"] = "stale"
        manifest["tag"][0]["recorded_reachable"] = False
        errors = validate_manifest(manifest)
        self.assertTrue(any("marked stale" in error for error in errors))

    def test_audited_at_must_be_a_calendar_date(self) -> None:
        manifest = valid_manifest()
        manifest["audited_at"] = "2026-02-30"
        errors = validate_manifest(manifest)
        self.assertTrue(any("audited_at" in error for error in errors))

    def test_committed_manifest_matches_schema(self) -> None:
        root = SCRIPTS.parent
        manifest = load_manifest(root / "policy/release-tag-provenance.toml")
        self.assertEqual([], validate_manifest(manifest))


class GitAvailabilityTests(unittest.TestCase):
    @patch("check_release_tag_provenance.shutil.which", return_value=None)
    def test_missing_git_returns_actionable_error(self, _which: object) -> None:
        self.assertEqual(
            ["git executable not found on PATH"],
            verify_git_refs({"tag": []}, Path("."))[0],
        )


@unittest.skipUnless(shutil.which("git"), "git executable not found on PATH")
class GitVerificationTests(unittest.TestCase):
    def run_git(self, root: Path, *args: str) -> str:
        result = subprocess.run(
            ["git", "-C", str(root), *args],
            env=isolated_git_env(),
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        return result.stdout.strip()

    def write_commit(self, root: Path, value: str) -> str:
        (root / "value.txt").write_text(value, encoding="utf-8")
        self.run_git(root, "add", "value.txt")
        self.run_git(root, "commit", "-m", value)
        return self.run_git(root, "rev-parse", "HEAD")

    def init_repo(self, root: Path) -> None:
        self.run_git(root, "init")
        self.run_git(root, "config", "user.name", "Tag Provenance Test")
        self.run_git(root, "config", "user.email", "test@example.invalid")

    def cloud_fixture(
        self, root: Path, *, shallow: bool = False, untagged_tip: bool = False
    ) -> tuple[Path, Path, Path]:
        upstream = root / "upstream"
        upstream.mkdir()
        self.init_repo(upstream)
        first = self.write_commit(upstream, "first")
        self.run_git(upstream, "tag", "v0.1.0")
        second = self.write_commit(upstream, "second")
        self.run_git(upstream, "tag", "-a", "v0.2.0", "-m", "annotated release")
        self.run_git(upstream, "tag", "v9.9.9")  # Undeclared remote tag must stay absent.
        if untagged_tip:
            self.write_commit(upstream, "untagged-tip")
        checkout = root / "cloud"
        args = ["clone", "--no-tags"]
        if shallow:
            args.extend(["--depth", "1"])
        self.run_git(root, *args, upstream.as_uri(), str(checkout))
        manifest = root / "manifest.toml"
        manifest.write_text(
            'schema_version = 1\nrepository = "fixture/cloud"\naudited_at = "2026-07-12"\n'
            '[[tag]]\nname = "v0.0.1"\ncurrent_sha = "' + "f" * 40 + '"\n'
            'unresolvable = true\nrecord_status = "pending"\nlineage = "root"\n'
            '[[tag]]\nname = "v0.1.0"\ncurrent_sha = "' + first + '"\n'
            'record_status = "match"\nrecorded_sha = "' + first[:8] + '"\n'
            'recorded_reachable = true\nlineage = "root"\n'
            '[[tag]]\nname = "v0.2.0"\ncurrent_sha = "' + second + '"\n'
            'record_status = "pending"\npredecessor = "v0.1.0"\nlineage = "linear"\n',
            encoding="utf-8",
        )
        return upstream, checkout, manifest

    def cloud_command(self, checkout: Path, manifest: Path, mode: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPTS / "check_release_tag_provenance.py"), mode,
             "--repo-root", str(checkout), "--manifest", str(manifest)],
            cwd=manifest.parent, text=True, capture_output=True, check=False,
        )

    def test_cloud_missing_refs_restore_exact_pins_and_preserve_config(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            _, checkout, manifest = self.cloud_fixture(Path(temp))
            missing = self.cloud_command(checkout, manifest, "--verify-git")
            self.assertNotEqual(0, missing.returncode)
            self.assertIn("v0.1.0 cannot be resolved", missing.stderr)
            self.assertNotIn("provenance OK", missing.stdout)
            git_dir = checkout / ".git"
            config = (git_dir / "config").read_bytes()
            (git_dir / "FETCH_HEAD").write_text("preserved fetch receipt\n")
            restored = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertEqual(0, restored.returncode, restored.stderr)
            self.assertIn("1 orphaned lineage", restored.stdout)
            self.assertEqual(["v0.1.0", "v0.2.0"], self.run_git(checkout, "tag", "--list").splitlines())
            data = load_manifest(manifest)
            for row in data["tag"][1:]:
                self.assertEqual(row["current_sha"], self.run_git(checkout, "rev-parse", row["name"] + "^{commit}"))
            self.assertEqual(config, (git_dir / "config").read_bytes())
            self.assertEqual("preserved fetch receipt\n", (git_dir / "FETCH_HEAD").read_text())
            # A prepared checkout requires no network on the next invocation.
            self.run_git(checkout, "remote", "set-url", "origin", str(Path(temp) / "offline"))
            again = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertEqual(0, again.returncode, again.stderr)

    def test_cloud_missing_remote_ref_fails_without_success(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            upstream, checkout, manifest = self.cloud_fixture(Path(temp))
            self.run_git(upstream, "tag", "-d", "v0.2.0")
            result = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertNotEqual(0, result.returncode)
            self.assertIn("required release refs unavailable", result.stderr)
            self.assertNotIn("provenance OK", result.stdout)
            self.assertEqual("", self.run_git(checkout, "tag", "--list"))

    def test_cloud_conflicting_local_tag_is_preserved_before_fetch(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            _, checkout, manifest = self.cloud_fixture(Path(temp))
            self.run_git(checkout, "tag", "v0.1.0")  # HEAD is the wrong release commit.
            before = self.run_git(checkout, "rev-parse", "refs/tags/v0.1.0")
            result = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertNotEqual(0, result.returncode)
            self.assertIn("existing tag preserved", result.stderr)
            self.assertEqual(before, self.run_git(checkout, "rev-parse", "refs/tags/v0.1.0"))
            self.assertEqual("v0.1.0", self.run_git(checkout, "tag", "--list"))

    def test_cloud_restored_remote_pin_mismatch_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            upstream, checkout, manifest = self.cloud_fixture(Path(temp))
            self.run_git(upstream, "tag", "-f", "v0.1.0", "HEAD")
            result = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertNotEqual(0, result.returncode)
            self.assertIn("v0.1.0 drifted", result.stderr)
            self.assertNotIn("provenance OK", result.stdout)

    def test_cloud_shallow_history_is_explicit_then_restored(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            _, checkout, manifest = self.cloud_fixture(
                Path(temp), shallow=True, untagged_tip=True
            )
            self.assertEqual("true", self.run_git(checkout, "rev-parse", "--is-shallow-repository"))
            missing = self.cloud_command(checkout, manifest, "--verify-git")
            self.assertNotEqual(0, missing.returncode)
            self.assertIn("full history unavailable", missing.stderr)
            restored = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertEqual(0, restored.returncode, restored.stderr)
            self.assertEqual("false", self.run_git(checkout, "rev-parse", "--is-shallow-repository"))
            self.assertEqual("3", self.run_git(checkout, "rev-list", "--count", "HEAD"))
            self.run_git(checkout, "fsck", "--connectivity-only")
            verified = self.cloud_command(checkout, manifest, "--verify-git")
            self.assertEqual(0, verified.returncode, verified.stderr)

    def test_cloud_shallow_reannotation_preserves_existing_tag_objects(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            upstream, checkout, manifest = self.cloud_fixture(
                Path(temp), shallow=True, untagged_tip=True
            )
            self.run_git(
                checkout, "fetch", "--no-tags", "origin",
                "refs/tags/v0.1.0:refs/tags/v0.1.0",
                "refs/tags/v0.2.0:refs/tags/v0.2.0",
            )
            self.assertEqual("true", self.run_git(checkout, "rev-parse", "--is-shallow-repository"))
            original = self.run_git(checkout, "rev-parse", "refs/tags/v0.2.0")
            commit = self.run_git(checkout, "rev-parse", "refs/tags/v0.2.0^{commit}")
            self.run_git(upstream, "tag", "-f", "-a", "v0.2.0", "-m", "new annotation", commit)
            self.assertNotEqual(original, self.run_git(upstream, "rev-parse", "refs/tags/v0.2.0"))
            # Configured refmaps must not turn a source-only request into a
            # forced tag update behind the preparation command's back.
            self.run_git(checkout, "config", "--add", "remote.origin.fetch", "+refs/tags/*:refs/tags/*")
            git_dir = checkout / ".git"
            config = (git_dir / "config").read_bytes()
            (git_dir / "FETCH_HEAD").write_text("preserved reannotation receipt\n")
            restored = self.cloud_command(checkout, manifest, "--prepare-git")
            self.assertEqual(0, restored.returncode, restored.stderr)
            self.assertEqual(original, self.run_git(checkout, "rev-parse", "refs/tags/v0.2.0"))
            self.assertEqual(commit, self.run_git(checkout, "rev-parse", "refs/tags/v0.2.0^{commit}"))
            self.assertEqual(["v0.1.0", "v0.2.0"], self.run_git(checkout, "tag", "--list").splitlines())
            self.assertEqual(config, (git_dir / "config").read_bytes())
            self.assertEqual("preserved reannotation receipt\n", (git_dir / "FETCH_HEAD").read_text())
            self.assertEqual("false", self.run_git(checkout, "rev-parse", "--is-shallow-repository"))
            self.assertEqual("3", self.run_git(checkout, "rev-list", "--count", "HEAD"))
            self.run_git(checkout, "fsck", "--connectivity-only")

    def test_local_ref_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.init_repo(root)
            first = self.write_commit(root, "first")
            self.run_git(root, "tag", "v0.1.0")
            second = self.write_commit(root, "second")
            self.run_git(root, "tag", "v0.2.0")

            manifest = {
                "tag": [
                    {
                        "name": "v0.1.0",
                        "current_sha": first,
                        "lineage": "root",
                    },
                    {
                        "name": "v0.2.0",
                        "current_sha": "f" * 40,
                        "predecessor": "v0.1.0",
                        "lineage": "linear",
                    },
                ]
            }
            errors = verify_git_refs(manifest, root)[0]
            self.assertTrue(any("v0.2.0 drifted" in error for error in errors))
            self.assertNotEqual(second, "f" * 40)

    def test_recorded_reachability_claim_is_verified(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.init_repo(root)
            first = self.write_commit(root, "first")
            self.run_git(root, "tag", "v0.1.0")

            manifest = {
                "tag": [
                    {
                        "name": "v0.1.0",
                        "current_sha": first,
                        "record_status": "match",
                        "recorded_sha": first[:8],
                        "recorded_reachable": False,
                        "lineage": "root",
                    }
                ]
            }
            errors = verify_git_refs(manifest, root)[0]
            self.assertTrue(
                any("recorded_sha" in error and "claimed unreachable" in error for error in errors)
            )

    def test_annotated_tag_object_is_not_a_reachable_commit(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.init_repo(root)
            first = self.write_commit(root, "first")
            self.run_git(root, "tag", "-a", "v0.1.0", "-m", "release")
            tag_object = self.run_git(root, "rev-parse", "v0.1.0")

            manifest = {
                "tag": [
                    {
                        "name": "v0.1.0",
                        "current_sha": first,
                        "record_status": "stale",
                        "recorded_sha": tag_object,
                        "recorded_reachable": False,
                        "lineage": "root",
                    }
                ]
            }
            self.assertEqual([], verify_git_refs(manifest, root)[0])

            manifest["tag"][0]["recorded_reachable"] = True
            errors = verify_git_refs(manifest, root)[0]
            self.assertTrue(any("not a reachable commit object" in error for error in errors))

    def test_unlisted_local_release_tag_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.init_repo(root)
            first = self.write_commit(root, "first")
            self.run_git(root, "tag", "v0.1.0")
            second = self.write_commit(root, "second")
            self.run_git(root, "tag", "v0.2.0")
            self.write_commit(root, "third")
            self.run_git(root, "tag", "v0.3.0")

            manifest = {
                "tag": [
                    {
                        "name": "v0.1.0",
                        "current_sha": first,
                        "lineage": "root",
                    },
                    {
                        "name": "v0.2.0",
                        "current_sha": second,
                        "predecessor": "v0.1.0",
                        "lineage": "linear",
                    },
                ]
            }
            errors = verify_git_refs(manifest, root)[0]
            self.assertIn(
                "local release tag is missing from manifest: v0.3.0",
                errors,
            )

    def test_diverged_lineage_is_accepted_when_neither_ref_is_ancestral(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.init_repo(root)
            base = self.write_commit(root, "base")

            self.run_git(root, "checkout", "-b", "old-line")
            old = self.write_commit(root, "old")
            self.run_git(root, "tag", "v0.1.0")

            self.run_git(root, "checkout", "--detach", base)
            self.run_git(root, "checkout", "-b", "new-line")
            new = self.write_commit(root, "new")
            self.run_git(root, "tag", "v0.2.0")

            manifest = {
                "tag": [
                    {
                        "name": "v0.1.0",
                        "current_sha": old,
                        "lineage": "root",
                    },
                    {
                        "name": "v0.2.0",
                        "current_sha": new,
                        "predecessor": "v0.1.0",
                        "lineage": "diverged",
                    },
                ]
            }
            self.assertEqual([], verify_git_refs(manifest, root)[0])




class OrphanClassificationTests(unittest.TestCase):
    """#15263: typed classification of orphaned lineage records."""

    def _git(self, repo, *args):
        return subprocess.run(["git", "-C", repo, *args], env=isolated_git_env(), capture_output=True, text=True)

    def _init(self, repo):
        subprocess.run(["git", "init", "-q", repo], env=isolated_git_env(), check=True)
        self._git(repo, "config", "user.name", "t")
        self._git(repo, "config", "user.email", "t@x")
        open(f"{repo}/f", "w").close()
        self._git(repo, "add", "f")
        self._git(repo, "commit", "-qm", "c")

    def test_audited_orphan_is_warning_not_drift(self):
        """Flagged row + commit object absent from the repo = typed warning.
        The tag itself never resolves, so the rev-parse arm is bypassed via
        the direct object probe in the classifier (#15263)."""
        manifest = valid_manifest()
        manifest["tag"] = manifest["tag"][:1]
        manifest["tag"][0]["current_sha"] = "3" * 40
        manifest["tag"][0]["unresolvable"] = True

        with tempfile.TemporaryDirectory() as repo:
            self._init(repo)
            drift, unresolvable = verify_git_refs(manifest, repo)

        self.assertEqual([], drift)
        self.assertEqual(1, len(unresolvable))
        self.assertIn("orphaned lineage record", unresolvable[0])

    def test_unflagged_missing_stays_drift(self):
        """Unflagged row + absent tag and commit = drift error."""
        manifest = valid_manifest()
        manifest["tag"].append({
            "name": "v9.9.9",
            "current_sha": "a" * 40,
            "record_status": "stale",
            "recorded_sha": "b" * 7,
            "recorded_reachable": False,
            "lineage": "root",
        })
        with tempfile.TemporaryDirectory() as repo:
            self._init(repo)
            drift, unresolvable = verify_git_refs(manifest, repo)
        self.assertTrue(any("cannot be resolved locally" in e for e in drift), drift)
        self.assertEqual([], unresolvable)

    def test_flagged_tag_with_reachable_commit_is_deleted_tag_drift(self):
        """A flagged row whose pinned commit IS locally reachable is
        deleted-release-tag drift, not an orphan (#15029 review)."""
        manifest = valid_manifest()
        manifest["tag"] = manifest["tag"][:1]
        manifest["tag"][0]["unresolvable"] = True
        with tempfile.TemporaryDirectory() as repo:
            self._init(repo)
            tree = self._git(repo, "rev-parse", "HEAD^{tree}").stdout.strip()
            parent = self._git(repo, "rev-parse", "HEAD").stdout.strip()
            commit = self._git(
                repo, "commit-tree", tree, "-p", parent, "-m", "pinned"
            ).stdout.strip()
            manifest["tag"][0]["current_sha"] = commit
            drift, unresolvable = verify_git_refs(manifest, repo)
        self.assertTrue(
            any("deleted-release-tag drift" in e for e in drift), drift
        )
        self.assertEqual([], unresolvable)

    def test_flagged_row_with_surviving_tag_is_drift(self):
        """A flagged row whose tag ref still exists locally is drift even
        when the tag points at a non-commit object (#15263)."""
        manifest = valid_manifest()
        manifest["tag"] = manifest["tag"][:1]
        manifest["tag"][0]["unresolvable"] = True
        with tempfile.TemporaryDirectory() as repo:
            self._init(repo)
            tree = self._git(repo, "rev-parse", "HEAD^{tree}").stdout.strip()
            self._git(repo, "update-ref", "refs/tags/v0.1.0", tree)
            drift, unresolvable = verify_git_refs(manifest, repo)
        self.assertTrue(any("the tag exists locally" in e for e in drift), drift)
        self.assertEqual([], unresolvable)

    def test_flagged_row_with_non_commit_pin_is_drift(self):
        """A flagged row whose pinned sha resolves to a blob passes no more:
        the pin must be absent for the orphan claim to hold (#15263)."""
        manifest = valid_manifest()
        manifest["tag"] = manifest["tag"][:1]
        manifest["tag"][0]["unresolvable"] = True
        with tempfile.TemporaryDirectory() as repo:
            self._init(repo)
            blob = subprocess.run(
                ["git", "-C", repo, "hash-object", "-w", "--stdin"],
                env=isolated_git_env(),
                input="payload", capture_output=True, text=True, check=True,
            ).stdout.strip()
            manifest["tag"][0]["current_sha"] = blob
            drift, unresolvable = verify_git_refs(manifest, repo)
        self.assertTrue(
            any("resolves to a local blob object" in e for e in drift), drift
        )
        self.assertEqual([], unresolvable)

if __name__ == "__main__":
    unittest.main()
