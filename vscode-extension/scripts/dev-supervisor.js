#!/usr/bin/env node
'use strict';

/**
 * The fail-closed combined bundle and TypeScript watch loop (#9848).
 *
 * Before this supervisor, a maintainer ran `watch:bundle` and `watch:types`
 * in two separate terminals. A green-looking bundle terminal said nothing
 * about the type watcher: it could have never started, died, or stopped
 * using the governed compiler, and lifecycle/cleanup ownership sat with the
 * operator — especially on Windows, where closing a terminal does not
 * guarantee the watcher tree is gone.
 *
 * `npm run dev` replaces that pair with ONE development service whose green
 * state means both watchers are ready and alive:
 *
 *   1. It starts the canonical #9842 watch surfaces (`watch:bundle`,
 *      `watch:types`) through `node --run`, so the checked command contract
 *      in `checked-command-contract.js` stays the single command authority:
 *      the supervisor adds no shell concatenation, spawns each surface as
 *      its own argument-array child, and re-implements no compiler or
 *      bundler semantics. A red TypeScript-authority gate therefore kills
 *      the `types` watcher before readiness and the supervisor goes red.
 *   2. Readiness is deterministic per child: a child is ready only after it
 *      emitted its real watcher marker ("Watching for file changes." from
 *      governed tsc, a completed first `built out in …` from Rolldown).
 *      Process spawn is not readiness, and overall readiness requires BOTH
 *      children within a bounded window.
 *   3. Failure is fail-closed: any child exiting (or failing to spawn)
 *      before or after readiness preserves its exit/signal evidence,
 *      terminates the sibling, and exits non-zero. The sibling is never
 *      left running and never left half-supervised.
 *   4. Shutdown owns the whole process tree. On POSIX the supervisor spawns
 *      each child detached (own process group) and escalates from a group
 *      SIGTERM to a group SIGKILL after a bounded grace. On Windows there
 *      is no graceful cross-process signal (Node delivers `SIGTERM` as an
 *      abrupt TerminateProcess), so tree ownership is `taskkill /T /F` from
 *      the start — reported as the forced escalation it is, never as
 *      graceful signal forwarding. Descendants of the canonical npm-style
 *      wrappers (for example governed-tsc's child tsc) die with the tree.
 *   5. Interrupt/termination signals are forwarded into that same owned
 *      shutdown path and produce the governed interrupt result
 *      (128 + signal number; SIGINT -> 130, SIGTERM -> 143).
 *   6. Every lifecycle transition is reported naming the child and phase,
 *      including one stable `[dev-supervisor] ready` line emitted only
 *      after both watchers are ready — the edge the #9851 watch launch
 *      task's background problem matcher waits on.
 *
 * The proof harness seam: `PERL_LSP_DEV_SUPERVISOR_CONFIG` points at a JSON
 * file with fixture child specs (and optional `stopWhenReady`). It exists so
 * `dev-supervisor.test.js` and the bounded real-loop smoke can drive this
 * exact binary without weakening `npm run dev`, which never sets the
 * variable. Unknown config fields are red by name.
 */

const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { StringDecoder } = require('node:string_decoder');
const { createReporter } = require('./reporter');

/** Reporter scope; also the stable prefix the #9851 problem matcher anchors on. */
const REPORT_SCOPE = 'dev-supervisor';

/** Env var selecting an alternate proof/dev config; never set by `npm run dev`. */
const CONFIG_ENV = 'PERL_LSP_DEV_SUPERVISOR_CONFIG';

/**
 * Governed tsc watch readiness: the first-pass completion marker tsc emits
 * after "Starting compilation in watch mode...", in both the success and
 * the found-errors case. Not spawn, not "output started" — the watch loop
 * reporting its first completed pass.
 */
const TYPES_READY_PATTERN = /Watching for file changes\./;

/**
 * Rolldown watch readiness: the first COMPLETED build receipt ("Built out
 * in 4.07 s." / "Rebuilt out in 4.07 s."). "Waiting for changes..." is
 * deliberately not readiness — it fires before the first build, and the
 * Extension Development Host must not load a freshly cleaned-out bundle.
 */
const BUNDLE_READY_PATTERN = /built out in/i;

/** Rolling per-stream tail kept for marker matching (markers are short). */
const STREAM_TAIL_LIMIT = 8 * 1024;

/** Hard cap on waiting for exit after the forced-kill escalation. */
const FORCE_KILL_WAIT_MS = 10_000;

/** How the supervisor reports stopping, keyed by cause. */
const STOP_REASONS = {
  CHILD_FAILURE: 'child-failure',
  READINESS_TIMEOUT: 'readiness-timeout',
  SIGNAL: 'signal',
  STOP_WHEN_READY: 'stop-when-ready',
  REQUESTED: 'requested',
};

/**
 * Exit codes for the governed interrupt result: 128 + signal number, the
 * conventional shell encoding a supervisor killed by a signal reports.
 *
 * @param {NodeJS.Signals} signal
 * @returns {number}
 */
function signalExitCode(signal) {
  const numbers = { SIGINT: 2, SIGTERM: 15, SIGHUP: 1, SIGBREAK: 21 };
  return 128 + (numbers[signal] ?? 1);
}

/** @param {string} name */
function startingMessage(name) {
  return `starting watcher "${name}"`;
}

/** @param {string} name */
function childReadyMessage(name) {
  return `watcher "${name}" ready`;
}

/**
 * The one stable readiness edge for the whole service. Emitted only after
 * BOTH watchers are ready — the #9851 watch task's endsPattern matches this
 * exact prefix, so the Extension Development Host cannot race a half-ready
 * dev loop.
 *
 * @param {number} readyCount
 * @param {number} totalCount
 */
function readyMessage(readyCount, totalCount) {
  return `ready (${readyCount}/${totalCount} watchers healthy)`;
}

/**
 * @param {string} name
 * @param {'pending' | 'ready'} phase
 * @param {number | null} code
 * @param {string | null} signal
 */
function childFailedMessage(name, phase, code, signal) {
  return `watcher "${name}" failed (phase=${phase}, code=${code === null ? 'none' : code}, signal=${signal ?? 'none'})`;
}

/**
 * @param {string} name
 * @param {string} detail
 */
function childTerminationMessage(name, detail) {
  return `watcher "${name}" ${detail}`;
}

/** @param {string} reason */
function stoppingMessage(reason) {
  return `stopping (reason=${reason})`;
}

/** @param {number} code @param {string} reason */
function exitedMessage(code, reason) {
  return `exited (code=${code}, reason=${reason})`;
}

/**
 * @typedef {object} WatchChildSpec
 * @property {string} name Stable lifecycle name used in every status line.
 * @property {string} command Executable (argument-array spawn, no shell).
 * @property {string[]} args
 * @property {string} cwd Working directory for the child.
 * @property {RegExp} readyPattern Marker that turns this child "ready".
 * @property {NodeJS.ProcessEnv} [env] Child environment; defaults to the
 *   supervisor's own environment.
 */

/**
 * @typedef {object} SupervisorOptions
 * @property {number} readinessTimeoutMs Bounded window for BOTH children to
 *   become ready; expiry is red and names the pending children.
 * @property {number} shutdownGraceMs Grace between the graceful POSIX group
 *   SIGTERM and the forced SIGKILL escalation. Windows terminates trees
 *   forcibly (taskkill) and does not use this grace.
 * @property {boolean} stopWhenReady Proof harness: perform the owned
 *   shutdown as soon as readiness is reached (exit 0).
 * @property {boolean} forwardOutput Forward each watcher's stdout/stderr to
 *   the supervisor's visible streams so build diagnostics stay visible.
 *   Fixture tests opt out; `npm run dev` always forwards.
 */

/**
 * @typedef {object} ChildOutcome
 * @property {string} name
 * @property {number | null} pid
 * @property {'pending' | 'ready' | 'stopping' | 'stopped' | 'failed'} phase
 * @property {{code: number | null, signal: string | null, error?: string}} [exit]
 */

/**
 * @typedef {object} SupervisorResult
 * @property {number} code Process exit code the CLI should use.
 * @property {string} reason Machine-readable terminal cause.
 * @property {ChildOutcome[]} children Per-child outcomes with PIDs so proof
 *   can verify the process tree is really gone.
 * @property {string[]} failures Named failure evidence (child + phase).
 * @property {string[]} escalations Named forced-kill escalations.
 */

/**
 * Spawns one canonical watch child. POSIX: detached so the child leads its
 * own process group and a group signal reaches the whole watcher tree
 * (wrapper shell, governed-tsc, its tsc grandchild). Windows: detached is
 * meaningless for groups there, so tree ownership uses taskkill /T.
 *
 * @param {WatchChildSpec} spec
 * @returns {import('node:child_process').ChildProcess}
 */
function spawnWatcher(spec) {
  return spawn(spec.command, spec.args, {
    cwd: spec.cwd,
    env: spec.env ?? process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: process.platform !== 'win32',
    windowsHide: true,
  });
}

/**
 * Graceful-step tree termination. POSIX sends SIGTERM to the child's whole
 * process group; Windows goes straight to `taskkill /T /F` because Node has
 * no graceful cross-process signal there — this is recorded as the forced
 * termination it is.
 *
 * @param {ChildState} child
 * @param {(message: string) => void} report
 * @returns {Promise<void>}
 */
async function requestTermination(child, report) {
  const proc = child.proc;
  if (proc === null || child.exit !== undefined) {
    return;
  }
  const pid = proc.pid;
  if (typeof pid !== 'number') {
    return;
  }
  if (process.platform === 'win32') {
    report(
      childTerminationMessage(
        child.spec.name,
        'taskkill /T /F sent (forced tree termination — Windows has no graceful cross-process signal)',
      ),
    );
    child.escalated = true;
    await runTaskkill(pid);
    return;
  }
  report(childTerminationMessage(child.spec.name, 'SIGTERM sent (process group)'));
  killGroup(pid, 'SIGTERM', report);
}

/**
 * Forced-kill escalation. POSIX: SIGKILL to the process group. Windows: the
 * same `taskkill /T /F` (already forced; retried for a tree that ignored
 * the first pass).
 *
 * @param {ChildState} child
 * @param {(message: string) => void} report
 * @returns {Promise<void>}
 */
async function escalateTermination(child, report) {
  const proc = child.proc;
  if (proc === null || child.exit !== undefined) {
    return;
  }
  const pid = proc.pid;
  if (typeof pid !== 'number') {
    return;
  }
  if (process.platform === 'win32') {
    report(childTerminationMessage(child.spec.name, 'taskkill /T /F escalation sent'));
    await runTaskkill(pid);
    return;
  }
  report(childTerminationMessage(child.spec.name, 'SIGKILL sent (process group, escalation)'));
  killGroup(pid, 'SIGKILL', report);
}

/**
 * @param {number} pid
 * @param {NodeJS.Signals} signal
 * @param {(message: string) => void} report
 */
function killGroup(pid, signal, report) {
  try {
    process.kill(-pid, signal);
  } catch {
    // ESRCH: the group is already gone — that is the desired state.
    report(`process group -${pid} already gone (${signal})`);
  }
}

/**
 * Runs `taskkill /T /F` for a pid. A non-zero exit usually means the tree
 * already died — treated as success, since the goal is tree absence.
 *
 * @param {number} pid
 * @returns {Promise<void>}
 */
function runTaskkill(pid) {
  return new Promise((resolve) => {
    const killer = spawn('taskkill', ['/pid', String(pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
    killer.once('error', () => resolve());
    killer.once('exit', () => resolve());
  });
}

/**
 * @typedef {object} ChildState
 * @property {WatchChildSpec} spec
 * @property {import('node:child_process').ChildProcess | null} proc
 * @property {'pending' | 'ready' | 'stopping' | 'stopped' | 'failed'} phase
 * @property {{code: number | null, signal: string | null, error?: string}} [exit]
 * @property {boolean} escalated
 * @property {string} stdoutTail
 * @property {string} stderrTail
 * @property {StringDecoder} stdoutDecoder
 * @property {StringDecoder} stderrDecoder
 * @property {() => void} [exitedNotify]
 */

/**
 * Runs the fail-closed dev service to a terminal state. Never rejects: a
 * watcher failure, a readiness timeout, and a signal shutdown are results,
 * not exceptions.
 *
 * @param {{
 *   children: WatchChildSpec[],
 *   reporter?: {info: (message: string) => void, error: (message: string) => void},
 *   options?: Partial<SupervisorOptions>,
 * }} input
 * @returns {{
 *   waitForExit: () => Promise<SupervisorResult>,
 *   stop: (cause?: NodeJS.Signals | string) => Promise<SupervisorResult>,
 * }}
 */
function runDevSupervisor(input) {
  const report = input.reporter?.info ?? (() => {});
  const reportError = input.reporter?.error ?? (() => {});
  const options = {
    readinessTimeoutMs: 300_000,
    shutdownGraceMs: 5_000,
    stopWhenReady: false,
    forwardOutput: true,
    ...input.options,
  };

  /** @type {ChildState[]} */
  const children = input.children.map((spec) => ({
    spec,
    proc: null,
    phase: 'pending',
    escalated: false,
    stdoutTail: '',
    stderrTail: '',
    stdoutDecoder: new StringDecoder('utf8'),
    stderrDecoder: new StringDecoder('utf8'),
  }));

  /** @type {SupervisorResult} */
  const result = {
    code: 0,
    reason: STOP_REASONS.REQUESTED,
    children: [],
    failures: [],
    escalations: [],
  };
  let readyCount = 0;
  /** @type {NodeJS.Timeout | null} */
  let readinessTimer = null;
  let shutdownStarted = false;
  /** @type {(value: SupervisorResult) => void} */
  let settle = () => {};
  const exitPromise = new Promise((resolve) => {
    settle = resolve;
  });

  const emit = (message) => report(message);

  /**
   * Waits until every child recorded an exit, or the deadline passes.
   *
   * @param {number} deadlineMs Epoch milliseconds.
   * @returns {Promise<void>}
   */
  function waitForExits(deadlineMs) {
    const pending = children.filter((child) => child.exit === undefined);
    if (pending.length === 0) {
      return Promise.resolve();
    }
    const remaining = deadlineMs - Date.now();
    if (remaining <= 0) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      /** @type {NodeJS.Timeout} */
      let timer;
      const done = () => {
        clearTimeout(timer);
        resolve();
      };
      const notify = () => {
        if (children.every((child) => child.exit !== undefined)) {
          done();
        }
      };
      for (const child of pending) {
        child.exitedNotify = notify;
      }
      timer = setTimeout(done, remaining);
    });
  }

  /**
   * Kills every live watcher, waits a bounded time for exits, escalates the
   * survivors, and settles the supervisor result. Idempotent.
   *
   * @param {string} reason
   * @param {number} code
   */
  async function shutdown(reason, code) {
    if (shutdownStarted) {
      return;
    }
    shutdownStarted = true;
    if (readinessTimer !== null) {
      clearTimeout(readinessTimer);
      readinessTimer = null;
    }
    result.reason = reason;
    result.code = code;
    emit(stoppingMessage(reason));

    for (const child of children) {
      if (child.exit === undefined && child.proc !== null) {
        child.phase = 'stopping';
        await requestTermination(child, emit);
        if (process.platform === 'win32' && child.exit === undefined && !result.escalations.includes(child.spec.name)) {
          // On Windows requestTermination is already the forced tree kill
          // (`taskkill /T /F`): record it as an escalation so the result
          // reports the forced termination instead of a silent pass.
          result.escalations.push(child.spec.name);
        }
      }
    }
    // A watcher that exited ON ITS OWN may have left descendants in the
    // process group it led. The supervisor still owns that group: on POSIX
    // the group id survives while any member lives, so it is terminated
    // exactly like a live child's group. On Windows there is no way to
    // enumerate (or force-kill) the descendants of an already-exited leader,
    // so the residual is named loudly instead of being silently dropped.
    for (const child of children) {
      if (child.exit !== undefined && child.phase === 'failed') {
        const pid = child.proc?.pid;
        if (typeof pid === 'number') {
          if (process.platform === 'win32') {
            result.failures.push(
              `watcher "${child.spec.name}" exited on its own — any descendants it left behind cannot be force-killed from Windows after the leader exited`,
            );
          } else {
            emit(
              childTerminationMessage(
                child.spec.name,
                'SIGTERM sent (orphaned process group of the exited watcher)',
              ),
            );
            killGroup(pid, 'SIGTERM', emit);
          }
        }
      }
    }
    const graceDeadline = Date.now() + (process.platform === 'win32' ? 0 : options.shutdownGraceMs);
    await waitForExits(graceDeadline);

    for (const child of children) {
      if (child.exit === undefined && child.proc !== null) {
        if (!child.escalated) {
          child.escalated = true;
          result.escalations.push(child.spec.name);
        }
        await escalateTermination(child, emit);
      } else if (
        child.exit !== undefined &&
        child.phase === 'failed' &&
        process.platform !== 'win32'
      ) {
        const pid = child.proc?.pid;
        if (typeof pid === 'number') {
          emit(
            childTerminationMessage(
              child.spec.name,
              'SIGKILL sent (orphaned process group, escalation)',
            ),
          );
          killGroup(pid, 'SIGKILL', emit);
        }
      }
    }
    await waitForExits(Date.now() + FORCE_KILL_WAIT_MS);

    for (const child of children) {
      if (child.exit === undefined) {
        result.failures.push(
          `watcher "${child.spec.name}" did not confirm termination during shutdown`,
        );
      }
    }
    for (const child of children) {
      result.children.push({
        name: child.spec.name,
        pid: child.proc?.pid ?? null,
        phase: child.exit === undefined ? 'stopping' : 'stopped',
        ...(child.exit !== undefined ? { exit: child.exit } : {}),
      });
    }
    if (result.failures.length > 0 && result.code === 0) {
      result.code = 1;
    }
    emit(exitedMessage(result.code, reason));
    settle({ ...result });
  }

  /**
   * A child died without the supervisor asking it to — the fail-closed
   * edge. Preserve the evidence, name the child and phase, stop everything.
   *
   * @param {ChildState} child
   * @param {{code: number | null, signal: string | null, error?: string}} exit
   */
  function onUnexpectedExit(child, exit) {
    const phaseAtFailure = child.phase === 'ready' ? 'ready' : 'pending';
    child.phase = 'failed';
    child.exit = exit;
    const detail = childFailedMessage(child.spec.name, phaseAtFailure, exit.code, exit.signal);
    if (exit.error !== undefined) {
      result.failures.push(`${detail}, error=${exit.error}`);
    } else {
      result.failures.push(detail);
    }
    emit(detail);
    if (exit.error !== undefined) {
      reportError(`FAIL: watcher "${child.spec.name}" could not be launched (${exit.error})`);
    }
    const code = exit.error !== undefined || exit.code === null || exit.code === 0 ? 1 : exit.code;
    void shutdown(`${STOP_REASONS.CHILD_FAILURE}:${child.spec.name}`, code);
  }

  /**
   * Marker scan over a rolling decoded tail, plus raw diagnostics
   * passthrough: a dev service that swallowed tsc/Rolldown output would be
   * unusable, so each chunk is forwarded to the supervisor's matching
   * visible stream (opt-out for fixture tests via `forwardOutput`).
   *
   * @param {ChildState} child
   * @param {'stdout' | 'stderr'} stream
   * @param {Buffer} chunk
   */
  function onChildOutput(child, stream, chunk) {
    const decoder = stream === 'stdout' ? child.stdoutDecoder : child.stderrDecoder;
    const text = decoder.write(chunk);
    if (options.forwardOutput) {
      const sink = stream === 'stdout' ? process.stdout : process.stderr;
      sink.write(chunk);
    }
    if (child.phase === 'pending') {
      const tailKey = stream === 'stdout' ? 'stdoutTail' : 'stderrTail';
      const tail = `${child[tailKey]}${text}`.slice(-STREAM_TAIL_LIMIT);
      child[tailKey] = tail;
      if (child.spec.readyPattern.test(tail)) {
        child.phase = 'ready';
        readyCount += 1;
        emit(childReadyMessage(child.spec.name));
        if (readyCount === children.length) {
          if (readinessTimer !== null) {
            clearTimeout(readinessTimer);
            readinessTimer = null;
          }
          emit(readyMessage(readyCount, children.length));
          if (options.stopWhenReady) {
            void shutdown(STOP_REASONS.STOP_WHEN_READY, 0);
          }
        }
      }
    }
  }

  // Spawn all watchers. Any single spawn failure is a supervisor failure.
  for (const child of children) {
    emit(startingMessage(child.spec.name));
    /** @type {import('node:child_process').ChildProcess} */
    let proc;
    try {
      proc = spawnWatcher(child.spec);
    } catch (error) {
      child.proc = null;
      onUnexpectedExit(child, {
        code: null,
        signal: null,
        error: error instanceof Error ? error.message : String(error),
      });
      break;
    }
    child.proc = proc;
    proc.stdout?.on('data', (chunk) =>
      onChildOutput(child, 'stdout', /** @type {Buffer} */ (chunk)),
    );
    proc.stderr?.on('data', (chunk) =>
      onChildOutput(child, 'stderr', /** @type {Buffer} */ (chunk)),
    );
    proc.once('error', (error) => {
      if (child.exit === undefined) {
        onUnexpectedExit(child, {
          code: null,
          signal: null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    });
    proc.once('exit', (code, signal) => {
      child.exit = { code, signal };
      if (child.phase !== 'stopping') {
        onUnexpectedExit(child, { code, signal });
        return;
      }
      child.phase = 'stopped';
      child.exitedNotify?.();
    });
  }

  // Bounded overall readiness window: expiry is red and names the watchers
  // that never became ready. Process spawn is not readiness.
  if (!shutdownStarted) {
    readinessTimer = setTimeout(() => {
      if (shutdownStarted) {
        return;
      }
      const pending = children
        .filter((child) => child.phase === 'pending')
        .map((child) => child.spec.name);
      reportError(
        `FAIL: readiness window expired before every watcher became ready (pending: ${
          pending.join(', ') || 'none'
        }) — the dev service is not healthy and will stop both watchers`,
      );
      for (const name of pending) {
        result.failures.push(`watcher "${name}" never became ready (readiness timeout)`);
      }
      void shutdown(STOP_REASONS.READINESS_TIMEOUT, 1);
    }, options.readinessTimeoutMs);
  }

  /**
   * Stops the service through the owned shutdown path. A Node signal name
   * produces the governed interrupt result (128 + signal number).
   *
   * Repeated stops are deliberately NOT ignored: a second interrupt arriving
   * during the graceful phase immediately escalates the live watchers
   * instead of falling back to Node's default termination, which would kill
   * the supervisor and orphan the watcher trees. The handler stays
   * installed for the whole shutdown.
   *
   * @param {NodeJS.Signals | string} [cause]
   * @returns {Promise<SupervisorResult>}
   */
  function stop(cause = STOP_REASONS.REQUESTED) {
    const isSignal = typeof cause === 'string' && cause.startsWith('SIG');
    const code = isSignal ? signalExitCode(/** @type {NodeJS.Signals} */ (cause)) : 0;
    if (shutdownStarted) {
      emit(`escalating on repeated stop request (${cause})`);
      for (const child of children) {
        if (child.exit === undefined && child.proc !== null) {
          if (!child.escalated) {
            child.escalated = true;
            result.escalations.push(child.spec.name);
          }
          void escalateTermination(child, emit);
        }
      }
      return exitPromise;
    }
    void shutdown(
      isSignal ? `${STOP_REASONS.SIGNAL}:${cause}` : /** @type {string} */ (cause),
      code,
    );
    return exitPromise;
  }

  return {
    waitForExit: () => exitPromise,
    stop,
  };
}

/**
 * The canonical production pair: exactly the #9842 watch surfaces, started
 * through `node --run` so `checked-command-contract.js` stays the only
 * command authority. `dev-supervisor.test.js` cross-checks this table
 * against `package.json` and this module's markers.
 *
 * @param {string} [extensionRoot]
 * @returns {WatchChildSpec[]}
 */
function createDefaultWatchChildren(extensionRoot = path.resolve(__dirname, '..')) {
  return [
    {
      name: 'types',
      command: process.execPath,
      args: ['--run', 'watch:types'],
      cwd: extensionRoot,
      readyPattern: TYPES_READY_PATTERN,
    },
    {
      name: 'bundle',
      command: process.execPath,
      args: ['--run', 'watch:bundle'],
      cwd: extensionRoot,
      readyPattern: BUNDLE_READY_PATTERN,
    },
  ];
}

/**
 * Parses and validates the proof-harness config. Unknown fields, missing
 * commands, and malformed patterns are red by name — a config that would
 * silently supervise nothing must never parse.
 *
 * @param {string} source Raw JSON text.
 * @param {string} origin Path or description for error messages.
 * @returns {{children: WatchChildSpec[], options: Partial<SupervisorOptions>}}
 */
function parseSupervisorConfig(source, origin) {
  /** @type {unknown} */
  let parsed;
  try {
    parsed = JSON.parse(source);
  } catch (error) {
    throw new Error(
      `${origin}: config is not valid JSON (${error instanceof Error ? error.message : String(error)})`,
    );
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new Error(`${origin}: config must be a JSON object`);
  }
  const config = /** @type {Record<string, unknown>} */ (parsed);
  const allowed = new Set([
    'children',
    'readinessTimeoutMs',
    'shutdownGraceMs',
    'stopWhenReady',
    'forwardOutput',
  ]);
  for (const key of Object.keys(config)) {
    if (!allowed.has(key)) {
      throw new Error(
        `${origin}: unknown config field "${key}" (allowed: ${[...allowed].join(', ')})`,
      );
    }
  }
  if (!Array.isArray(config.children) || config.children.length === 0) {
    throw new Error(`${origin}: "children" must be a non-empty array of child specs`);
  }
  /** @type {WatchChildSpec[]} */
  const children = config.children.map((entry, index) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      throw new Error(`${origin}: children[${index}] must be an object`);
    }
    const spec = /** @type {Record<string, unknown>} */ (entry);
    const specAllowed = new Set(['name', 'command', 'args', 'cwd', 'readyPattern', 'env']);
    for (const key of Object.keys(spec)) {
      if (!specAllowed.has(key)) {
        throw new Error(`${origin}: children[${index}] has unknown field "${key}"`);
      }
    }
    for (const key of ['name', 'command', 'readyPattern']) {
      if (typeof spec[key] !== 'string' || /** @type {string} */ (spec[key]).length === 0) {
        throw new Error(`${origin}: children[${index}].${key} must be a non-empty string`);
      }
    }
    if (
      spec.args !== undefined &&
      (!Array.isArray(spec.args) || spec.args.some((a) => typeof a !== 'string'))
    ) {
      throw new Error(`${origin}: children[${index}].args must be an array of strings`);
    }
    if (spec.cwd !== undefined && typeof spec.cwd !== 'string') {
      throw new Error(`${origin}: children[${index}].cwd must be a string`);
    }
    if (
      spec.env !== undefined &&
      (typeof spec.env !== 'object' ||
        spec.env === null ||
        Array.isArray(spec.env) ||
        Object.values(spec.env).some((value) => typeof value !== 'string'))
    ) {
      throw new Error(`${origin}: children[${index}].env must be an object of string values`);
    }
    let readyPattern;
    try {
      readyPattern = new RegExp(/** @type {string} */ (spec.readyPattern));
    } catch (error) {
      throw new Error(
        `${origin}: children[${index}].readyPattern is not a valid regular expression (${
          error instanceof Error ? error.message : String(error)
        })`,
      );
    }
    return {
      name: /** @type {string} */ (spec.name),
      command: /** @type {string} */ (spec.command),
      args: Array.isArray(spec.args) ? /** @type {string[]} */ (spec.args) : [],
      cwd: typeof spec.cwd === 'string' ? spec.cwd : process.cwd(),
      readyPattern,
      ...(typeof spec.env === 'object' && spec.env !== null && !Array.isArray(spec.env)
        ? { env: /** @type {NodeJS.ProcessEnv} */ (spec.env) }
        : {}),
    };
  });
  /** @type {Partial<SupervisorOptions>} */
  const options = {};
  for (const key of ['readinessTimeoutMs', 'shutdownGraceMs']) {
    const value = config[key];
    if (value === undefined) {
      continue;
    }
    if (typeof value !== 'number' || !Number.isInteger(value) || value <= 0) {
      throw new Error(`${origin}: "${key}" must be a positive integer (milliseconds)`);
    }
    options[/** @type {'readinessTimeoutMs' | 'shutdownGraceMs'} */ (key)] = value;
  }
  if (config.stopWhenReady !== undefined) {
    if (typeof config.stopWhenReady !== 'boolean') {
      throw new Error(`${origin}: "stopWhenReady" must be a boolean`);
    }
    options.stopWhenReady = config.stopWhenReady;
  }
  if (config.forwardOutput !== undefined) {
    if (typeof config.forwardOutput !== 'boolean') {
      throw new Error(`${origin}: "forwardOutput" must be a boolean`);
    }
    options.forwardOutput = config.forwardOutput;
  }
  return { children, options };
}

/**
 * Loads the proof/dev harness config when `PERL_LSP_DEV_SUPERVISOR_CONFIG`
 * is set. `npm run dev` never sets it and gets the canonical pair.
 *
 * @returns {{children: WatchChildSpec[], options: Partial<SupervisorOptions>} | null}
 */
function loadConfigOverride() {
  const configPath = process.env[CONFIG_ENV];
  if (configPath === undefined || configPath.length === 0) {
    return null;
  }
  return parseSupervisorConfig(fs.readFileSync(configPath, 'utf8'), configPath);
}

function main() {
  const reporter = createReporter(REPORT_SCOPE);
  /** @type {{children: WatchChildSpec[], options: Partial<SupervisorOptions>}} */
  let config;
  try {
    config = loadConfigOverride() ?? { children: createDefaultWatchChildren(), options: {} };
  } catch (error) {
    reporter.error(`FAIL: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 2;
    return;
  }
  const controller = runDevSupervisor({
    children: config.children,
    reporter,
    options: config.options,
  });
  // Handlers stay installed for the whole shutdown: a repeated interrupt
  // must escalate inside the owned path, never restore Node's default
  // termination while watcher trees are still alive.
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGBREAK']) {
    process.on(signal, () => {
      void controller.stop(signal);
    });
  }
  controller
    .waitForExit()
    .then((result) => {
      process.exitCode = result.code;
    })
    .catch((error) => {
      // runDevSupervisor resolves rather than throwing; this guards only a
      // genuinely unexpected internal failure, which must still exit red.
      reporter.error(
        `unexpected failure: ${error instanceof Error ? error.message : String(error)}`,
      );
      process.exitCode = 1;
    });
}

if (require.main === module) {
  main();
}

module.exports = {
  REPORT_SCOPE,
  CONFIG_ENV,
  TYPES_READY_PATTERN,
  BUNDLE_READY_PATTERN,
  STOP_REASONS,
  signalExitCode,
  startingMessage,
  childReadyMessage,
  readyMessage,
  childFailedMessage,
  stoppingMessage,
  exitedMessage,
  runDevSupervisor,
  createDefaultWatchChildren,
  parseSupervisorConfig,
  loadConfigOverride,
};
