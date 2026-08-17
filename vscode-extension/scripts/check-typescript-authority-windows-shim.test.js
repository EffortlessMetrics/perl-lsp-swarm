'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const test = require('node:test');
const { resolveGeneratedShimTarget } = require('./check-typescript-authority');

const windowsBinDir =
  'D:\\a\\perl-lsp-swarm\\perl-lsp-swarm\\vscode-extension\\node_modules\\.bin';
const windowsTsc =
  'D:\\a\\perl-lsp-swarm\\perl-lsp-swarm\\vscode-extension\\node_modules\\typescript\\bin\\tsc';

function assertTarget(result, expected) {
  assert.ok('target' in result, `expected a resolved target, received ${JSON.stringify(result)}`);
  assert.equal(result.target, expected);
}

void test('npm Windows %dp0% wrapper target stays relative to node_modules/.bin', () => {
  const wrapper = '@ECHO off\r\n"%dp0%\\node.exe" "%dp0%\\..\\typescript\\bin\\tsc" %*\r\n';
  assertTarget(resolveGeneratedShimTarget(wrapper, windowsBinDir, path.win32), windowsTsc);
});

void test('npm Windows %~dp0% wrapper target stays relative to node_modules/.bin', () => {
  const wrapper = '@ECHO off\r\n"%~dp0%\\node.exe" "%~dp0%\\..\\typescript\\bin\\tsc" %*\r\n';
  assertTarget(resolveGeneratedShimTarget(wrapper, windowsBinDir, path.win32), windowsTsc);
});

void test('POSIX generated wrapper target remains relative to node_modules/.bin', () => {
  const binDir = '/work/vscode-extension/node_modules/.bin';
  const wrapper =
    '#!/bin/sh\n' +
    'basedir=$(dirname "$(echo "$0" | sed -e \'s,\\\\,/,g\')")\n' +
    'if [ -x "$basedir/node" ]; then\n' +
    '  exec "$basedir/node"  "$basedir/../typescript/bin/tsc" "$@"\n' +
    'else\n' +
    '  exec node  "$basedir/../typescript/bin/tsc" "$@"\n' +
    'fi\n';
  assertTarget(
    resolveGeneratedShimTarget(wrapper, binDir, path.posix),
    '/work/vscode-extension/node_modules/typescript/bin/tsc',
  );
});

void test('a genuinely rooted Windows target is rejected rather than normalized', () => {
  const result = resolveGeneratedShimTarget(
    '@ECHO off\r\nnode "\\outside\\typescript\\bin\\tsc" %*\r\n',
    windowsBinDir,
    path.win32,
  );
  assert.ok('reason' in result, `expected a rejection, received ${JSON.stringify(result)}`);
  assert.match(result.reason, /rooted rather than relative/);
});

void test('a wrapper with no TypeScript target is rejected', () => {
  const result = resolveGeneratedShimTarget('@ECHO off\r\nnode other-tool.js %*\r\n', windowsBinDir, path.win32);
  assert.ok('reason' in result, `expected a rejection, received ${JSON.stringify(result)}`);
  assert.match(result.reason, /names no typescript\/bin\/tsc target/);
});
