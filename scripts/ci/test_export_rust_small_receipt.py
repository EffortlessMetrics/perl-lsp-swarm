"""Executed custody controls; canonical semantic validation stays in Rust."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest

from scripts.ci.export_rust_small_receipt import export, MAX_RECEIPT_BYTES


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.workspace = self.base / "workspace"
        self.runner = self.base / "runner"
        self.root = self.runner / "private-custody"
        self.root.mkdir(parents=True)
        self.root.chmod(0o700)
        self.source = self.workspace / "target/receipts/rust-small-proof.json"
        self.source.parent.mkdir(parents=True)
        self.env = dict(GITHUB_WORKSPACE=str(self.workspace), RUNNER_TEMP=str(self.runner),
                        EM_CI_ARTIFACT_DIR=str(self.root),
                        GITHUB_REPOSITORY="EffortlessMetrics/perl-lsp-swarm",
                        GITHUB_SHA="a" * 40, GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="2",
                        GITHUB_JOB="standard", GITHUB_WORKFLOW="EM CI Routed Rust",
                        GITHUB_WORKFLOW_REF="EffortlessMetrics/perl-lsp-swarm/.github/workflows/em-ci-routed-rust.yml@refs/heads/feat/contabo-rust-standard",
                        GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_REF="refs/heads/feat/contabo-rust-standard")
        entered = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(seconds=2)
        self.context = dict(schema="em-ci-job-context.v1", repository=self.env["GITHUB_REPOSITORY"],
                            sha=self.env["GITHUB_SHA"], run_id="123", run_attempt="2", profile="standard",
                            proof_started_at=entered.strftime("%Y-%m-%dT%H:%M:%SZ"))
        self.context.update(job=self.env["GITHUB_JOB"], workflow=self.env["GITHUB_WORKFLOW"],
                            workflow_ref=self.env["GITHUB_WORKFLOW_REF"], event=self.env["GITHUB_EVENT_NAME"],
                            ref=self.env["GITHUB_REF"])
        self.context_path = self.runner / "em-ci-job-context.json"
        self.context_path.write_text(json.dumps(self.context))
        # Transport fixture, deliberately not a substitute for Rust's step verifier.
        self.receipt = dict(schema_version="rust_small_proof.v1", result="success",
                            subject=dict(git_sha=self.env["GITHUB_SHA"], worktree_dirty=False))
        self.source.write_text(json.dumps(self.receipt))

    def refused(self):
        before = self.source.read_bytes() if self.source.exists() else None
        with self.assertRaises((ValueError, KeyError, TypeError, OSError)):
            export(self.env)
        if before is not None:
            self.assertEqual(self.source.read_bytes(), before)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_exact_bytes_current_native_subject_and_digest_are_retained(self):
        data = self.source.read_bytes()
        manifest = export(self.env)
        self.assertEqual((self.root / "rust-small-proof.json").read_bytes(), data)
        self.assertEqual(manifest["member"], dict(path="rust-small-proof.json", bytes=len(data),
                                                  sha256=hashlib.sha256(data).hexdigest()))
        self.assertEqual(manifest["run_attempt"], "2")
        self.assertEqual(manifest["sha"], self.env["GITHUB_SHA"])
        self.assertEqual(set(p.name for p in self.root.iterdir()),
                         {"rust-small-proof.json", "perl-rust-small-export.json"})

    def test_optional_command_owner_bytes_extend_only_existing_private_manifest(self):
        self.context['product_workspace']=str(self.workspace.resolve())
        self.context_path.write_text(json.dumps(self.context))
        source=self.runner/'command-proof-set.json'
        raw=json.dumps(dict(schema_version='command-proof-set.v1',commands=[{'id':'actual-owner'}],result='FAILURE')).encode()
        source.write_bytes(raw)
        manifest=export(self.env,command_receipt=source)
        self.assertEqual((self.root/source.name).read_bytes(),raw)
        self.assertEqual(manifest['command_member'],dict(path=source.name,bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest()))
        self.assertEqual(json.loads(raw)['result'],'FAILURE')
        self.assertEqual(manifest['result'],'success')  # Rust result remains its owner's result.

    def test_optional_command_proof_missing_capture_old_linked_or_foreign_refuses_before_writes(self):
        source=self.runner/'command-proof-set.json'
        source.write_text(json.dumps(dict(schema_version='command-proof-set.v1',commands=[{}],result='SUCCESS')))
        with self.assertRaises(ValueError):export(self.env,command_receipt=source)
        self.assertEqual(list(self.root.iterdir()),[])
        self.context['product_workspace']=str(self.workspace.resolve())
        self.context_path.write_text(json.dumps(self.context))
        os.utime(source,(1,1))
        with self.assertRaises(ValueError):export(self.env,command_receipt=source)
        self.assertEqual(list(self.root.iterdir()),[])
        source.unlink();source.symlink_to(self.source)
        with self.assertRaises(ValueError):export(self.env,command_receipt=source)
        self.assertEqual(list(self.root.iterdir()),[])

    def test_failure_receipt_is_diagnostic_and_never_relabelled_success(self):
        self.receipt["result"] = "product_failure"
        self.source.write_text(json.dumps(self.receipt))
        self.assertEqual(export(self.env)["result"], "product_failure")

    def test_prior_attempt_context_is_rejected(self):
        self.context["run_attempt"] = "1"
        self.context_path.write_text(json.dumps(self.context))
        self.refused()

    def test_unrelated_producer_and_workflow_context_are_rejected(self):
        for field in ("job", "workflow", "workflow_ref", "event", "ref"):
            with self.subTest(field=field):
                context = {**self.context, field: "unrelated"}
                self.context_path.write_text(json.dumps(context))
                self.refused()

    def test_world_writable_root_is_rejected(self):
        self.root.chmod(0o777)
        self.refused()

    def test_same_head_old_receipt_is_rejected(self):
        os.utime(self.source, (1, 1))
        self.refused()

    def test_wrong_source_and_dirty_receipts_are_rejected(self):
        for field, value in (("git_sha", "b" * 40), ("worktree_dirty", True)):
            with self.subTest(field=field):
                receipt = {**self.receipt, "subject": {**self.receipt["subject"], field: value}}
                self.source.write_text(json.dumps(receipt))
                self.refused()

    def test_missing_and_oversized_receipts_are_rejected(self):
        self.source.unlink()
        self.refused()
        self.source.write_bytes(b"x" * (MAX_RECEIPT_BYTES + 1))
        self.refused()

    def test_symlink_and_hardlink_receipts_are_rejected(self):
        original = self.source.with_name("original.json")
        self.source.rename(original)
        self.source.symlink_to(original)
        self.refused()
        self.source.unlink()
        os.link(original, self.source)
        self.refused()

    def test_existing_export_is_never_overwritten(self):
        export(self.env)
        before = {p.name: p.read_bytes() for p in self.root.iterdir()}
        with self.assertRaises(ValueError):
            export(self.env)
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.root.iterdir()})

    def test_workspace_staging_is_rejected(self):
        root = self.workspace / "artifact"
        root.mkdir()
        self.env["EM_CI_ARTIFACT_DIR"] = str(root)
        self.refused()

    def test_linked_parent_and_linked_central_context_are_rejected(self):
        parent = self.source.parent
        real_parent = parent.with_name("real-receipts")
        parent.rename(real_parent)
        parent.symlink_to(real_parent, target_is_directory=True)
        self.refused()
        parent.unlink()
        real_parent.rename(parent)
        context = self.context_path.with_name("real-context.json")
        self.context_path.rename(context)
        self.context_path.symlink_to(context)
        self.refused()


if __name__ == "__main__":
    unittest.main()
