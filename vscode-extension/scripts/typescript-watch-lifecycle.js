#!/usr/bin/env node
'use strict';

/** Bounded error/recovery proof for the repository-governed TypeScript watcher (#9857). */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');
const { StringDecoder } = require('node:string_decoder');
const { inspectPosixProcessGroup } = require('./dev-supervisor');

const PHASE_TIMEOUT_MS = 30_000;
const EXIT_TIMEOUT_MS = 10_000;
const OUTPUT_LIMIT = 32_768;
const COMPLETE = /Found (\d+) errors?\. Watching for file changes\./g;

/** Decode arbitrary chunks and emit one completion per new watch generation. */
class WatchGenerations {
  constructor() {
    this.decoder = new StringDecoder('utf8');
    this.buffer = '';
    this.generation = 0;
    this.events = [];
    this.listeners = new Set();
    this.failure = null;
    this.failures = new Set();
  }

  push(chunk) {
    this.buffer += this.decoder.write(chunk);
    if (/fanotify_mark[^\n]*operation not supported/i.test(this.buffer)) {
      this.abort('native TypeScript watch requires fanotify support on this filesystem');
    }
    let match;
    let consumed = 0;
    COMPLETE.lastIndex = 0;
    while ((match = COMPLETE.exec(this.buffer)) !== null) {
      const before = this.buffer.slice(consumed, match.index);
      const event = {
        generation: ++this.generation,
        errors: Number(match[1]),
        diagnostic: /(?:^|\n)[^\n]*\b(?:error TS2322)\b/.test(before),
        timestamp: new Date().toISOString(),
      };
      this.events.push(event);
      for (const listener of this.listeners) listener(event);
      consumed = COMPLETE.lastIndex;
    }
    this.buffer = this.buffer.slice(consumed);
    // Once matches have been consumed, keep only enough of the incomplete
    // generation to catch a diagnostic and a marker split across chunks.
    if (this.buffer.length > OUTPUT_LIMIT) this.buffer = this.buffer.slice(-OUTPUT_LIMIT);
  }

  abort(reason) {
    this.failure = reason;
    for (const listener of this.failures) listener(reason);
  }

  waitAfter(previous, predicate, timeoutMs, exit) {
    if (this.failure) return Promise.reject(new Error(this.failure));
    const existing = this.events.find((event) => event.generation > previous && predicate(event));
    if (existing) return Promise.resolve(existing);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => finish(new Error(`watch generation after ${previous} timed out`)),
        timeoutMs,
      );
      const listener = (event) => {
        if (event.generation > previous && predicate(event)) finish(null, event);
      };
      const onExit = (detail) =>
        finish(new Error(`watch exited before generation ${previous + 1}: ${detail}`));
      const onFailure = (detail) => finish(new Error(detail));
      const finish = (error, event) => {
        clearTimeout(timer);
        this.listeners.delete(listener);
        this.failures.delete(onFailure);
        exit.delete(onExit);
        if (error) reject(error);
        else resolve(event);
      };
      this.listeners.add(listener);
      this.failures.add(onFailure);
      exit.add(onExit);
    });
  }
}

function atomicReplace(file, contents) {
  const staging = `${file}.next`;
  fs.writeFileSync(staging, contents);
  fs.renameSync(staging, file);
}

function assertPhase(event, phase, previous) {
  if (event.generation <= previous)
    throw new Error(`${phase} reused stale generation ${event.generation}`);
  if (phase === 'initial' && event.errors !== 0)
    throw new Error(`initial generation ${event.generation} has ${event.errors} errors`);
  if (phase === 'error' && (event.errors < 1 || !event.diagnostic))
    throw new Error(`generation ${event.generation} missed fixture TS2322 diagnostic`);
  if (phase === 'repaired' && event.errors !== 0)
    throw new Error(`repair generation ${event.generation} has ${event.errors} errors`);
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === 'ESRCH') return false;
    throw error;
  }
}

async function stopTree(child) {
  if (!child.pid) return { exited: true, method: 'not-started' };
  if (process.platform === 'win32') {
    const killed = spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      timeout: EXIT_TIMEOUT_MS,
      encoding: 'utf8',
      windowsHide: true,
    });
    // A previously exited process can make taskkill nonzero. Verify rather than
    // treating its status as proof of either cleanup or failure.
    if (killed.error) throw killed.error;
    if (killed.status !== 0 && alive(child.pid))
      throw new Error(`taskkill failed to stop watch tree: ${killed.stderr}`);
    // taskkill's leader-exit race cannot establish the fate of a previously
    // detached descendant without Job Object ownership (#14766).
    if (killed.status !== 0)
      throw new Error('Windows watcher leader exited before subtree cleanup could be proven');
  } else {
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch (error) {
      if (error.code !== 'ESRCH') throw error;
    }
  }
  const deadline = Date.now() + EXIT_TIMEOUT_MS;
  const treeStopped = () => {
    if (process.platform === 'win32') return !alive(child.pid);
    try {
      process.kill(-child.pid, 0);
    } catch (error) {
      if (error.code === 'ESRCH') return true;
      return false;
    }
    return inspectPosixProcessGroup(child.pid) === false;
  };
  while (Date.now() < deadline) {
    if (treeStopped())
      return {
        exited: true,
        method: process.platform === 'win32' ? 'taskkill' : 'SIGTERM',
        tree_proven: process.platform !== 'win32',
      };
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  if (process.platform !== 'win32') {
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch (error) {
      if (error.code !== 'ESRCH') throw error;
    }
  }
  const forcedDeadline = Date.now() + EXIT_TIMEOUT_MS;
  while (Date.now() < forcedDeadline) {
    if (treeStopped())
      return { exited: true, method: 'forced', tree_proven: process.platform !== 'win32' };
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`watch process ${child.pid} survived tree shutdown`);
}

/** The injectable launcher is used only by the subprocess signal test. */
/** @param {{spawnWatch?: typeof spawn, compilerVersion?: string}} [options] */
async function runWatchLifecycle(extensionRoot = path.resolve(__dirname, '..'), options = {}) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp ts7 watch '));
  const source = path.join(project, 'watch-fixture.ts');
  const config = path.join(project, 'tsconfig.json');
  const good = 'export const count: number = 1;\n';
  const bad = 'export const count: number = "bad";\n';
  /** @type {{schema_version: string, verdict: string, compiler_version: string | null, config_identity: string, expected_diagnostic: string, project_has_spaces: boolean, phases: Record<string, unknown>, cleanup: null | {exited: boolean, method?: string, tree_proven?: boolean, reason?: string}, reason?: string, output_tail?: string, interrupt_signal?: string}} */
  const receipt = {
    schema_version: 'typescript_watch_lifecycle.v1',
    verdict: 'not_proven',
    compiler_version: null,
    config_identity: 'temporary isolated tsconfig.json with explicit watch-fixture.ts',
    expected_diagnostic: 'TS2322',
    project_has_spaces: project.includes(' '),
    phases: {},
    cleanup: null,
  };
  let child;
  let outputTail = '';
  const generations = new WatchGenerations();
  // Node's default signal action exits immediately, bypassing finally and
  // leaving governed-tsc's compiler descendant alive. Own the signal until
  // tree shutdown and fixture cleanup have completed, including while the
  // watcher is between generations or the receipt is being assembled.
  const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'];
  const interrupt = (signal) => {
    if (!receipt.interrupt_signal) receipt.interrupt_signal = signal;
    generations.abort(`watch interrupted by ${signal}`);
  };
  const handlers = signals.map((signal) => {
    const handler = () => interrupt(signal);
    process.on(signal, handler);
    return { signal, handler };
  });
  try {
    receipt.compiler_version =
      options.compilerVersion ??
      JSON.parse(
        fs.readFileSync(path.join(extensionRoot, 'node_modules/typescript/package.json'), 'utf8'),
      ).version;
    fs.writeFileSync(source, good);
    fs.writeFileSync(
      config,
      JSON.stringify({
        compilerOptions: { strict: true, noEmit: true, target: 'ES2022' },
        files: ['./watch-fixture.ts'],
      }),
    );
    const exits = new Set();
    child = (options.spawnWatch ?? spawn)(
      process.execPath,
      [
        path.join(extensionRoot, 'scripts/governed-tsc.js'),
        '-watch',
        '--noEmit',
        '-p',
        config,
        '--pretty',
        'false',
      ],
      {
        cwd: extensionRoot,
        stdio: ['ignore', 'pipe', 'pipe'],
        detached: process.platform !== 'win32',
        windowsHide: true,
      },
    );
    const collect = (chunk) => {
      outputTail = `${outputTail}${chunk.toString('utf8')}`.slice(-4096);
      generations.push(chunk);
    };
    child.stdout.on('data', collect);
    child.stderr.on('data', collect);
    child.once('error', (error) => {
      generations.abort(`watch launch failed: ${error.message}`);
      for (const listener of exits) listener(error.message);
    });
    child.once('exit', (code, signal) => {
      generations.abort(`watch exited: code=${code} signal=${signal}`);
      for (const listener of exits) listener(`code=${code} signal=${signal}`);
    });
    const first = await generations.waitAfter(0, () => true, PHASE_TIMEOUT_MS, exits);
    assertPhase(first, 'initial', 0);
    receipt.phases.initial = first;
    atomicReplace(source, bad);
    const broken = await generations.waitAfter(
      first.generation,
      () => true,
      PHASE_TIMEOUT_MS,
      exits,
    );
    assertPhase(broken, 'error', first.generation);
    receipt.phases.error = broken;
    atomicReplace(source, good);
    const repaired = await generations.waitAfter(
      broken.generation,
      () => true,
      PHASE_TIMEOUT_MS,
      exits,
    );
    assertPhase(repaired, 'repaired', broken.generation);
    receipt.phases.repaired = repaired;
    receipt.verdict = 'pass';
  } catch (error) {
    receipt.reason = error instanceof Error ? error.message : String(error);
    receipt.verdict = /fanotify support/.test(receipt.reason) ? 'not_proven' : 'failed';
    if (receipt.interrupt_signal) receipt.verdict = 'not_proven';
    receipt.output_tail = outputTail;
  } finally {
    try {
      if (child) receipt.cleanup = await stopTree(child);
      else receipt.cleanup = { exited: true, method: 'not-started' };
      if (receipt.interrupt_signal) receipt.verdict = 'not_proven';
      if (receipt.verdict === 'pass' && receipt.cleanup.tree_proven !== true) {
        receipt.verdict = 'not_proven';
        receipt.reason = 'watcher subtree cleanup was not independently proven';
      }
    } catch (error) {
      receipt.verdict = 'not_proven';
      receipt.cleanup = {
        exited: false,
        reason: error instanceof Error ? error.message : String(error),
      };
    }
    try {
      fs.rmSync(project, { recursive: true, force: true });
    } catch (error) {
      receipt.verdict = 'not_proven';
      receipt.reason = `fixture cleanup failed: ${error instanceof Error ? error.message : String(error)}`;
    }
    for (const { signal, handler } of handlers) process.removeListener(signal, handler);
  }
  return receipt;
}

function receiptExitCode(receipt) {
  if (receipt.interrupt_signal === 'SIGINT') return 130;
  if (receipt.interrupt_signal === 'SIGTERM') return 143;
  if (receipt.interrupt_signal === 'SIGHUP') return 129;
  return receipt.verdict === 'pass' ? 0 : 1;
}

if (require.main === module) {
  runWatchLifecycle()
    .then((receipt) => {
      process.stdout.write(`${JSON.stringify(receipt)}\n`);
      process.exitCode = receiptExitCode(receipt);
    })
    .catch((error) => {
      process.stderr.write(`${error.stack || error}\n`);
      process.exitCode = 1;
    });
}

module.exports = { WatchGenerations, assertPhase, runWatchLifecycle, stopTree, receiptExitCode };
