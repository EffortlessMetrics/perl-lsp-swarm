"""Trusted-control composition of existing qualification owners; no activation.

Native workflow installation supplies the authenticated central importer and
actual admitted allocation lifetime. No request JSON, environment module name,
fixture, or local receipt constructs those authorities. The producer remains the
existing installed xtask, and Git worktree operations execute no candidate hook.
"""
import datetime
import json
import os
from pathlib import Path
import subprocess

from .export_rust_small_receipt import regular_bytes
from .qualification_native_adapters import NativeEntryOwners, allocate_and_execute
from .qualification_runtime import RustSmallOwners
from .request_qualification import Refused, decode
from .rust_small_subject import product_subject

SOURCE_ROOT = Path(__file__).resolve().parents[2]


def checkout_candidate(adapter, environment, *, control_root, workspace):
    """Materialize H using native Git; retain checkout for proof/recovery.

    This is the CI detached checkout seam, not a developer worktree/provisioning
    flow. Git hooks/fsmonitor and configured filters are disabled so checkout cannot start candidate
    product work before allocation. An absent H object is fetched by exact SHA without moving a branch or deleting a worktree.
    """
    if (not isinstance(control_root, Path) or not isinstance(workspace, Path)
            or not control_root.is_absolute() or not workspace.is_absolute()
            or control_root.resolve() != SOURCE_ROOT
            or workspace.resolve().is_relative_to(control_root.resolve())):
        raise Refused('native factory needs its own control S and separate absolute H checkout')
    plan = adapter.snapshot()
    if environment.get('GITHUB_SHA') != plan['control']:
        raise Refused('native factory control differs from current plan')
    child = {key:value for key,value in environment.items() if not key.startswith('GIT_')
        and key not in {'GH_TOKEN','GITHUB_TOKEN','GH_ENTERPRISE_TOKEN','GITHUB_ENTERPRISE_TOKEN'}}
    def git(root,*args,missing=False):
        base=['git','--no-pager','-c','core.fsmonitor=false','-c','core.hooksPath=/dev/null','-C',str(root)]
        # includeIf may select different config for H. Inspect its own config
        # after --no-checkout, then override every executable filter in that
        # invocation; no global/local setting is changed.
        filters=subprocess.run([*base,'config','--null','--name-only','--get-regexp',
            r'^filter\..*\.(smudge|process|clean)$'],env=child,capture_output=True,text=True,timeout=10,check=False)
        if filters.returncode not in (0,1) or len(filters.stdout)>65536:
            raise Refused('native Git filter configuration unavailable')
        overrides=[]
        for prefix in sorted({key.rsplit('.',1)[0] for key in filters.stdout.split('\0') if key}):
            for field,value in (('smudge',''),('process',''),('clean',''),('required','false')):
                overrides.extend(['-c',prefix+'.'+field+'='+value])
        process=subprocess.run([*base[:1],*overrides,*base[1:],*args],env=child,
            capture_output=True,text=True,timeout=60,check=False)
        if process.returncode and missing:
            return None
        if process.returncode or len(process.stdout)>1024**2:
            raise Refused('native detached checkout or identity observation failed')
        return process.stdout.strip()
    if (git(control_root,'rev-parse','HEAD') != plan['control']
            or git(control_root,'status','--porcelain','--untracked-files=all')):
        raise Refused('native factory requires exact clean loaded control S')
    for parent in (workspace,*workspace.parents):
        if parent.is_symlink():
            raise Refused('native candidate checkout path is linked')
    # Existing ci-scope owns triple-dot/merge-base planning. A shallow S
    # boundary can hide the common ancestor even when H/B objects are present.
    if git(control_root,'rev-parse','--is-shallow-repository')=='true':
        git(control_root,'-c','fetch.recurseSubmodules=false','-c','maintenance.auto=false','-c','gc.auto=0',
            'fetch','--unshallow','--no-tags','--no-write-fetch-head','origin')
    for commit in dict.fromkeys((plan['head'],plan['base'])):
        if git(control_root,'cat-file','-e',commit+'^{commit}',missing=True) is None:
            git(control_root,'-c','fetch.recurseSubmodules=false','-c','maintenance.auto=false','-c','gc.auto=0',
                'fetch','--no-tags','--no-write-fetch-head','origin',commit)
    if not workspace.exists():
        git(control_root,'worktree','add','--detach','--no-checkout',str(workspace),plan['head'])
        git(workspace,'checkout','--detach',plan['head'])
    if (Path(git(workspace,'rev-parse','--show-toplevel')).resolve()!=workspace.resolve()
            or git(workspace,'rev-parse','HEAD')!=plan['head']
            or git(workspace,'rev-parse','HEAD^{tree}')!=plan['tree']
            or git(workspace,'status','--porcelain','--untracked-files=all')):
        raise Refused('native candidate checkout is not exact clean H/tree')
    adapter.current(plan)
    return workspace


def compose_native(adapter, environment, *, canonical_owner, control_root,
                   native_warm_import, warm_run_id, warm_run_attempt, native_import):
    """Bind actual owners once, with no alternate eligibility implementation."""
    checkout_candidate(adapter,environment,control_root=control_root,workspace=canonical_owner.workspace)
    canonical_owner.executable_identity()
    environment=dict(environment,GITHUB_WORKSPACE=str(canonical_owner.workspace))
    entry=NativeEntryOwners(adapter,environment,canonical_owner=canonical_owner,
        control_root=control_root,native_warm_import=native_warm_import,
        warm_run_id=warm_run_id,warm_run_attempt=warm_run_attempt)
    owners=RustSmallOwners(adapter,environment,selection_owner=entry.selection,
        applicability_owner=entry.applicability,entry_committed_owner=entry.entry_committed,
        native_import=native_import,canonical_owner=canonical_owner)
    adapter.eligibility=owners.eligibility
    return owners


def execute_native(owners,event,central_capture,*,allocation_owner,log_path,
                   command_output=None,command_timeout_secs=None):
    """Use actual entry/election, central capture, allocation and product owners.

    allocation_owner is the existing native lifetime, not a receipt claiming a
    lease. command_output is optional: qualification can reuse its distinct warm
    command proof without rerunning it. Supplying it executes the actual local
    selection first; any failure stops before Rust qualification/export rather
    than pretending that a successful Rust receipt proves failed local work.
    Native producer completion/import remains in the later policy job.
    """
    canonical=owners.canonical_owner
    if not isinstance(log_path,Path) or not log_path.is_absolute() or log_path.exists():
        raise Refused('native producer needs a fresh absolute log path')
    environment=owners.environment
    environment['EM_CI_ARTIFACT_LANE']='perl-rust-small'
    context_path=Path(environment['RUNNER_TEMP'])/'em-ci-job-context.json'
    def capture(native,*,acquire_plan):
        context=central_capture(native,acquire_plan=acquire_plan)
        # The central native owner is the source of this context. The factory
        # persists its result before starting any H command.
        # A native context step may already have recorded this job. Use the
        # same owner write semantics; never follow a replacement link.
        descriptor=os.open(context_path,os.O_WRONLY|os.O_CREAT|os.O_TRUNC|os.O_NOFOLLOW,0o600)
        with os.fdopen(descriptor,'w') as stream:
            json.dump(context,stream)
        return context
    def begin(native,*,acquire_plan):
        raw,_=regular_bytes(context_path,16384)
        context=decode(raw)
        product_subject(native,context,acquire_plan)
        context['proof_started_at']=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        context_path.write_text(json.dumps(context))
        if native.get('GITHUB_OUTPUT'):
            with Path(native['GITHUB_OUTPUT']).open('a') as stream:
                stream.write('proof_entered_at='+context['proof_started_at']+'\n')
    def produce(native,*,acquire_plan):
        if command_output is not None:
            from .qualification_local_commands import produce as local_produce
            status,_=local_produce(canonical,native,acquire_plan=acquire_plan,
                                  output=command_output,command_timeout_secs=command_timeout_secs)
            if status:
                raise Refused('actual selected local command failed; Rust qualification did not run')
        identity=canonical.executable_identity()
        child={key:value for key,value in native.items() if key not in {'PYTHONPATH','PYTHONHOME'}}
        child.update(RUSTUP_TOOLCHAIN='1.95.0',RUSTUP_AUTO_INSTALL='0')
        with log_path.open('xb') as log:
            result=subprocess.run([str(canonical.verifier),'--worktree-root',str(canonical.workspace),
                'rust-small-proof'],cwd=canonical.workspace,env=child,stdout=log,
                stderr=subprocess.STDOUT,check=False)
        if canonical.executable_identity()!=identity:
            raise Refused('native installed producer changed during execution')
        return result.returncode
    command_receipt=command_output/'command-proof-set.json' if command_output is not None else None
    return allocate_and_execute(owners,event,capture,allocation_owner=allocation_owner,
        proof_owner=produce,command_receipt=command_receipt,before_execute=begin)
