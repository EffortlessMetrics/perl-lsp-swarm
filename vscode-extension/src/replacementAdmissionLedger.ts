/**
 * Production routes that may construct a replacement language-client generation.
 *
 * #14448 requires every replacement path to consume the same lifecycle terminal
 * gate (`LanguageClientLifecycle.restart()` with process witness + terminality).
 * The extension funnels those paths through `restartServer()`; this ledger is
 * the checked denominator so a new restart call site cannot land without a
 * deliberate row and proof update.
 */

export interface ReplacementAdmissionRoute {
  /** Stable id for tests and issue cross-reference. */
  readonly id: string;
  /** Substring that must remain present in `extension.ts` while the route exists. */
  readonly sourceMarker: string;
  readonly summary: string;
}

/**
 * Checked list of production replacement entrypoints in `extension.ts`.
 *
 * Each marker is intentionally unique so accidental deletion or consolidation
 * fails a named test rather than silently dropping a route from the denominator.
 */
export const REPLACEMENT_ADMISSION_ROUTES: readonly ReplacementAdmissionRoute[] = [
  {
    id: 'registered_restart_command',
    sourceMarker: "restartServer: () => restartServerFromExplicitRecovery(context)",
    summary: 'perl-lsp.restart command via server command group',
  },
  {
    id: 'settings_refresh_restart',
    sourceMarker: "if (choice === 'Restart Now')",
    summary: 'Settings-changed prompt chooses Restart Now',
  },
  {
    id: 'crash_recovery_automatic',
    sourceMarker: 'const restartBlockedByIncompleteCleanup = await restartServer(context)',
    summary: 'Automatic crash recovery after arbiter authorization',
  },
  {
    id: 'watchdog_unresponsive_recovery',
    sourceMarker: "await recoverFromObservedCrash('watchdog'",
    summary: 'Watchdog hang observation enters shared crash recovery (replacement via restartServer)',
  },
  {
    id: 'crash_budget_manual_restart',
    sourceMarker: "if (selection === 'Restart Server' && context)",
    summary: 'Manual restart after auto-restart budget exhausted',
  },
  {
    id: 'reinstall_restore_after_download_failure',
    sourceMarker: '[reinstall] restoring previous binary after incomplete download',
    summary: 'Reinstall rollback after incomplete download while server was running',
  },
  {
    id: 'reinstall_restore_after_health_failure',
    sourceMarker: '[reinstall] restoring previous binary after failed health check',
    summary: 'Reinstall rollback after downloaded binary fails health check',
  },
  {
    id: 'reinstall_success_restart',
    sourceMarker: '[reinstall] restarting language client with the freshly installed binary',
    summary: 'Successful reinstall while server was running',
  },
  {
    id: 'reinstall_retry_without_running_client',
    sourceMarker: 'A retry after first-install failure has no running client to restart',
    summary: 'Reinstall health ok but lifecycle was not running (retry/start path)',
  },
];

/** The single lifecycle gate every replacement route must reach. */
export const LIFECYCLE_REPLACEMENT_GATE_MARKER = 'await lifecycle.restart()';

/** Authoritative funnel for replacement construction in production. */
export const REPLACEMENT_FUNNEL_FUNCTION = 'async function restartServer(';
