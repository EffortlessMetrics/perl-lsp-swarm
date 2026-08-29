import { settleLspProviderCall } from '../lspProviderCall';

class Deferred<T> {
  readonly promise: Promise<T>;
  private rejectPromise!: (reason: unknown) => void;

  constructor() {
    this.promise = new Promise<T>((_resolve, reject) => {
      this.rejectPromise = reject;
    });
  }

  reject(reason: unknown): void {
    this.rejectPromise(reason);
  }
}

describe('settleLspProviderCall', () => {
  test('treats disposal during an awaited provider call as a neutral fallback', async () => {
    const pending = new Deferred<string[]>();
    const onFailure = jest.fn();

    const settled = settleLspProviderCall(() => pending.promise, [], onFailure);
    pending.reject(new Error("Client got disposed and can't be restarted."));

    await expect(settled).resolves.toEqual([]);
    expect(onFailure).not.toHaveBeenCalled();
  });

  test('treats protocol cancellation as a neutral fallback', async () => {
    const onFailure = jest.fn();

    await expect(
      settleLspProviderCall(() => Promise.reject({ code: -32800 }), null, onFailure),
    ).resolves.toBeNull();
    expect(onFailure).not.toHaveBeenCalled();
  });

  test('reports a real provider failure before returning the fallback', async () => {
    const failure = new Error('provider failed');
    const onFailure = jest.fn();

    await expect(
      settleLspProviderCall(() => Promise.reject(failure), null, onFailure),
    ).resolves.toBeNull();
    expect(onFailure).toHaveBeenCalledWith(failure);
  });
});
