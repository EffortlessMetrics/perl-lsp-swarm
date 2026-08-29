import {
  StaleDocumentReplayError,
  replayOpenPerlDocuments,
  replayOpenPerlDocumentsWhenReady,
} from '../languageClientDocumentSync';

class Deferred<T> {
  readonly promise: Promise<T>;
  private resolvePromise!: (value: T) => void;

  constructor() {
    this.promise = new Promise<T>((resolve) => {
      this.resolvePromise = resolve;
    });
  }

  resolve(value: T): void {
    this.resolvePromise(value);
  }
}

function replayClient(initialState = 1) {
  const listeners = new Set<(event: { newState: number }) => void>();
  return {
    state: initialState,
    sendNotification: jest.fn().mockResolvedValue(undefined),
    onDidChangeState(listener: (event: { newState: number }) => void) {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
    transitionTo(state: number) {
      this.state = state;
      for (const listener of listeners) listener({ newState: state });
    },
    listenerCount() {
      return listeners.size;
    },
  };
}

const PERL_DOCUMENT = {
  uri: 'file:///workspace/probe.pl',
  languageId: 'perl',
  version: 7,
  text: 'my $value = 7;\n',
};

describe('replayOpenPerlDocuments', () => {
  test('replays current Perl buffers and ignores other languages', async () => {
    const sendNotification = jest.fn().mockResolvedValue(undefined);

    await replayOpenPerlDocuments({ sendNotification }, [
      {
        uri: 'file:///workspace/probe.pl',
        languageId: 'perl',
        version: 7,
        text: 'my $value = 7;\n',
      },
      {
        uri: 'file:///workspace/readme.md',
        languageId: 'markdown',
        version: 2,
        text: '# ignored\n',
      },
    ]);

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendNotification).toHaveBeenCalledWith('textDocument/didOpen', {
      textDocument: {
        uri: 'file:///workspace/probe.pl',
        languageId: 'perl',
        version: 7,
        text: 'my $value = 7;\n',
      },
    });
  });

  test('preserves notification failures for restart callers', async () => {
    const failure = new Error('client stopped');
    const sendNotification = jest.fn().mockRejectedValue(failure);

    await expect(
      replayOpenPerlDocuments({ sendNotification }, [
        { uri: 'file:///workspace/probe.pl', languageId: 'perl', version: 1, text: '1;\n' },
      ]),
    ).rejects.toBe(failure);
  });

  test('waits for the observable running transition before replaying', async () => {
    const client = replayClient();

    const replay = replayOpenPerlDocumentsWhenReady(client, [PERL_DOCUMENT], 2, () => true, 1000);
    expect(client.sendNotification).not.toHaveBeenCalled();
    expect(client.listenerCount()).toBe(1);

    client.transitionTo(2);
    await expect(replay).resolves.toBeUndefined();
    expect(client.sendNotification).toHaveBeenCalledTimes(1);
    expect(client.listenerCount()).toBe(0);
  });

  test('rejects a generation that becomes stale while replay is awaiting the client', async () => {
    const client = replayClient(2);
    const notification = new Deferred<void>();
    client.sendNotification.mockReturnValue(notification.promise);
    let current = true;

    const replay = replayOpenPerlDocumentsWhenReady(
      client,
      [PERL_DOCUMENT],
      2,
      () => current,
      1000,
    );
    await Promise.resolve();
    current = false;
    notification.resolve(undefined);

    await expect(replay).rejects.toBeInstanceOf(StaleDocumentReplayError);
  });

  test('propagates replay failure so startup cannot finalize as recovered', async () => {
    const client = replayClient(2);
    const failure = new Error('client disposed during didOpen');
    client.sendNotification.mockRejectedValue(failure);

    await expect(
      replayOpenPerlDocumentsWhenReady(client, [PERL_DOCUMENT], 2, () => true, 1000),
    ).rejects.toBe(failure);
  });
});
