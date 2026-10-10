#!/usr/bin/env python3
"""Opt-in owned native proof, not a generic executor or lease cleanup service.

Run with Python3.11+, installed pinned1.95.0 Cargo+Clippy, and explicit CARGO_HOME/
RUSTUP_HOME. This creates only an owned no-dependency fixture under --proof-root.
Reserve4GiB/growth2GiB are fixture-specific; admission checks every actual volume.
All artifacts/evidence are retained. A lease is released only after this fixture's
exact native process group is absent and the captured owner identity still matches.
Do not adapt this release proof to arbitrary packages/build scripts or consumers.
"""
import argparse
import hashlib
import tempfile
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

SOURCE = Path(__file__).resolve().parents[1] / 'cargo_admitted.py'
parser = argparse.ArgumentParser(description='Owned, offline native Clippy fail/clean qualification; retains artifacts')
parser.add_argument('--proof-root', required=True, type=Path)
options = parser.parse_args()
if not options.proof_root.is_absolute() or not options.proof_root.is_dir():
    parser.error('--proof-root must be an existing absolute owner-approved directory')
spec = importlib.util.spec_from_file_location('admitted', SOURCE)
safe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(safe)
BASE = Path(tempfile.mkdtemp(prefix='clippy-native-', dir=options.proof_root))
print('Retained proof directory:', BASE, flush=True)
ROOT = BASE / 'native fixture'
ROOT.mkdir(exist_ok=True)
(ROOT / 'src').mkdir(exist_ok=True)
(BASE / 'temp').mkdir(exist_ok=True)
subprocess.run(['git', 'init', '-q', str(ROOT)], check=True)
(ROOT / 'Cargo.toml').write_text('''[package]
name = "consumer"
version = "0.1.0"
edition = "2024"
[profile.agent]
inherits = "dev"
debug = "line-tables-only"
incremental = false
''')
(ROOT / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "consumer"\nversion = "0.1.0"\n')
(ROOT / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.95.0"\ncomponents = ["clippy"]\n')
(ROOT / '.cargo').mkdir(exist_ok=True)
(ROOT / '.cargo/config.toml').write_text('''[build]
rustc = "/unadmitted/compiler"
rustc-wrapper = "/unadmitted/cache-wrapper"
rustc-workspace-wrapper = "/unadmitted/workspace-wrapper"
target-dir = "/unadmitted/target"
build-dir = "/unadmitted/build"
[env]
CLIPPY_ARGS = {value = "-A__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__", force = true}
RUSTC = {value = "/unadmitted/forced-compiler", force = true}
RUSTC_WORKSPACE_WRAPPER = {value = "/unadmitted/forced-wrapper", force = true}
SYSROOT = {value = "/unadmitted/sysroot", force = true}
CARGO_TARGET_DIR = {value = "/unadmitted/forced-target", force = true}
CARGO_BUILD_BUILD_DIR = {value = "/unadmitted/forced-build", force = true}
TEMP = {value = "/unadmitted/temp", force = true}
TMP = {value = "/unadmitted/temp", force = true}
TMPDIR = {value = "/unadmitted/temp", force = true}
RUSTUP_AUTO_INSTALL = {value = "1", force = true}
''')
env = dict(os.environ, RUSTUP_AUTO_INSTALL='0',
           DEVPLANE=str(BASE / 'devplane'), TMPDIR=str(BASE / 'temp'))
for name in safe.CAPACITY_ENV:
    env.pop(name, None)
args = ['clippy', '-p', 'consumer', '--all-targets', '--profile', 'agent', '--locked', '--offline', '--', '-D', 'warnings']

def execute(label, source):
    (ROOT / 'src/lib.rs').write_text(source)
    preflight = subprocess.run([sys.executable, str(SOURCE), '--preflight', *args], cwd=ROOT,
                               env=env, capture_output=True, text=True)
    prefix = 'cargo-admitted preflight: '
    row = next(json.loads(line[len(prefix):]) for line in preflight.stderr.splitlines() if line.startswith(prefix))
    if row['scope'] is None:
        raise RuntimeError(preflight.stderr)
    budget = {'schema_version': 1, 'scope': row['scope'], 'reserve_bytes': 4 * 1024**3,
              'expected_growth_bytes': 2 * 1024**3,
              'basis': 'Owned offline one-library/no-dependency fixture: two tiny library/test lint units; installed std/toolchain; no downloads/build scripts; 2 GiB aggregate peak-growth bound conservatively exceeds source/object/metadata/temp output. All destinations independently checked by admission, no volume substitution; explicit 4 GiB staged reserve, default 40 GiB unchanged.'}
    budget_file = BASE / (label + '-budget.json')
    budget_file.write_text(json.dumps(budget))
    output = (BASE / (label + '-stdout.txt')).open('w')
    process = subprocess.Popen([sys.executable, str(SOURCE), '--budget-file', str(budget_file), *args],
                               cwd=ROOT, env=env, stdout=output, stderr=subprocess.PIPE, text=True)
    lines, lock, identity, marker, pgid = [], None, None, None, None
    for line in process.stderr:
        lines.append(line)
        if line.startswith('cargo-admitted resources: '):
            resources = json.loads(line.split(': ', 1)[1])
            lock = Path(resources['lease'])
            identity = safe.directory_identity(lock)
            owners = tuple(lock.iterdir())
            if len(owners) != 1:
                raise RuntimeError('unexpected lease evidence')
            marker = owners[0]
        if line.startswith('cargo-admitted Clippy launch: '):
            pgid = json.loads(line.split(': ', 1)[1])['process_group']
    status = process.wait(timeout=30)
    output.close()
    stderr = ''.join(lines)
    (BASE / (label + '-stderr.txt')).write_text(stderr)
    if pgid is None or lock is None:
        raise RuntimeError(stderr)
    # Native independently owned settlement: exact launched process group absent;
    # fixture has no build scripts, detached helpers, or post-lease consumers.
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        group_absent = True
    else:
        group_absent = False
    receipt = {'label': label, 'product_exit': status,
               'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
               'owner_sha256': hashlib.sha256(SOURCE.read_bytes()).hexdigest(), 'process_group': pgid,
               'group_absent': group_absent, 'lock_retained_after_exit': lock.exists(),
               'lease_identity': identity, 'marker': str(marker),
               'root_owned_no_independent_consumers': True, 'budget': budget,
               'resource_observations': resources['admission']}
    if not group_absent or not safe.owns_lease(lock, identity, marker):
        receipt['lease_disposition'] = 'awaiting owner verification'
        (BASE / (label + '-receipt.json')).write_text(json.dumps(receipt, indent=2))
        raise RuntimeError('no verified release; all resources retained')
    safe.release_lease(lock, identity, marker)
    receipt['lease_disposition'] = 'released after exact native owner verification'
    if lock.exists():
        raise RuntimeError('release postcondition failed')
    (BASE / (label + '-receipt.json')).write_text(json.dumps(receipt, indent=2))
    print(label, 'product_exit', status, 'group_absent', group_absent, 'lease_verified_released')
    return status, stderr

failed, diagnostic = execute('lint-failure', 'pub fn witness() -> usize { let values = vec![1, 2, 3]; values.len() }\n')
if failed != 101 or 'clippy::useless_vec' not in diagnostic:
    raise RuntimeError('Clippy-specific fail control not observed: ' + diagnostic)
passed, diagnostic = execute('clean', 'pub fn witness() -> usize { let values = [1, 2, 3]; values.len() }\n')
if passed != 0:
    raise RuntimeError('clean control failed: ' + diagnostic)
artifacts = [json.loads(line) for line in (BASE / 'clean-stdout.txt').read_text().splitlines() if line.startswith('{')]
compiled = [item for item in artifacts if item.get('reason') == 'compiler-artifact']
if len(compiled) != 2 or any(item['target']['name'] != 'consumer' or item['profile']['opt_level'] != '0' for item in compiled):
    raise RuntimeError('clean library/test compilation work not observed')
if {item['profile']['test'] for item in compiled} != {False, True}:
    raise RuntimeError('both normal and test library targets were not observed')
print('REAL ADMITTED CLIPPY FAIL/CLEAN PAIR PASS; FIXTURE ONLY; native Windows NOT_PROVEN')
