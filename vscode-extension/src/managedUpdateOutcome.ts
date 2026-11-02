/**
 * One typed owner for managed update outcomes and user-visible rendering (#16804).
 *
 * This module composes producer-owned facts into deterministic presentation.
 * It is deliberately `vscode`-free: callers apply {@link ManagedUpdatePresentation}
 * to notifications, the output channel, and commands.
 *
 * Slice on main: admission refusals (`check_skipped`) and manual terminal results
 * (`up_to_date`, `updated_reload_required`, `updated_reload_unverified`). Download
 * transport, integrity, cancellation, and network classification remain owned by
 * their producers and will compose here in follow-on slices.
 */

import type { ManagedUpdateCheckDecision } from './managedUpdateCheckAdmission';

export type ManagedUpdateOutcome =
  | { readonly kind: 'check_skipped'; readonly decision: Exclude<ManagedUpdateCheckDecision, { kind: 'run' }> }
  | { readonly kind: 'up_to_date'; readonly installedVersion: string; readonly manual: boolean }
  | { readonly kind: 'updated_reload_required'; readonly installedVersion: string }
  | { readonly kind: 'updated_reload_unverified'; readonly installedPath: string };

export type ManagedUpdateNotificationVisibility =
  | 'none'
  | 'once-per-session'
  | 'explicit-command'
  | 'always';

export type ManagedUpdateSeverity = 'information' | 'warning' | 'error';

export type ManagedUpdateAction = 'Reload Window' | 'View Logs';

export interface ManagedUpdateNotificationPresentation {
  readonly visibility: ManagedUpdateNotificationVisibility;
  readonly severity: ManagedUpdateSeverity;
  /** Short user-facing summary when visibility is not `none`. */
  readonly summary: string;
}

export interface ManagedUpdateOutputPresentation {
  readonly line: string;
}

export interface ManagedUpdatePresentation {
  readonly dedupIdentity: string;
  readonly notification: ManagedUpdateNotificationPresentation;
  readonly outputChannel: ManagedUpdateOutputPresentation;
  readonly actions: readonly ManagedUpdateAction[];
}

export function composeCheckSkippedOutcome(
  decision: Exclude<ManagedUpdateCheckDecision, { kind: 'run' }>,
): Extract<ManagedUpdateOutcome, { kind: 'check_skipped' }> {
  return { kind: 'check_skipped', decision };
}

export function composeUpToDateOutcome(
  installedVersion: string,
  manual = false,
): Extract<ManagedUpdateOutcome, { kind: 'up_to_date' }> {
  return { kind: 'up_to_date', installedVersion, manual };
}

export function composeUpdatedReloadRequiredOutcome(
  installedVersion: string,
): Extract<ManagedUpdateOutcome, { kind: 'updated_reload_required' }> {
  return { kind: 'updated_reload_required', installedVersion };
}

export function composeUpdatedReloadUnverifiedOutcome(
  installedPath: string,
): Extract<ManagedUpdateOutcome, { kind: 'updated_reload_unverified' }> {
  return { kind: 'updated_reload_unverified', installedPath };
}

export function projectManagedUpdateOutcome(outcome: ManagedUpdateOutcome): ManagedUpdatePresentation {
  switch (outcome.kind) {
    case 'check_skipped':
      return projectCheckSkipped(outcome.decision);
    case 'up_to_date':
      return projectUpToDate(outcome.installedVersion, outcome.manual);
    case 'updated_reload_required':
      return projectUpdatedReloadRequired(outcome.installedVersion);
    case 'updated_reload_unverified':
      return projectUpdatedReloadUnverified(outcome.installedPath);
  }
}

function projectCheckSkipped(
  decision: Exclude<ManagedUpdateCheckDecision, { kind: 'run' }>,
): ManagedUpdatePresentation {
  const explanation = decision.kind === 'skip' ? decision.detail : decision.remediation;
  const outputLine = `[update-check] ${decision.kind}: ${decision.reason} — ${explanation}`;
  const dedupIdentity = `check_skipped:${decision.reason}`;

  if (decision.userVisibility !== 'report') {
    return {
      dedupIdentity,
      notification: {
        visibility: 'none',
        severity: 'information',
        summary: '',
      },
      outputChannel: { line: outputLine },
      actions: [],
    };
  }

  const notification = manualCheckSkippedNotification(decision, explanation);
  return {
    dedupIdentity,
    notification,
    outputChannel: { line: outputLine },
    actions: [],
  };
}

function manualCheckSkippedNotification(
  decision: Exclude<ManagedUpdateCheckDecision, { kind: 'run' }>,
  explanation: string,
): ManagedUpdateNotificationPresentation {
  switch (decision.reason) {
    case 'pinned_exact_tag':
      return {
        visibility: 'explicit-command',
        severity: 'information',
        summary: `Binary update check skipped: ${explanation}`,
      };
    case 'explicit_user_binary_selected':
      return {
        visibility: 'explicit-command',
        severity: 'information',
        summary: 'Binary update checks are unavailable while perl-lsp.serverPath is configured.',
      };
    case 'binary_not_managed':
      return {
        visibility: 'explicit-command',
        severity: 'information',
        summary:
          'Binary update checks are unavailable because this binary is not managed by perl-lsp.',
      };
    case 'local_binary_missing':
      return {
        visibility: 'explicit-command',
        severity: 'warning',
        summary:
          'Binary update check could not run because the managed binary is missing. Run Perl: Reinstall Server Binary.',
      };
    default:
      return {
        visibility: 'explicit-command',
        severity: 'warning',
        summary: `Binary update check skipped: ${explanation}`,
      };
  }
}

function projectUpToDate(installedVersion: string, manual: boolean): ManagedUpdatePresentation {
  return {
    dedupIdentity: `up_to_date:${installedVersion}`,
    notification: {
      visibility: manual ? 'explicit-command' : 'none',
      severity: 'information',
      summary: manual ? `You are up to date (${installedVersion}).` : '',
    },
    outputChannel: {
      line: `[update-check] Up to date (${installedVersion})`,
    },
    actions: [],
  };
}

function projectUpdatedReloadRequired(installedVersion: string): ManagedUpdatePresentation {
  return {
    dedupIdentity: `updated_reload_required:${installedVersion}`,
    notification: {
      visibility: 'explicit-command',
      severity: 'information',
      summary: `perllsp ${installedVersion} downloaded. Reload the window to switch to it.`,
    },
    outputChannel: {
      line: `[update-check] Installed ${installedVersion}; reload required to switch the running server.`,
    },
    actions: ['Reload Window'],
  };
}

function projectUpdatedReloadUnverified(installedPath: string): ManagedUpdatePresentation {
  return {
    dedupIdentity: 'updated_reload_unverified',
    notification: {
      visibility: 'explicit-command',
      severity: 'warning',
      summary:
        'perllsp update was downloaded, but its installed version could not be verified. The running server is unchanged; check the Perl Language Server output before reloading.',
    },
    outputChannel: {
      line: `[update-check] Installed binary at ${installedPath} has no readable version; reload confirmation withheld.`,
    },
    actions: ['View Logs'],
  };
}

/** Session-local dedupe for quiet background outcomes (#16804). */
export class ManagedUpdateOutcomeDedupe {
  private readonly seenOutput = new Set<string>();

  shouldEmitOutput(presentation: ManagedUpdatePresentation): boolean {
    if (presentation.dedupIdentity.startsWith('up_to_date:')) {
      return true;
    }
    if (presentation.notification.visibility === 'explicit-command') {
      return true;
    }
    if (presentation.notification.visibility !== 'none') {
      return true;
    }
    if (this.seenOutput.has(presentation.dedupIdentity)) {
      return false;
    }
    this.seenOutput.add(presentation.dedupIdentity);
    return true;
  }

  clear(): void {
    this.seenOutput.clear();
  }
}

const sessionManagedUpdateOutcomeDedupe = new ManagedUpdateOutcomeDedupe();

/** One dedupe owner per extension host session (#16804); shared across downloaders. */
export function getSessionManagedUpdateOutcomeDedupe(): ManagedUpdateOutcomeDedupe {
  return sessionManagedUpdateOutcomeDedupe;
}
