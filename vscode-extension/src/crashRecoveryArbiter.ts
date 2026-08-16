export type CrashObservationSource = 'process_exit' | 'watchdog';
export type CrashObservationSummary = 'process_exit' | 'watchdog' | 'both_deduped';
export type RecoveryDecisionDisposition =
  | 'start_recovery'
  | 'deduped_existing_episode'
  | 'deduped_previous_episode'
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

interface RecoveryEpisodeState {
  decision: CrashRecoveryDecision;
  process_exit_observed: boolean;
  watchdog_observed: boolean;
  terminal: RecoveryTerminalDisposition | null;
  replacement_generation: number | null;
}

interface StableRunState {
  generation: number;
  since_ms: number;
}

const RECENT_EPISODE_LIMIT = 16;

function observationSummary(episode: RecoveryEpisodeState): CrashObservationSummary {
  if (episode.process_exit_observed && episode.watchdog_observed) {
    return 'both_deduped';
  }
  return episode.watchdog_observed ? 'watchdog' : 'process_exit';
}

function episodeKey(generation: number, processIdentity: string): string {
  return `${generation}\u0000${processIdentity}`;
}

export class CrashRecoveryArbiter {
  private automaticAttempts = 0;
  private stableRun: StableRunState | null = null;
  private activeEpisode: RecoveryEpisodeState | null = null;
  private readonly recentEpisodes = new Map<string, RecoveryEpisodeState>();

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

  public settleActiveEpisode(
    terminal: RecoveryTerminalDisposition,
    replacementGeneration: number | null,
  ): void {
    if (this.activeEpisode === null) {
      return;
    }
    this.activeEpisode.terminal = terminal;
    this.activeEpisode.replacement_generation = replacementGeneration;
    this.activeEpisode = null;
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
    } else {
      episode.watchdog_observed = true;
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
