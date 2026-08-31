import {
  classifyNeutralLspProviderFailure,
  settleLspProviderCall,
  settleLspProviderCallWithDisposition,
} from '../lspProviderCall';

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

describe('typed LSP provider call settlement', () => {
  test('retains a returned empty result as a real returned value', async () => {
    const settlement = await settleLspProviderCallWithDisposition(async () => [], [] as string[]);

    expect(settlement).toEqual({ kind: 'returned', value: [], wireValue: [] });
  });

  test('retains an ordinary provider failure even when the wire fallback is empty', async () => {
    const error = new Error('provider failed');
    const settlement = await settleLspProviderCallWithDisposition<string[]>(
      async () => {
        throw error;
      },
      [],
    );

    expect(settlement).toEqual({ kind: 'failed', error, wireValue: [] });
    expect(settlement.kind).not.toBe('returned');
  });

  test('classifies request cancellation independently from provider failure', async () => {
    const error = { code: -32800, message: 'Request cancelled' };

    expect(classifyNeutralLspProviderFailure(error)).toBe('cancelled');
    await expect(
      settleLspProviderCallWithDisposition(
        async () => {
          throw error;
        },
        null,
      ),
    ).resolves.toEqual({ kind: 'cancelled', error, wireValue: null });
  });

  test('classifies client disposal independently from provider failure', async () => {
    const pending = new Deferred<string[]>();
    const error = new Error("Client got disposed and can't be restarted.");
    const settled = settleLspProviderCallWithDisposition(() => pending.promise, []);
    pending.reject(error);

    expect(classifyNeutralLspProviderFailure(error)).toBe('client_disposed');
    await expect(settled).resolves.toEqual({
      kind: 'client_disposed',
      error,
      wireValue: [],
    });
  });

  test('compatibility adapter reports failure while preserving the required wire fallback', async () => {
    const error = new Error('definition failed');
    const onFailure = jest.fn();

    const result = await settleLspProviderCall(
      async () => {
        throw error;
      },
      null,
      onFailure,
    );

    expect(result).toBeNull();
    expect(onFailure).toHaveBeenCalledWith(error);
  });

  test('compatibility adapter does not report cancellation as a product failure', async () => {
    const error = { code: -32800 };
    const onFailure = jest.fn();

    const result = await settleLspProviderCall(
      async () => {
        throw error;
      },
      [] as string[],
      onFailure,
    );

    expect(result).toEqual([]);
    expect(onFailure).not.toHaveBeenCalled();
  });

  test('typed callers retain disposition before choosing the wire projection', async () => {
    const error = new Error('hover failed');
    const settlement = await settleLspProviderCallWithDisposition(
      async () => {
        throw error;
      },
      null,
    );
    const observed: string[] = [];

    observed.push(settlement.kind);
    const wireValue = settlement.wireValue;

    expect(observed).toEqual(['failed']);
    expect(wireValue).toBeNull();
  });
});
