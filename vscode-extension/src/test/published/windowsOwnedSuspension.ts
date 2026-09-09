import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import * as path from 'node:path';

export interface WindowsSuspendResult {
  outcome: 'suspended' | 'resumed' | 'already_gone' | 'error';
  detail: string;
}

interface Completion {
  code: number | null;
  error: string;
  stdout: string;
}

interface Worker {
  child: ChildProcessWithoutNullStreams;
  completion: Promise<Completion>;
  handshake: Promise<boolean>;
  completed: boolean;
}

interface Suspension {
  pid: number;
  creation: string;
  worker: Worker;
  releasing: boolean;
  consumed: boolean;
  recovery?: Promise<WindowsSuspendResult>;
}

const helperPath = path.resolve(__dirname, '../../../../scripts/tests/windows-owned-suspend.ps1');
const suspensions = new Map<number, Suspension>();

function launch(args: string[]): Worker {
  const child = spawn(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', helperPath, ...args],
    { stdio: 'pipe', windowsHide: true },
  );
  let complete!: (value: Completion) => void;
  let handshake!: (value: boolean) => void;
  const completion = new Promise<Completion>((resolve) => {
    complete = resolve;
  });
  const handshakeResult = new Promise<boolean>((resolve) => {
    handshake = resolve;
  });
  const worker: Worker = { child, completion, handshake: handshakeResult, completed: false };
  let stdout = '';
  let error = '';
  const finish = (code: number | null): void => {
    if (worker.completed) return;
    worker.completed = true;
    handshake(false);
    complete({ code, error, stdout });
  };
  child.stdout.on('data', (chunk: Buffer) => {
    stdout = (stdout + chunk.toString('utf8')).slice(-65536);
    if (/^SUSPENDED [1-9]\d* THREADS\r?$/m.test(stdout)) handshake(true);
  });
  child.stderr.on('data', (chunk: Buffer) => {
    error = (error + chunk.toString('utf8')).slice(-65536);
  });
  child.stdin.on('error', (cause: Error) => {
    error = (error + cause.message).slice(-65536);
  });
  child.on('error', (cause: Error) => {
    error = cause.message;
    finish(null);
  });
  child.on('close', finish);
  return worker;
}

async function bounded<T>(promise: Promise<T>, milliseconds: number): Promise<T | undefined> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<undefined>((resolve) => {
        timer = setTimeout(() => resolve(undefined), milliseconds);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

function requestResume(worker: Worker): void {
  if (!worker.completed && !worker.child.stdin.destroyed && !worker.child.stdin.writableEnded) {
    worker.child.stdin.end('resume\n');
  }
}

function consume(state: Suspension): void {
  state.consumed = true;
  if (state.worker.completed && suspensions.get(state.pid) === state) {
    suspensions.delete(state.pid);
  }
}

async function recover(state: Suspension): Promise<WindowsSuspendResult> {
  if (state.recovery !== undefined) return await state.recovery;
  state.releasing = true;
  state.recovery = (async () => {
    // Give the pinned owner the first opportunity to undo its own suspensions.
    requestResume(state.worker);
    const graceful = await bounded(state.worker.completion, 2000);
    if (graceful?.code === 3 && /^ALREADY_GONE\r?$/m.test(graceful.stdout)) {
      return { outcome: 'already_gone', detail: 'Pinned target exited before suspension release' };
    }
    if (graceful?.code === 0) {
      return {
        outcome: 'error',
        detail: 'Windows fault helper failed; owned suspension was released',
      };
    }
    const cleanup = launch([
      '-Cleanup',
      '-ProcessId',
      String(state.pid),
      '-CreationTimeFileTime',
      state.creation,
    ]);
    const terminal = await bounded(cleanup.completion, 15000);
    if (terminal === undefined) {
      // This worker never suspends anything. The suspension owner remains alive
      // with its own finite hold timeout if target cleanup cannot be confirmed.
      cleanup.child.kill();
      await bounded(cleanup.completion, 2000);
      return {
        outcome: 'error',
        detail: 'Owned target cleanup timed out; suspension owner was not killed',
      };
    }
    if (terminal.code !== 0 || !/^(ALREADY_GONE|TERMINATED)\r?$/m.test(terminal.stdout)) {
      return {
        outcome: 'error',
        detail: `Owned target cleanup unproven: ${terminal.error || terminal.stdout}`,
      };
    }
    // Only confirmed target termination/absence permits killing a stuck helper.
    if (!state.worker.completed) state.worker.child.kill();
    const reaped = await bounded(state.worker.completion, 2000);
    if (reaped === undefined)
      return { outcome: 'error', detail: 'Owned target is gone; helper exit unproven' };
    return /^ALREADY_GONE\r?$/m.test(terminal.stdout)
      ? { outcome: 'already_gone', detail: 'Pinned target identity has already exited' }
      : {
          outcome: 'error',
          detail: 'Fault helper failed; exact owned target was terminated during cleanup',
        };
  })();
  return await state.recovery;
}

export async function suspendOwnedWindowsProcess(
  pid: number,
  creation: string,
  proof: {
    onHelperSpawn?: (child: ChildProcessWithoutNullStreams) => void;
    handshakeMilliseconds?: number;
    holdMilliseconds?: number;
  } = {},
): Promise<WindowsSuspendResult> {
  const holdMilliseconds = proof.holdMilliseconds ?? 120000;
  const handshakeMilliseconds = proof.handshakeMilliseconds ?? 10000;
  if (
    !Number.isSafeInteger(pid) ||
    pid <= 0 ||
    !/^\d+$/.test(creation) ||
    suspensions.has(pid) ||
    !Number.isSafeInteger(holdMilliseconds) ||
    holdMilliseconds < 1 ||
    holdMilliseconds > 120000 ||
    !Number.isSafeInteger(handshakeMilliseconds) ||
    handshakeMilliseconds < 1 ||
    handshakeMilliseconds > 10000
  ) {
    return { outcome: 'error', detail: 'Invalid or duplicate owned Windows process identity' };
  }
  const worker = launch([
    '-ProcessId',
    String(pid),
    '-CreationTimeFileTime',
    creation,
    '-TimeoutMilliseconds',
    String(holdMilliseconds),
  ]);
  const state: Suspension = { pid, creation, worker, releasing: false, consumed: false };
  suspensions.set(pid, state);
  // Register completion immediately; even an exit just after the handshake is
  // retained and cleaned up instead of being lost before resume adds a listener.
  void worker.completion.then(() => {
    if (state.consumed && suspensions.get(state.pid) === state) suspensions.delete(state.pid);
    if (!state.releasing) void recover(state);
  });
  try {
    proof.onHelperSpawn?.(worker.child);
  } catch {
    const cleanup = await recover(state);
    consume(state);
    return { outcome: 'error', detail: `Windows proof setup failed; ${cleanup.detail}` };
  }
  const ready = await bounded(worker.handshake, handshakeMilliseconds);
  if (ready === true && !worker.completed) {
    return { outcome: 'suspended', detail: `Pinned Windows suspension for pid ${pid}` };
  }
  const cleanup = await recover(state);
  consume(state);
  return { outcome: 'error', detail: `Windows suspension handshake failed; ${cleanup.detail}` };
}

export async function resumeOwnedWindowsProcess(pid: number): Promise<WindowsSuspendResult> {
  const state = suspensions.get(pid);
  if (state === undefined)
    return { outcome: 'error', detail: `No owned suspension for pid ${pid}` };
  state.releasing = true;
  let result: WindowsSuspendResult;
  if (state.recovery !== undefined) {
    result = await state.recovery;
  } else {
    requestResume(state.worker);
    const terminal = await bounded(state.worker.completion, 10000);
    if (terminal?.code === 0) {
      result = { outcome: 'resumed', detail: `Released pinned Windows suspension for pid ${pid}` };
    } else {
      result = await recover(state);
    }
  }
  consume(state);
  return result;
}
