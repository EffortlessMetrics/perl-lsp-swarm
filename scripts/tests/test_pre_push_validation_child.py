"""Qualify complete generated dispatch with retained native Git push evidence.

The caller chooses --evidence and owns retention of every unique run directory,
including successful mutation controls. This helper does not delete evidence.

Cargo/just recorders represent expensive gate commands only inside these fixtures.
Optional retained test instrumentation exercises unchanged repository tests too.
"""
from pathlib import Path
import argparse, hashlib, json, os, shlex, shutil, subprocess, sys, uuid

SELECTORS = ['GIT_DIR', 'GIT_COMMON_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY']

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hook', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    source = args.hook.read_text(encoding='utf-8')
    run = args.evidence / ('complete-' + uuid.uuid4().hex[:8]); run.mkdir()
    git = shutil.which('git'); assert git
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith('GIT_')}
    templates = run / 'templates'; templates.mkdir()
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=str(run / 'no-global'),
               GIT_TEMPLATE_DIR=str(templates), GIT_OPTIONAL_LOCKS='0', PYTHONDONTWRITEBYTECODE='1', RUST_BACKTRACE='0')
    instrument = os.environ.get('PERL_LSP_HOOK_LIBRARY_TEST_EXE')
    names_file = os.environ.get('PERL_LSP_HOOK_LIBRARY_TEST_NAMES')
    bin_instrument = os.environ.get('PERL_LSP_HOOK_BIN_TEST_EXE')
    names = [x['test'] for x in json.loads(Path(names_file).read_text())] if names_file else []
    if instrument:
        assert names and Path(instrument).is_file()
        binary_sha = hashlib.sha256(Path(instrument).read_bytes()).hexdigest()
    else:
        binary_sha = None
    recorder = run / 'recorder.py'
    recorder.write_text('''from pathlib import Path
import json,os,subprocess,sys
label,*argv=sys.argv[1:];case=Path(os.environ['HOOK_PROOF_CASE'])
selectors={k:v for k,v in os.environ.items() if k in ['GIT_DIR','GIT_COMMON_DIR','GIT_WORK_TREE','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY']}
with (case/'calls.jsonl').open('a') as f:f.write(json.dumps(dict(label=label,argv=argv,selectors=selectors))+'\\n')
if label=='cargo' and argv[:2]==['xtask','resolve-package-name']:print('fixture');raise SystemExit(0)
if label=='cargo' and argv and argv[0] in ['fmt','clippy']:raise SystemExit(0)
if os.environ.get('HOOK_PROOF_EXIT') and label=='just':raise SystemExit(int(os.environ['HOOK_PROOF_EXIT']))
binary=os.environ.get('PERL_LSP_HOOK_LIBRARY_TEST_EXE')
if binary:
    names=[x['test'] for x in json.loads(Path(os.environ['PERL_LSP_HOOK_LIBRARY_TEST_NAMES']).read_text())]
    selected=names if os.environ['HOOK_PROOF_MODE']=='full' and label=='just' else ['git_ancestry::tests::complete_linear_history_is_ancestor']
    if label=='status':selected=['source_authority::verify::verify_tests::unclassified_files_in_the_tree_are_named']
    r=subprocess.run([binary,'--exact',*selected,'--nocapture','--test-threads=2'],capture_output=True)
    (case/(label+'-tests.stdout')).write_bytes(r.stdout);(case/(label+'-tests.stderr')).write_bytes(r.stderr)
    wanted=('test result: ok. '+str(len(selected))+' passed; 0 failed;').encode()
    if r.returncode!=0 or wanted not in r.stdout:raise SystemExit(1)
    bins=os.environ.get('PERL_LSP_HOOK_BIN_TEST_EXE')
    if bins and os.environ['HOOK_PROOF_MODE']=='full' and label=='just':
        bin_names=['tasks::gates::tests::'+n for n in ['plan_gates_threads_staged_tree_oid_into_non_commit_tiers','isolated_repo_helper_publishes_origin_main_and_seed_commit','is_latest_commit_compares_configured_upstream']]
        b=subprocess.run([bins,'--exact',*bin_names,'--nocapture','--test-threads=1'],capture_output=True)
        (case/'original-bin-tests.stdout').write_bytes(b.stdout);(case/'original-bin-tests.stderr').write_bytes(b.stderr)
        assert b.returncode==0 and b'test result: ok. 3 passed; 0 failed;' in b.stdout
    raise SystemExit(0)
fixture=case/('child-'+label);fixture.mkdir()
def git(*a):subprocess.run(['git','-C',str(fixture),*a],check=True,capture_output=True)
git('init','--initial-branch=child');git('config','user.name','Child Fixture');git('config','user.email','child@example.invalid')
(fixture/'child.txt').write_text('child content\\n');git('add','child.txt');git('commit','-m','child')
assert not selectors,'validation inherited outer Git selectors'
''', encoding='utf-8')
    def command(root, *argv):
        r = subprocess.run([git, '-C', str(root), *argv], env=env, capture_output=True)
        assert r.returncode == 0, (argv, r.stderr)
        return r.stdout.decode().strip()
    def snapshot(common, extra):
        files = [p for p in common.rglob('*') if p.is_file()] + extra
        return {str(p): (hashlib.sha256(p.read_bytes()).hexdigest(), p.stat().st_dev, p.stat().st_ino, p.stat().st_mtime_ns) for p in files}
    report = {'hook_sha256': hashlib.sha256(args.hook.read_bytes()).hexdigest(), 'preserved_binary_sha256': binary_sha,
              'preserved_test_count': len(names), 'preserved_bin_count': 3 if bin_instrument else 0, 'qualification': 'complete generated shell dispatch with fixture gate recorders; no workspace portfolio', 'cases': []}
    modes = ['full', 'single', 'status', 'full-removal', 'single-removal', 'status-removal', 'binding-refusal', 'gate-failure', 'docs', 'legacy-common-api']
    for number, mode in enumerate(modes):
        case = run / mode; case.mkdir(); outer = case / 'outer'; outer.mkdir(); writer = case / 'writer'; remote = case / 'remote.git'
        command(outer, 'init', '--initial-branch=main')
        command(outer, 'config', 'user.name', 'Outer Fixture'); command(outer, 'config', 'user.email', 'outer@example.invalid')
        command(outer, 'config', 'commit.gpgsign', 'false')
        (outer / 'sentinel.txt').write_text('must survive validation\n')
        command(outer, 'add', 'sentinel.txt'); command(outer, 'commit', '-m', 'seed')
        command(outer, 'worktree', 'add', '-b', 'candidate', str(writer)); command(case, 'init', '--bare', str(remote))
        command(writer, 'push', str(remote), 'HEAD:refs/heads/proof')
        paths = ['docs/probe.md'] if mode == 'docs' else ['crates/fixture/src/lib.rs'] if mode.startswith('single') else ['xtask/src/probe.rs']
        if mode.startswith('status'): paths += ['crates/fixture/tests/probe.rs']
        for path in paths:
            file = writer / path; file.parent.mkdir(parents=True, exist_ok=True); file.write_text('probe\n')
        command(writer, 'add', *paths); command(writer, 'commit', '-m', 'candidate')
        policy = writer / '.ci/gate-policy.yaml'; policy.parent.mkdir(); policy.write_text('gates:\n  - name: clippy_tests_kernel\n    command:\n      -p fixture\n')
        status = writer / 'scripts/update-current-status.py'; status.parent.mkdir()
        status.write_text('import runpy,sys\nsys.argv=[' + repr(str(recorder)) + ',"status"]\nrunpy.run_path(' + repr(str(recorder)) + ',run_name="__main__")\n')
        tools = case / 'bin'; tools.mkdir()
        for label in ['cargo', 'just']:
            file = tools / label
            file.write_text('#!/usr/bin/env bash\nexec ' + ' '.join(shlex.quote(str(v).replace('\\', '/')) for v in [sys.executable, recorder, label]) + ' "$@"\n', newline='\n'); file.chmod(0o755)
        if mode == 'legacy-common-api':
            shim = tools / 'git'
            shim.write_text('#!/usr/bin/env bash\nfor arg in "$@"; do\n case "$arg" in --path-format*) exit 97;; esac\ndone\nif [ "$1" = rev-parse ] && [ "$2" = --git-common-dir ]; then\n printf "common-dir\\n" >> ' + shlex.quote(str(case / 'common-lookups').replace('\\', '/')) + '\nfi\nexec ' + shlex.quote(str(git).replace('\\', '/')) + ' "$@"\n', newline='\n')
            shim.chmod(0o755)
        selected = source
        if mode == 'legacy-common-api':
            shell_tools = tools.as_posix()
            if os.name == 'nt': shell_tools = '/' + shell_tools[0].lower() + shell_tools[2:]
            selected = selected.replace('set -euo pipefail', 'set -euo pipefail\nPATH=' + shlex.quote(shell_tools) + ':"$PATH"\nexport PATH', 1)
        if mode == 'full-removal': selected = selected.replace('run_validation_child run_gate 2>&1', 'run_gate 2>&1')
        if mode == 'single-removal': selected = selected.replace('run_validation_child run_single_crate_gate 2>&1', 'run_single_crate_gate 2>&1')
        if mode == 'status-removal': selected = selected.replace('run_validation_child python3 "$REPO_ROOT/scripts/update-current-status.py"', 'python3 "$REPO_ROOT/scripts/update-current-status.py"')
        if mode == 'binding-refusal': selected = selected.replace('VALIDATION_GIT_DIR="$(git rev-parse --absolute-git-dir)"', 'VALIDATION_GIT_DIR="deliberately-wrong"')
        hook = writer / '.githooks/pre-push'; hook.parent.mkdir(); hook.write_text(selected, encoding='utf-8', newline='\n'); hook.chmod(0o755)
        command(writer, 'config', 'core.hooksPath', '.githooks')
        common = outer / '.git'; gitdir = Path(command(writer, 'rev-parse', '--absolute-git-dir'))
        before = snapshot(common, [hook, writer / '.git', writer / 'sentinel.txt', outer / 'sentinel.txt'])
        local = dict(env, PATH=str(tools) + os.pathsep + env.get('PATH', ''), HOOK_PROOF_CASE=str(case), HOOK_PROOF_MODE=mode,
                     GIT_DIR=gitdir.as_posix(), GIT_COMMON_DIR=common.as_posix(), GIT_WORK_TREE=writer.as_posix(),
                     GIT_INDEX_FILE=(gitdir / 'index').as_posix(), GIT_OBJECT_DIRECTORY=(common / 'objects').as_posix())
        if mode == 'legacy-common-api':
            for key in ['PERL_LSP_HOOK_LIBRARY_TEST_EXE', 'PERL_LSP_HOOK_LIBRARY_TEST_NAMES', 'PERL_LSP_HOOK_BIN_TEST_EXE']: local.pop(key, None)
        if mode == 'gate-failure': local['HOOK_PROOF_EXIT'] = '23'
        r = subprocess.run([git, '-C', str(writer), 'push', str(remote), 'HEAD:refs/heads/proof'], env=local, capture_output=True)
        (case / 'push.stdout').write_bytes(r.stdout); (case / 'push.stderr').write_bytes(r.stderr)
        calls = [json.loads(x) for x in (case / 'calls.jsonl').read_text().splitlines()] if (case / 'calls.jsonl').exists() else []
        validation = [x for x in calls if x['label'] != 'cargo' or x['argv'][:2] != ['xtask', 'resolve-package-name']]
        preserved = before == snapshot(common, [hook, writer / '.git', writer / 'sentinel.txt', outer / 'sentinel.txt'])
        if mode in ['full-removal', 'single-removal']:
            assert r.returncode != 0 and any(x['selectors'] for x in validation), (mode, r.stderr)
        elif mode == 'status-removal':
            assert any(x['label'] == 'status' and x['selectors'] for x in calls), 'status removal escaped control'
        elif mode == 'binding-refusal':
            assert r.returncode != 0 and not validation and b'worktree binding changed' in r.stderr + r.stdout
            assert preserved
        elif mode == 'gate-failure':
            assert r.returncode != 0 and b'exit 23' in r.stdout + r.stderr and preserved
        else:
            assert r.returncode == 0 and preserved, (mode, r.stderr, r.stdout)
            assert all(not x['selectors'] for x in validation), (mode, calls)
            if mode == 'docs': assert not calls
            elif mode == 'single':
                assert [x['argv'][0] for x in validation] == ['fmt', 'clippy', 'test']
                assert '--all-targets' in validation[1]['argv']
            elif mode == 'status': assert [x['label'] for x in validation] == ['status', 'just']
            else: assert [x['label'] for x in validation] == ['just']
        if mode == 'legacy-common-api':
            assert len((case / 'common-lookups').read_text().splitlines()) >= 2, 'capture and recheck must use portable common-dir API'
        report['cases'].append({'mode': mode, 'push_exit': r.returncode, 'outer_preserved': preserved, 'calls': calls, 'control_detected': True})
        (run / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if instrument: assert hashlib.sha256(Path(instrument).read_bytes()).hexdigest() == binary_sha
    print(json.dumps({'evidence': str(run), 'cases_passed': len(modes), 'original_tests': len(names)}))

if __name__ == '__main__':
    main()
