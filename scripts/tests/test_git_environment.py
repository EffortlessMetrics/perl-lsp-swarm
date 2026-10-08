"""Foreign-repository Git commands must not mutate a calling hook's checkout."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from git_environment import isolated_git_env

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
else:
    result = OrphanClassificationTests()._git(str(root), 'config', '--local', 'fixture.marker', 'target')
    assert result.returncode == 0, result.stderr
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
        for boundary in ('production', 'verification_fixture', 'orphan_fixture'):
            with self.subTest(boundary=boundary), tempfile.TemporaryDirectory() as temp:
                target, sentinel = Path(temp) / 'target', Path(temp) / 'sentinel'
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
