#!/usr/bin/env python3
"""Measure real parser Clippy before runtime; replay its frozen raw result."""
import json
import os
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parent))
import disallowed_fields_prepare as shared
owner = shared.owner
LIMIT = owner.PARSER_OCCUPANCY_STREAM_LIMIT


def context(env):
    descriptor = json.loads(env['CARGO_ADMITTED_RESOURCES'])
    snapshot, _ = owner.bounded_json(descriptor['nested_snapshot']['path'])
    binding = snapshot['plan']['parser_occupancy']
    if owner.file_subject(Path(sys.executable).resolve(strict=True)) != binding['python']:
        raise owner.Denied('parser occupancy interpreter differs from bound subject')
    path = Path(descriptor['resources']['temp']) / ('parser-occupancy-measurement-' + str(descriptor['pid']) + '.json')
    return descriptor, snapshot, path


def validate_stream(stdout, code, worktree):
    if type(code) is not int or code not in (0, 101):raise owner.Denied('native parser Clippy has unknown/signal status')
    terminals=[];artifacts=[]
    for line in stdout.splitlines():
        event=json.loads(line)
        if not isinstance(event,dict):raise owner.Denied('parser Clippy JSON must contain objects')
        if terminals:raise owner.Denied('post-terminal parser Clippy output')
        if event.get('reason')=='build-finished':
            if type(event.get('success')) is not bool:raise owner.Denied('invalid parser Clippy terminal type')
            terminals.append(event['success'])
        if event.get('reason')=='compiler-artifact':artifacts.append(event)
    if terminals != [code==0]:raise owner.Denied('missing/contradictory native parser Clippy terminal')
    if code==0 and not any(a.get('manifest_path')==str(worktree/'crates/perl-parser/Cargo.toml')
                           and a.get('target',{}).get('src_path')==str(worktree/'crates/perl-parser/src/lib.rs')
                           and a.get('target',{}).get('kind')==['lib']
                           and 'incremental' in a.get('features',[]) for a in artifacts):
        raise owner.Denied('successful parser Clippy did not measure any parser target')
    # Preserve diagnostics and native nonzero status for the Rust oracle. A
    # matching lint hit is occupancy even on failure; silence on failure is an
    # instrument error. This reader never synthesizes an empty success.
    return artifacts


def measure(env=None, invoke=subprocess.run):
    env=dict(os.environ if env is None else env)
    command, child, cwd=owner.nested_command(owner.PARSER_OCCUPANCY_ROW,env)
    descriptor,snapshot,path=context(env)
    if path.exists():raise owner.Denied('parser occupancy already measured under this original owner')
    result=invoke(command,env=child,cwd=cwd,capture_output=True,text=True,encoding='utf-8',errors='strict')
    owner.nested_command(owner.PARSER_OCCUPANCY_ROW,env)
    error=None
    try:
        if any(len(text.encode())>LIMIT for text in (result.stdout,result.stderr)):
            raise owner.Denied('parser occupancy raw stream exceeds finite all-target bound')
        validate_stream(result.stdout,result.returncode,Path(descriptor['worktree']))
    except (owner.Denied,ValueError,TypeError,KeyError) as failure:error=failure
    record={'schema_version':1,'row':owner.PARSER_OCCUPANCY_ROW,'tested_source':snapshot['plan']['source']['head'],
            'argv':command,'cwd':str(cwd),'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr,
            'snapshot':descriptor['nested_snapshot'],'owner_process':descriptor['owner_process'],
            'lease_identity':descriptor['lease_identity'],'marker_identity':descriptor['marker_identity'],
            'recorded_under_original_live_owner':True,'instrument_validation':{'passed':error is None,'error':str(error) if error else None}}
    # Never write an over-bound stream. Admission already failed in that case.
    if any(len(text.encode())>LIMIT for text in (result.stdout,result.stderr)):
        raise error
    with path.open('x',encoding='utf-8') as output:json.dump(record,output)
    if error is not None:raise error
    owner.nested_command(owner.PARSER_OCCUPANCY_ROW,env)
    print('Parser occupancy native measurement: '+json.dumps({'path':str(path),'exit_code':result.returncode,'sha256':owner.file_subject(path)['sha256']}),flush=True)
    return result.returncode


def read(env=None):
    env=dict(os.environ if env is None else env)
    frozen=json.loads(env['CARGO_ADMITTED_OCCUPANCY_MEASUREMENT'])
    owner.nested_command(owner.PARSER_OCCUPANCY_TEST_ROW,env)
    descriptor,snapshot,path=context(env)
    record,subject=owner.parser_occupancy_measurement(snapshot['plan'],descriptor)
    if subject!=frozen:raise owner.Denied('parser occupancy receipt differs from owning frozen subject')
    validate_stream(record['stdout'],record['exit_code'],Path(descriptor['worktree']))
    owner.nested_command(owner.PARSER_OCCUPANCY_TEST_ROW,env)
    if owner.file_subject(path)!=frozen:raise owner.Denied('parser occupancy receipt changed while reading')
    print(record['stdout'],end='',flush=True)
    print(record['stderr'],end='',file=sys.stderr,flush=True)
    return record['exit_code']


def test(env=None, invoke=subprocess.run):
    env=dict(os.environ if env is None else env)
    code=measure(env,invoke)
    if code:return code  # Actual preparation failure blocks runtime.
    _,_,path=context(env)
    env['CARGO_ADMITTED_OCCUPANCY_MEASUREMENT']=json.dumps(owner.file_subject(path),sort_keys=True)
    return shared.owning_test(env,invoke,fixture_row=owner.PARSER_OCCUPANCY_ROW,
                             test_row=owner.PARSER_OCCUPANCY_TEST_ROW,
                             test_name=owner.PARSER_OCCUPANCY_TESTS,copy_prefix='parser-occupancy-owning')


if __name__=='__main__':
    try:
        if sys.argv[1:]==['--test']:sys.exit(test())
        elif sys.argv[1:]==['--measure']:sys.exit(measure())
        elif sys.argv[1:]==['--read']:sys.exit(read())
        else:raise owner.Denied('expected --test, --measure or --read')
    except (owner.Denied,OSError,ValueError,KeyError,TypeError) as error:
        print('parser occupancy instrument refused: '+str(error),file=sys.stderr)
        sys.exit(75)
