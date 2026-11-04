"""Concrete callback for the existing prebuilt Rust Small semantic verifier.

Native packet authentication belongs to the central importer. Executable identity
must be supplied by the trusted control/tool installer, never candidate JSON.
This adapter invokes the owner CLI and projects its verified observations; it
contains no replacement command vocabulary, selector, compiler or product runner.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess

from .candidate_readiness import CommittedProof, Selection
from .export_rust_small_receipt import MAX_RECEIPT_BYTES, regular_bytes
from .request_qualification import Refused
from .rust_small_evidence import unique_object

DIGEST = re.compile(r'[0-9a-f]{64}')
CUSTODY_FIELDS = ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns', 'st_nlink')


def custody(metadata):
    return tuple(getattr(metadata, field) for field in CUSTODY_FIELDS)


class CanonicalOwner:
    def __init__(self, *, verifier: Path, verifier_sha256: str, control_sha: str, workspace: Path):
        if (not isinstance(verifier, Path) or not verifier.is_absolute()
                or not isinstance(workspace, Path) or not workspace.is_absolute()
                or not isinstance(verifier_sha256, str) or not DIGEST.fullmatch(verifier_sha256)):
            raise Refused('canonical owner requires trusted absolute tool identity/workspace')
        self.verifier, self.verifier_sha256 = verifier, verifier_sha256
        self.control_sha, self.workspace = control_sha, workspace

    def executable_identity(self):
        path = self.verifier
        if '..' in path.parts or any(p.is_symlink() for p in (path, *path.parents)):
            raise Refused('canonical verifier path is not immutable custody')
        before = path.stat()
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not os.access(path, os.X_OK):
            raise Refused('canonical verifier is not a regular executable')
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(65536), b''):
                digest.update(chunk)
        after = path.stat()
        fields = ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns', 'st_nlink')
        if any(getattr(before, key) != getattr(after, key) for key in fields) or digest.hexdigest() != self.verifier_sha256:
            raise Refused('canonical verifier differs from trusted installer identity')
        return tuple(getattr(after, key) for key in fields)

    def __call__(self, receipt_path, plan, selection):
        if (self.control_sha != plan['control'] or not isinstance(selection, Selection)
                or selection.subject_key != plan['request_key']
                or selection.portfolio_sha256 != plan['portfolio_sha256']
                or selection.policy_sha256 != plan['policy_sha256']):
            raise Refused('canonical verifier/selection differs from acquired control subject')
        receipt_path = Path(receipt_path)
        raw, receipt_metadata = regular_bytes(receipt_path, MAX_RECEIPT_BYTES)
        context_path = receipt_path.parent / 'em-ci-job-context.json'
        context_raw, context_metadata = regular_bytes(context_path, 16384)
        context = json.loads(context_raw, object_pairs_hook=unique_object)
        captured = {key: plan[key] for key in ('head', 'tree', 'base', 'control', 'request_key')}
        if (context.get('schema') != 'em-ci-job-context.v1'
                or context.get('repository') != plan['repository'] or context.get('sha') != plan['control']
                or context.get('profile') != 'standard' or context.get('qualification_subject') != captured
                or any(not isinstance(context.get(key), str) or not re.fullmatch('[1-9][0-9]*', context[key])
                       for key in ('run_id', 'run_attempt'))
                or type(context.get('producer_check_run_id')) is not int or context['producer_check_run_id'] <= 0):
            raise Refused('canonical projection lacks authenticated native custody identity')
        reference = (f'https://github.com/{plan["repository"]}/actions/runs/{context["run_id"]}'
                     f'/attempts/{context["run_attempt"]}#job-{context["producer_check_run_id"]}')
        digest = hashlib.sha256(raw).hexdigest()
        identity = self.executable_identity()
        environment = {key: value for key, value in os.environ.items()
            if not key.startswith('GIT_') and key not in
            {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN'}}
        # Match the existing imported-receipt verifier's pinned toolchain and
        # no-auto-install policy. This CLI runs no product command/build.
        environment.update(RUSTUP_TOOLCHAIN='1.95.0', RUSTUP_AUTO_INSTALL='0')
        try:
            process = subprocess.run([str(self.verifier), '--worktree-root', str(self.workspace), 'rust-small-proof', '--verify-receipt', str(receipt_path)],
                cwd=self.workspace, env=environment, capture_output=True, timeout=60, check=False)
            completed = process.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            completed = False
        after_receipt, after_receipt_metadata = regular_bytes(receipt_path, MAX_RECEIPT_BYTES)
        after_context, after_context_metadata = regular_bytes(context_path, 16384)
        if (self.executable_identity() != identity
                or after_receipt != raw or custody(after_receipt_metadata) != custody(receipt_metadata)
                or after_context != context_raw or custody(after_context_metadata) != custody(context_metadata)):
            raise Refused('canonical executable/receipt/custody changed during verification')
        if not completed:
            return CommittedProof(plan['request_key'], 'unavailable', (), (), digest, reference,
                                  'canonical semantic verifier refused or did not complete')
        # The actual Rust owner has now checked schema, exact clean H/toolchain,
        # full ordered argv vocabulary, census, failure tail and typed outcomes.
        # Project observations only; a zero exit is not a product-success verdict.
        receipt = json.loads(raw, object_pairs_hook=unique_object)
        if (receipt.get('schema_version') != 'rust_small_proof.v1'
                or receipt.get('subject', {}).get('git_sha') != plan['head']
                or receipt.get('subject', {}).get('worktree_dirty') is not False):
            raise Refused('canonical verified receipt omitted exact clean H')
        steps = receipt.get('steps')
        if (not isinstance(steps, list) or not steps or any(not isinstance(row, dict) for row in steps)
                or tuple(row.get('name') for row in steps) != selection.selected):
            raise Refused('canonical observed step identities differ from selected owner commands')
        executed = tuple(row['name'] for row in steps if row.get('outcome') != 'not_run')
        failed = tuple(row['name'] for row in steps if row.get('outcome') == 'product_failure')
        result = receipt.get('result')
        if result == 'success' and all(row.get('outcome') == 'ok' for row in steps):
            disposition = 'verified'
        elif result == 'product_failure' and failed:
            disposition = 'failure'
        elif result in {'not_completed', 'instrument_failure'}:
            disposition = 'unavailable'
        else:
            raise Refused('canonical outcome cannot certify the claimed result')
        return CommittedProof(plan['request_key'], disposition, executed, failed, digest, reference,
                              'projected actual canonical owner verdict')
