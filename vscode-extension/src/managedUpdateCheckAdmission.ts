/**
 * One typed owner for managed update-check admission (#16803).
 *
 * Before this module, `BinaryDownloader.checkForUpdateSilent` answered "should
 * this check run?" with a chain of independent early `return`s — one per
 * pinned tag, zero interval, elapsed timestamp, user-configured `serverPath`,
 * and a bundled-vs-managed binary test — each with no shared reason. Three
 * consequences followed from that shape, all reproducible on `main`:
 *
 * - The manual `Check for Updates` command reset the *unscoped* legacy
 *   timestamp before delegating, while the check itself read the *scoped*
 *   (#9847) timestamp first. Once a scoped value existed, the manual command
 *   was silently suppressed by a state write that could no longer be read.
 * - `updateCheckInterval = 0` doubled as "Don't ask again" prompt state, so
 *   suppressing one recurring prompt also switched off every future check.
 * - `channel = 'tag'`, an unknown channel, and `versionTag` under a non-tag
 *   channel all returned without a disposition at all.
 *
 * This module replaces the branch chain with a single pure decision over facts
 * the caller has already established. It is deliberately pure and
 * `vscode`-free: the decision depends on no editor API, no clock, and no
 * filesystem, so every branch below is reachable from a plain unit test.
 *
 * Explicit non-ownership:
 * - which host target may be used (#10073/#9096) — consumed, not recomputed;
 * - which release is selected (#9098/#9925) — this module never fetches;
 * - download, install, restart, and notification presentation (#16804).
 *
 * Why there is no `repair_or_reinstall` intent: `perl-lsp.reinstall` calls
 * `ensureBinary(true)`, a forced download, and never consulted update-check
 * admission. Routing it here would gate a repair behind the very cadence the
 * user disabled, so the intent is absent rather than modeled and unreachable.
 */

/** Why a check was attempted, and therefore which suppressions it may bypass. */
export type ManagedUpdateCheckIntent =
  /** Fired once during `activate()` after startup completes. */
  | 'activation_background'
  /** Fired by any later timer-driven check. */
  | 'scheduled_background'
  /** The user explicitly ran `Check for Updates`. */
  | 'manual_force';

export type ManagedUpdateCheckChannel = 'stable' | 'latest' | 'tag';

const CHANNELS: ReadonlySet<string> = new Set<ManagedUpdateCheckChannel>([
  'stable',
  'latest',
  'tag',
]);

const HOUR_MS = 1000 * 60 * 60;

/**
 * Everything the decision needs, already established by the caller.
 *
 * Ownership facts (`configuredServerPath`, `managedBinaryPath`,
 * `localBinaryPresent`, `hostCompatibilityProven`, `scopedStateKey`) are passed
 * in rather than derived: the owners named above already compute them, and
 * rediscovering them here would be exactly the duplicate authority this module
 * exists to remove.
 */
export interface ManagedUpdateCheckFacts {
  readonly intent: ManagedUpdateCheckIntent;
  /** Raw `perl-lsp.channel`; an unknown value is an invalid configuration. */
  readonly channel: string;
  /** Raw `perl-lsp.versionTag`. */
  readonly versionTag: string;
  /** Raw `perl-lsp.updateCheckInterval`, in hours. */
  readonly updateCheckIntervalHours: number;
  /** Raw `perl-lsp.autoUpdate` — the action taken *after* an admitted check. */
  readonly autoUpdate: boolean;
  /** Whether the user chose "Don't ask again" for this compatibility target. */
  readonly promptSuppressed: boolean;
  /** Raw `perl-lsp.serverPath`; non-empty means the user owns their binary. */
  readonly configuredServerPath: string;
  /** False when this host has no proven compatibility key (#10073/#9096). */
  readonly hostCompatibilityProven: boolean;
  /** True when the resolved binary lives under the extension's managed storage. */
  readonly managedBinaryPath: boolean;
  readonly localBinaryPresent: boolean;
  /**
   * The compatibility-scoped state key (#9847) that owns the cadence
   * timestamp. Empty when the host has no proven key, which is refused above.
   */
  readonly scopedStateKey: string;
  /**
   * Effective last-check time: the scoped value when established, otherwise
   * the unscoped legacy seed. Callers must not resolve this precedence twice.
   */
  readonly lastCheckMs: number;
  readonly nowMs: number;
}

/** Why a check actually ran. */
export type ManagedUpdateCheckRunReason = 'manual_forced' | 'scheduled_due';

/**
 * What to do once an admitted check finds an update.
 *
 * `prompt_suppressed` is the "Don't ask again" disposition: it suppresses the
 * recurring prompt *only*. It is deliberately not a skip reason, because the
 * check itself still has to run — suppressing a prompt must never be able to
 * suppress a check, and it never masks an explicitly enabled `autoUpdate`.
 */
export type ManagedUpdateCheckAction = 'auto_update' | 'prompt' | 'prompt_suppressed';

export type ManagedUpdateCheckSkipReason =
  | 'interval_not_elapsed'
  | 'background_checking_disabled'
  | 'explicit_user_binary_selected'
  | 'pinned_exact_tag'
  | 'unsupported_or_unproven_host'
  | 'binary_not_managed'
  | 'local_binary_missing';

export type ManagedUpdateCheckInvalidReason =
  | 'unknown_channel'
  | 'missing_or_invalid_tag'
  | 'version_tag_ignored_for_channel'
  | 'invalid_update_check_interval';

/**
 * Whether a refusal is worth the user's attention. A background check stays
 * quiet and reports through the output channel; a manual one was asked for, so
 * it gets an answer.
 */
export type ManagedUpdateCheckVisibility = 'silent' | 'report';

export interface RunManagedUpdateCheck {
  readonly kind: 'run';
  readonly reason: ManagedUpdateCheckRunReason;
  readonly action: ManagedUpdateCheckAction;
  /** The cadence key to advance once this check has been attempted. */
  readonly stateKey: string;
}

export interface SkipManagedUpdateCheck {
  readonly kind: 'skip';
  readonly reason: ManagedUpdateCheckSkipReason;
  readonly userVisibility: ManagedUpdateCheckVisibility;
  /** Exact, user-presentable statement of why no check ran. */
  readonly detail: string;
}

export interface InvalidManagedUpdateCheck {
  readonly kind: 'invalid';
  readonly reason: ManagedUpdateCheckInvalidReason;
  readonly userVisibility: ManagedUpdateCheckVisibility;
  /** Exact, user-presentable statement of how to repair the configuration. */
  readonly remediation: string;
}

export type ManagedUpdateCheckDecision =
  | RunManagedUpdateCheck
  | SkipManagedUpdateCheck
  | InvalidManagedUpdateCheck;

function isManual(intent: ManagedUpdateCheckIntent): boolean {
  return intent === 'manual_force';
}

function trimmed(value: string): string {
  return value.trim();
}

/**
 * Decide whether one managed update check may run, and what it should do.
 *
 * Precedence is the contract, so it is stated here once:
 *
 * 1. **Host identity** — an unproven compatibility key is refused before
 *    anything is read or written, because no state may be attributed to a
 *    target this host cannot name.
 * 2. **Configuration validity** — an unusable channel/tag/interval is `invalid`,
 *    not `skip`. It is a fact the user can repair, and it must not masquerade
 *    as "nothing to report".
 * 3. **Binary ownership** — a user-managed, bundled, or absent binary is a
 *    `skip` with an exact reason rather than an early return.
 * 4. **Exact tag pinning** — "no automatic update check" is a decision with a
 *    name, and must never read as "up to date".
 * 5. **Manual force** — bypasses cadence and timestamp suppression, but *not*
 *    the refusals above, because a pinned tag or a broken config is an answer
 *    the user asked for.
 * 6. **Background cadence** — `updateCheckInterval = 0` disables scheduled
 *    checks only, and an unelapsed interval is its own reason.
 */
export function decideManagedUpdateCheck(
  facts: ManagedUpdateCheckFacts,
): ManagedUpdateCheckDecision {
  const visibility: ManagedUpdateCheckVisibility = isManual(facts.intent) ? 'report' : 'silent';

  if (!facts.hostCompatibilityProven || facts.scopedStateKey.length === 0) {
    return {
      kind: 'skip',
      reason: 'unsupported_or_unproven_host',
      userVisibility: visibility,
      detail:
        'This host has no proven perllsp compatibility target, so the extension cannot attribute managed update state to it.',
    };
  }

  const invalid = invalidConfiguration(facts, visibility);
  if (invalid !== null) {
    return invalid;
  }

  if (trimmed(facts.configuredServerPath).length > 0) {
    return {
      kind: 'skip',
      reason: 'explicit_user_binary_selected',
      userVisibility: visibility,
      detail:
        'perl-lsp.serverPath is set, so the extension does not manage this binary and will not check for updates for it.',
    };
  }

  if (!facts.managedBinaryPath) {
    return {
      kind: 'skip',
      reason: 'binary_not_managed',
      userVisibility: visibility,
      detail:
        'The active perllsp binary is not managed by the extension, so there is nothing to update-check.',
    };
  }

  if (!facts.localBinaryPresent) {
    return {
      kind: 'skip',
      reason: 'local_binary_missing',
      userVisibility: visibility,
      detail:
        'No managed perllsp binary is installed for this host yet. Run "Perl: Reinstall Server Binary" to install one.',
    };
  }

  const channel = trimmed(facts.channel) as ManagedUpdateCheckChannel;
  if (channel === 'tag') {
    return {
      kind: 'skip',
      reason: 'pinned_exact_tag',
      userVisibility: visibility,
      detail: `perl-lsp.channel is pinned to the exact tag ${trimmed(facts.versionTag)}; automatic update checks are disabled by that pin.`,
    };
  }

  if (isManual(facts.intent)) {
    return {
      kind: 'run',
      reason: 'manual_forced',
      action: facts.autoUpdate ? 'auto_update' : 'prompt',
      stateKey: facts.scopedStateKey,
    };
  }

  if (facts.updateCheckIntervalHours <= 0) {
    return {
      kind: 'skip',
      reason: 'background_checking_disabled',
      userVisibility: visibility,
      detail: `perl-lsp.updateCheckInterval is ${facts.updateCheckIntervalHours}, which disables scheduled background update checks. Manual "Check for Updates" still works.`,
    };
  }

  // A timestamp this decision cannot interpret is not evidence that the
  // interval elapsed, so it is treated as "never checked" rather than trusted.
  const lastCheckMs =
    Number.isFinite(facts.lastCheckMs) && facts.lastCheckMs > 0 && facts.lastCheckMs <= facts.nowMs
      ? facts.lastCheckMs
      : 0;
  if (facts.nowMs - lastCheckMs < facts.updateCheckIntervalHours * HOUR_MS) {
    return {
      kind: 'skip',
      reason: 'interval_not_elapsed',
      userVisibility: visibility,
      detail: `The last update check was less than ${facts.updateCheckIntervalHours} hour(s) ago. Manual "Check for Updates" still works.`,
    };
  }

  return {
    kind: 'run',
    reason: 'scheduled_due',
    action: facts.autoUpdate
      ? 'auto_update'
      : facts.promptSuppressed
        ? 'prompt_suppressed'
        : 'prompt',
    stateKey: facts.scopedStateKey,
  };
}

function invalidConfiguration(
  facts: ManagedUpdateCheckFacts,
  visibility: ManagedUpdateCheckVisibility,
): InvalidManagedUpdateCheck | null {
  const channel = trimmed(facts.channel);
  const versionTag = trimmed(facts.versionTag);

  if (!CHANNELS.has(channel)) {
    return {
      kind: 'invalid',
      reason: 'unknown_channel',
      userVisibility: visibility,
      remediation: `perl-lsp.channel is ${JSON.stringify(facts.channel)}; expected one of stable, latest, or tag.`,
    };
  }

  if (channel === 'tag' && versionTag.length === 0) {
    return {
      kind: 'invalid',
      reason: 'missing_or_invalid_tag',
      userVisibility: visibility,
      remediation:
        'perl-lsp.channel is set to "tag" but perl-lsp.versionTag is empty; set the exact tag to pin, or choose "stable" or "latest".',
    };
  }

  if (channel !== 'tag' && versionTag.length > 0) {
    return {
      kind: 'invalid',
      reason: 'version_tag_ignored_for_channel',
      userVisibility: visibility,
      remediation: `perl-lsp.versionTag is set to ${JSON.stringify(facts.versionTag)} but perl-lsp.channel is ${JSON.stringify(channel)}; versionTag is only honoured when channel is "tag".`,
    };
  }

  if (!Number.isFinite(facts.updateCheckIntervalHours)) {
    return {
      kind: 'invalid',
      reason: 'invalid_update_check_interval',
      userVisibility: visibility,
      remediation: `perl-lsp.updateCheckInterval is ${JSON.stringify(facts.updateCheckIntervalHours)}; expected a number of hours.`,
    };
  }

  return null;
}
