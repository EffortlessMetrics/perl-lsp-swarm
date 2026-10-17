"""Inactive native Rust Small entry adapters; no policy clone or allocation grant.

The central authenticated importer and desktop allocator remain trusted owners.
Import is explicitly addressed to a distinct completed warm run. Planning uses
existing xtask commands; full Rust Small selection comes only from Rust-verified
receipt observations, never from the bounded pre-push command subset.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

from .candidate_readiness import CommittedProof, IntegrationFinding, Selection
from .export_rust_small_receipt import MAX_RECEIPT_BYTES, regular_bytes
from .request_qualification import Refused, decode, positive
from .rust_small_canonical_owner import custody


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def invoke(owner, *arguments):
    """Use trusted installed xtask, explicitly selecting its runtime H root."""
    identity = owner.executable_identity()
    environment = {key: value for key, value in os.environ.items()
        if not key.startswith('GIT_') and key not in {'GH_TOKEN', 'GITHUB_TOKEN',
            'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN', 'PYTHONPATH', 'PYTHONHOME'}}
    environment.update(RUSTUP_TOOLCHAIN='1.95.0', RUSTUP_AUTO_INSTALL='0', CARGO_NET_OFFLINE='true')
    result = subprocess.run([str(owner.verifier), '--worktree-root', str(owner.workspace), *arguments],
        cwd=owner.workspace, env=environment, capture_output=True, timeout=60, check=False)
    if owner.executable_identity() != identity or result.returncode != 0:
        raise Refused('native owner command refused or installed tool changed')
    return result.stdout


class NativeEntryOwners:
    def __init__(self, adapter, environment, *, canonical_owner, control_root,
                 native_warm_import, warm_run_id, warm_run_attempt):
        if (not callable(native_warm_import) or not isinstance(control_root, Path)
                or not control_root.is_absolute() or not positive(warm_run_id)
                or not positive(warm_run_attempt)):
            raise Refused('native warm importer and exact prior attempt required')
        self.adapter, self.environment = adapter, dict(environment)
        self.canonical = canonical_owner
        self.control_root, self.native_warm_import = control_root, native_warm_import
        self.warm_run_id, self.warm_run_attempt = warm_run_id, warm_run_attempt

    def current(self, plan):
        if self.canonical.control_sha != plan['control']:
            raise Refused('native entry tool differs from control S')
        self.adapter.current(plan)
        return plan

    def required(self, plan):
        raw, _ = regular_bytes(self.control_root / '.ci/policies/required-checks.toml', MAX_RECEIPT_BYTES)
        if hashlib.sha256(raw).hexdigest() != plan['policy_sha256']:
            raise Refused('native entry policy bytes differ from requested policy')
        rows = tomllib.loads(raw.decode())['checks']
        required = {row['name']: row['ruleset_integration_id'] for row in rows if row.get('required') is True}
        if not required or any(not isinstance(name, str) or not positive(app) for name, app in required.items()):
            raise Refused('native required context/app inventory incomplete')
        return required

    def warm(self, plan, *, with_packet=False):
        self.current(plan)
        if str(self.warm_run_id) == self.environment.get('GITHUB_RUN_ID'):
            raise Refused('entry proof cannot use its own native run, including earlier attempts')
        # Installation binds this call to the existing central authenticated
        # importer for this prior run/attempt. A path alone is not provenance.
        root = self.native_warm_import(self.warm_run_id, self.warm_run_attempt,
                                      acquire_plan=lambda: self.current(plan))
        if not isinstance(root, Path) or not root.is_absolute():
            raise Refused('native warm importer omitted authenticated private packet')
        context_path, receipt_path = root / 'em-ci-job-context.json', root / 'rust-small-proof.json'
        context_raw, context_stat = regular_bytes(context_path, 16384)
        raw, receipt_stat = regular_bytes(receipt_path, MAX_RECEIPT_BYTES)
        context = decode(context_raw)
        run_path = f'{self.adapter.root}/actions/runs/{self.warm_run_id}'
        run = self.adapter.api.get(run_path)
        check_id = context.get('producer_check_run_id')
        if (context.get('schema') != 'em-ci-job-context.v1' or context.get('profile') != 'standard'
                or context.get('repository') != plan['repository']
                or context.get('run_id') != str(self.warm_run_id)
                or context.get('run_attempt') != str(self.warm_run_attempt) or not positive(check_id)
                or run.get('id') != self.warm_run_id or run.get('run_attempt') != self.warm_run_attempt
                or run.get('repository', {}).get('id') != plan['repository_id']
                or run.get('head_sha') != context.get('sha')
                or run.get('status') != 'completed' or run.get('conclusion') != 'success'
                or context.get('sha') not in {plan['head'], plan['control']}):
            raise Refused('warm packet is not the distinct completed native producer')
        if context['sha'] == plan['control']:
            captured = {key: plan[key] for key in ('head', 'tree', 'base', 'control', 'request_key')}
            if context.get('qualification_subject') != captured:
                raise Refused('warm control-S packet lacks exact captured H subject')
        check = self.adapter.api.get(f'{self.adapter.root}/check-runs/{check_id}')
        if (check.get('id') != check_id or check.get('head_sha') != context['sha']
                or check.get('app', {}).get('id') != plan['app_id']
                or check.get('status') != 'completed' or check.get('conclusion') != 'success'):
            raise Refused('warm producer native check association differs')
        jobs = self.adapter.api.get(f'{self.adapter.root}/actions/runs/{self.warm_run_id}/attempts/{self.warm_run_attempt}/jobs?per_page=100')
        rows = jobs.get('jobs')
        check_url = f'https://api.github.com/{self.adapter.root}/check-runs/{check_id}'
        if (not isinstance(rows, list) or jobs.get('total_count') != len(rows) or len(rows) >= 100
                or len([job for job in rows if job.get('check_run_url') == check_url
                        and job.get('head_sha') == context['sha']
                        and job.get('status') == 'completed' and job.get('conclusion') == 'success']) != 1):
            raise Refused('warm check is not the selected native run/attempt job')
        invoke(self.canonical, 'rust-small-proof', '--verify-receipt', str(receipt_path))
        document = decode(raw)
        if (document.get('schema_version') != 'rust_small_proof.v1'
                or document.get('subject', {}).get('git_sha') != plan['head']
                or document.get('subject', {}).get('worktree_dirty') is not False):
            raise Refused('verified warm receipt omitted clean exact H')
        for path, before, metadata, limit in ((context_path, context_raw, context_stat, 16384),
                (receipt_path, raw, receipt_stat, MAX_RECEIPT_BYTES)):
            after, after_stat = regular_bytes(path, limit)
            if after != before or custody(after_stat) != custody(metadata):
                raise Refused('warm packet changed during owner verification')
        if (self.adapter.api.get(run_path) != run
                or self.adapter.api.get(f'{self.adapter.root}/check-runs/{check_id}') != check):
            raise Refused('warm native run/attempt changed during verification')
        self.current(plan)
        reference = (f'https://github.com/{plan["repository"]}/actions/runs/{self.warm_run_id}'
                     f'/attempts/{self.warm_run_attempt}#job-{check_id}')
        result = document, hashlib.sha256(raw).hexdigest(), reference
        return (*result, root) if with_packet else result

    def portfolio(self, plan):
        """Return actual owner outputs/digest for Subject creation or validation.

        Portfolio digest excludes request_key (which itself includes this digest).
        It retains deferred commands: local bounded proof never satisfies them.
        """
        receipt, _, reference, packet = self.warm(plan, with_packet=True)
        prepush = decode(invoke(self.canonical, 'pre-push-plan', '--base', plan['base'],
                                '--head', plan['head'], '--format', 'json'))
        scope = decode(invoke(self.canonical, 'ci-scope', '--base', plan['base'],
                              '--root', str(self.canonical.workspace), '--format', 'json'))
        if (prepush.get('schema') != 'pre_push_proof_plan.v1' or prepush.get('posture') != 'PROCEED'
                or prepush.get('base_sha') != plan['base'] or prepush.get('head_sha') != plan['head']
                or scope.get('schema_version') != 2 or scope.get('base') != plan['base']
                or scope.get('head_sha') != plan['head']):
            raise Refused('existing planner subject/schema/posture differs')
        from .qualification_local_commands import MEMBER, bind_authenticated, selected_argv
        observed = {tuple(step['argv']) for step in receipt['steps'] if step.get('outcome') == 'ok'}
        local = None
        if any(argv not in observed for argv in selected_argv(prepush, plan)):
            if not (packet / MEMBER).exists():
                raise Refused('selected local command lacks actual canonical warm execution; native owner proof required')
            local = bind_authenticated(packet, plan, prepush)
        required = self.required(plan)
        portfolio = dict(pre_push=prepush, ci_scope=scope,
                         rust_small=[{'name': step['name'], 'argv': step['argv']} for step in receipt['steps']],
                         required_checks=required)
        if local is not None:
            portfolio['local_commands'] = dict(receipt_sha256=local.receipt_sha256,
                                                argv=[list(argv) for argv in local.argv])
        # Planning may resolve metadata. Reverify clean exact H and immutable
        # warm proof rather than trusting that planning was side-effect free.
        again, _, _, packet = self.warm(plan, with_packet=True)
        if again != receipt or (local is not None and bind_authenticated(packet, plan, prepush) != local):
            raise Refused('warm proof moved while acquiring owner portfolio')
        return portfolio, digest(portfolio), reference

    def selection(self, plan):
        portfolio, actual, reference = self.portfolio(plan)
        if actual != plan['portfolio_sha256']:
            raise Refused('requested portfolio differs from actual owner selection')
        obligations = tuple(sorted(portfolio['required_checks'])) + tuple(
            'deferred:' + row['command'] for row in portfolio['pre_push']['deferred'])
        return Selection(plan['request_key'], actual, plan['policy_sha256'],
            tuple(step['name'] for step in portfolio['rust_small']),
            tuple(dict.fromkeys(obligations)), reference)

    def entry_committed(self, plan, selected):
        # Rebind every selected local command at admission, not just the fixed
        # Rust Small receipt that supplies the committed-proof vocabulary.
        self.selection(plan)
        receipt, receipt_digest, reference = self.warm(plan)
        names = tuple(row['name'] for row in receipt['steps'])
        if (not isinstance(selected, Selection) or selected.subject_key != plan['request_key']
                or selected.selected != names or selected.portfolio_sha256 != plan['portfolio_sha256']
                or selected.policy_sha256 != plan['policy_sha256']):
            raise Refused('warm canonical proof differs from exact selected portfolio')
        executed = tuple(row['name'] for row in receipt['steps'] if row['outcome'] != 'not_run')
        failed = tuple(row['name'] for row in receipt['steps'] if row['outcome'] == 'product_failure')
        result = receipt.get('result')
        state = 'verified' if result == 'success' else 'failure' if result == 'product_failure' else 'unavailable'
        return CommittedProof(plan['request_key'], state, executed, failed, receipt_digest, reference)

    def applicability(self, plan):
        """Observe H results, retaining a known failure through pending replacement."""
        self.current(plan)
        required = self.required(plan)
        response = self.adapter.api.get(f'{self.adapter.root}/commits/{plan["head"]}/check-runs?filter=all&per_page=100')
        rows = response.get('check_runs')
        if not isinstance(rows, list) or response.get('total_count') != len(rows) or len(rows) >= 100:
            raise Refused('native H check inventory incomplete')
        groups = {}
        for row in rows:
            if (row.get('head_sha') != plan['head'] or not positive(row.get('id'))
                    or not isinstance(row.get('name'), str) or not row.get('html_url')):
                raise Refused('native H check identity unavailable')
            app = row.get('app', {}).get('id')
            if row['name'] in required and app != required[row['name']]:
                raise Refused('native required result has wrong source app')
            groups.setdefault((row['name'], app), []).append(row)
        findings = []
        for (name, app), matching in sorted(groups.items()):
            matching.sort(key=lambda row: row['id'], reverse=True)
            for row in matching:
                if (row.get('status') not in {'queued', 'in_progress', 'completed'}
                        or (row['status'] == 'completed' and row.get('conclusion') not in
                            {'success', 'failure', 'timed_out', 'cancelled', 'action_required',
                             'neutral', 'skipped', 'stale', 'startup_failure'})):
                    raise Refused('native check result state unknown')
            # Only actual success repairs an observed failure. A new pending,
            # skipped, neutral or cancelled producer cannot erase that fact.
            decisive = next((row for row in matching if row.get('status') == 'completed'
                             and row.get('conclusion') in {'success', 'failure', 'timed_out', 'startup_failure'}), None)
            if decisive and decisive['conclusion'] != 'success':
                state, reference = 'unresolved_failure', decisive['html_url']
            else:
                latest = matching[0]
                state = 'repaired' if latest.get('status') == 'completed' and latest.get('conclusion') == 'success' else 'pending'
                reference = latest['html_url']
            findings.append(IntegrationFinding(plan['request_key'], state, reference))
        for name in sorted(set(required) - {key[0] for key in groups}):
            findings.append(IntegrationFinding(plan['request_key'], 'absent',
                f'https://github.com/{plan["repository"]}/commit/{plan["head"]}'))
        legacy = decode(self.adapter.api.get_bytes(f'{self.adapter.root}/commits/{plan["head"]}/statuses?per_page=100'))
        if not isinstance(legacy, list) or len(legacy) >= 100:
            raise Refused('native legacy status history incomplete')
        history = {}
        for row in legacy:
            creator = row.get('creator', {}).get('id')
            if (row.get('state') not in {'success', 'failure', 'error', 'pending'}
                    or not row.get('context') or not row.get('url')
                    or not positive(row.get('id')) or not positive(creator)):
                raise Refused('native legacy result identity/state unknown')
            history.setdefault((row['context'], creator), []).append(row)
        for rows in history.values():
            rows.sort(key=lambda row: row['id'], reverse=True)
            decisive = next((row for row in rows if row['state'] != 'pending'), None)
            adverse = decisive and decisive['state'] in {'failure', 'error'}
            state = 'unresolved_failure' if adverse else 'repaired' if rows[0]['state'] == 'success' else 'pending'
            findings.append(IntegrationFinding(plan['request_key'], state, (decisive if adverse else rows[0])['url']))
        self.current(plan)
        return tuple(findings)


def allocate_and_execute(owners, event, capture, *, allocation_owner, proof_owner, command_receipt=None, before_execute=None):
    """Bind the existing allocator's context-manager lifetime to actual execution.

    allocation_owner(plan, selection) must acquire/release the native desktop
    lease and enforce its budgets. No boolean/receipt can substitute for it.
    No allocator implementation or resource policy is introduced here.
    """
    entered = owners.enter_native(event, capture)
    if entered['status'] != 'READY_FOR_ALLOCATION':
        return entered
    plan = owners.acquire()
    selected = owners.selection_owner(plan)
    with allocation_owner(plan, selected):
        # Queuing for the existing lease can invalidate admission/election.
        def retained_context(environment, *, acquire_plan):
            if acquire_plan() != plan:
                raise Refused('captured subject moved while awaiting allocation')
            return entered['context']
        again = owners.enter_native(event, retained_context)
        if again['status'] != 'READY_FOR_ALLOCATION' or owners.acquire() != plan:
            raise Refused('native admission moved while awaiting allocation')
        if before_execute is not None:
            before_execute(owners.environment, acquire_plan=owners.acquire)
        return (owners.execute_proof(proof_owner, command_receipt=command_receipt)
                if command_receipt is not None else owners.execute_proof(proof_owner))
