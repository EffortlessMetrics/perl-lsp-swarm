import { execFile } from 'child_process';

export const MANAGED_BINARY_HEALTH_TIMEOUT_MS = 30_000;

export interface HealthCheckLog {
  appendLine(message: string): void;
}

export interface HealthCheckProcessOptions {
  execFile?: typeof execFile;
  timeoutMs?: number;
}

type HealthCheckChild = { kill?: () => boolean } | undefined;

function safeAppendLine(log: HealthCheckLog, message: string): void {
  try {
    log.appendLine(message);
  } catch {
    // Logging is diagnostic only; it must not prevent health-check settlement.
  }
}

function isHealthyOutput(stdout: string): boolean {
  // The server emits `ok <version>`. Strip terminal colour escapes because a
  // caller can still inherit a colour-enabled environment through execFile.
  const withoutAnsi = stdout.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, '').trim();
  return /^ok(?:\s|$)/.test(withoutAnsi);
}

/**
 * Run `perllsp --health` without coupling binary probing to activation state.
 * The returned promise settles once, even if a process callback races the
 * module-owned timeout or a diagnostic logger throws.
 */
export async function runLanguageServerHealthCheck(
  serverPath: string,
  log: HealthCheckLog,
  options: HealthCheckProcessOptions = {},
): Promise<boolean> {
  const runProcess = options.execFile ?? execFile;
  const timeoutMs = options.timeoutMs ?? MANAGED_BINARY_HEALTH_TIMEOUT_MS;

  return new Promise((resolve) => {
    let settled = false;
    let child: HealthCheckChild;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const finish = (healthy: boolean): void => {
      if (settled) {
        return;
      }
      settled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      resolve(healthy);
    };

    const onTimeout = (): void => {
      if (settled) {
        return;
      }
      try {
        child?.kill?.();
      } catch {
        // The callback still owns settlement if a child exits during timeout.
      }
      safeAppendLine(log, `[health-check] Timed out after ${timeoutMs}ms`);
      finish(false);
    };

    timer = setTimeout(onTimeout, timeoutMs);

    try {
      child = runProcess(
        serverPath,
        ['--health'],
        { timeout: timeoutMs },
        (error: Error | null, stdout: string, stderr: string) => {
          if (settled) {
            return;
          }

          if (error) {
            safeAppendLine(log, `[health-check] Failed: ${error.message}`);
            const stderrText = stderr.trim();
            if (stderrText) {
              safeAppendLine(log, `[health-check] stderr: ${stderrText}`);
            }
            const stdoutText = stdout.trim();
            if (stdoutText) {
              safeAppendLine(log, `[health-check] stdout: ${stdoutText}`);
            }
            finish(false);
            return;
          }

          const stdoutText = stdout.trim();
          const healthy = isHealthyOutput(stdout);
          if (!healthy) {
            safeAppendLine(log, `[health-check] Unexpected output: ${stdoutText}`);
          }
          finish(healthy);
        },
      );
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      safeAppendLine(log, `[health-check] Failed: ${message}`);
      finish(false);
    }
  });
}
