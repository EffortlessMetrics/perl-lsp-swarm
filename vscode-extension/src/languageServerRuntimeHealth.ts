import { HealthCheckStatus, type HealthCheckResult } from './onboarding';
import type { LifecycleSnapshot } from './languageClientLifecycle';

function describeLifecycleError(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (error === undefined) {
    return 'unknown startup error';
  }
  return String(error);
}

/**
 * Project the authoritative language-client lifecycle into a health result.
 *
 * A binary path is only setup evidence. Runtime health requires the current
 * lifecycle generation to have reached `running`; retaining a path after a
 * failed start must never turn that failure into a passing check.
 */
export function languageServerRuntimeHealth(
  snapshot: Pick<LifecycleSnapshot, 'state' | 'generation' | 'error'>,
): HealthCheckResult {
  const label = 'LSP runtime';
  if (snapshot.state === 'running') {
    return {
      label,
      ok: true,
      status: HealthCheckStatus.Ok,
      detail: `Language server running (generation ${snapshot.generation}).`,
    };
  }

  if (snapshot.state === 'failed') {
    return {
      label,
      ok: false,
      status: HealthCheckStatus.Error,
      detail:
        `Language server failed to start (generation ${snapshot.generation}): ` +
        describeLifecycleError(snapshot.error),
    };
  }

  return {
    label,
    ok: false,
    status: HealthCheckStatus.Error,
    detail: `Language server is not running (state ${snapshot.state}, generation ${snapshot.generation}).`,
  };
}
