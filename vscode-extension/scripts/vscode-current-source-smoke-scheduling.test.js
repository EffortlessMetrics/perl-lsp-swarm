'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const repositoryRoot = path.resolve(__dirname, '..', '..');

void test('VS Code current-source smoke scheduling observer self-test (#6887)', () => {
  const result = spawnSync(
    'python3',
    ['-m', 'unittest', 'scripts.ci.test_vscode_current_source_smoke_scheduling'],
    {
      cwd: repositoryRoot,
      encoding: 'utf8',
      env: { ...process.env, PYTHONPATH: repositoryRoot },
    },
  );
  assert.equal(
    result.status,
    0,
    result.stderr || result.stdout || 'scheduling observer unittest failed',
  );
});
