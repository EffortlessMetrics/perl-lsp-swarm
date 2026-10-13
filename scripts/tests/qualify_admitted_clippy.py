#!/usr/bin/env python3
"""Opt-in owned native proof, not a generic executor or lease cleanup service.

Run with Python3.11+, installed pinned1.95.0 Cargo+Clippy, and explicit CARGO_HOME/
RUSTUP_HOME. This creates only an owned no-dependency fixture under --proof-root.
Reserve4GiB/growth2GiB are fixture-specific; admission checks every actual volume.
All artifacts/evidence are retained. The original owner now releases only after
its subreaper observes kernel ECHILD and its captured lease identity still matches.
The fixture separately records group absence; that observation never authorizes
arbitrary workload release or proves independent artifact consumers have stopped.
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


def fixture_inputs(request):
    paths = {'src/lib.rs', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml'}
    if '--test' in request:
        paths.update({'tests/' + request[request.index('--test') + 1] + '.rs', 'tests/other.rs'})
    return paths


def bind_inputs(root, destination, request):
    destination.mkdir()
    binding = {}
    for relative in sorted(fixture_inputs(request)):
        data = (root / relative).read_bytes()
        retained = destination / relative
        retained.parent.mkdir(parents=True, exist_ok=True)
        retained.write_bytes(data)
        binding[relative] = {'sha256': hashlib.sha256(data).hexdigest(), 'size_bytes': len(data)}
    return binding


def verify_inputs(root, destination, request, binding):
    if set(binding) != fixture_inputs(request):
        raise RuntimeError('missing or unexpected fixture input binding')
    for relative, identity in binding.items():
        for path in (root / relative, destination / relative):
            data = path.read_bytes()
            if identity != {'sha256': hashlib.sha256(data).hexdigest(), 'size_bytes': len(data)}:
                raise RuntimeError('fixture input binding mismatch: ' + relative)


parser = argparse.ArgumentParser(description='Owned, offline native Clippy fail/clean qualification; retains artifacts')
parser.add_argument('--proof-root', required=True, type=Path)
parser.add_argument('--named-only', action='store_true', help='Run only the affected named-test pair')
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
[features]
default = ["witness"]
witness = []
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

def execute(label, source, request=args):
    (ROOT / 'src/lib.rs').write_text(source)
    retained_inputs = BASE / (label + '-inputs')
    binding = bind_inputs(ROOT, retained_inputs, request)
    preflight = subprocess.run([sys.executable, str(SOURCE), '--preflight', *request], cwd=ROOT,
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
    verify_inputs(ROOT, retained_inputs, request, binding)
    process = subprocess.Popen([sys.executable, str(SOURCE), '--budget-file', str(budget_file), *request],
                               cwd=ROOT, env=env, stdout=output, stderr=subprocess.PIPE, text=True)
    lines, lock, identity, marker, pgid = [], None, None, None, None
    for line in process.stderr:
        lines.append(line)
        if line.startswith('cargo-admitted resources: '):
            resources = json.loads(line.split(': ', 1)[1])
            lock = Path(resources['lease'])
            identity = tuple(resources['lease_identity'])
            marker = Path(resources['lease_marker'])
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
    settlement = next((json.loads(line.split(': ',1)[1]) for line in lines
                       if line.startswith('cargo-admitted Clippy settlement: ')), None)
    receipt = {'label': label, 'product_exit': status,
               'requested_argv': request,
               'retained_inputs': str(retained_inputs), 'input_bindings': binding,
               'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
               'owner_sha256': hashlib.sha256(SOURCE.read_bytes()).hexdigest(), 'process_group': pgid,
               'group_absent': group_absent, 'lock_retained_after_exit': lock.exists(),
               'lease_identity': identity, 'marker': str(marker),
               'root_owned_no_independent_consumers': True, 'budget': budget,
               'resource_observations': resources['admission'], 'kernel_settlement': settlement}
    if (not group_absent or settlement is None or not settlement['tree_settled']
            or not settlement['lease_released'] or os.path.lexists(lock)):
        receipt['lease_disposition'] = 'awaiting owner verification'
        (BASE / (label + '-receipt.json')).write_text(json.dumps(receipt, indent=2))
        raise RuntimeError('no verified release; all resources retained')
    receipt['lease_disposition'] = 'released by original owner after kernel ECHILD proof and matching identity'
    if lock.exists():
        raise RuntimeError('release postcondition failed')
    verify_inputs(ROOT, retained_inputs, request, binding)
    receipt['inputs_verified'] = True
    (BASE / (label + '-receipt.json')).write_text(json.dumps(receipt, indent=2))
    print(label, 'product_exit', status, 'group_absent', group_absent, 'lease_verified_released')
    return status, stderr

if not options.named_only:
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

# A real named target must retain default features and lint policy, while a
# deliberately invalid unselected target proves Cargo did not expand selection.
(ROOT / 'tests').mkdir()
(ROOT / 'tests/other.rs').write_text('compile_error!("unselected target compiled");\n')
named = ROOT / 'tests/named-proof.rs'
prefix = '#[cfg(not(feature = "witness"))]\ncompile_error!("default feature lost");\n'
request = ['clippy', '-p', 'consumer', '--test', 'named-proof', '--profile', 'agent',
           '--locked', '--offline', '--', '-D', 'warnings']
named.write_text(prefix + '#[test]\nfn witness() { let values = vec![1, 2, 3]; assert_eq!(values.len(), 3); }\n')
failed, diagnostic = execute('named-lint-failure', 'pub fn witness() -> usize { 3 }\n', request)
if failed != 101 or 'clippy::useless_vec' not in diagnostic or 'tests/named-proof.rs' not in diagnostic:
    raise RuntimeError('named-target lint failure not observed: ' + diagnostic)
named.write_text(prefix + '#[test]\nfn witness() { let values = [1, 2, 3]; assert_eq!(values.len(), 3); }\n')
passed, diagnostic = execute('named-clean', 'pub fn witness() -> usize { 3 }\n', request)
if passed != 0:
    raise RuntimeError('named clean control failed: ' + diagnostic)
artifacts = [json.loads(line) for line in (BASE / 'named-clean-stdout.txt').read_text().splitlines() if line.startswith('{')]
compiled = [item for item in artifacts if item.get('reason') == 'compiler-artifact']
if (len(compiled) != 2 or {item['target']['name'] for item in compiled} != {'consumer', 'named-proof'}
        or any(item['features'] != ['default', 'witness'] or item['profile']['opt_level'] != '0' for item in compiled)
        or [item['profile']['test'] for item in compiled if item['target']['name'] == 'named-proof'] != [True]):
    raise RuntimeError('named-target/default-features/agent-profile identity not observed')
print('REAL NAMED TEST FAIL/CLEAN PAIR PASS; UNSELECTED TARGET EXCLUDED; FIXTURE ONLY')
