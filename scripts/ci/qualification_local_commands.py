"""Bind the existing command-evidence owner to the actual pre-push selection.

No shell executor, command selector or replacement receipt schema lives here.
Production is called only inside the existing allocation lifetime. Consumption
requires the existing authenticated native importer to supply the whole packet;
JSON or a local path alone cannot authenticate command execution.
"""
from dataclasses import dataclass
import datetime as dt
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shlex
import subprocess

from .export_rust_small_receipt import MAX_RECEIPT_BYTES, regular_bytes
from .request_qualification import Refused, decode
from .rust_small_subject import product_subject

MEMBER = 'command-proof-set.json'


def selected_argv(prepush, plan):
    if (prepush.get('schema') != 'pre_push_proof_plan.v1'
            or prepush.get('posture') != 'PROCEED'
            or prepush.get('base_sha') != plan['base'] or prepush.get('head_sha') != plan['head']
            or not isinstance(prepush.get('selected'), list) or not prepush['selected']):
        raise Refused('actual pre-push selection is missing or belongs to another subject')
    commands = []
    for row in prepush['selected']:
        if not isinstance(row, dict) or not isinstance(row.get('command'), str):
            raise Refused('pre-push command identity missing')
        try:
            lexer = shlex.shlex(row['command'], posix=True, punctuation_chars=';&|<>()')
            lexer.whitespace_split = True
            tokens = tuple(lexer)
            argv = tuple(shlex.split(row['command']))
        except ValueError as error:
            raise Refused('selected command cannot be parsed as direct argv') from error
        # command-evidence owns direct argv execution. Shell programs are not
        # silently interpreted as an equivalent direct command or dropped.
        if (not argv or argv[0] == 'cd' or '\n' in row['command'] or '\r' in row['command']
                or any(token and all(char in ';&|<>()' for char in token) for token in tokens)):
            raise Refused('selected shell command needs its existing shell proof owner')
        if argv in commands:
            raise Refused('pre-push selection contains duplicate command identities')
        commands.append(argv)
    return tuple(commands)


def produce(canonical, environment, *, acquire_plan, output, command_timeout_secs):
    """Execute real planner argv through trusted xtask's existing proof-set CLI.

    The caller owns resource admission and its absolute deadline. No check,
    artifact upload, allocation or claim of authenticated native origin is made.
    Failure JSON is preserved verbatim; only the actual owner writes outcomes.
    """
    from .qualification_native_adapters import invoke
    if (not callable(acquire_plan) or not isinstance(output, Path) or not output.is_absolute()
            or type(command_timeout_secs) is not int or command_timeout_secs <= 0):
        raise Refused('local proof needs the trusted acquirer, private output and admitted timeout')
    plan = dict(acquire_plan())
    if canonical.control_sha != plan['control']:
        raise Refused('local producer executable belongs to another control S')
    temporary = Path(environment['RUNNER_TEMP']).resolve()
    workspace = canonical.workspace.resolve()
    if Path(environment.get('GITHUB_WORKSPACE', '')).resolve() != workspace:
        raise Refused('command owner and acquired product workspace differ')
    if (output.resolve().is_relative_to(workspace) or not output.resolve().is_relative_to(temporary)
            or output.exists() or output.is_symlink()):
        raise Refused('local proof output must be fresh private runner-temporary custody')
    context_path = temporary / 'em-ci-job-context.json'
    context_raw, _ = regular_bytes(context_path, 16384)
    context = decode(context_raw)
    if context.get('product_workspace') != str(workspace):
        raise Refused('native capture differs from actual product workspace')
    product_subject(environment, context, acquire_plan)
    identity = canonical.executable_identity()
    prepush = decode(invoke(canonical, 'pre-push-plan', '--base', plan['base'],
                            '--head', plan['head'], '--format', 'json'))
    commands = selected_argv(prepush, plan)
    for ancestor in (output.parent, *output.parent.parents):
        if ancestor.is_symlink():
            raise Refused('linked local proof custody parent')
    output.mkdir(mode=0o700)
    logs = output / 'logs'
    spec = {'schema_version': 'command-proof-set.v1', 'commands': [
        dict(id=f'pre-push-{index}', program=argv[0], args=list(argv[1:]), cwd=str(workspace),
             candidate=plan['head'], timeout_secs=command_timeout_secs, out_dir=str(logs))
        for index, argv in enumerate(commands)]}
    spec_path, partial, receipt = output / 'spec.json', output / (MEMBER + '.partial'), output / MEMBER
    spec_path.write_text(json.dumps(spec))
    child_environment = {key: value for key, value in environment.items()
        if not key.startswith('GIT_') and key not in {'GH_TOKEN', 'GITHUB_TOKEN',
            'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN', 'PYTHONPATH', 'PYTHONHOME'}}
    child_environment.update(RUSTUP_TOOLCHAIN='1.95.0', RUSTUP_AUTO_INSTALL='0')
    with partial.open('xb') as stdout, (output / 'owner-stderr.log').open('xb') as stderr:
        process = subprocess.run([str(canonical.verifier), '--worktree-root', str(workspace),
            'command-evidence', 'proof-set', '--spec', str(spec_path), '--json'], cwd=workspace,
            env=child_environment, stdout=stdout, stderr=stderr, check=False)
    raw, _ = regular_bytes(partial, MAX_RECEIPT_BYTES)
    document = decode(raw)
    if (process.returncode < 0 or document.get('schema_version') != 'command-proof-set.v1'
            or (process.returncode == 0) != (document.get('result') == 'SUCCESS')):
        raise Refused('command-evidence process and actual aggregate outcome disagree')
    after, _ = regular_bytes(context_path, 16384)
    if after != context_raw or acquire_plan() != plan or canonical.executable_identity() != identity:
        raise Refused('local proof subject, context or trusted executable changed')
    product_subject(environment, context, acquire_plan)
    partial.rename(receipt)
    return process.returncode, receipt


@dataclass(frozen=True)
class LocalCommands:
    receipt_sha256: str
    argv: tuple[tuple[str, ...], ...]


def bind_authenticated(root, plan, prepush):
    """Project complete SUCCESS from an already authenticated native packet.

    Native artifact/call-instance authentication remains with the central owner.
    The trusted producer records product_workspace before executing H. Consumer
    paths may differ, so match receipt cwd to that captured producer workspace,
    never to the consumer's local path. Missing capture refuses old packets.
    """
    expected = selected_argv(prepush, plan)
    path, context_path = root / MEMBER, root / 'em-ci-job-context.json'
    raw, _ = regular_bytes(path, MAX_RECEIPT_BYTES)
    context_raw, _ = regular_bytes(context_path, 16384)
    context, proof = decode(context_raw), decode(raw)
    captured = context.get('qualification_subject')
    if context.get('sha') == plan['control']:
        if captured != {key: plan[key] for key in ('head', 'tree', 'base', 'control', 'request_key')}:
            raise Refused('local proof lacks exact captured H/tree/B/S/request key')
    elif context.get('sha') != plan['head'] or captured is not None:
        raise Refused('local proof is neither authenticated native H nor captured control S')
    # A completed native H run can bootstrap selection before a request key
    # exists. It remains an H producer: never relabel its context as control S.
    workspace = context.get('product_workspace')
    if (not isinstance(workspace, str) or not PurePosixPath(workspace).is_absolute()
            or '..' in PurePosixPath(workspace).parts):
        raise Refused('local proof lacks native producer workspace capture')
    if (proof.get('schema_version') != 'command-proof-set.v1' or proof.get('result') != 'SUCCESS'
            or not isinstance(proof.get('commands'), list) or len(proof['commands']) != len(expected)):
        raise Refused('local proof is incomplete or did not succeed')
    def instant(value):
        try:
            parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
            if parsed.tzinfo is None:
                raise ValueError('timezone missing')
            return parsed
        except (AttributeError, TypeError, ValueError) as error:
            raise Refused('local proof timestamp invalid') from error
    previous = instant(context.get('proof_started_at'))
    finished = instant(context.get('proof_finished_at'))
    # Native capture currently records whole seconds; Rust emits fractional
    # seconds. Its finish stamp denotes that second, not its initial instant.
    if isinstance(context.get('proof_finished_at'), str) and '.' not in context['proof_finished_at']:
        finished += dt.timedelta(seconds=1)
    for index, (item, argv) in enumerate(zip(proof['commands'], expected)):
        if not isinstance(item, dict) or not isinstance(item.get('receipt'), dict):
            raise Refused('local command receipt missing')
        row = item['receipt']
        if (item.get('id') != f'pre-push-{index}' or row.get('schema_version') != 'command-evidence.v1'
                or row.get('argv') != list(argv) or row.get('cwd') != workspace
                or row.get('candidate_identity') != plan['head'] or row.get('result') != 'SUCCESS'
                or type(row.get('exit_code')) is not int or row['exit_code'] != 0
                or row.get('termination') != 'exit_code:0'
                or type(row.get('duration_ms')) is not int or row['duration_ms'] < 0
                or any(not isinstance(row.get(key), str) or not row[key]
                       for key in ('stdout_reference', 'stderr_reference'))):
            raise Refused('local command identity or actual successful termination differs')
        start, end = instant(row.get('started_at')), instant(row.get('ended_at'))
        if not previous <= start <= end < finished:
            raise Refused('local command lies outside native proof generation or serial order')
        previous = end
    if regular_bytes(path, MAX_RECEIPT_BYTES)[0] != raw or regular_bytes(context_path, 16384)[0] != context_raw:
        raise Refused('local command packet changed during binding')
    return LocalCommands(hashlib.sha256(raw).hexdigest(), expected)
