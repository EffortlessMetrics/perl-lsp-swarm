"""Execute the skill's read-only setup observation against real Git layouts."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
SKILL = REPO / '.agents/skills/worktree-manager/SKILL.md'


def observation_source():
    text = SKILL.read_text()
    return text.split('```python\n# setup-observation:', 1)[1].split('\n```', 1)[0].split('\n', 1)[1]


class SetupExamples(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='setup-contract-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        self.env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
                        GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid')
        self.template = self.base / 'empty-template'
        self.template.mkdir()
        self.repo = self.base / 'repo'
        self.git('init', '-q', '--template', str(self.template), str(self.repo))
        (self.repo / 'scripts').mkdir()
        shutil.copyfile(REPO / 'scripts/cargo_admitted.py', self.repo / 'scripts/cargo_admitted.py')
        self.git('add', 'scripts', cwd=self.repo)
        self.git('commit', '-q', '-m', 'fixture', cwd=self.repo)
        self.git('remote', 'add', 'origin', str(self.base / 'fixture-origin'), cwd=self.repo)
        for name, leaf in [('DEVPLANE', 'plane'), ('CARGO_HOME', 'cargo'), ('TMPDIR', 'tmp')]:
            self.env[name] = str(self.base / 'unallocated storage' / leaf)
        self.source = observation_source()

    def git(self, *args, cwd=None):
        return subprocess.check_output(['git', *args], cwd=cwd or self.base,
                                       env=self.env, stderr=subprocess.PIPE).removesuffix(b'\n')

    def observe(self, cwd, source=None, env=None):
        result = subprocess.run([sys.executable, '-B', '-c', source or self.source],
                                cwd=cwd, env=env or self.env, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        value, end = json.JSONDecoder().raw_decode(result.stdout.decode())
        listing = result.stdout.decode()[end:].lstrip('\n')
        self.assertEqual(listing.encode(), self.git('worktree', 'list', '--porcelain', '-z', cwd=cwd))
        return value

    def assert_identity(self, value, root, common):
        self.assertEqual(value['root'], str(root.resolve()))
        self.assertEqual(value['common_dir'], str(common.resolve()))
        self.assertEqual(value['head'], self.git('rev-parse', 'HEAD', cwd=root).decode())
        self.assertTrue(value['user'])
        self.assertEqual(value['origin'], self.git('remote', 'get-url', 'origin', cwd=root).decode())

    def test_clone_root_nested_measurements_and_no_provisioning(self):
        nested = self.repo / 'nested'
        nested.mkdir()
        before = sorted(str(p.relative_to(self.repo)) for p in self.repo.rglob('*'))
        value = self.observe(nested)
        self.assert_identity(value, self.repo, self.repo / '.git')
        for key, env_name in [('cargo_home', 'CARGO_HOME'), ('temp', 'TMPDIR')]:
            row = value['resources'][key]
            self.assertEqual(row['path'], self.env[env_name])
            self.assertEqual(row['observed_ancestor'], str(self.base))
            self.assertGreater(row['total_bytes'], 0)
            self.assertGreaterEqual(row['free_bytes'], 0)
            self.assertLessEqual(row['free_bytes'], row['total_bytes'])
        self.assertTrue(value['resources']['slot']['path'].startswith(self.env['DEVPLANE'] + os.sep))
        self.assertFalse((self.base / 'unallocated storage').exists())
        self.assertEqual(before, sorted(str(p.relative_to(self.repo)) for p in self.repo.rglob('*')))
        self.assertNotEqual(self.git('rev-parse', '--git-dir', cwd=self.repo), b'')

    def test_linked_worktrees_share_slot_but_not_outputs(self):
        sibling = self.base / 'linked tree'
        self.git('worktree', 'add', '-q', '-b', 'candidate', str(sibling), cwd=self.repo)
        original, linked = self.observe(self.repo), self.observe(sibling / 'scripts')
        self.assert_identity(linked, sibling, self.repo / '.git')
        self.assertEqual(linked['branch'], 'candidate')
        self.assertEqual(original['resources']['slot']['path'], linked['resources']['slot']['path'])
        for key in ('target', 'build'):
            self.assertNotEqual(original['resources'][key]['path'], linked['resources'][key]['path'])
        mutant = self.source.replace('common_dir=str(common)', 'common_dir=str(root / ".git")')
        wrong = self.observe(sibling, mutant)
        with self.assertRaises(AssertionError):
            self.assert_identity(wrong, sibling, self.repo / '.git')

    def test_independent_clone_has_own_exclusion_domain(self):
        clone = self.base / 'clone'
        self.git('clone', '-q', '--no-hardlinks', '--template', str(self.template), str(self.repo), str(clone))
        first, second = self.observe(self.repo), self.observe(clone)
        self.assert_identity(second, clone, clone / '.git')
        self.assertNotEqual(first['resources']['slot']['path'], second['resources']['slot']['path'])

    def test_separate_git_directory_is_not_root_dot_git(self):
        separate = self.base / 'external git metadata'
        self.git('init', '-q', '--separate-git-dir', str(separate), str(self.repo))
        value = self.observe(self.repo)
        self.assert_identity(value, self.repo, separate)
        self.assertTrue((self.repo / '.git').is_file())

    @unittest.skipIf(os.name == 'nt', 'POSIX newline path identity')
    def test_trailing_newline_in_root_survives(self):
        linked = self.base / 'linked\n'
        self.git('worktree', 'add', '-q', '-b', 'newline', str(linked), cwd=self.repo)
        self.assert_identity(self.observe(linked), linked, self.repo / '.git')
        mutant = self.source.replace('os.fsdecode(value[:-1])', 'os.fsdecode(value).strip()')
        result = subprocess.run([sys.executable, '-B', '-c', mutant], cwd=linked,
                                env=self.env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'FileNotFoundError', result.stderr)

    def test_missing_selection_and_git_redirection_refuse(self):
        for name in ('DEVPLANE', 'CARGO_HOME', 'TMPDIR', 'GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR'):
            with self.subTest(name=name):
                env = dict(self.env)
                if name.startswith('GIT_'):
                    env[name] = str(self.repo / '.git')
                    expected = 'unset ' + name
                else:
                    env.pop(name)
                    expected = 'select ' + name
                result = subprocess.run([sys.executable, '-B', '-c', self.source], cwd=self.repo,
                                        env=env, capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected.encode(), result.stderr)
                self.assertFalse((self.base / 'unallocated storage').exists())

    def test_task_subshell_selection_example(self):
        shell = SKILL.read_text().split('```bash\n', 1)[1].split('\n```', 1)[0]
        marker = '  # Run the observation below, then the separately admitted task commands here.'
        shell = shell.replace(marker, "  python3 -B - <<'PYOBS'\n" + self.source + "\nPYOBS")
        env = dict(self.env, SETUP_STORAGE_ROOT=str(self.base / 'selected storage'))
        result = subprocess.run(['bash', '-c', shell], cwd=self.repo, env=env, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        value, _ = json.JSONDecoder().raw_decode(result.stdout.decode())
        self.assertEqual(value['resources']['cargo_home']['path'], env['SETUP_STORAGE_ROOT'] + '/cargo')
        self.assertEqual(value['resources']['temp']['path'], env['SETUP_STORAGE_ROOT'] + '/tmp')
        self.assertTrue(value['resources']['slot']['path'].startswith(env['SETUP_STORAGE_ROOT'] + '/devplane/'))
        self.assertFalse((self.base / 'selected storage').exists())
        env.pop('SETUP_STORAGE_ROOT')
        refused = subprocess.run(['bash', '-c', shell], cwd=self.repo, env=env, capture_output=True)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn(b'SETUP_STORAGE_ROOT', refused.stderr)

    def test_foreign_output_override_still_refuses(self):
        env = dict(self.env, CARGO_TARGET_DIR=str(self.base / 'foreign target'))
        result = subprocess.run([sys.executable, '-B', '-c', self.source], cwd=self.repo,
                                env=env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'CARGO_TARGET_DIR override unsupported', result.stderr)
        self.assertFalse((self.base / 'foreign target').exists())


if __name__ == '__main__':
    unittest.main()
