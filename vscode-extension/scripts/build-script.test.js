const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const extensionRoot = path.resolve(__dirname, '..');
const buildScript = path.join(extensionRoot, 'build.sh');
const bashPath =
  process.platform === 'win32' ? 'C:\\Program Files\\Git\\bin\\bash.exe' : 'bash';
const steps = ['doctor', 'ci', 'build', 'bundle-lsp', 'package'];

function runBuild(failAt = '') {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-build-script-'));
  const fakeNpm = path.join(directory, 'npm');
  const logPath = path.join(directory, 'calls.log');
  fs.writeFileSync(
    fakeNpm,
    `#!/bin/sh
step="$1"
if [ "$1" = "run" ]; then step="$2"; fi
printf '%s\\n' "$step" >> "$FAKE_NPM_LOG"
if [ "$step" = "$FAKE_NPM_FAIL_AT" ]; then exit 23; fi
exit 0
`,
    { mode: 0o755 },
  );
  const result = spawnSync(bashPath, [buildScript], {
    cwd: extensionRoot,
    encoding: 'utf8',
    env: {
      ...process.env,
      PATH: `${directory};${process.env.PATH}`,
      FAKE_NPM_LOG: logPath,
      FAKE_NPM_FAIL_AT: failAt,
    },
    windowsHide: true,
  });
  const log = fs.existsSync(logPath)
    ? fs.readFileSync(logPath, 'utf8').trim().split(/\r?\n/).filter(Boolean)
    : [];
  fs.rmSync(directory, { recursive: true, force: true });
  return { result, log };
}

for (const step of steps) {
  void test(`build.sh stops when ${step} fails`, () => {
    const { result, log } = runBuild(step);
    assert.notEqual(result.status, 0, `${step} failure was reported as success`);
    assert.deepEqual(log, steps.slice(0, steps.indexOf(step) + 1));
  });
}

void test('build.sh completes every step when the tools succeed', () => {
  const { result, log } = runBuild();
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(log, steps);
});
