import { CrashRecoveryArbiter } from '../crashRecoveryArbiter';

describe('CrashRecoveryArbiter', () => {
  test('starts one recovery episode and deduplicates watchdog plus process-exit observations', () => {
    const arbiter = new CrashRecoveryArbiter(3, 30_000);

    const first = arbiter.observeFailure({
      failed_generation: 7,
      process_identity: 'pid-100',
      source: 'watchdog',
      observed_at_ms: 10_000,
    });
    const duplicate = arbiter.observeFailure({
      failed_generation: 7,
      process_identity: 'pid-100',
      source: 'process_exit',
      observed_at_ms: 10_001,
    });

    expect(first).toMatchObject({
      disposition: 'start_recovery',
      automatic_attempt: 1,
      observation_source: 'watchdog',
    });
    expect(duplicate).toMatchObject({
      disposition: 'deduped_existing_episode',
      automatic_attempt: 1,
      observation_source: 'both_deduped',
    });
    expect(arbiter.automaticAttemptCount()).toBe(1);
  });

  test('does not let a delayed callback reopen a settled failed generation', () => {
    const arbiter = new CrashRecoveryArbiter();
    arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    arbiter.settleActiveEpisode('recovered', 2);

    const delayed = arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'watchdog',
      observed_at_ms: 2_000,
    });

    expect(delayed.disposition).toBe('deduped_previous_episode');
    expect(delayed.observation_source).toBe('both_deduped');
    expect(arbiter.automaticAttemptCount()).toBe(1);
  });

  test('exhausts automatic recovery after the configured budget', () => {
    const arbiter = new CrashRecoveryArbiter(3, 30_000);

    for (let generation = 1; generation <= 3; generation += 1) {
      const decision = arbiter.observeFailure({
        failed_generation: generation,
        process_identity: `pid-${generation}`,
        source: 'process_exit',
        observed_at_ms: generation * 1_000,
      });
      expect(decision.disposition).toBe('start_recovery');
      expect(decision.automatic_attempt).toBe(generation);
      arbiter.settleActiveEpisode('recovery_failed', generation + 10);
    }

    const exhausted = arbiter.observeFailure({
      failed_generation: 4,
      process_identity: 'pid-4',
      source: 'process_exit',
      observed_at_ms: 4_000,
    });
    expect(exhausted).toMatchObject({
      disposition: 'crash_budget_exhausted',
      automatic_attempt: 3,
      automatic_budget: 3,
    });
    expect(arbiter.activeEpisodeDecision()).toBeNull();
    expect(arbiter.automaticAttemptCount()).toBe(3);
  });

  test('resets the episode budget only after the accepted stable-run grace', () => {
    const arbiter = new CrashRecoveryArbiter(3, 30_000);

    arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    arbiter.settleActiveEpisode('recovered', 2);
    arbiter.markRunning(2, 2_000);

    const shortRun = arbiter.observeFailure({
      failed_generation: 2,
      process_identity: 'pid-2',
      source: 'process_exit',
      observed_at_ms: 20_000,
    });
    expect(shortRun.automatic_attempt).toBe(2);
    arbiter.settleActiveEpisode('recovered', 3);

    arbiter.markRunning(3, 30_000);
    const stableRun = arbiter.observeFailure({
      failed_generation: 3,
      process_identity: 'pid-3',
      source: 'process_exit',
      observed_at_ms: 60_001,
    });
    expect(stableRun.automatic_attempt).toBe(1);
  });

  test('explicit recovery reset starts a fresh user-owned episode budget', () => {
    const arbiter = new CrashRecoveryArbiter(1, 30_000);
    arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    arbiter.settleActiveEpisode('recovery_failed', null);

    expect(
      arbiter.observeFailure({
        failed_generation: 2,
        process_identity: 'pid-2',
        source: 'process_exit',
        observed_at_ms: 2_000,
      }).disposition,
    ).toBe('crash_budget_exhausted');

    arbiter.resetForExplicitRecovery();
    expect(arbiter.automaticAttemptCount()).toBe(0);
    expect(
      arbiter.observeFailure({
        failed_generation: 3,
        process_identity: 'pid-3',
        source: 'process_exit',
        observed_at_ms: 3_000,
      }).disposition,
    ).toBe('start_recovery');
  });

  test.each([
    'user_restart',
    'extension_deactivation',
    'managed_update_restart',
    'activation_failed',
  ] as const)('%s never consumes automatic crash budget', (action) => {
    expect(CrashRecoveryArbiter.consumesCrashBudget(action)).toBe(false);
  });
});
