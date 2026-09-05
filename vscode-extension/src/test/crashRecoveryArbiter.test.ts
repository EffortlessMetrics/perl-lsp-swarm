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
    const first = arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    expect(arbiter.settleEpisode(first, 'recovered', 2)).toBe(true);

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
      expect(arbiter.settleEpisode(decision, 'recovery_failed', generation + 10)).toBe(true);
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

    const firstEpisode = arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    expect(arbiter.settleEpisode(firstEpisode, 'recovered', 2)).toBe(true);
    arbiter.markRunning(2, 2_000);

    const shortRun = arbiter.observeFailure({
      failed_generation: 2,
      process_identity: 'pid-2',
      source: 'process_exit',
      observed_at_ms: 20_000,
    });
    expect(shortRun.automatic_attempt).toBe(2);
    expect(arbiter.settleEpisode(shortRun, 'recovered', 3)).toBe(true);

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
    const firstEpisode = arbiter.observeFailure({
      failed_generation: 1,
      process_identity: 'pid-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    expect(arbiter.settleEpisode(firstEpisode, 'recovery_failed', null)).toBe(true);

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

  // ------------------------------------------------------------------
  // #7845 review falsifier: a replacement generation (G+1) fails while
  // generation G's recovery continuation is still awaiting its restart
  // promise. The continuation must settle exactly its own episode handle;
  // the G+1 failure must defer behind the active episode and recover
  // serially, never overwrite the active slot or settle another episode.
  // ------------------------------------------------------------------
  test('a different-generation failure while an episode is active defers and settles exactly the authorizing handle', () => {
    const arbiter = new CrashRecoveryArbiter(3, 30_000);

    // Generation 5 crashes; its recovery episode is active while its
    // continuation awaits the restart promise.
    const generationFive = arbiter.observeFailure({
      failed_generation: 5,
      process_identity: 'pid-5',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    expect(generationFive.disposition).toBe('start_recovery');

    // The pending restart already spawned generation 6, and generation 6
    // fails BEFORE generation 5's restart promise resolves. This must not
    // start a second concurrent episode nor steal the active slot.
    const generationSix = arbiter.observeFailure({
      failed_generation: 6,
      process_identity: 'pid-6',
      source: 'watchdog',
      observed_at_ms: 1_500,
    });
    expect(generationSix.disposition).toBe('deferred_active_episode');
    expect(generationSix.episode_id).toBe(generationFive.episode_id);
    expect(arbiter.activeEpisodeDecision()?.episode_id).toBe(generationFive.episode_id);
    expect(arbiter.automaticAttemptCount()).toBe(1);

    // A duplicate deferred observation for generation 6 must not queue a
    // second entry.
    expect(
      arbiter.observeFailure({
        failed_generation: 6,
        process_identity: 'pid-6',
        source: 'process_exit',
        observed_at_ms: 1_600,
      }).disposition,
    ).toBe('deferred_active_episode');

    // Generation 5's continuation resumes and settles exactly its own
    // handle: the active episode is released, the deferred failure is not
    // touched.
    expect(arbiter.settleEpisode(generationFive, 'recovered', 6)).toBe(true);
    expect(arbiter.activeEpisodeDecision()).toBeNull();

    // The deferred observation drains in arrival order and re-arbitrates
    // serially into its own episode (budget evaluation included).
    const drained = arbiter.takePendingFailureObservation();
    expect(drained).toEqual({
      failed_generation: 6,
      process_identity: 'pid-6',
      source: 'process_exit',
      observed_at_ms: 1_600,
    });
    expect(arbiter.takePendingFailureObservation()).toBeNull();

    const retried = arbiter.observeFailure({
      failed_generation: 6,
      process_identity: 'pid-6',
      source: 'process_exit',
      observed_at_ms: 2_000,
    });
    expect(retried).toMatchObject({
      disposition: 'start_recovery',
      automatic_attempt: 2,
    });
    expect(arbiter.activeEpisodeDecision()?.episode_id).toBe(retried.episode_id);

    // A late duplicate settle for the already-settled generation-5 handle
    // must not disturb the newer active episode.
    expect(arbiter.settleEpisode(generationFive, 'recovery_failed', null)).toBe(false);
    expect(arbiter.activeEpisodeDecision()?.episode_id).toBe(retried.episode_id);
    expect(arbiter.automaticAttemptCount()).toBe(2);
  });

  test('an explicit recovery reset discards deferred observations with the budget', () => {
    const arbiter = new CrashRecoveryArbiter(3, 30_000);
    expect(
      arbiter.observeFailure({
        failed_generation: 1,
        process_identity: 'pid-1',
        source: 'process_exit',
        observed_at_ms: 1_000,
      }).disposition,
    ).toBe('start_recovery');
    expect(
      arbiter.observeFailure({
        failed_generation: 2,
        process_identity: 'pid-2',
        source: 'watchdog',
        observed_at_ms: 1_500,
      }).disposition,
    ).toBe('deferred_active_episode');

    arbiter.resetForExplicitRecovery();
    expect(arbiter.takePendingFailureObservation()).toBeNull();
    expect(
      arbiter.observeFailure({
        failed_generation: 3,
        process_identity: 'pid-3',
        source: 'process_exit',
        observed_at_ms: 2_000,
      }).disposition,
    ).toBe('start_recovery');
  });
});
