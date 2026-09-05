/**
 * Observation of language-server child-process termination.
 *
 * `LanguageClient.stop()` in vscode-languageclient moves the client to
 * `State.Stopped` in `BaseLanguageClient.shutdown().finally()` and only then,
 * in the node subclass's own `finally`, clears `serverProcess` and schedules
 * `checkProcessDied()` on a two-second timer that kills the child and ignores
 * kill failure. `State.Stopped` therefore precedes process termination and
 * cannot stand in for it (#14155). The lifecycle captures the handle before
 * `stop()` and admits a replacement only once that process has been observed
 * to exit.
 */

/** The subset of `child_process.ChildProcess` this module reads. */
export interface ServerProcessLike {
  readonly pid?: number | undefined;
  readonly exitCode: number | null;
  readonly signalCode: NodeJS.Signals | string | null;
  once(event: 'exit', listener: (...args: unknown[]) => void): unknown;
  removeListener(event: 'exit', listener: (...args: unknown[]) => void): unknown;
}

/** Probe whether a pid is still alive; returns false once the process is gone. */
export type ProcessAliveProbe = (pid: number) => boolean;

const defaultProbe: ProcessAliveProbe = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/**
 * Read the server child-process handle off a language client, if the client
 * exposes one. Returns `undefined` for clients that never spawned a process or
 * whose transport is not a child process.
 */
export function serverProcessOf(client: unknown): ServerProcessLike | undefined {
  if (client === null || typeof client !== 'object' || !('serverProcess' in client)) {
    return undefined;
  }
  const candidate = (client as { serverProcess?: unknown }).serverProcess;
  return isServerProcessLike(candidate) ? candidate : undefined;
}

function isServerProcessLike(value: unknown): value is ServerProcessLike {
  return (
    value !== null &&
    typeof value === 'object' &&
    'exitCode' in value &&
    'signalCode' in value &&
    typeof (value as { once?: unknown }).once === 'function' &&
    typeof (value as { removeListener?: unknown }).removeListener === 'function'
  );
}

/** True once the child has reported an exit code or a terminating signal. */
export function hasExited(child: ServerProcessLike, probe: ProcessAliveProbe = defaultProbe): boolean {
  if (child.exitCode !== null || child.signalCode !== null) {
    return true;
  }
  if (child.pid === undefined) {
    // Never spawned: nothing is alive to overlap a replacement.
    return true;
  }
  return !probe(child.pid);
}

/**
 * Resolve `true` when the child has exited, waiting at most `graceMs` for the
 * `exit` event. Resolves `false` if the process is still alive at the bound;
 * the caller treats that as incomplete cleanup and blocks the replacement.
 */
export function awaitServerProcessExit(
  child: ServerProcessLike | undefined,
  graceMs: number,
  probe: ProcessAliveProbe = defaultProbe,
): Promise<boolean> {
  if (child === undefined) {
    return Promise.resolve(true);
  }
  if (hasExited(child, probe)) {
    return Promise.resolve(true);
  }
  return new Promise<boolean>((resolve) => {
    let settled = false;
    const finish = (exited: boolean): void => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      child.removeListener('exit', onExit);
      resolve(exited);
    };
    const onExit = (): void => finish(true);
    const timer = setTimeout(() => finish(hasExited(child, probe)), graceMs);
    child.once('exit', onExit);
  });
}
