#!/usr/bin/env python3
"""Three exact JSON-RPC compiler phases under the existing original owner."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
sys.dont_write_bytecode = True
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
import disallowed_fields_prepare as shared
owner = shared.owner
MODES = {'lock': owner.JSONRPC_LOCK_ROW, 'neutral': owner.JSONRPC_NEUTRAL_ROW,
         'rejected': owner.JSONRPC_REJECTED_ROW}
REVIEWED = {'serde': ('1.0.229', '4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba'),
            'serde_json': ('1.0.151', 'c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14')}


def prepare(env=None):
    env = dict(os.environ if env is None else env)
    worktree, slot, paths = owner.resource_plan(env)
    if (slot/'cargo-active').exists():raise owner.Denied('cannot prepare under active owner')
    root = owner.native_path(str(paths['temp']/'jsonrpc-17479'))
    template = worktree/'.spec/17479-nested-admission/jsonrpc-fixture'
    model = str(worktree/'crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs').replace('\\','\\\\').replace('"','\\"')
    for path in (root,root/'neutral/src',root/'rejected/src',root/'neutral/target',
                 root/'neutral/build',root/'rejected/target',root/'rejected/build'):
        owner.native_path(str(path)).mkdir(parents=True,exist_ok=True)
    for mode in ('neutral','rejected'):
        for name, source in (('Cargo.toml','Cargo.toml'),('src/lib.rs',mode+'.rs.in')):
            path=template/source;subject=owner.file_subject(path)
            if subject['file_identity'][2]>owner.BUDGET_FILE_LIMIT:raise owner.Denied('JSON-RPC template too large')
            data=path.read_text().replace('@MODEL@',model).encode()
            if owner.file_subject(path)!=subject:raise owner.Denied('JSON-RPC template changed')
            destination=owner.native_path(str(root/mode/name))
            if destination.exists():
                if destination.read_bytes()!=data:raise owner.Denied('preserve changed existing JSON-RPC fixture for diagnosis')
            else:
                with destination.open('xb') as output:output.write(data)
    binding=owner.jsonrpc_fixture(worktree,paths)
    print(json.dumps({'prepared':binding['cwd'],'compiler_launched':False,'lock_generated':False}))


def validate_lock(data):
    if len(data)>owner.BUDGET_FILE_LIMIT:raise owner.Denied('generated JSON-RPC lock too large')
    doc=tomllib.loads(data.decode('utf-8'));packages=doc.get('package',[])
    if doc.get('version')!=4 or not isinstance(packages,list):raise owner.Denied('invalid generated lock schema')
    root=[];reviewed={name:0 for name in REVIEWED}
    for p in packages:
        name=p.get('name')
        if name=='lsp-jsonrpc-boundary-probe':
            root.append(p)
            if 'source' in p or 'checksum' in p or sorted(p.get('dependencies',[]))!=sorted(REVIEWED):
                raise owner.Denied('JSON-RPC root closure widened or source-bound')
            continue
        if p.get('source')!='registry+https://github.com/rust-lang/crates.io-index' or not re.fullmatch('[0-9a-fA-F]{64}',p.get('checksum','')):
            raise owner.Denied('generated dependency lacks reviewed registry/checksum authority')
        if name in REVIEWED:
            if (p.get('version'),p.get('checksum'))!=REVIEWED[name]:raise owner.Denied('reviewed direct package changed')
            reviewed[name]+=1
    if len(root)!=1 or any(n!=1 for n in reviewed.values()):raise owner.Denied('missing/duplicate generated root/direct package')


def phase(mode, env=None, invoke=subprocess.run):
    env=dict(os.environ if env is None else env)
    if mode not in MODES:raise owner.Denied('unknown finite JSON-RPC phase')
    row=MODES[mode];command,child,cwd=owner.nested_command(row,env)
    replayed=shared.prepared_output(row,env)
    if replayed is not None:
        print(replayed.stdout,end='',flush=True);print(replayed.stderr,end='',file=sys.stderr,flush=True)
        return replayed.returncode
    descriptor=json.loads(env['CARGO_ADMITTED_RESOURCES']);snapshot,_=owner.bounded_json(descriptor['nested_snapshot']['path']);binding=snapshot['plan']['jsonrpc_fixture']
    if owner.file_subject(Path(sys.executable).resolve(strict=True))!=binding['python']:raise owner.Denied('JSON-RPC interpreter differs from bound subject')
    root=Path(binding['cwd']);name=('lock-generation-' if mode=='lock' else 'native-'+mode+'-')+str(descriptor['pid'])+'.json'
    if (root/name).exists():raise owner.Denied('JSON-RPC phase already attempted under this owner')
    result=invoke(command,env=child,cwd=cwd,capture_output=True,text=True,encoding='utf-8',errors='strict')
    owner.nested_command(row,env)
    if len(result.stdout.encode())>shared.LIMIT or len(result.stderr.encode())>shared.LIMIT:raise owner.Denied('JSON-RPC instrument output exceeds bound')
    locks=None;error=None
    try:
        if mode=='lock':
            if result.returncode!=0:raise owner.Denied('native lock generation failed')
            path=cwd/'Cargo.lock';before=owner.file_subject(path);data=path.read_bytes();validate_lock(data)
            if owner.file_subject(path)!=before:raise owner.Denied('generated lock changed while validating')
            destination=Path(binding['directories']['rejected']['path'])/'Cargo.lock'
            # This is a measured output transfer, never a checked-in lock seed.
            if destination.exists() and destination.read_bytes()!=data:raise owner.Denied('preserve conflicting previous negative lock')
            if not destination.exists():
                with destination.open('xb') as output:output.write(data)
            locks={'neutral':before,'rejected':owner.file_subject(destination)}
        elif mode=='neutral':
            if result.returncode!=0:raise owner.Denied('neutral native compiler failed')
        elif result.returncode!=101 or not re.search(r'(?m)^error\[E0432\]: unresolved import `perl_parser_core`$',result.stderr):
            raise owner.Denied('negative native compiler has unexpected success or unrelated diagnostic')
    except (owner.Denied,ValueError,TypeError,KeyError,OSError) as failure:error=failure
    record={'schema_version':1,'phase':mode,'row':row,'tested_source':snapshot['plan']['source']['head'],
            'argv':command,'cwd':str(cwd),'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr,
            'snapshot':descriptor['nested_snapshot'],'owner_process':descriptor['owner_process'],
            'lease_identity':descriptor['lease_identity'],'marker_identity':descriptor['marker_identity'],
            'recorded_under_original_live_owner':True,'diagnostic_validation':{'passed':error is None,'error':str(error) if error else None}}
    if mode=='lock':record.update(generated_by_native_command=error is None,locks=locks)
    with (root/name).open('x',encoding='utf-8') as output:json.dump(record,output)
    if error is not None:raise error
    owner.nested_command(row,env)
    print(result.stdout,end='',flush=True);print(result.stderr,end='',file=sys.stderr,flush=True)
    return result.returncode


if __name__=='__main__':
    try:
        if sys.argv[1:]==['--prepare']:prepare()
        elif sys.argv[1:]==['--test']:
            sys.exit(shared.owning_test(fixture_row=owner.JSONRPC_LOCK_ROW,test_row=owner.JSONRPC_TEST_ROW,
                                       test_name=owner.JSONRPC_TESTS,copy_prefix='jsonrpc-owning'))
        elif len(sys.argv)==3 and sys.argv[1]=='--phase':sys.exit(phase(sys.argv[2]))
        else:raise owner.Denied('expected finite prepare, test or phase')
    except (owner.Denied,KeyError,TypeError,ValueError,OSError) as error:
        print('JSON-RPC instrument refused: '+str(error),file=sys.stderr);sys.exit(75)
