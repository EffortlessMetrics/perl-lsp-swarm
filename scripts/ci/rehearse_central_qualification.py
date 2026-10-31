"""Local central/Perl composition rehearsal; never native/GitHub qualification.

The default mode retains synthetic canonical semantics. Actual mode consumes a
separate existing warm receipt and invokes an operator-installed canonical tool
only after the fixture entry join. Resource admission belongs to the operator.
No allocator, hosted dispatch, H-check publisher or production workflow is added.
"""
import argparse
from dataclasses import asdict
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
from unittest import mock

from scripts.ci import candidate_readiness as admission
from scripts.ci import qualification_completion as completion
from scripts.ci.export_rust_small_receipt import MAX_RECEIPT_BYTES, regular_bytes
from scripts.ci.qualification_runtime import RustSmallOwners
from scripts.ci.request_qualification import Adapter, Refused
from scripts.ci.rust_small_canonical_owner import CanonicalOwner
from scripts.ci.rust_small_evidence import unique_object


def load_central(path):
    spec = importlib.util.spec_from_file_location('central_rehearsal',
        path / 'tests/rust-perl-artifact-contract.py')
    central = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(central)
    return central


def run_fixture(central):
    from scripts.ci.test_qualification_runtime import RuntimeCompositionTests
    t=RuntimeCompositionTests(); t.setUp()
    try:
     with tempfile.TemporaryDirectory() as directory:
      helper=central.QualificationSubjectHooks(); f=helper.fixture(directory)
      # Real request adapter fixture owns PR/H/B; native API envelope owns S.
      f.run['repository']['id']=f.run['head_repository']['id']=7
      f.artifact['workflow_run'].update(repository_id=7,head_repository_id=7)
      for name in ('subject','evidence','run','producer','policy','check','artifact','context'):
       value=getattr(f,name)
       value=json.loads(json.dumps(value).replace('a'*40,t.plan['control']))
       setattr(f,name,value)
      f.plan=t.plan
      for source in (f.subject, f.context):
       source.update(workflow=t.owners.environment['GITHUB_WORKFLOW'],
          workflow_ref=t.owners.environment['GITHUB_WORKFLOW_REF'], event='workflow_dispatch',
          ref=t.owners.environment['GITHUB_REF'])
      f.context['qualification_subject']={k:t.plan[k] for k in ('head','tree','base','control','request_key')}
      f.evidence['sha']=t.plan['head']
      receipt=json.loads(f.files['rust-small-proof.json']); receipt['subject']['git_sha']=t.plan['head']
      raw=json.dumps(receipt).encode(); f.files['rust-small-proof.json']=raw
      manifest=json.loads(f.files['perl-rust-small-export.json']); manifest['sha']=t.plan['control']
      manifest['member'].update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
      f.files['perl-rust-small-export.json']=json.dumps(manifest).encode();f.write_zip()
      env=helper.environment(f,GITHUB_REPOSITORY_ID='7')
      captured=central.QualificationSubjectHooks.CAPTURE['capture_context'](env,acquire_plan=t.owners.acquire)
      assert captured['qualification_subject']==f.context['qualification_subject']
      def imported(*,acquire_plan):
       root=helper.imported(f,acquire_plan)
       helper.custody(f,root,acquire_plan)
       return root
      t.owners.native_import=imported
      def semantic(receipt,plan,selection):
       # Semantic product verification remains synthetic; real canonical verifier not run.
       from scripts.ci.candidate_readiness import CommittedProof
       return CommittedProof(plan['request_key'],'verified',selection.selected,(),
          hashlib.sha256(receipt.read_bytes()).hexdigest(),
          f'https://github.com/{plan["repository"]}/actions/runs/123/attempts/2#job-201')
      t.owners.canonical_owner=semantic
      result=t.owners.prepare_publication(123,2)
      assert result['status']=='PREPARED_NOT_PUBLISHED' and result['payload']['head_sha']==t.plan['head']
      assert all(method=='GET' for method,_ in t.api.calls)
      f.check['head_sha']=t.plan['head']
      try:t.owners.prepare_publication(123,2)
      except ValueError:pass
      else:raise AssertionError('native check relabelled H accepted')
      print('PASS actual central capture/import/custody -> actual H/S runtime -> publication preparation; native H relabel refused; semantic callback synthetic; no publication')
    finally:t.doCleanups()


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def git(workspace, *args):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith('GIT_') and key not in
                   {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN'}}
    process = subprocess.run(['git', '-c', 'core.fsmonitor=false', '-C', str(workspace), *args],
        env=environment, capture_output=True, text=True, check=True, timeout=10)
    return process.stdout.strip()


class FixtureEntryAuthorities(admission.ExistingControlAuthorities):
    """Explicit bounded review fixtures, never production review/readiness facts."""
    def semantic_review(self, plan):
        return admission.Review(plan['request_key'], 'REVIEW_CURRENT', 'fixture:semantic-review', 'local rehearsal only')

    def native_review(self, plan):
        return admission.Review(plan['request_key'], 'CONVERGED', 'fixture:native-review', 'local rehearsal only')


def run_actual(central, *, xtask, xtask_sha256, control_sha, workspace, warm_receipt, output):
    """Execute qualification after distinct canonical warm proof; native facts are fixtures."""
    from scripts.ci.test_qualification_runtime import NativeAPI
    for path in (xtask, workspace, warm_receipt, output):
        if not isinstance(path, Path) or not path.is_absolute():
            raise Refused('actual rehearsal requires absolute operator paths')
    if output.resolve().is_relative_to(workspace.resolve()):
        raise Refused('rehearsal output must remain outside the evaluated checkout')
    if git(workspace, 'status', '--porcelain', '--untracked-files=all'):
        raise Refused('actual rehearsal requires a clean H checkout')
    observed = dict(repository='EffortlessMetrics/perl-lsp-swarm',
        head=git(workspace, 'rev-parse', 'HEAD'), tree=git(workspace, 'rev-parse', 'HEAD^{tree}'),
        base=git(workspace, 'rev-parse', 'HEAD^'), control=control_sha)
    verifier = CanonicalOwner(verifier=xtask, verifier_sha256=xtask_sha256,
                              control_sha=control_sha, workspace=workspace)
    tool_identity = verifier.executable_identity()
    warm_bytes, _ = regular_bytes(warm_receipt, MAX_RECEIPT_BYTES)
    output.mkdir(mode=0o700)
    warm_root = output / 'warm'
    warm_root.mkdir(mode=0o700)
    warm_path = warm_root / 'rust-small-proof.json'
    warm_path.write_bytes(warm_bytes)
    # This is a provisional projection, used only after the Rust owner verifies
    # the warm bytes against this exact clean H and its canonical vocabulary.
    warm_document = json.loads(warm_bytes, object_pairs_hook=unique_object)
    names = tuple(row['name'] for row in warm_document['steps'])
    api = NativeAPI(SimpleNamespace(plan=observed))
    adapter = Adapter(api, api.subject)
    plan = adapter.snapshot()
    api.row = dict(id=123, run_attempt=2, event='workflow_dispatch', repository={'id': 7},
        workflow_id=9, path=plan['workflow_path'], head_sha=control_sha,
        display_title='qualification:' + plan['request_key'], status='in_progress', conclusion=None)
    selected = admission.Selection(plan['request_key'], plan['portfolio_sha256'], plan['policy_sha256'],
        names, ('Rust Small result',), 'fixture:selection-projected-from-canonical-warm-receipt')
    with tempfile.TemporaryDirectory(prefix='qualification-native-fixture-') as directory:
        helper = central.QualificationSubjectHooks()
        fixture = helper.fixture(directory)
        temporary = Path(directory) / 'producer'
        temporary.mkdir(mode=0o700)
        packet = temporary / 'packet'
        packet.mkdir(mode=0o700)
        environment = dict(os.environ, **helper.environment(fixture))
        environment.update(GITHUB_REPOSITORY=plan['repository'], GITHUB_REPOSITORY_ID='7',
            GITHUB_SHA=control_sha, GITHUB_WORKSPACE=str(workspace), RUNNER_TEMP=str(temporary),
            EM_CI_ARTIFACT_DIR=str(packet), EM_CI_LANE='perl-rust-small',
            GITHUB_WORKFLOW='Qualification Adapter Rehearsal',
            GITHUB_WORKFLOW_REF=(f'{plan["repository"]}/{plan["workflow_path"]}'
                                 f'@refs/heads/{plan["control_branch"]}'),
            GITHUB_REF='refs/heads/' + plan['control_branch'])
        fixture.plan = plan
        fixture.subject.update(sha=control_sha, event='workflow_dispatch',
            workflow=environment['GITHUB_WORKFLOW'], workflow_ref=environment['GITHUB_WORKFLOW_REF'],
            ref=environment['GITHUB_REF'])
        fixture.run.update(event='workflow_dispatch', head_sha=control_sha)
        fixture.run['repository']['id'] = fixture.run['head_repository']['id'] = 7
        fixture.producer['head_sha'] = fixture.check['head_sha'] = control_sha
        fixture.artifact['workflow_run'].update(head_sha=control_sha, repository_id=7, head_repository_id=7)
        fixture.evidence['sha'] = plan['head']

        def imported(*, acquire_plan):
            root = helper.imported(fixture, acquire_plan)
            helper.custody(fixture, root, acquire_plan)
            return root

        def warm_committed(acquired, selection):
            # A distinct existing warm packet, never the current-run importer.
            return verifier(warm_path, acquired, selection)

        owners = RustSmallOwners(adapter, environment, selection_owner=lambda acquired: selected,
            native_import=imported, canonical_owner=verifier, applicability_owner=lambda acquired: (),
            entry_committed_owner=warm_committed)
        warm_environment = dict(environment, GITHUB_RUN_ID='122', GITHUB_RUN_ATTEMPT='1')
        capture = central.QualificationSubjectHooks.CAPTURE['capture_context']
        warm_context = capture(warm_environment, acquire_plan=owners.acquire)
        warm_context['producer_check_run_id'] = 200
        (warm_root / 'em-ci-job-context.json').write_text(json.dumps(warm_context))
        authorities = FixtureEntryAuthorities(Path(__file__).resolve().parents[2],
            selection_owner=owners.selection_owner, committed_owner=owners.entry_committed_proof,
            applicability_owner=owners.applicability_owner)
        entry = admission.evaluate(plan, authorities)
        entry['acquisition_mode'] = 'local-fixture-reviews-and-native-api'
        adapter.eligibility = owners.eligibility
        # Only review acquisition is fixture-injected. The existing production
        # join still checks its real clean control checkout S, calls the warm
        # proof port and lets Adapter.explain reacquire the current subject.
        with mock.patch.object(admission.ExistingControlAuthorities, 'semantic_review',
                               FixtureEntryAuthorities.semantic_review), \
             mock.patch.object(admission.ExistingControlAuthorities, 'native_review',
                               FixtureEntryAuthorities.native_review):
            explained = adapter.explain()
        entry['adapter_explain'] = explained
        (output / 'entry.json').write_text(json.dumps(entry, indent=2) + '\n')
        if (entry['classification'] != 'READY' or explained['status'] != 'READY'
                or explained['subject'] != plan):
            raise Refused('canonical warm proof or clean-control adapter boundary did not satisfy fixture entry admission')
        # Entry success is a local fixture join, not authority to allocate or
        # publish. The operator separately admitted this local fixed execution.
        context = owners.capture(capture)
        context['proof_started_at'] = stamp()
        context_path = temporary / 'em-ci-job-context.json'
        context_path.write_text(json.dumps(context))

        def producer(child_environment, *, acquire_plan):
            if acquire_plan() != plan or verifier.executable_identity() != tool_identity:
                raise Refused('canonical producer tool/request identity moved')
            child_environment = {key: value for key, value in child_environment.items()
                                 if key not in {'PYTHONPATH', 'PYTHONHOME'}}
            with (output / 'qualification.log').open('wb') as log:
                process = subprocess.run([str(xtask), '--worktree-root', str(workspace), 'rust-small-proof'], cwd=workspace,
                    env=child_environment, stdout=log, stderr=subprocess.STDOUT, check=False)
            if verifier.executable_identity() != tool_identity:
                raise Refused('canonical producer executable changed during execution')
            return process.returncode

        status, exported = owners.execute_proof(producer)
        # Complete the synthetic native API envelope around actual executed
        # bytes. These timestamps/IDs are fixtures, not authenticated GitHub facts.
        finished = stamp()
        context.update(job_status='success' if status == 0 else 'failure',
            proof_finished_at=finished, producer_check_run_id=201)
        fixture.context.update(context, artifact_name=fixture.name)
        fixture.evidence.update(proof_entered_at=context['proof_started_at'],
            result='success' if status == 0 else 'failure', proof_outcome='success' if status == 0 else 'failure')
        fixture.needs['standard']['outputs'].update(proof_entered_at=context['proof_started_at'])
        fixture.producer.update(started_at=context['admission_recorded_at'], completed_at=finished,
            conclusion='success' if status == 0 else 'failure')
        fixture.producer['steps'][0].update(started_at=context['proof_started_at'], completed_at=finished,
            conclusion='success' if status == 0 else 'failure')
        fixture.artifact.update(created_at=finished, updated_at=finished)
        fixture.files = {name: (packet / name).read_bytes()
            for name in ('rust-small-proof.json', 'perl-rust-small-export.json')}
        fixture.write_zip()
        for name, raw in fixture.files.items():
            (output / name).write_bytes(raw)
        if status:
            # The current central successful-import interface refuses failed
            # producers. Preserve actual failure and stop before success projection.
            report = {'mode': 'actual-local', 'native_api_facts': 'fixture',
                'producer_exit_status': status, 'qualification_result': 'NOT_PROVEN',
                'reason': 'producer failed; successful central import unavailable'}
        else:
            verified = owners.verified_qualification()
            manifest = completion.build(adapter, verified)
            (output / 'qualification-result.json').write_text(json.dumps(manifest, indent=2) + '\n')
            report = {'mode': 'actual-local', 'native_api_facts': 'fixture',
                'review_facts': 'fixture', 'canonical_semantics': 'actual-rust-owner',
                'producer_exit_status': status, 'verified_qualification': asdict(verified),
                'qualification_result': 'NOT_PROVEN', 'check_run_id': manifest['check_run_id'],
                'reason': manifest['reason']}
        report.update(control_sha=control_sha, evaluated_sha=plan['head'],
            canonical_verifier_sha256=xtask_sha256, warm_receipt_sha256=hashlib.sha256(warm_bytes).hexdigest())
        if any(method != 'GET' for method, _ in api.calls):
            raise Refused('local rehearsal attempted metadata mutation')
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('central', type=Path)
    parser.add_argument('--actual', action='store_true')
    parser.add_argument('--xtask', type=Path)
    parser.add_argument('--xtask-sha256')
    parser.add_argument('--control-sha')
    parser.add_argument('--workspace', type=Path)
    parser.add_argument('--warm-receipt', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args(argv)
    central = load_central(args.central.resolve())
    if args.actual:
        if any(getattr(args, key) is None for key in
               ('xtask', 'xtask_sha256', 'control_sha', 'workspace', 'warm_receipt', 'output')):
            parser.error('--actual requires --xtask, --xtask-sha256, --control-sha, --workspace, --warm-receipt and --output')
        report = run_actual(central, xtask=args.xtask, xtask_sha256=args.xtask_sha256,
            control_sha=args.control_sha, workspace=args.workspace,
            warm_receipt=args.warm_receipt, output=args.output)
        print(json.dumps(report, indent=2))
        return report['producer_exit_status']
    run_fixture(central)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
