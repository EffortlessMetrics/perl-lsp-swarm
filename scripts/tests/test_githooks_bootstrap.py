"""No-build first-mile controls: real wrappers, fake admission boundary."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HookBootstrapTests(unittest.TestCase):
    def setUp(self):
        if os.name == 'nt':
            # Native Git Bash only: Windows' generic bash may invoke WSL.
            candidates = [Path('C:/Program Files/Git/bin/bash.exe')]
            git = shutil.which('git')
            if git:
                candidates.insert(0, Path(git).resolve().parent.parent / 'bin/bash.exe')
            configured = os.environ.get('GITHOOKS_BOOTSTRAP_TEST_BASH')
            if configured:
                candidates.insert(0, Path(configured))
            self.bash = next((str(path) for path in candidates if path.is_file()), None)
        else:
            self.bash = shutil.which('bash')
        if not self.bash:
            self.skipTest('Bash is required for the maintained shell entrypoints')
        temp_root = os.environ.get('GITHOOKS_BOOTSTRAP_TEST_TMPDIR')
        self.temp = tempfile.TemporaryDirectory(prefix='hook-bootstrap-proof-', dir=temp_root)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'repo with spaces'
        scripts = self.root / 'scripts'
        (scripts / 'lib').mkdir(parents=True)
        for name in ('install-githooks.sh', 'check-githooks.sh', 'githooks-bootstrap.sh',
                     'cargo-admitted', 'lib/cargo-toolchain-guard.sh'):
            source = ROOT / 'scripts' / name
            if source.exists():
                shutil.copy2(source, scripts / name)
        for name in ('Cargo.toml', 'rust-toolchain.toml'):
            shutil.copy2(ROOT / name, self.root / name)
        self.log = Path(self.temp.name) / 'argv.txt'
        self.cwd_log = Path(self.temp.name) / 'cwd.txt'
        self.stale_log = Path(self.temp.name) / 'stale.txt'
        bin_dir = Path(self.temp.name) / 'bin'
        bin_dir.mkdir()
        self.write_executable(bin_dir / 'cargo', '''#!/usr/bin/env bash
if [[ "${1:-}" == --version ]]; then
  printf 'cargo %s (fixture)\\n' "${FAKE_CARGO_VERSION:-1.95.0}"
  exit 0
fi
printf '%s\\n' raw-cargo "$@" > "$ARGV_LOG"
exit 0
''')
        self.write_executable(bin_dir / 'perl-ci-hygiene', '''#!/usr/bin/env bash
printf 'stale PATH binary invoked\\n' > "$STALE_LOG"
exit 0
''')
        self.write_executable(scripts / 'cargo-admitted', '''#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$@" > "$ARGV_LOG"
printf '%s\\n' "$PWD" > "$CWD_LOG"
if [[ ${GIT_DIR+x} || ${GIT_WORK_TREE+x} || ${GIT_COMMON_DIR+x} ]]; then exit 44; fi
exit "${ADMISSION_EXIT:-0}"
''')
        self.env = os.environ.copy()
        self.env.update(PATH=str(bin_dir) + os.pathsep + self.env.get('PATH', ''),
                        ARGV_LOG=str(self.log), CWD_LOG=str(self.cwd_log),
                        STALE_LOG=str(self.stale_log))
        for name in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR'):
            self.env.pop(name, None)

    @staticmethod
    def write_executable(path, contents):
        path.write_text(contents, encoding='utf-8', newline='\n')
        path.chmod(0o755)

    def run_wrapper(self, name='install-githooks.sh', args=(), **overrides):
        env = dict(self.env, **overrides)
        return subprocess.run([self.bash, str(self.root / 'scripts' / name), *args],
                              cwd=self.temp.name, env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def arguments(self):
        return self.log.read_text().splitlines()

    def expected(self, operation):
        return ['run', '-p', 'perl-ci-hygiene', '--bin', 'perl-ci-hygiene',
                '--profile', 'agent', '--locked', '--', operation]

    def test_install_uses_literal_admitted_graph(self):
        result = self.run_wrapper()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.arguments(), self.expected('install-githooks'))

    def test_check_is_the_read_only_owner(self):
        result = self.run_wrapper('check-githooks.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.arguments(), self.expected('check-githooks'))

    def test_script_revision_root_and_git_selectors(self):
        result = self.run_wrapper(GIT_DIR='foreign.git', GIT_WORK_TREE='foreign tree',
                                  GIT_COMMON_DIR='foreign common')
        self.assertEqual(result.returncode, 0, result.stderr)
        # Bash reports a native MSYS spelling on Windows; ask that same Bash
        # for the expected root rather than folding distinct path identities.
        expected = subprocess.check_output([self.bash, '-c', 'cd -- "$1" && pwd',
                                            'fixture', str(self.root)], text=True).strip()
        self.assertEqual(self.cwd_log.read_text().strip(), expected)

    def test_preflight_and_budget_preserve_identity(self):
        budget = str(Path(self.temp.name) / 'budget with spaces.json')
        for operation, name in [('install-githooks', 'install-githooks.sh'),
                                ('check-githooks', 'check-githooks.sh')]:
            for options in (['--preflight', '--budget-file', budget],
                            ['--budget-file', budget, '--preflight']):
                with self.subTest(operation=operation, options=options):
                    result = self.run_wrapper(name, [*options, '--help'])
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(self.arguments(), [*options, *self.expected(operation), '--help'])

    def test_invalid_wrapper_options_refuse_before_admission(self):
        for options in (['--budget-file'], ['--budget-file', ''],
                        ['--preflight', '--preflight'],
                        ['--budget-file', 'one', '--budget-file', 'two']):
            with self.subTest(options=options):
                self.log.unlink(missing_ok=True)
                result = self.run_wrapper(args=options)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertFalse(self.log.exists())

    def test_failure_status_and_stale_path_binary(self):
        for name in ('install-githooks.sh', 'check-githooks.sh'):
            for status in ('37', '75'):
                with self.subTest(name=name, status=status):
                    result = self.run_wrapper(name, ADMISSION_EXIT=status)
                    self.assertEqual(result.returncode, int(status), result.stderr)
                    self.assertFalse(self.stale_log.exists())

    def test_missing_admission_cannot_succeed(self):
        (self.root / 'scripts' / 'cargo-admitted').unlink()
        result = self.run_wrapper()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())
        self.assertFalse(self.stale_log.exists())

    def test_missing_delegate_cannot_succeed(self):
        (self.root / 'scripts' / 'githooks-bootstrap.sh').unlink(missing_ok=True)
        result = self.run_wrapper()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_stale_cargo_guard_still_refuses(self):
        shutil.copy2(ROOT / 'scripts' / 'cargo-admitted', self.root / 'scripts' / 'cargo-admitted')
        result = self.run_wrapper(FAKE_CARGO_VERSION='1.75.0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('cargo-toolchain-guard: REFUSED', result.stderr)
        self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()
