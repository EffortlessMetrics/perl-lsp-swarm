import { observeActiveDocumentReadiness } from './published/journeySupport';

describe('published journey readiness gate', () => {
  test('withholds provider action until the registered readiness promise resolves', async () => {
    let release: (() => void) | undefined;
    const waitForReady = () =>
      new Promise<void>((resolve) => {
        release = resolve;
      });
    const pending = observeActiveDocumentReadiness(waitForReady, 'file:///probe.pl', 100);

    await Promise.resolve();
    release?.();
    await expect(pending).resolves.toEqual({ status: 'ready' });
  });

  test('runs provider action only after readiness and records refusal as not proven', async () => {
    const waitForReady = jest.fn(async () => undefined);
    const ready = await observeActiveDocumentReadiness(waitForReady, 'file:///probe.pl', 100);
    expect(waitForReady).toHaveBeenCalledWith('file:///probe.pl', 100);
    expect(ready).toEqual({ status: 'ready' });

    const refused = await observeActiveDocumentReadiness(
      jest.fn(async () => {
        throw new Error('readiness refused');
      }),
      'file:///probe.pl',
      100,
    );
    expect(refused.status).toBe('not_proven');

    const missing = await observeActiveDocumentReadiness(undefined, 'file:///probe.pl', 100);
    expect(missing.status).toBe('not_proven');

    const timedOut = await observeActiveDocumentReadiness(
      () => new Promise<void>(() => undefined),
      'file:///probe.pl',
      1,
    );
    expect(timedOut.status).toBe('not_proven');
    expect(String(timedOut.reason)).toMatch(/timed out after 1ms/);
  });
});
