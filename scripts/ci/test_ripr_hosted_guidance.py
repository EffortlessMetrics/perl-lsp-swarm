import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HostedGuidance(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'scripts/ci').mkdir(parents=True)
        for name in ('ripr-hosted-guidance.sh', 'ripr-hosted-guidance.py'):
            shutil.copy(ROOT / 'scripts/ci' / name, self.root / 'scripts/ci' / name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.write('sudo', '''#!/bin/bash
printf '%s\\0' "$@" > "$LOG"
[ "${FAIL_SUDO:-0}" = 0 ] || exit 43
shift
exec "$@"
''')
        self.write('systemd-run', '''#!/bin/bash
[ "${FAIL_SCOPE:-0}" = 0 ] || exit 44
while [ "$1" != -- ]; do shift; done
shift
exec "$@"
''')
        self.write('cargo', '''#!/usr/bin/python3
import json, os, sys
with open(os.environ['OUT'], 'w') as f:
 json.dump({'argv': sys.argv[1:], 'canary': os.environ['CANARY'], 'empty': os.environ['EMPTY']}, f)
sys.exit(int(os.environ.get('EXIT_CODE', '0')))
''')
        self.write('bash', '''#!/bin/bash
if [ "$1" = scripts/ci/observe-xtask-build.sh ]; then
 printf '%s\\0' "$@" > "$WRAPPER_LOG"
 shift 4
 exec "$@"
fi
exec /bin/bash "$@"
''')
        self.env = {'PATH': str(self.bin) + ':/usr/bin:/bin', 'LOG': str(self.root/'log'),
                    'OUT': str(self.root/'out'), 'WRAPPER_LOG': str(self.root/'wrapper'), 'CANARY': 'private-unique\nvalue=$!', 'EMPTY': '',
                    'RIPR_BASE_SHA': 'base', 'RIPR_EVALUATED_SHA': 'head', 'PR_HEAD_SHA': 'pr',
                    'RIPR_FRESHNESS_HANDOFF': 'handoff'}

    def write(self, name, body):
        path = self.bin / name
        path.write_text(body)
        path.chmod(0o755)

    def run_mode(self, mode):
        return subprocess.run(['/bin/bash', 'scripts/ci/ripr-hosted-guidance.sh', mode],
                              cwd=self.root, env=self.env, capture_output=True)

    def test_both_paths_preserve_environment_without_argv_or_log_leaks(self):
        for mode in ('primary', 'fallback'):
            with self.subTest(mode=mode):
                (self.root/'wrapper').unlink(missing_ok=True)
                result = self.run_mode(mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((self.root/'wrapper').exists(), mode == 'primary')
                if mode == 'primary':
                    self.assertTrue((self.root/'wrapper').read_bytes().startswith(b'scripts/ci/observe-xtask-build.sh\0host-guidance\0handoff/xtask-builds\0--\0'))
                observed = json.loads((self.root/'out').read_text())
                self.assertEqual(observed['canary'], self.env['CANARY'])
                self.assertEqual(observed['empty'], '')
                self.assertEqual(observed['argv'], ['xtask', 'ripr-review-comments', '--base', 'base',
                                 '--head', 'head', '--pr-head', 'pr', '--timeout-seconds', '3600'])
                recorded = (self.root/'log').read_bytes() + result.stdout + result.stderr
                self.assertNotIn(self.env['CANARY'].encode(), recorded)
                self.assertNotIn(b'--setenv', recorded)
                self.assertIn(b'MemoryMax=14G', recorded)
                self.assertIn(b'MemorySwapMax=0', recorded)
                self.assertIn(b'--uid=', recorded)
                self.assertIn(b'--gid=', recorded)

    def test_scope_and_guidance_failure_propagate(self):
        for key, code in [('FAIL_SUDO',43), ('FAIL_SCOPE',44), ('EXIT_CODE',37)]:
            for mode in ('primary','fallback'):
                self.env[key] = str(code if key == 'EXIT_CODE' else 1)
                (self.root/'out').unlink(missing_ok=True)
                result = self.run_mode(mode)
                self.assertEqual(result.returncode, code)
                self.assertEqual((self.root/'out').exists(), key == 'EXIT_CODE')
                del self.env[key]

    def test_incomplete_snapshot_fails_closed(self):
        path = self.root/'snapshot'
        path.write_bytes(b'broken')
        result = subprocess.run(['/usr/bin/python3', str(self.root/'scripts/ci/ripr-hosted-guidance.py'),
                                 str(path), 'fallback'], env=self.env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root/'out').exists())
        self.assertFalse(path.exists())

if __name__ == '__main__':
    unittest.main()
