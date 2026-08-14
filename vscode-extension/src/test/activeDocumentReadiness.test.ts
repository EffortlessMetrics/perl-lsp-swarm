import { ActiveDocumentReadiness } from '../activeDocumentReadiness';

describe('ActiveDocumentReadiness', () => {
  test('resolves immediately for a URI already ready in the current generation', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    readiness.markReady('file:///workspace/probe.pl');

    await expect(readiness.waitFor('file:///workspace/probe.pl', 10)).resolves.toBeUndefined();
  });

  test('resolves a pending waiter when the active document becomes ready', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);

    readiness.markReady('file:///workspace/probe.pl');

    await expect(pending).resolves.toBeUndefined();
  });

  test('resolves a pending waiter when workspace indexing becomes ready', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);

    readiness.markIndexReady();

    await expect(pending).resolves.toBeUndefined();
  });

  test('preserves limited workspace readiness without treating it as fully ready', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);

    readiness.markIndexReady(undefined, 'ready_limited', 'resource limit');

    expect(readiness.currentIndexState()).toBe('ready_limited');
    expect(readiness.currentIndexReason()).toBe('resource limit');
    await expect(pending).rejects.toThrow('was not ready after 100ms');
  });

  test('exposes an honest readiness snapshot for installed-path receipts', () => {
    const readiness = new ActiveDocumentReadiness();
    const generation = readiness.beginGeneration();
    readiness.markIndexReady(generation, 'ready_limited', 'resource limit');

    expect(readiness.snapshot()).toEqual({
      generation,
      indexState: 'ready_limited',
      indexReason: 'resource limit',
      fullyReady: false,
    });
  });

  test('rejects pending waiters when a restart begins a new generation', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();
    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);

    readiness.beginGeneration();

    await expect(pending).rejects.toThrow('superseded by a restart');
  });

  test('ignores a late readiness event from an older generation', async () => {
    const readiness = new ActiveDocumentReadiness();
    const oldGeneration = readiness.beginGeneration();
    readiness.markReady('file:///workspace/probe.pl', oldGeneration);
    readiness.beginGeneration();

    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);
    readiness.markReady('file:///workspace/probe.pl', oldGeneration);

    await expect(pending).rejects.toThrow('was not ready after 100ms');
  });

  test('ignores an index-ready event from an older generation', async () => {
    const readiness = new ActiveDocumentReadiness();
    const oldGeneration = readiness.beginGeneration();
    readiness.beginGeneration();

    const pending = readiness.waitFor('file:///workspace/probe.pl', 100);
    readiness.markIndexReady(oldGeneration);

    await expect(pending).rejects.toThrow('was not ready after 100ms');
  });

  test('rejects when readiness does not arrive before the timeout', async () => {
    const readiness = new ActiveDocumentReadiness();
    readiness.beginGeneration();

    await expect(readiness.waitFor('file:///workspace/probe.pl', 1)).rejects.toThrow(
      'was not ready after 1ms',
    );
  });
});
