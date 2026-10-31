"""Inactive composition of existing canonical owners; no executor or HTTP writer.

All ports are trusted in-process control code. In particular, native_import is
bound to the central authenticated importer and canonical_owner to the existing
Rust verifier plus its command-observation adapter. Objects/paths constructed by
candidate JSON do not authenticate evidence. There is no CLI or environment flag
that installs these ports into the production request path.
"""
import hashlib
import json
from pathlib import Path

from . import candidate_readiness as admission
from . import qualification_publication as publication
from .export_rust_small_receipt import MAX_RECEIPT_BYTES, regular_bytes
from .request_qualification import Eligibility, Refused, rehearse_event
from .rust_small_evidence import unique_object
from .rust_small_subject import product_subject


class RustSmallOwners:
    """Bind central custody and canonical semantics to existing admission ports.

    native_import(*, acquire_plan) returns the authenticated private packet Path.
    canonical_owner(receipt_path, plan, selection) returns CommittedProof only
    after canonical semantic verification. It owns execution/command semantics;
    this adapter checks identity, immutable bytes and producer association only.
    """
    def __init__(self, adapter, environment, *, selection_owner, native_import,
                 canonical_owner, applicability_owner, entry_committed_owner=None):
        if not all(callable(port) for port in (selection_owner, native_import,
                                               canonical_owner, applicability_owner)):
            raise ValueError('existing trusted owner callback missing')
        if entry_committed_owner is not None and not callable(entry_committed_owner):
            raise ValueError('entry committed proof owner must be a trusted callable')
        self.entry_committed_owner = entry_committed_owner
        self.adapter, self.environment = adapter, dict(environment)
        self.selection_owner, self.native_import = selection_owner, native_import
        self.canonical_owner, self.applicability_owner = canonical_owner, applicability_owner

    def acquire(self):
        plan = self.adapter.snapshot()
        if (self.environment.get('GITHUB_EVENT_NAME') != 'workflow_dispatch'
                or plan['repository'] != self.environment.get('GITHUB_REPOSITORY')
                or plan['control'] != self.environment.get('GITHUB_SHA')):
            raise Refused('native control differs from acquired plan')
        dispatcher = (f'{plan["repository"]}/{plan["workflow_path"]}'
                      f'@refs/heads/{plan["control_branch"]}')
        if self.environment.get('GITHUB_WORKFLOW_REF') != dispatcher:
            raise Refused('native dispatcher workflow differs from acquired plan')
        return plan

    def capture(self, central_capture):
        """Use the central owner's native-context writer, never request JSON."""
        if not callable(central_capture):
            raise Refused('central capture owner missing')
        return central_capture(self.environment, acquire_plan=self.acquire)

    def enter_native(self, event, central_capture):
        """Join native election and live owner admission before context capture.

        Trusted workflow code supplies this object and the central capture owner;
        dispatch JSON cannot install either. READY_FOR_ALLOCATION is a source
        admission result, never an allocation, product result or merge authority.
        No callback is installed by the metadata-only workflow today.
        """
        elected = rehearse_event(self.adapter.api, event, self.environment)
        plan = self.acquire()
        if elected['request_key'] != plan['request_key']:
            raise Refused('native entry subject moved after election')
        result = dict(elected, status='NOT_PROVEN', context=None,
                      execution_authorized=False, merge_authorized=False)
        if not elected['rehearsal_elected']:
            return dict(result, reason='duplicate native run; elected owner retains execution')

        def current_attempt():
            row = self.adapter.api.get(f'{self.adapter.root}/actions/runs/{elected["run_id"]}')
            if (not self.adapter.matches(row, plan, run_id=elected['run_id'])
                    or row['run_attempt'] != elected['run_attempt']
                    or row.get('status') != 'in_progress' or row.get('conclusion') is not None):
                raise Refused('native entry requires the current in-progress control attempt')

        current_attempt()
        decision = self.eligibility(plan)
        self.adapter.current(plan)
        if decision.classification != 'READY':
            return dict(result, status=decision.classification, reason=decision.reason)
        # Review/warm-proof acquisition may take time. Reelect with the same
        # complete native inventory and refuse a changed attempt before capture.
        if rehearse_event(self.adapter.api, event, self.environment) != elected:
            raise Refused('native election changed during owner admission')
        current_attempt()
        context = self.capture(central_capture)
        if rehearse_event(self.adapter.api, event, self.environment) != elected:
            raise Refused('native election changed during context capture')
        self.adapter.current(plan)
        current_attempt()
        return dict(result, status='READY_FOR_ALLOCATION', context=context,
                    reason='native election and current owner admission joined; allocator still required')

    def export(self, *, expected_process_status=None, command_receipt=None):
        from .export_rust_small_receipt import export
        return export(self.environment, acquire_plan=self.acquire,
                      expected_process_status=expected_process_status, command_receipt=command_receipt)

    def execute_proof(self, proof_owner, *, command_receipt=None):
        """Invoke the bound canonical producer after the existing allocation gate.

        proof_owner(environment, *, acquire_plan) is trusted control code bound
        to the existing proof entry point. This seam neither allocates a runner
        nor grants admission. It preserves the producer's actual process status;
        export metadata alone never certifies canonical product semantics.
        """
        if not callable(proof_owner):
            raise Refused('canonical producer callback missing')
        plan = self.acquire()
        context_path = Path(self.environment['RUNNER_TEMP']) / 'em-ci-job-context.json'
        before, _ = regular_bytes(context_path, 16384)
        context = json.loads(before, object_pairs_hook=unique_object)
        product_subject(self.environment, context, self.acquire)
        metadata_tokens = {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN'}
        proof_environment = {key: value for key, value in self.environment.items()
                             if key not in metadata_tokens and not key.startswith('GIT_')}
        status = proof_owner(proof_environment, acquire_plan=self.acquire)
        if type(status) is not int or not 0 <= status <= 255:
            raise Refused('canonical producer did not return a process exit status')
        after, _ = regular_bytes(context_path, 16384)
        if after != before or self.acquire() != plan:
            raise Refused('producer context/request changed during execution')
        product_subject(self.environment, context, self.acquire)
        manifest = self.export(expected_process_status=status, command_receipt=command_receipt)
        if (status == 0) != (manifest['result'] == 'success'):
            raise Refused('canonical producer exit differs from exported receipt outcome')
        return status, manifest

    def result_environment(self):
        from .rust_standard_result import policy_environment
        return policy_environment(self.environment, acquire_plan=self.acquire)

    def committed_proof(self, plan, selected):
        if (not isinstance(selected, admission.Selection)
                or selected.subject_key != plan['request_key']
                or selected.portfolio_sha256 != plan['portfolio_sha256']
                or selected.policy_sha256 != plan['policy_sha256']
                or not selected.reference or not isinstance(selected.selected, tuple)
                or not selected.selected or selected.inapplicable_reason
                or any(not isinstance(command, str) or not command for command in selected.selected)
                or len(set(selected.selected)) != len(selected.selected)):
            raise Refused('canonical import requires selected owner subject')
        if self.acquire() != plan:
            raise Refused('requested subject changed before native import')
        root = self.native_import(acquire_plan=self.acquire)
        if not isinstance(root, Path) or not root.is_absolute():
            raise Refused('central native importer omitted its private packet')
        context_raw, _ = regular_bytes(root / 'em-ci-job-context.json', 16384)
        context = json.loads(context_raw, object_pairs_hook=unique_object)
        if any(context.get(field) != self.environment.get(native) for field, native in
               (('workflow', 'GITHUB_WORKFLOW'), ('workflow_ref', 'GITHUB_WORKFLOW_REF'),
                ('event', 'GITHUB_EVENT_NAME'), ('ref', 'GITHUB_REF'))):
            raise Refused('imported producer differs from native dispatcher workflow/event/ref')
        product_subject(self.environment, context, self.acquire)
        receipt_path = root / 'rust-small-proof.json'
        raw, _ = regular_bytes(receipt_path, MAX_RECEIPT_BYTES)
        digest = hashlib.sha256(raw).hexdigest()
        check = context.get('producer_check_run_id')
        if type(check) is not int or check <= 0:
            raise Refused('canonical imported producer identity missing')
        reference = (f'https://github.com/{plan["repository"]}/actions/runs/'
                     f'{context["run_id"]}/attempts/{context["run_attempt"]}#job-{check}')
        proof = self.canonical_owner(receipt_path, plan, selected)
        if (not isinstance(proof, admission.CommittedProof)
                or proof.subject_key != plan['request_key']
                or proof.canonical_receipt_sha256 != digest
                or proof.producer_reference != reference):
            raise Refused('canonical owner omitted exact imported bytes/producer association')
        # A verifier may return zero for coherent failure diagnostics. Preserve
        # its typed disposition; never manufacture success from an exit code.
        if proof.disposition not in {'verified', 'failure', 'untrusted', 'unavailable'}:
            raise Refused('canonical disposition unsupported')
        if proof.disposition in {'verified', 'failure'}:
            if (not isinstance(proof.executed, tuple) or not isinstance(proof.failed, tuple)
                    or proof.executed != selected.selected[:len(proof.executed)]
                    or len(set(proof.executed)) != len(proof.executed)
                    or len(set(proof.failed)) != len(proof.failed)
                    or any(command not in proof.executed for command in proof.failed)):
                raise Refused('canonical execution differs from selected owner commands')
            if proof.disposition == 'verified' and (proof.executed != selected.selected or proof.failed):
                raise Refused('canonical success lacks complete selected execution')
            if proof.disposition == 'failure' and not proof.failed:
                raise Refused('canonical product failure lacks failed execution')
        again, _ = regular_bytes(receipt_path, MAX_RECEIPT_BYTES)
        context_again, _ = regular_bytes(root / 'em-ci-job-context.json', 16384)
        if again != raw or context_again != context_raw or self.acquire() != plan:
            raise Refused('imported proof/context/subject changed during canonical verification')
        return proof

    def entry_committed_proof(self, plan, selected):
        """Entry consumes existing warm proof, never this requested run's import."""
        if self.entry_committed_owner is None:
            return admission.CommittedProof(plan['request_key'], 'unavailable', (), (), '', '',
                                            'distinct existing warm proof owner unavailable')
        if self.entry_committed_owner == self.committed_proof:
            raise Refused('entry proof cannot depend on current-run qualification import')
        return self.entry_committed_owner(plan, selected)

    def eligibility(self, plan):
        """Concrete callable for Adapter(..., eligibility=owners.eligibility)."""
        result = admission.production(plan, selection_owner=self.selection_owner,
            committed_owner=self.entry_committed_proof, applicability_owner=self.applicability_owner)
        return Eligibility(result['classification'], result.get('reason', 'existing owner join'))

    def verified_qualification(self):
        """Verify the current run once; share its typed observation with consumers."""
        plan = self.acquire()
        selected = self.selection_owner(plan)
        proof = self.committed_proof(plan, selected)
        if (proof.disposition not in {'verified', 'failure'}
                or not isinstance(proof.executed, tuple) or not isinstance(proof.failed, tuple)):
            raise Refused('canonical owner has no publishable execution')
        return publication.VerifiedQualification(
            **{name: plan[name] for name in ('request_key', 'head', 'tree', 'base', 'control',
                                           'portfolio_sha256', 'policy_sha256')},
            run_id=int(self.environment['GITHUB_RUN_ID']),
            run_attempt=int(self.environment['GITHUB_RUN_ATTEMPT']),
            selected=len(selected.selected), executed=len(proof.executed), failed=len(proof.failed),
            result='success' if proof.disposition == 'verified' else 'product_failure',
            canonical_receipt_sha256=proof.canonical_receipt_sha256,
            authenticated_producer_reference=proof.producer_reference)

    def prepare_publication(self, run_id, run_attempt):
        """Prepare only; publisher permission and mutation remain uninstalled."""
        return publication.prepare(self.adapter, run_id, run_attempt, self.verified_qualification())
