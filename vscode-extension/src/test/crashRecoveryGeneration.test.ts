import { ActiveDocumentReadiness } from '../activeDocumentReadiness';
import { CrashRecoveryArbiter } from '../crashRecoveryArbiter';

const URI = 'file:///workspace/lib/Foo.pm';

describe('crash recovery generation proof', () => {
  test('invalidates dead-document readiness before the replacement generation becomes usable', () => {
    const readiness = new ActiveDocumentReadiness();
    const arbiter = new CrashRecoveryArbiter(3, 30_000);
    const failedGeneration = readiness.beginGeneration();
    readiness.markReady(URI, failedGeneration);
    readiness.markIndexReady(failedGeneration, 'ready');
    expect(readiness.isReady(URI)).toBe(true);

    const decision = arbiter.observeFailure({
      failed_generation: failedGeneration,
      process_identity: 'server-1',
      source: 'process_exit',
      observed_at_ms: 1_000,
    });
    expect(decision.disposition).toBe('start_recovery');

    const replacementGeneration = readiness.beginGeneration();
    expect(replacementGeneration).toBe(failedGeneration + 1);
    expect(readiness.snapshot()).toMatchObject({
      generation: replacementGeneration,
      indexState: 'building',
      fullyReady: false,
    });
    expect(readiness.isReady(URI)).toBe(false);

    readiness.markReady(URI, replacementGeneration);
    expect(readiness.isReady(URI)).toBe(true);
  });

  test('rejects late ready and index-ready events from the dead generation', () => {
    const readiness = new ActiveDocumentReadiness();
    const failedGeneration = readiness.beginGeneration();
    const replacementGeneration = readiness.beginGeneration();

    readiness.markReady(URI, failedGeneration);
    readiness.markIndexReady(failedGeneration, 'ready');

    expect(readiness.snapshot()).toEqual({
      generation: replacementGeneration,
      indexState: 'building',
      fullyReady: false,
    });
    expect(readiness.isReady(URI)).toBe(false);

    readiness.markReady(URI, replacementGeneration);
    readiness.markIndexReady(replacementGeneration, 'ready');
    expect(readiness.snapshot()).toEqual({
      generation: replacementGeneration,
      indexState: 'ready',
      fullyReady: true,
    });
  });

  test('deduplicates watchdog plus process-exit while stale readiness stays rejected', () => {
    const readiness = new ActiveDocumentReadiness();
    const arbiter = new CrashRecoveryArbiter(3, 30_000);
    const failedGeneration = readiness.beginGeneration();
    readiness.markReady(URI, failedGeneration);

    const watchdog = arbiter.observeFailure({
      failed_generation: failedGeneration,
      process_identity: 'server-1',
      source: 'watchdog',
      observed_at_ms: 1_000,
    });
    const processExit = arbiter.observeFailure({
      failed_generation: failedGeneration,
      process_identity: 'server-1',
      source: 'process_exit',
      observed_at_ms: 1_001,
    });

    expect(watchdog).toMatchObject({
      disposition: 'start_recovery',
      automatic_attempt: 1,
    });
    expect(processExit).toMatchObject({
      disposition: 'deduped_existing_episode',
      observation_source: 'both_deduped',
      automatic_attempt: 1,
    });
    expect(arbiter.automaticAttemptCount()).toBe(1);

    const replacementGeneration = readiness.beginGeneration();
    readiness.markReady(URI, failedGeneration);
    expect(readiness.isReady(URI)).toBe(false);
    readiness.markReady(URI, replacementGeneration);
    expect(readiness.isReady(URI)).toBe(true);
  });

  test('restart generation supersedes pending readiness waiters deterministically', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    const pending = readiness.waitFor(URI, 30_000);

    readiness.beginGeneration();

    await expect(pending).rejects.toThrow(
      'Active-document readiness was superseded by a restart.',
    );
  });
});
