export type CrashObservationSource = 'process_exit' | 'watchdog' | 'startup_failure';
export type CrashObservationSummary =
  | 'process_exit'
  | 'watchdog'
  | 'startup_failure'
  | 'both_deduped';
export type RecoveryDecisionDisposition =
  | 'start_recovery'
  | 'deduped_existing_episode'
  | 'deduped_previous_episode'
  | 'deferred_active_episode'
  | 'crash_budget_exhausted';
export type RecoveryTerminalDisposition = 'recovered' | 'recovery_failed' | 'cancelled';
export type NonCrashLifecycleAction =
  | 'user_restart'
  | 'extension_deactivation'
  | 'managed_update_restart'
  | 'activation_failed';

export interface CrashFailureObservation {
  failed_generation: number;
  process_identity: string;
  source: CrashObservationSource;
  observed_at_ms: number;
}

export interface CrashRecoveryDecision {
  episode_id: string;
  failed_generation: number;
  process_identity: string;
  observation_source: CrashObservationSummary;
  automatic_attempt: number;
  automatic_budget: number;
  disposition: RecoveryDecisionDisposition;
}

/**
 * Handle naming exactly one recovery episode. A `CrashRecoveryDecision`
 * structurally satisfies this interface, so the decision returned by
 * `observeFailure` is the handle its continuation must later settle through
 * `settleEpisode` — settlement can never name a different (newer) episode
 * than the one that authorized the continuation (#7845).
 */
export interface RecoveryEpisodeHandle {
  readonly episode_id: string;
}

interface RecoveryEpisodeState {
  decision: CrashRecoveryDecision;
  process_exit_observed: boolean;
  watchdog_observed: boolean;
  startup_failure_observed: boolean;
  terminal: RecoveryTerminalDisposition | null;
  replacement_generation: number | null;
}

interface StableRunState {
  generation: number;
  since_ms: number;
}

const RECENT_EPISODE_LIMIT = 16;

/**
 * Bound on deferred failure observations. While an episode is active only
 * the replacement generation spawned by that episode's in-flight restart
 * can newly fail, so more than a couple of distinct pending keys is already
 * anomalous; the cap keeps worst-case memory bounded regardless.
 */
const PENDING_OBSERVATION_LIMIT = 4;

function observationSummary(episode: RecoveryEpisodeState): CrashObservationSummary {
  const observed: CrashObservationSource[] = [];
  if (episode.process_exit_observed) {
    observed.push('process_exit');
  }
  if (episode.watchdog_observed) {
    observed.push('watchdog');
  }
  if (episode.startup_failure_observed) {
    observed.push('startup_failure');
  }
  // A single observation reports itself; several observations deduplicated
  // into one episode report the existing combined marker.
  const [single] = observed;
  if (observed.length === 1 && single !== undefined) {
    return single;
  }
  return 'both_deduped';
}

function episodeKey(generation: number, processIdentity: string): string {
  return `${generation}\u0000${processIdentity}`;
}

export class CrashRecoveryArbiter {
  private automaticAttempts = 0;
  private stableRun: StableRunState | null = null;
  private activeEpisode: RecoveryEpisodeState | null = null;
  private readonly recentEpisodes = new Map<string, RecoveryEpisodeState>();
  private readonly pendingObservations = new Map<string, CrashFailureObservation>();

  public constructor(
    private readonly maxAutomaticRestarts = 3,
    private readonly stableRunGraceMs = 30_000,
  ) {
    if (!Number.isInteger(maxAutomaticRestarts) || maxAutomaticRestarts < 1) {
      throw new Error('maxAutomaticRestarts must be a positive integer');
    }
    if (!Number.isFinite(stableRunGraceMs) || stableRunGraceMs < 0) {
      throw new Error('stableRunGraceMs must be a finite non-negative number');
    }
  }

  public observeFailure(observation: CrashFailureObservation): CrashRecoveryDecision {
    this.validateObservation(observation);
    const key = episodeKey(observation.failed_generation, observation.process_identity);

    if (
      this.activeEpisode !== null &&
      this.activeEpisode.decision.failed_generation === observation.failed_generation &&
      this.activeEpisode.decision.process_identity === observation.process_identity
    ) {
      this.addObservationSource(this.activeEpisode, observation.source);
      this.activeEpisode.decision.observation_source = observationSummary(this.activeEpisode);
      return {
        ...this.activeEpisode.decision,
        disposition: 'deduped_existing_episode',
      };
    }

    const previous = this.recentEpisodes.get(key);
    if (previous !== undefined) {
      this.addObservationSource(previous, observation.source);
      previous.decision.observation_source = observationSummary(previous);
      return {
        ...previous.decision,
        disposition: 'deduped_previous_episode',
      };
    }

    // A different generation failing while an episode is still active must
    // not start a second concurrent recovery nor silently steal the active
    // slot: the active continuation is still awaiting its restart promise,
    // and its eventual settle is bound to its own episode handle. Defer the
    // observation; the continuation drains and re-arbitrates it (through
    // this same method, budget evaluation included) once it settles. This
    // serializes different-generation failures behind the active episode.
    if (this.activeEpisode !== null) {
      this.rememberPendingObservation(observation);
      return {
        episode_id: this.activeEpisode.decision.episode_id,
        failed_generation: observation.failed_generation,
        process_identity: observation.process_identity,
        observation_source: observation.source,
        automatic_attempt: 0,
        automatic_budget: this.maxAutomaticRestarts,
        disposition: 'deferred_active_episode',
      };
    }

    if (
      this.stableRun !== null &&
      this.stableRun.generation === observation.failed_generation &&
      observation.observed_at_ms - this.stableRun.since_ms >= this.stableRunGraceMs
    ) {
      this.automaticAttempts = 0;
    }
    this.stableRun = null;

    const nextAttempt = this.automaticAttempts + 1;
    const exhausted = nextAttempt > this.maxAutomaticRestarts;
    const decision: CrashRecoveryDecision = {
      episode_id: `recovery-${observation.failed_generation}-${nextAttempt}`,
      failed_generation: observation.failed_generation,
      process_identity: observation.process_identity,
      observation_source: observation.source,
      automatic_attempt: exhausted ? this.automaticAttempts : nextAttempt,
      automatic_budget: this.maxAutomaticRestarts,
      disposition: exhausted ? 'crash_budget_exhausted' : 'start_recovery',
    };

    const episode: RecoveryEpisodeState = {
      decision,
      process_exit_observed: observation.source === 'process_exit',
      watchdog_observed: observation.source === 'watchdog',
      startup_failure_observed: observation.source === 'startup_failure',
      terminal: exhausted ? 'recovery_failed' : null,
      replacement_generation: null,
    };

    if (!exhausted) {
      this.automaticAttempts = nextAttempt;
      this.activeEpisode = episode;
    }
    this.rememberEpisode(key, episode);
    return { ...decision };
  }

  /**
   * Settle exactly the episode named by `handle` — the decision that
   * authorized the calling recovery continuation.
   *
   * A continuation whose restart promise resolved late must never mutate a
   * newer episode that became active in the meantime: if the named episode
   * is no longer the active one (already settled, or cleared by an explicit
   * recovery reset), only its own history entry is updated and the active
   * episode is left untouched. Returns `true` when the named episode was the
   * active one and the arbiter is idle again.
   */
  public settleEpisode(
    handle: RecoveryEpisodeHandle,
    terminal: RecoveryTerminalDisposition,
    replacementGeneration: number | null,
  ): boolean {
    const episode = this.episodeById(handle.episode_id);
    if (episode === null || episode.terminal !== null) {
      return false;
    }
    const wasActive = this.activeEpisode === episode;
    episode.terminal = terminal;
    episode.replacement_generation = replacementGeneration;
    if (wasActive) {
      this.activeEpisode = null;
    }
    return wasActive;
  }

  /**
   * Remove and return the oldest deferred failure observation, if any.
   *
   * The continuation that just settled its episode calls this once per
   * settle and re-arbitrates the returned observation through
   * `observeFailure`, so a different-generation failure that arrived while
   * an episode was active recovers serially instead of being lost or
   * overwriting the active slot.
   */
  public takePendingFailureObservation(): CrashFailureObservation | null {
    const oldestKey = this.pendingObservations.keys().next().value as string | undefined;
    if (oldestKey === undefined) {
      return null;
    }
    const observation = this.pendingObservations.get(oldestKey);
    if (observation === undefined) {
      return null;
    }
    this.pendingObservations.delete(oldestKey);
    return { ...observation };
  }

  public markRunning(generation: number, sinceMs: number): void {
    if (!Number.isInteger(generation) || generation < 0) {
      throw new Error('generation must be a non-negative integer');
    }
    if (!Number.isFinite(sinceMs) || sinceMs < 0) {
      throw new Error('sinceMs must be a finite non-negative number');
    }
    this.stableRun = { generation, since_ms: sinceMs };
  }

  public resetForExplicitRecovery(): void {
    this.automaticAttempts = 0;
    this.stableRun = null;
    this.activeEpisode = null;
    // An explicit recovery (user restart, managed update, deactivation)
    // supersedes deferred observations: the explicit restart already moved
    // the generation forward, so re-arbitrating a deferred failure of a
    // dead generation would be stale by construction.
    this.pendingObservations.clear();
  }

  /**
   * Clear all episode state, including the recent-episode dedupe history.
   *
   * Unlike `resetForExplicitRecovery` (used by explicit user restarts,
   * managed updates, and deactivation, which keep the dedupe history so a
   * stale observation for a still-failed generation stays deduplicated),
   * this method exists for test isolation between independent cases and is
   * not part of the production recovery flow.
   */
  public resetAllEpisodeMemory(): void {
    this.resetForExplicitRecovery();
    this.recentEpisodes.clear();
  }

  public automaticAttemptCount(): number {
    return this.automaticAttempts;
  }

  public activeEpisodeDecision(): CrashRecoveryDecision | null {
    return this.activeEpisode === null ? null : { ...this.activeEpisode.decision };
  }

  public static consumesCrashBudget(action: NonCrashLifecycleAction): boolean {
    switch (action) {
      case 'user_restart':
      case 'extension_deactivation':
      case 'managed_update_restart':
      case 'activation_failed':
        return false;
    }
  }

  private addObservationSource(
    episode: RecoveryEpisodeState,
    source: CrashObservationSource,
  ): void {
    if (source === 'process_exit') {
      episode.process_exit_observed = true;
    } else if (source === 'watchdog') {
      episode.watchdog_observed = true;
    } else {
      episode.startup_failure_observed = true;
    }
  }

  private episodeById(episodeId: string): RecoveryEpisodeState | null {
    if (this.activeEpisode !== null && this.activeEpisode.decision.episode_id === episodeId) {
      return this.activeEpisode;
    }
    for (const episode of this.recentEpisodes.values()) {
      if (episode.decision.episode_id === episodeId) {
        return episode;
      }
    }
    return null;
  }

  private rememberPendingObservation(observation: CrashFailureObservation): void {
    const key = episodeKey(observation.failed_generation, observation.process_identity);
    // Re-observing the same deferred generation refreshes the queued
    // observation (latest source timestamp wins) instead of queueing a
    // duplicate.
    this.pendingObservations.set(key, { ...observation });
    while (this.pendingObservations.size > PENDING_OBSERVATION_LIMIT) {
      const oldestKey = this.pendingObservations.keys().next().value as string | undefined;
      if (oldestKey === undefined) {
        break;
      }
      this.pendingObservations.delete(oldestKey);
    }
  }

  private rememberEpisode(key: string, episode: RecoveryEpisodeState): void {
    this.recentEpisodes.set(key, episode);
    while (this.recentEpisodes.size > RECENT_EPISODE_LIMIT) {
      const oldestKey = this.recentEpisodes.keys().next().value as string | undefined;
      if (oldestKey === undefined) {
        break;
      }
      this.recentEpisodes.delete(oldestKey);
    }
  }

  private validateObservation(observation: CrashFailureObservation): void {
    if (!Number.isInteger(observation.failed_generation) || observation.failed_generation < 0) {
      throw new Error('failed_generation must be a non-negative integer');
    }
    if (observation.process_identity.trim().length === 0) {
      throw new Error('process_identity must be non-empty');
    }
    if (!Number.isFinite(observation.observed_at_ms) || observation.observed_at_ms < 0) {
      throw new Error('observed_at_ms must be a finite non-negative number');
    }
  }
}
