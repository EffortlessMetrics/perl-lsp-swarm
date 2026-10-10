#!/usr/bin/env python3
"""Prepare fixed native diagnostic measurements, then replay under the same owner.

No executor, lease, profile, storage policy or test-topology replacement. This
is the finite nine-package adapter; policy activation remains with its owner.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import disallowed_fields_prepare as fields
import lock_union_prepare as locks
import jsonrpc_prepare as rpc
import parser_occupancy_prepare as occupancy
owner = fields.owner
ROWS = (*owner.ROUTED_FIXTURE_ROWS, owner.PARSER_OCCUPANCY_ROW)


def context(env):
    descriptor = json.loads(env['CARGO_ADMITTED_RESOURCES'])
    snapshot, _ = owner.bounded_json(descriptor['nested_snapshot']['path'])
    plan = snapshot['plan']
    binding = plan['routed_preparation']
    if owner.routed_preparation_binding(Path(descriptor['worktree']), binding["runtime_row"]) != binding:
        raise owner.Denied('routed source/mapping/interpreter binding changed')
    return descriptor, plan


def preflight(env, expected_runtime=None):
    owner.nested_command('perllsp-build', env)
    descriptor, plan = context(env)
    runtime = plan['routed_preparation']['runtime_row']
    if expected_runtime is not None and runtime != expected_runtime:
        raise owner.Denied('requested runtime differs from finite preparation binding')
    required = {*owner.ROUTED_FIXTURE_ROWS, runtime, 'perllsp-build'}
    if runtime == 'routed-runtime':required.update((owner.PARSER_OCCUPANCY_ROW, owner.PARSER_OCCUPANCY_TEST_ROW))
    if not required.issubset(plan['request']['rows']):
        raise owner.Denied('routed runtime lacks a finite prerequisite row')
    owner.routed_mapping(plan)
    if owner.ROUTED_MEASUREMENTS in env:
        raise owner.Denied('routed preparation cannot adopt previous measured outputs')
    return descriptor, plan


def prepare(env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    descriptor, plan = preflight(env)
    subjects = {}
    for row in owner.ROUTED_FIXTURE_ROWS:
        path = Path(descriptor['resources']['temp']) / ('routed-measurement-' + str(descriptor['pid']) + '-' + row + '.json')
        if path.exists():raise owner.Denied('routed fixture already measured under this original owner')
        observed = []
        def measured(command, **kwargs):
            result = invoke(command, **kwargs)
            observed.append((command, kwargs['cwd'], result))
            return result
        if row == owner.DISALLOWED_FIXTURE_ROW:code = fields.fixture(env, measured)
        elif row in owner.LOCK_FIXTURE_ROWS:
            mode = next(mode for mode, member in locks.MEASUREMENTS.items() if member == row)
            code = locks.fixture(env, measured, mode)
        else:
            mode = next(mode for mode, member in rpc.MODES.items() if member == row)
            code = rpc.phase(mode, env, measured)
        expected = 101 if row in (owner.DISALLOWED_FIXTURE_ROW, owner.JSONRPC_REJECTED_ROW) else 0
        if code != expected or len(observed) != 1:
            raise owner.Denied('routed fixture did not perform its exact native measurement')
        command, cwd, result = observed[0]
        if result.returncode != code or any(len(s.encode()) > fields.LIMIT for s in (result.stdout, result.stderr)):
            raise owner.Denied('routed fixture raw result differs/exceeds bound')
        owner.nested_command(row, env)
        record = {'schema_version':1,'row':row,'tested_source':plan['source']['head'],
                  'argv':command,'cwd':str(cwd),'exit_code':code,'stdout':result.stdout,'stderr':result.stderr,
                  'diagnostic_validation':{'passed':True,'error':None},
                  'snapshot':descriptor['nested_snapshot'],'owner_process':descriptor['owner_process'],
                  'lease_identity':descriptor['lease_identity'],'marker_identity':descriptor['marker_identity'],
                  'recorded_under_original_live_owner':True}
        with path.open('x',encoding='utf-8') as output:json.dump(record,output)
        owner.nested_command(row, env)
        subjects[row] = owner.file_subject(path)
    env[owner.ROUTED_MEASUREMENTS] = json.dumps(subjects,sort_keys=True)
    runtime = plan['routed_preparation']['runtime_row']
    if runtime == owner.HELPER_RUNTIME:
        owner.nested_command(runtime,env)
        return env
    # Occupancy already has its actual native raw-record/frozen-reader protocol.
    if occupancy.measure(env, invoke) != 0:
        raise owner.Denied('failed actual occupancy preparation blocks runtime')
    _,_,path = occupancy.context(env)
    env.update({owner.ROUTED_MEASUREMENTS:json.dumps(subjects,sort_keys=True),
                'CARGO_ADMITTED_OCCUPANCY_MEASUREMENT':json.dumps(owner.file_subject(path),sort_keys=True)})
    owner.nested_command('routed-runtime',env)  # Entire projection before runtime.
    return env


def replay(row, env):
    owner.nested_command(row, env)
    descriptor, plan = context(env)
    record, subject = owner.routed_phase_record(plan, descriptor, row, env)
    owner.nested_command(row, env)
    if owner.file_subject(subject['path']) != subject:
        raise owner.Denied('prepared fixture changed while reading')
    return subprocess.CompletedProcess(record['argv'],record['exit_code'],record['stdout'],record['stderr'])
