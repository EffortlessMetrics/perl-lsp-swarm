"""Foreign-repository Git commands must not mutate a calling hook's checkout."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from git_environment import isolated_git_env


class GitEnvironmentSchemaTests(unittest.TestCase):
    """Only Git's selector names may be reused, never a caller environment."""

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.executable = Path(temp.name) / 'git-fixture'
        self.executable.write_bytes(b'fixture executable identity')

    def test_reuses_schema_but_reads_a_fresh_environment_each_time(self):
        names = 'GIT_DIR\nGIT_WORK_TREE\nGIT_CONFIG_COUNT\n'
        with patch('shutil.which', return_value=str(self.executable)), \
             patch('git_environment.subprocess.check_output', return_value=names) as probe, \
             patch.dict(os.environ, {'GIT_DIR': 'first', 'KEEP': 'first'}, clear=True):
            first = isolated_git_env()
            first['KEEP'] = 'mutated result'
            os.environ.update(GIT_DIR='second', GIT_WORK_TREE='foreign', KEEP='second',
                              GIT_CONFIG_COUNT='1', GIT_CONFIG_KEY_42='core.bare',
                              GIT_CONFIG_VALUE_42='true')
            before = dict(os.environ)
            second = isolated_git_env()
            self.assertEqual(second, {'KEEP': 'second'})
            self.assertEqual(dict(os.environ), before)
            self.assertEqual(probe.call_count, 1)
            self.assertEqual(probe.call_args.kwargs['executable'],
                             os.path.realpath(self.executable))

    def test_changing_selected_git_refreshes_the_name_set(self):
        other = self.executable.with_name('other-git')
        other.write_bytes(b'other executable')
        with patch('shutil.which', side_effect=[str(self.executable), str(other), str(self.executable)]), \
             patch('git_environment.subprocess.check_output',
                   side_effect=['GIT_DIR\n', 'GIT_DIR\nGIT_NEW_SELECTOR\n', 'GIT_DIR\n']) as probe, \
             patch.dict(os.environ, {'GIT_DIR': 'foreign', 'GIT_NEW_SELECTOR': 'new'}, clear=True):
            self.assertEqual(isolated_git_env(), {'GIT_NEW_SELECTOR': 'new'})
            self.assertEqual(isolated_git_env(), {})
            self.assertEqual(isolated_git_env(), {'GIT_NEW_SELECTOR': 'new'})
            self.assertEqual(probe.call_count, 2)

    def test_replacing_selected_executable_refreshes_the_name_set(self):
        with patch('shutil.which', return_value=str(self.executable)), \
             patch('git_environment.subprocess.check_output',
                   side_effect=['GIT_DIR\n', 'GIT_DIR\nGIT_NEW_SELECTOR\n']) as probe, \
             patch.dict(os.environ, {'GIT_DIR': 'foreign', 'GIT_NEW_SELECTOR': 'new'}, clear=True):
            self.assertEqual(isolated_git_env(), {'GIT_NEW_SELECTOR': 'new'})
            self.executable.write_bytes(b'replaced executable with a different size')
            self.assertEqual(isolated_git_env(), {})
            self.assertEqual(probe.call_count, 2)

    def test_discovery_failure_is_not_cached_and_retry_stays_fail_closed(self):
        failure = subprocess.CalledProcessError(128, ['git', 'rev-parse', '--local-env-vars'])
        with patch('shutil.which', return_value=str(self.executable)), \
             patch('git_environment.subprocess.check_output',
                   side_effect=[failure, 'GIT_DIR\n']) as probe, \
             patch.dict(os.environ, {'GIT_DIR': 'foreign', 'KEEP': 'value'}, clear=True):
            with self.assertRaises(subprocess.CalledProcessError):
                isolated_git_env()
            self.assertEqual(isolated_git_env(), {'KEEP': 'value'})
            self.assertEqual(probe.call_count, 2)

    def test_missing_git_does_not_reuse_a_previous_schema(self):
        with patch('shutil.which', side_effect=[str(self.executable), None]), \
             patch('git_environment.subprocess.check_output', return_value='GIT_DIR\n'), \
             patch.dict(os.environ, {'GIT_DIR': 'foreign'}, clear=True):
            self.assertEqual(isolated_git_env(), {})
            with self.assertRaises(FileNotFoundError):
                isolated_git_env()

    def test_empty_discovery_is_not_treated_as_an_environment_without_selectors(self):
        with patch('shutil.which', return_value=str(self.executable)), \
             patch('git_environment.subprocess.check_output',
                   side_effect=['\n', 'GIT_DIR\n']) as probe, \
             patch.dict(os.environ, {'GIT_DIR': 'foreign'}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'empty local environment-variable schema'):
                isolated_git_env()
            self.assertEqual(isolated_git_env(), {})
            self.assertEqual(probe.call_count, 2)

    def test_disappearing_executable_does_not_reuse_a_previous_schema(self):
        with patch('shutil.which', return_value=str(self.executable)), \
             patch('git_environment.subprocess.check_output', return_value='GIT_DIR\n'), \
             patch.dict(os.environ, {'GIT_DIR': 'foreign'}, clear=True):
            self.assertEqual(isolated_git_env(), {})
            self.executable.unlink()
            with self.assertRaises(FileNotFoundError):
                isolated_git_env()

CHILD = r'''
import pathlib, sys
scripts, root, boundary = sys.argv[1:]
sys.path[:0] = [scripts, str(pathlib.Path(scripts) / 'tests')]
from check_release_tag_provenance import _git, verify_git_refs
from test_release_tag_provenance import GitVerificationTests, OrphanClassificationTests
root = pathlib.Path(root)
if boundary == 'production':
    result = _git(root, 'config', '--local', 'fixture.marker', 'target')
    assert result.returncode == 0, result.stderr
elif boundary == 'verification_fixture':
    GitVerificationTests().run_git(root, 'config', '--local', 'fixture.marker', 'target')
elif boundary == 'orphan_fixture':
    result = OrphanClassificationTests()._git(str(root), 'config', '--local', 'fixture.marker', 'target')
    assert result.returncode == 0, result.stderr
elif boundary == 'orphan_init':
    OrphanClassificationTests()._init(str(root))
    result = _git(root, 'rev-parse', 'HEAD')
    assert result.returncode == 0, result.stderr
    assert _git(root, 'config', 'user.email').stdout.strip() == 't@x'
    assert _git(root, 'tag', 'v0.2.0').returncode == 0
    assert _git(root, 'config', '--local', 'fixture.marker', 'target').returncode == 0
elif boundary == 'hash_object':
    OrphanClassificationTests().test_flagged_row_with_non_commit_pin_is_drift()
    assert _git(root, 'config', '--local', 'fixture.marker', 'target').returncode == 0
head = _git(root, 'rev-parse', 'HEAD').stdout.strip()
manifest = {'tag': [{'name': 'v0.2.0', 'current_sha': head, 'lineage': 'root'}]}
assert verify_git_refs(manifest, root) == ([], []), verify_git_refs(manifest, root)
'''


def snapshot(root):
    return {
        str(path.relative_to(root)): path.read_bytes()
        for path in root.rglob('*')
        if path.is_file()
    }


class GitEnvironmentTests(unittest.TestCase):
    def git(self, root, *args):
        return subprocess.check_output(
            ['git', '-C', str(root), *args], env=isolated_git_env(),
            text=True, stderr=subprocess.PIPE,
        ).strip()

    def init(self, root, tag):
        root.mkdir()
        self.git(root, 'init', '-q')
        self.git(root, 'config', 'user.name', 'Git isolation fixture')
        self.git(root, 'config', 'user.email', 'fixture@example.invalid')
        self.git(root, 'config', 'commit.gpgsign', 'false')
        (root / 'value.txt').write_text(tag)
        self.git(root, 'add', 'value.txt')
        self.git(root, 'commit', '-qm', 'fixture')
        self.git(root, 'tag', tag)

    def test_explicit_roots_preserve_sentinel_under_hook_environment(self):
        # All hostile values exist only in the re-entered child. Neither the
        # test runner nor a parallel test's environment is changed.
        for boundary in ('production', 'verification_fixture', 'orphan_fixture',
                         'orphan_init', 'hash_object'):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as temp:
                target, sentinel = Path(temp) / 'target', Path(temp) / 'sentinel'
                if boundary == 'orphan_init':
                    target.mkdir()
                else:
                    self.init(target, 'v0.2.0')
                self.init(sentinel, 'v0.1.0')
                before = snapshot(sentinel)
                env = isolated_git_env()
                env.update(
                    GIT_DIR=str(sentinel / '.git'), GIT_COMMON_DIR=str(sentinel / '.git'),
                    GIT_WORK_TREE=str(sentinel), GIT_INDEX_FILE=str(sentinel / '.git/index'),
                    GIT_OBJECT_DIRECTORY=str(sentinel / '.git/objects'),
                    GIT_ALTERNATE_OBJECT_DIRECTORIES=str(sentinel / '.git/objects'),
                    GIT_CONFIG_COUNT='1', GIT_CONFIG_KEY_0='core.bare', GIT_CONFIG_VALUE_0='true',
                )
                result = subprocess.run(
                    [sys.executable, '-c', CHILD, str(SCRIPTS), str(target), boundary],
                    env=env, capture_output=True, text=True,
                )
                self.assertEqual(before, snapshot(sentinel), 'calling repository changed')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.git(target, 'config', '--local', 'fixture.marker'), 'target')


if __name__ == '__main__':
    unittest.main()
