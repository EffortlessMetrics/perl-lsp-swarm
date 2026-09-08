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
  snapshot: Pick<LifecycleSnapshot, 'state' | 'generation' | 'error' | 'serverPath'>,
  expectedPath?: string | null,
): HealthCheckResult {
  const label = 'LSP runtime';
  if (expectedPath !== undefined && snapshot.serverPath !== expectedPath) {
    return {
      label,
      ok: false,
      status: HealthCheckStatus.Error,
      detail:
        'The language server changed while health checks were running. Run Health Check again.',
    };
  }

  if (snapshot.state === 'running') {
    return {
      label,
      ok: true,
      status: HealthCheckStatus.Ok,
      detail: 'Language server is running.',
    };
  }

  if (snapshot.state === 'failed') {
    return {
      label,
      ok: false,
      status: HealthCheckStatus.Error,
      detail: `Language server failed to start: ${describeLifecycleError(snapshot.error)}`,
    };
  }

  return {
    label,
    ok: false,
    status: HealthCheckStatus.Error,
    detail: 'Language server is not running.',
  };
}
