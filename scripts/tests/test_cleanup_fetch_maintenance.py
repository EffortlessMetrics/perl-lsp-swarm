"""Real Git maintenance regression; every mutation is inside owned fixtures."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[2]


class FetchMaintenanceTests(unittest.TestCase):
    def test_real_fetch_preserves_expired_registration(self):
        bash = Path('C:/Program Files/Git/bin/bash.exe') if os.name == 'nt' else Path('/bin/bash')
        if not bash.exists():
            self.skipTest('Git Bash not installed')
        with tempfile.TemporaryDirectory(prefix='fetch-maintenance-proof-') as directory:
            root = Path(directory)
            env = os.environ.copy()
            env.update(GIT_CONFIG_GLOBAL=str(root/'no-global'), GIT_CONFIG_NOSYSTEM='1',
                       GIT_TERMINAL_PROMPT='0', CLEANUP_BASE_BRANCH='main')
            def git(*args, cwd=root):
                return subprocess.check_output(['git', *args], cwd=cwd, env=env, text=True, stderr=subprocess.STDOUT)
            git('init', '--bare', 'remote.git')
            git('init', '-b', 'main', 'repo')
            repo = root/'repo'
            git('config', 'user.name', 'fixture', cwd=repo)
            git('config', 'user.email', 'fixture@example.invalid', cwd=repo)
            (repo/'source').write_text('preserved source', encoding='utf-8')
            git('add', '.', cwd=repo)
            git('commit', '-m', 'fixture', cwd=repo)
            git('remote', 'add', 'origin', str(root/'remote.git'), cwd=repo)
            git('push', 'origin', 'main', cwd=repo)
            for key, value in [('gc.worktreePruneExpire', 'now'), ('gc.auto', '1'),
                               ('gc.autoDetach', 'false'), ('maintenance.auto', 'true')]:
                git('config', key, value, cwd=repo)
            def seed():
                # Git's cheap loose-object estimate samples fanout 17. Populate
                # only a few real valid blobs there; no large pack/build needed.
                count = 0
                for n in range(100000):
                    payload = ('fixture-' + str(n)).encode()
                    obj = b'blob ' + str(len(payload)).encode() + b'\0' + payload
                    digest = hashlib.sha1(obj).hexdigest()
                    if digest.startswith('17'):
                        path = repo/'.git/objects'/digest[:2]/digest[2:]
                        path.parent.mkdir(exist_ok=True)
                        path.write_bytes(zlib.compress(obj))
                        count += 1
                        if count == 8:
                            return
                self.fail('could not seed loose object trigger')
            git('worktree', 'add', '-b', 'control', str(root/'control'), cwd=repo)
            (root/'control').rename(root/'control-preserved')
            seed()
            git('fetch', '--quiet', 'origin', 'main', cwd=repo)
            lost = not (repo/'.git/worktrees/control').exists()
            self.assertTrue(lost, 'negative control did not reproduce automatic registration pruning')
            git('worktree', 'add', '-b', 'guarded', str(root/'guarded'), cwd=repo)
            (root/'guarded').rename(root/'guarded-preserved')
            seed()
            # Use actual production entrypoint; no other disposable worktree is
            # reachable, so its report retains missing paths for review.
            script = ROOT/'scripts/cleanup-completed-worktrees.sh'
            command = [str(bash), '-c', 'export PATH=/usr/bin:/mingw64/bin:$PATH; bash "$1"', 'proof', str(script)]
            result = subprocess.run(command, cwd=repo, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertTrue((repo/'.git/worktrees/guarded').exists())
            self.assertEqual(git('config', 'gc.worktreePruneExpire', cwd=repo).strip(), 'now')
            self.assertEqual(git('config', 'maintenance.auto', cwd=repo).strip(), 'true')
            self.assertEqual((root/'guarded-preserved/source').read_text(), 'preserved source')
            print('unguarded fetch removed expired registration:', lost)
            # Control reproduction above is mandatory: preserving a registration
            # without exercising the hazard would not establish this regression.


if __name__ == '__main__':
    unittest.main()
