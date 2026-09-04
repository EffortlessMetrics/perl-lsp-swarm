'use strict';

/**
 * Structural contract for the checked Extension Development Host F5
 * workflow (#9851).
 *
 * These tests prove the launch/task wiring without launching an editor:
 *
 *   - the one-shot F5 launch runs the checked `build` task (authority ->
 *     inventory -> all-config type-check -> bundle), so a stale
 *     `out/extension.js` is impossible and a type error blocks the host;
 *   - the watch F5 launch consumes ONLY the canonical `npm run dev`
 *     supervisor service from #9848 through a background task whose ready
 *     edge is the supervisor's own stable `[dev-supervisor] ready` line —
 *     it must never re-parse raw tsc/Rolldown watcher state;
 *   - the host opens the bounded dev workspace, not arbitrary user state;
 *   - breakpoints bind because the bundle keeps its source map;
 *   - the authoring surface stays out of the VSIX;
 *   - the repository-root Rust/Jest launch contracts are untouched.
 */

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const { REPORT_SCOPE, readyMessage, startingMessage } = require('./dev-supervisor');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..');

const launchJson = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, '.vscode', 'launch.json'), 'utf8'),
);
const tasksJson = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, '.vscode', 'tasks.json'), 'utf8'),
);
const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));

const BUILD_TASK_LABEL = 'perl-lsp: checked build';
const DEV_TASK_LABEL = 'perl-lsp: dev watch (supervisor)';
const ONE_SHOT_LAUNCH_NAME = 'Run Extension (checked build)';
const WATCH_LAUNCH_NAME = 'Run Extension (dev watch)';
const DEV_WORKSPACE_ARG = '${workspaceFolder}/.vscode/dev-workspace';

/** @param {string} name */
function launchByName(name) {
  const configuration = launchJson.configurations.find((entry) => entry.name === name);
  assert.ok(configuration, `launch "${name}" is missing from .vscode/launch.json`);
  return configuration;
}

/** @param {string} label */
function taskByLabel(label) {
  const task = tasksJson.tasks.find((entry) => entry.label === label);
  assert.ok(task, `task "${label}" is missing from .vscode/tasks.json`);
  return task;
}

void test('the one-shot F5 launch loads this checkout through the checked build task', () => {
  const launch = launchByName(ONE_SHOT_LAUNCH_NAME);
  assert.equal(launch.type, 'extensionHost');
  assert.equal(launch.request, 'launch');
  assert.equal(launch.preLaunchTask, BUILD_TASK_LABEL);
  assert.ok(
    launch.args.includes('--extensionDevelopmentPath=${workspaceFolder}'),
    'the launch must develop THIS checkout, not an installed extension',
  );
  assert.ok(
    launch.args.includes(DEV_WORKSPACE_ARG),
    'the launch must open the bounded dev workspace, not arbitrary user state',
  );
  assert.ok(
    launch.outFiles.some((pattern) => pattern === '${workspaceFolder}/out/**/*.js'),
    'the launch must load the development bundle output',
  );
});

void test('the checked build task is the fail-closed npm build with the tsc matcher', () => {
  const task = taskByLabel(BUILD_TASK_LABEL);
  assert.equal(task.type, 'npm');
  assert.equal(task.script, 'build');
  assert.equal(
    packageJson.scripts.build,
    'npm run typecheck:authority && npm run check:tsconfig-inventory && npm run typecheck:all && npm run bundle',
    'the pre-launch build must be the exact checked build the command contract pins',
  );
  assert.ok(
    Array.isArray(task.problemMatcher) && task.problemMatcher.includes('$tsc'),
    'type errors must surface as problems so a red build blocks the host',
  );
  assert.equal(task.isBackground, undefined, 'the checked build is a one-shot foreground task');
});

void test('the watch F5 launch consumes only the #9848 supervisor-ready edge', () => {
  const launch = launchByName(WATCH_LAUNCH_NAME);
  assert.equal(launch.type, 'extensionHost');
  assert.equal(launch.preLaunchTask, DEV_TASK_LABEL);
  assert.notEqual(launch.preLaunchTask, BUILD_TASK_LABEL);
  assert.ok(
    launch.args.includes('--extensionDevelopmentPath=${workspaceFolder}') &&
      launch.args.includes(DEV_WORKSPACE_ARG),
  );

  const task = taskByLabel(DEV_TASK_LABEL);
  assert.equal(task.type, 'npm');
  assert.equal(task.script, 'dev');
  assert.equal(packageJson.scripts.dev, 'node scripts/dev-supervisor.js');
  assert.equal(task.isBackground, true, 'the dev service is a long-running background task');

  // Pure-consumer boundary: the editor config must not re-implement or
  // re-parse the dual-watcher lifecycle that #9848 owns.
  const serialized = JSON.stringify(task);
  for (const forbidden of ['watch:types', 'watch:bundle', 'governed-tsc', 'rolldown', '$tsc']) {
    assert.ok(
      !serialized.includes(forbidden),
      `the dev watch task must not reference "${forbidden}" — watcher state belongs to the supervisor`,
    );
  }

  const matcher = task.problemMatcher;
  assert.ok(matcher && typeof matcher === 'object' && !Array.isArray(matcher));
  assert.ok(matcher.background, 'a background task needs a background problem matcher');
  assert.equal(matcher.background.activeOnStart, true);
  const begins = new RegExp(matcher.background.beginsPattern);
  const ends = new RegExp(matcher.background.endsPattern);

  // The ready edge is the supervisor's own emitted line, matched as the
  // module emits it — the single source of truth for both watchers' health.
  assert.ok(
    begins.test(`[${REPORT_SCOPE}] ${startingMessage('types')}`),
    'beginsPattern must match the supervisor starting line',
  );
  assert.ok(
    ends.test(`[${REPORT_SCOPE}] ${readyMessage(2, 2)}`),
    'endsPattern must match the supervisor combined-ready line',
  );
  assert.ok(
    !ends.test(`[${REPORT_SCOPE}] ${startingMessage('bundle')}`),
    'endsPattern must not fire on spawn (spawn is not readiness)',
  );
  assert.ok(
    !ends.test(`[${REPORT_SCOPE}] watcher "types" ready`),
    'endsPattern must not fire on a single watcher — readiness requires BOTH',
  );
});

void test('the bounded dev workspace exists and contains a minimal Perl file', () => {
  const sample = path.join(extensionRoot, '.vscode', 'dev-workspace', 'sample.pl');
  const source = fs.readFileSync(sample, 'utf8');
  assert.match(
    source,
    /print /,
    'the bounded workspace should contain a minimal runnable Perl file',
  );
});

void test('breakpoint binding stays provable: the bundle keeps its source map', () => {
  const rolldownConfig = fs.readFileSync(path.join(extensionRoot, 'rolldown.config.mjs'), 'utf8');
  assert.match(rolldownConfig, /sourcemap: true/, 'the dev bundle must keep its source map');
  assert.match(rolldownConfig, /file: 'out\/extension\.js'/);
  assert.equal(packageJson.main, './out/extension.js');
  const launch = launchByName(ONE_SHOT_LAUNCH_NAME);
  assert.ok(launch.outFiles.includes('${workspaceFolder}/out/**/*.js'));
});

void test('the F5 authoring surface is excluded from the VSIX', () => {
  const vscodeignore = fs.readFileSync(path.join(extensionRoot, '.vscodeignore'), 'utf8');
  const excluded = vscodeignore
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith('#'));
  assert.ok(
    excluded.includes('.vscode/**'),
    '.vscode authoring files must stay out of the packaged VSIX',
  );
});

void test('the repository-root Rust and Jest launch contracts are preserved', () => {
  const rootLaunch = fs.readFileSync(path.join(repositoryRoot, '.vscode', 'launch.json'), 'utf8');
  const rootTasks = fs.readFileSync(path.join(repositoryRoot, '.vscode', 'tasks.json'), 'utf8');
  assert.match(rootLaunch, /"type": "lldb"/, 'the root Rust LLDB launches must remain');
  assert.match(rootLaunch, /Debug perllsp \(stdio\)/);
  assert.match(rootLaunch, /Debug Extension \(Jest\)/);
  assert.match(rootTasks, /vscode-extension: compile:test/);
  assert.doesNotMatch(
    rootLaunch,
    /extensionHost/,
    'the Extension Development Host launch belongs to vscode-extension/.vscode, not the root',
  );
});
