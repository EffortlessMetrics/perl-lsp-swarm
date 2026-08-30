import * as vscode from 'vscode';

// Mock vscode-languageclient/node and vscode-jsonrpc before importing the controller
jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  Trace: {
    Off: 'off',
    Messages: 'messages',
    Verbose: 'verbose',
  },
  TransportKind: {
    stdio: 0,
  },
}));

jest.mock('vscode-jsonrpc', () => ({
  ProgressType: class {},
}));

import { StreamingCompletionController } from '../streamingCompletion';
import type { LanguageClient } from 'vscode-languageclient/node';

/** Create a mock LanguageClient with the methods needed by StreamingCompletionController. */
function createMockClient(): LanguageClient {
  return {
    onProgress: jest.fn(() => ({ dispose: jest.fn() })),
    sendRequest: jest.fn(async () => ({})),
    sendNotification: jest.fn(),
    // The adapter gates readiness on the server actually advertising the
    // custom stream method, so the mock must advertise it too.
    initializeResult: { capabilities: { experimental: { perlInlineCompletionStream: true } } },
  } as unknown as LanguageClient;
}

describe('StreamingCompletionController', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;

  beforeEach(() => {
    jest.clearAllMocks();
    // Extend mock with needed symbols for inline completions
    (vscode as Record<string, unknown>).Position = class {
      constructor(
        public line: number,
        public character: number,
      ) {}
    };
    (vscode as Record<string, unknown>).InlineCompletionItem = class {
      constructor(
        public insertText: string,
        public range?: unknown,
      ) {}
    };
    (vscode.languages as Record<string, unknown>).registerInlineCompletionItemProvider = jest.fn(
      () => ({
        dispose: jest.fn(),
      }),
    );
    (vscode.window as Record<string, unknown>).onDidChangeTextEditorSelection = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    (vscode.workspace as Record<string, unknown>).onDidChangeTextDocument = jest.fn(() => ({
      dispose: jest.fn(),
    }));

    mockClient = createMockClient();
    controller = new StreamingCompletionController(mockClient);
  });

  afterEach(() => {
    controller.dispose();
  });

  test('does not register a second inline completion provider (#8282)', () => {
    // The language client already registers a provider for the Perl selector
    // from the server's inlineCompletionProvider capability. A registration
    // here would make one editor trigger dispatch two server requests.
    expect(vscode.languages.registerInlineCompletionItemProvider).not.toHaveBeenCalled();
  });

  test('exposes itself as the stream adapter rather than a provider', () => {
    expect(typeof controller.provideInlineCompletionItems).toBe('function');
    expect(typeof controller.isStreamReady).toBe('function');
  });

  test('registers cursor and document change listeners', () => {
    expect(vscode.window.onDidChangeTextEditorSelection).toHaveBeenCalledTimes(1);
    expect(vscode.workspace.onDidChangeTextDocument).toHaveBeenCalledTimes(1);
  });

  test('dispose cleans up all disposables', () => {
    const cursorDispose = jest.fn();
    const docChangeDispose = jest.fn();

    (vscode.window.onDidChangeTextEditorSelection as jest.Mock).mockReturnValue({
      dispose: cursorDispose,
    });
    (vscode.workspace.onDidChangeTextDocument as jest.Mock).mockReturnValue({
      dispose: docChangeDispose,
    });

    const ctrl = new StreamingCompletionController(createMockClient());
    ctrl.dispose();

    expect(cursorDispose).toHaveBeenCalled();
    expect(docChangeDispose).toHaveBeenCalled();
  });

  test('a disposed adapter is never stream-ready', () => {
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
      get: jest.fn(() => true),
    }));
    const ctrl = new StreamingCompletionController(createMockClient());
    expect(ctrl.isStreamReady()).toBe(true);

    ctrl.dispose();

    // A controller disposed by restart or configuration reconstruction belongs
    // to a superseded client generation and must not take a route.
    expect(ctrl.isStreamReady()).toBe(false);
  });

  test('notifyAccepted sends notification to client', () => {
    controller.notifyAccepted('session-1', 0);
    expect(mockClient.sendNotification as jest.Mock).toHaveBeenCalledWith(
      'perl/didAcceptInlineCompletion',
      { sessionId: 'session-1', candidate: 0 },
    );
  });

  test('notifyShown sends notification to client', () => {
    controller.notifyShown('session-2');
    expect(mockClient.sendNotification as jest.Mock).toHaveBeenCalledWith(
      'perl/didShowInlineCompletion',
      { sessionId: 'session-2' },
    );
  });
});

describe('CachedCandidate shape', () => {
  test('CachedCandidate has expected fields', () => {
    const candidate = {
      uri: 'file:///test.pl',
      version: 1,
      line: 5,
      character: 10,
      text: '->find_user($id)',
      sessionId: 'sess-abc',
      sequence: 3,
      isFinal: false,
    };
    expect(candidate.text).toBe('->find_user($id)');
    expect(candidate.isFinal).toBe(false);
  });

  test('Progress values with higher sequence supersede lower', () => {
    const seq1 = 1;
    const seq2 = 3;
    expect(seq2).toBeGreaterThan(seq1);
  });
});

/**
 * Provider and progress-handler integration tests.
 *
 * These tests drive the registered InlineCompletionItemProvider directly and
 * exercise the progress callback to verify that cache-hit decisions require an
 * exact match on URI + document version + cursor line + cursor character.
 */
describe('StreamingCompletionController — request identity and cache correctness', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;

  /**
   * Returns the stream adapter under test.
   *
   * The controller no longer registers its own provider (#8282) — it is called
   * directly by `InlineCompletionOwner`, so the tests call it the same way.
   */
  function getStreamAdapter(): {
    provideInlineCompletionItems: (
      document: vscode.TextDocument,
      position: vscode.Position,
      context: vscode.InlineCompletionContext,
      token: vscode.CancellationToken,
    ) => vscode.InlineCompletionItem[] | undefined;
  } {
    return controller;
  }

  /** Returns the most-recently-registered progress handler from `onProgress`. */
  function getLastProgressHandler(): (value: unknown) => void {
    const calls = (mockClient.onProgress as jest.Mock).mock.calls;
    const lastCall = calls[calls.length - 1];
    return lastCall[2] as (value: unknown) => void;
  }

  /** Builds a minimal mock TextDocument for the given URI and version. */
  function makeMockDoc(uri: string, version: number): vscode.TextDocument {
    return {
      uri: { toString: () => uri },
      version,
    } as unknown as vscode.TextDocument;
  }

  /** Builds a minimal mock Position. */
  function makeMockPos(line: number, character: number): vscode.Position {
    return { line, character } as unknown as vscode.Position;
  }

  /** Builds a well-formed stream progress value. */
  function makeProgress(
    sessionId: string,
    sequence: number,
    text: string,
    options: {
      isFinal?: boolean;
      range?: unknown;
    } = {},
  ): unknown {
    return {
      kind: 'perlInlineCompletionStream',
      sessionId,
      sequence,
      isFinal: options.isFinal ?? false,
      items: [{ insertText: text, range: options.range }],
    };
  }

  beforeEach(() => {
    jest.clearAllMocks();

    (vscode as Record<string, unknown>).Position = class {
      constructor(
        public line: number,
        public character: number,
      ) {}
    };
    (vscode as Record<string, unknown>).InlineCompletionItem = class {
      constructor(
        public insertText: string,
        public range?: unknown,
      ) {}
    };
    (vscode.languages as Record<string, unknown>).registerInlineCompletionItemProvider = jest.fn(
      () => ({ dispose: jest.fn() }),
    );
    (vscode.window as Record<string, unknown>).onDidChangeTextEditorSelection = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    (vscode.workspace as Record<string, unknown>).onDidChangeTextDocument = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    // Enable AI streaming completion for all provider tests
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(
      (_section?: string) => ({
        get: jest.fn((key: string, defaultValue?: unknown) => {
          if (key === 'aiCompletion.enabled') return true;
          if (key === 'aiCompletion.streaming.enabled') return true;
          return defaultValue;
        }),
      }),
    );

    mockClient = createMockClient();
    controller = new StreamingCompletionController(mockClient);
  });

  afterEach(() => {
    controller.dispose();
  });

  test('returns a candidate for the exact same URI/version/line/character key', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    // First call — no cache, triggers stream
    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();

    // Deliver a progress update
    const handler = getLastProgressHandler();
    handler(makeProgress('sess-1', 1, 'my $result = '));

    // Second call with identical key — should return cached candidate
    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeDefined();
    expect(items).toHaveLength(1);
    expect((items![0] as { insertText: string }).insertText).toBe('my $result = ');
  });

  test('same line/character in a different URI does not return cached ghost text', () => {
    const provider = getStreamAdapter();
    const docA = makeMockDoc('file:///a.pl', 1);
    const docB = makeMockDoc('file:///b.pl', 1);
    const pos = makeMockPos(5, 10);

    // Start stream for a.pl and populate cache
    provider.provideInlineCompletionItems(
      docA,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-1', 1, 'ghost for a.pl'));

    // Provider called for b.pl at same line/character — must NOT return cached ghost
    const items = provider.provideInlineCompletionItems(
      docB,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('same URI and position at a newer document version does not return cached ghost text', () => {
    const provider = getStreamAdapter();
    const docV1 = makeMockDoc('file:///a.pl', 1);
    const docV2 = makeMockDoc('file:///a.pl', 2);
    const pos = makeMockPos(5, 10);

    // Start stream for version 1 and populate cache
    provider.provideInlineCompletionItems(
      docV1,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-1', 1, 'ghost for v1'));

    // Provider called at version 2 — must NOT return v1 cached ghost
    const items = provider.provideInlineCompletionItems(
      docV2,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('same URI and version at a different cursor does not return cached ghost text', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const cachedPos = makeMockPos(5, 10);
    const differentPos = makeMockPos(5, 11);

    provider.provideInlineCompletionItems(
      doc,
      cachedPos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-1', 1, 'ghost for position 10'));

    const items = provider.provideInlineCompletionItems(
      doc,
      differentPos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('late progress callback from a cancelled stream cannot repopulate the cache', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    // Start stream and capture the handler before cancellation
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const staleHandler = getLastProgressHandler();

    // Simulate cursor movement, which calls cancelActiveStream
    const cursorCb = (vscode.window.onDidChangeTextEditorSelection as jest.Mock).mock
      .calls[0][0] as () => void;
    cursorCb();

    // Fire the stale handler — must be ignored because the stream was cancelled
    staleHandler(makeProgress('sess-1', 1, 'stale ghost'));

    // No cached candidate remains
    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('progress from a superseded stream (new request started) cannot repopulate the cache', () => {
    const provider = getStreamAdapter();
    const docA = makeMockDoc('file:///a.pl', 1);
    const posA = makeMockPos(5, 10);
    // A different cursor is a genuinely different request identity, so it
    // supersedes rather than being deduplicated as a display re-query.
    const posB = makeMockPos(6, 0);

    // Start first stream, capture its handler
    provider.provideInlineCompletionItems(
      docA,
      posA,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const staleHandler = getLastProgressHandler();

    // Start a second stream at a new cursor, superseding the first
    provider.provideInlineCompletionItems(
      docA,
      posB,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    // Now the stale handler fires for the first (cancelled) stream
    staleHandler(makeProgress('sess-old', 1, 'ghost from old stream'));

    // Cache should be empty even though the stale stream has the exact key
    const items = provider.provideInlineCompletionItems(
      docA,
      posA,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('a display re-query for an in-flight identity starts no second backend generation', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);

    // handleProgress retriggers the suggest widget on every chunk, so the
    // provider is re-entered for the identity already in flight. Restarting
    // here would cancel the stream that produced the chunk and dispatch a
    // fresh backend generation on every update.
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    const counters = controller.snapshotStreamCounters();
    expect(counters.streamGenerationsStarted).toBe(1);
    expect(counters.duplicateDisplayRequeries).toBe(2);
    expect((mockClient.sendRequest as jest.Mock).mock.calls).toHaveLength(1);
  });

  test('a settled stream releases the identity so re-invocation can retry', async () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);

    // Let the request promise resolve: the generation is no longer in flight.
    await Promise.resolve();
    await Promise.resolve();

    // The re-query guard covers in-flight generations only. A stream that
    // settled without a candidate must not suppress an explicit re-invocation
    // at the same cursor forever.
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });

  /** Builds a terminal progress value carrying no candidate. */
  function makeEmptyFinal(sessionId: string, sequence: number): unknown {
    return {
      kind: 'perlInlineCompletionStream',
      sessionId,
      sequence,
      isFinal: true,
      items: [],
    };
  }

  /** Drives one stream to the point where ghost text is cached and servable. */
  function showGhostText(
    provider: ReturnType<typeof getStreamAdapter>,
    doc: vscode.TextDocument,
    pos: vscode.Position,
  ): (value: unknown) => void {
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const handler = getLastProgressHandler();
    handler(makeProgress('sess-terminal', 0, 'my $partial = 1'));
    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toHaveLength(1);
    return handler;
  }

  test('an empty terminal value revokes ghost text it previously showed', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    // The server decided the stream produces nothing after all. Before this
    // fix the handler returned early on `items: []` and the stale suggestion
    // stayed servable.
    handler(makeEmptyFinal('sess-terminal', 1));

    // The first re-query is the one the revocation itself triggers.
    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
  });

  test('revocation dismisses the suggestion instead of re-querying', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);
    (vscode.commands.executeCommand as jest.Mock).mockClear();

    handler(makeEmptyFinal('sess-terminal', 1));

    const commands = (vscode.commands.executeCommand as jest.Mock).mock.calls.map(
      (call) => call[0] as string,
    );
    // `trigger` would re-enter this provider at the cursor the server just
    // answered "nothing" for, dispatching another generation that revokes again
    // and loops. `hide` clears the text without a re-query.
    expect(commands).toContain('editor.action.inlineSuggest.hide');
    expect(commands).not.toContain('editor.action.inlineSuggest.trigger');
  });

  test('revoking ghost text starts no further backend generation', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);

    handler(makeEmptyFinal('sess-terminal', 1));

    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);
  });

  test('an explicit re-invocation after a revocation still retries', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    handler(makeEmptyFinal('sess-terminal', 1));

    // The user asks again at the same cursor. An AI backend is not
    // deterministic, so a deliberate retry is a real request, not a duplicate:
    // it must reach the backend rather than being swallowed.
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });

  test('an empty intermediate frame is skipped, not treated as a revocation', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    // The server skips a cumulative chunk it judged unsafe. That is not a
    // decision about the stream, and must not revoke what is already showing.
    handler({
      kind: 'perlInlineCompletionStream',
      sessionId: 'sess-terminal',
      sequence: 1,
      isFinal: false,
      items: [],
    });

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toHaveLength(1);
    expect(items?.[0]?.insertText).toBe('my $partial = 1');
  });

  test('an out-of-order terminal frame settles without installing its candidate', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    // Sequence 0 is already cached by showGhostText; a terminal frame at a
    // lower-or-equal sequence is stale and must not overwrite it, but it still
    // terminates the stream.
    handler({
      kind: 'perlInlineCompletionStream',
      sessionId: 'sess-terminal',
      sequence: 0,
      isFinal: true,
      items: [{ insertText: 'stale terminal text' }],
    });

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items?.[0]?.insertText).toBe('my $partial = 1');

    // Settled: a later frame for this generation is ignored rather than
    // reopening the stream.
    handler(makeProgress('sess-terminal', 5, 'reopened text'));
    const after = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(after?.[0]?.insertText).toBe('my $partial = 1');
  });

  test('a late rejection from a superseded stream cannot revoke its successor', async () => {
    let rejectFirst: (reason: unknown) => void = () => {};
    const firstRequest = new Promise((_resolve, reject) => {
      rejectFirst = reject;
    });
    (mockClient.sendRequest as jest.Mock)
      .mockReturnValueOnce(firstRequest)
      .mockReturnValue(new Promise(() => {}));

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    // Generation 1 at the cursor, then superseded, then a third generation
    // lands back on the *same* field values — a distinct object with an
    // identical key, which is what makes the reference guard load-bearing.
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    provider.provideInlineCompletionItems(
      doc,
      makeMockPos(6, 0),
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-3', 0, 'live text from gen3'));

    rejectFirst(new Error('backend exploded'));
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items?.[0]?.insertText).toBe('live text from gen3');
  });

  test('a non-empty terminal value keeps its candidate servable', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    handler({
      kind: 'perlInlineCompletionStream',
      sessionId: 'sess-terminal',
      sequence: 1,
      isFinal: true,
      items: [{ insertText: 'my $complete = 1;' }],
    });

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toHaveLength(1);
    expect(items?.[0]?.insertText).toBe('my $complete = 1;');
  });

  test('the request resolving after a successful final does not revoke it', async () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    handler({
      kind: 'perlInlineCompletionStream',
      sessionId: 'sess-terminal',
      sequence: 1,
      isFinal: true,
      items: [{ insertText: 'my $complete = 1;' }],
    });

    // The custom request answers `null` once the stream has finished. That
    // acknowledgement must not be mistaken for "resolved without a terminal
    // value" and revoke the candidate the stream just installed.
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toHaveLength(1);
    expect(items?.[0]?.insertText).toBe('my $complete = 1;');
  });

  test('a frame arriving after the terminal value cannot reopen the stream', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    const handler = showGhostText(provider, doc, pos);

    handler(makeEmptyFinal('sess-terminal', 1));
    // A late, higher-sequence frame from the same session must not resurrect
    // a stream the server already terminated.
    handler(makeProgress('sess-terminal', 2, 'my $late = 1'));

    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
  });

  test('a request resolving without a terminal value revokes its partial text', async () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    showGhostText(provider, doc, pos);

    // `sendRequest` resolves with `{}` — no terminal progress value ever
    // arrived, so the partial text on screen is unconfirmed.
    await Promise.resolve();
    await Promise.resolve();

    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
    // The stale text is dismissed, not merely dropped from the cache.
    expect(vscode.commands.executeCommand as jest.Mock).toHaveBeenCalledWith(
      'editor.action.inlineSuggest.hide',
    );
  });

  test('a backend failure after partial text revokes it', async () => {
    (mockClient.sendRequest as jest.Mock).mockReturnValue(
      Promise.reject(new Error('backend exploded')),
    );

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    showGhostText(provider, doc, pos);

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // A failed stream's partial cumulative text must not stay on screen
    // indistinguishable from a completed suggestion.
    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
    // The partial text is dismissed on screen, not merely dropped from the
    // cache; and dismissing costs no further backend generation.
    expect(vscode.commands.executeCommand as jest.Mock).toHaveBeenCalledWith(
      'editor.action.inlineSuggest.hide',
    );
  });

  test('a backend failure whose message merely mentions cancelling still revokes', async () => {
    // The failure is real, but its prose contains the word this arm used to
    // match on as a bare substring. Classifying it as a cancellation would
    // settle quietly and strand the partial text on screen.
    (mockClient.sendRequest as jest.Mock).mockReturnValue(
      Promise.reject(new Error('upstream provider refused to honour the cancellation handshake')),
    );

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    showGhostText(provider, doc, pos);

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
    expect(vscode.commands.executeCommand as jest.Mock).toHaveBeenCalledWith(
      'editor.action.inlineSuggest.hide',
    );
  });

  test('a server-initiated cancellation revokes, because no local cleanup ran', async () => {
    // LSP `ServerCancelled`. The server dropped the request on its own, so
    // `cancelActiveStream` never ran and the cached partial is still servable.
    // Settling quietly here would strand it on screen.
    (mockClient.sendRequest as jest.Mock).mockReturnValue(
      Promise.reject(Object.assign(new Error('request superseded'), { code: -32802 })),
    );

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    showGhostText(provider, doc, pos);

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(
      provider.provideInlineCompletionItems(
        doc,
        pos,
        {} as vscode.InlineCompletionContext,
        {} as vscode.CancellationToken,
      ),
    ).toBeUndefined();
    expect(vscode.commands.executeCommand as jest.Mock).toHaveBeenCalledWith(
      'editor.action.inlineSuggest.hide',
    );
  });

  test('a locally cancelled request never reaches the reject arm at all', async () => {
    // The load-bearing half of the argument for revoking unconditionally above:
    // cursor movement runs `cancelActiveStream`, which discards the candidate
    // and nulls the active identity, so the late rejection returns at the
    // identity guard and cannot dismiss whatever is on screen by then.
    let rejectRequest: (reason: unknown) => void = () => {};
    (mockClient.sendRequest as jest.Mock).mockReturnValue(
      new Promise((_resolve, reject) => {
        rejectRequest = reject;
      }),
    );

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);
    showGhostText(provider, doc, pos);

    // Cursor movement cancels the stream and discards its candidate.
    const cursorCb = (vscode.window.onDidChangeTextEditorSelection as jest.Mock).mock
      .calls[0][0] as () => void;
    cursorCb();
    (vscode.commands.executeCommand as jest.Mock).mockClear();

    rejectRequest(Object.assign(new Error('request cancelled'), { code: -32800 }));
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(vscode.commands.executeCommand as jest.Mock).not.toHaveBeenCalledWith(
      'editor.action.inlineSuggest.hide',
    );
  });

  test('a rejected request stays retryable whatever the rejection is called', async () => {
    (mockClient.sendRequest as jest.Mock).mockReturnValue(Promise.reject(new Error('Canceled')));

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // Revoking dismisses rather than re-queries, so a rejected generation --
    // however it is spelled, and VS Code spells it "Canceled" -- never
    // swallows the next explicit invocation.
    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });

  test('a late completion from a superseded stream cannot settle its successor', async () => {
    let resolveFirst: (value: unknown) => void = () => {};
    const firstRequest = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    // The second generation never settles, so only a wrongly-unconditional
    // settle could clear its in-flight marker.
    (mockClient.sendRequest as jest.Mock)
      .mockReturnValueOnce(firstRequest)
      .mockReturnValue(new Promise(() => {}));

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const posA = makeMockPos(5, 10);
    const posB = makeMockPos(6, 0);

    provider.provideInlineCompletionItems(
      doc,
      posA,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    provider.provideInlineCompletionItems(
      doc,
      posB,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);

    // The superseded first request completes late.
    resolveFirst({});
    await Promise.resolve();
    await Promise.resolve();

    // The second generation is still in flight, so its identity must survive
    // and a re-query at that cursor must still be deduplicated.
    provider.provideInlineCompletionItems(
      doc,
      posB,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });

  test('a new cursor position does start a distinct backend generation', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);

    provider.provideInlineCompletionItems(
      doc,
      makeMockPos(5, 10),
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    provider.provideInlineCompletionItems(
      doc,
      makeMockPos(7, 2),
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    // Opposite-direction control for the dedupe guard above: it must suppress
    // re-queries, not genuine new requests.
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });

  test('higher sequence values supersede lower; out-of-order updates are ignored', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(3, 5);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const handler = getLastProgressHandler();

    // Deliver sequence 1, then 5, then 3 (out-of-order)
    handler(makeProgress('sess-1', 1, 'seq1'));
    handler(makeProgress('sess-1', 5, 'seq5'));
    handler(makeProgress('sess-1', 3, 'seq3-late')); // must be ignored

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeDefined();
    expect((items![0] as { insertText: string }).insertText).toBe('seq5');
  });

  test('server replacement range beginning before the cursor is preserved', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10); // cursor at (5, 10)

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const handler = getLastProgressHandler();

    // Server supplies a range starting at column 0, before the cursor
    handler(
      makeProgress('sess-1', 1, 'replacement text', {
        range: { start: { line: 5, character: 0 }, end: { line: 5, character: 10 } },
      }),
    );

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeDefined();
    expect(items).toHaveLength(1);

    // Range should reflect the server-supplied extent, not a zero-length at cursor
    const range = (
      items![0] as {
        insertText: string;
        range: {
          start: { line: number; character: number };
          end: { line: number; character: number };
        };
      }
    ).range;
    expect(range.start.line).toBe(5);
    expect(range.start.character).toBe(0);
    expect(range.end.line).toBe(5);
    expect(range.end.character).toBe(10);
  });

  test('uses a zero-length range at the request cursor when no server range is supplied', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    const handler = getLastProgressHandler();

    // No range field in the progress value
    handler(makeProgress('sess-1', 1, 'insert text'));

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeDefined();

    const range = (
      items![0] as {
        insertText: string;
        range: {
          start: { line: number; character: number };
          end: { line: number; character: number };
        };
      }
    ).range;
    // Zero-length range: start === end === request cursor
    expect(range.start.line).toBe(5);
    expect(range.start.character).toBe(10);
    expect(range.end.line).toBe(5);
    expect(range.end.character).toBe(10);
  });

  test('uses a zero-length range when the server replacement range is malformed', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(
      makeProgress('sess-1', 1, 'insert text', {
        range: { start: { line: 5, character: 0 } },
      }),
    );

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeDefined();
    const range = (items![0] as { range: { start: vscode.Position; end: vscode.Position } }).range;
    expect(range.start.line).toBe(5);
    expect(range.start.character).toBe(10);
    expect(range.end.line).toBe(5);
    expect(range.end.character).toBe(10);
  });

  test('cancelActiveStream clears both cached candidate and request identity', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-1', 1, 'ghost'));

    // Simulate cursor movement to cancel
    const cursorCb = (vscode.window.onDidChangeTextEditorSelection as jest.Mock).mock
      .calls[0][0] as () => void;
    cursorCb();

    // Provider should find no cached candidate
    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('dispose clears both cached candidate and request identity', () => {
    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    getLastProgressHandler()(makeProgress('sess-1', 1, 'ghost'));

    const staleHandler = getLastProgressHandler();
    controller.dispose();

    // dispose() nulls activeRequestIdentity; the closure still holds its old
    // identity, so its late progress must be rejected by the reference guard.
    staleHandler(makeProgress('sess-1', 2, 'post-dispose ghost'));
    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('returns undefined when aiCompletion is disabled', () => {
    // Override to disable AI completion
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(
      (_section?: string) => ({
        get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
      }),
    );
    // Rebuild controller with the disabled config
    controller.dispose();
    controller = new StreamingCompletionController(mockClient);

    const provider = getStreamAdapter();
    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(0, 0);

    const items = provider.provideInlineCompletionItems(
      doc,
      pos,
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
    // No stream request should have been triggered
    expect(mockClient.sendRequest).not.toHaveBeenCalled();
  });
});

/**
 * Actual-context forwarding (#8282).
 *
 * The custom stream request previously hardcoded `context.triggerKind: 2`.
 * That is LSP `Automatic`, which the server refuses before any backend
 * dispatch (`external_completion_permitted`), so an explicit invocation could
 * never produce a streamed candidate.
 */
describe('StreamingCompletionController — actual request context forwarding', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;

  function makeMockDoc(uri: string, version: number): vscode.TextDocument {
    return { uri: { toString: () => uri }, version } as unknown as vscode.TextDocument;
  }

  function makeMockPos(line: number, character: number): vscode.Position {
    return { line, character } as unknown as vscode.Position;
  }

  /** The params object sent with the most recent custom stream request. */
  function lastRequestParams(): {
    context?: { triggerKind?: number; selectedCompletionInfo?: unknown };
    textDocument?: { uri?: string; version?: number };
    position?: { line?: number; character?: number };
  } {
    const calls = (mockClient.sendRequest as jest.Mock).mock.calls;
    return calls[calls.length - 1][1];
  }

  beforeEach(() => {
    jest.clearAllMocks();
    (vscode as Record<string, unknown>).Position = class {
      constructor(
        public line: number,
        public character: number,
      ) {}
    };
    (vscode as Record<string, unknown>).InlineCompletionItem = class {
      constructor(
        public insertText: string,
        public range?: unknown,
      ) {}
    };
    (vscode.window as Record<string, unknown>).onDidChangeTextEditorSelection = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    (vscode.workspace as Record<string, unknown>).onDidChangeTextDocument = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
      get: jest.fn((key: string, defaultValue?: unknown) => {
        if (key === 'aiCompletion.enabled') return true;
        if (key === 'aiCompletion.streaming.enabled') return true;
        return defaultValue;
      }),
    }));
    mockClient = createMockClient();
    controller = new StreamingCompletionController(mockClient);
  });

  afterEach(() => {
    controller.dispose();
  });

  test('an explicit invocation is forwarded as LSP Invoked (1), not Automatic', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 3),
      makeMockPos(4, 8),
      { triggerKind: 0 } as vscode.InlineCompletionContext, // vscode Invoke
      {} as vscode.CancellationToken,
    );

    // Fails against the hardcoded `triggerKind: 2`, which the server refuses.
    expect(lastRequestParams().context?.triggerKind).toBe(1);
  });

  test('an automatic trigger is forwarded as LSP Automatic (2)', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 3),
      makeMockPos(4, 8),
      { triggerKind: 1 } as vscode.InlineCompletionContext, // vscode Automatic
      {} as vscode.CancellationToken,
    );

    expect(lastRequestParams().context?.triggerKind).toBe(2);
  });

  test('an absent trigger kind fails closed to Automatic', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 3),
      makeMockPos(4, 8),
      {} as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    // An unknown context must not gain the stronger Invoked permission.
    expect(lastRequestParams().context?.triggerKind).toBe(2);
  });

  test('selectedCompletionInfo is forwarded with its exact range and text', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 3),
      makeMockPos(4, 8),
      {
        triggerKind: 0,
        selectedCompletionInfo: {
          range: { start: { line: 4, character: 4 }, end: { line: 4, character: 8 } },
          text: 'find_user',
        },
      } as unknown as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    expect(lastRequestParams().context?.selectedCompletionInfo).toEqual({
      range: { start: { line: 4, character: 4 }, end: { line: 4, character: 8 } },
      text: 'find_user',
    });
  });

  test('an absent selectedCompletionInfo is omitted rather than sent empty', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 3),
      makeMockPos(4, 8),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    expect(lastRequestParams().context?.selectedCompletionInfo).toBeUndefined();
  });

  test('the exact document identity and cursor reach the request', () => {
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///deep/module.pl', 42),
      makeMockPos(11, 3),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    const params = lastRequestParams();
    expect(params.textDocument).toEqual({ uri: 'file:///deep/module.pl', version: 42 });
    expect(params.position).toEqual({ line: 11, character: 3 });
  });

  test('a one-shot fallback response is served instead of discarded', async () => {
    // The server answers this custom request with ordinary inline completion
    // items when it declines to stream. The owner never calls the standard
    // route afterwards, so discarding them would lose completions outright.
    (mockClient.sendRequest as jest.Mock).mockResolvedValueOnce({
      items: [{ insertText: 'my $deterministic = 1;' }],
    });

    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    controller.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    await Promise.resolve();
    await Promise.resolve();

    const items = controller.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toHaveLength(1);
    const [served] = items as unknown as { insertText: string }[];
    expect(served?.insertText).toBe('my $deterministic = 1;');
  });

  test('an empty one-shot response caches nothing', async () => {
    (mockClient.sendRequest as jest.Mock).mockResolvedValueOnce({ items: [] });

    const doc = makeMockDoc('file:///a.pl', 1);
    const pos = makeMockPos(5, 10);

    controller.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    await Promise.resolve();
    await Promise.resolve();

    // Opposite-direction control: an empty result must not install a candidate.
    // The next call is a fresh generation, not a cache hit.
    const items = controller.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(items).toBeUndefined();
  });

  test('a server that does not advertise the stream method is not stream-ready', () => {
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
      get: jest.fn(() => true),
    }));

    const withoutCapability = {
      onProgress: jest.fn(() => ({ dispose: jest.fn() })),
      sendRequest: jest.fn(async () => ({})),
      sendNotification: jest.fn(),
      initializeResult: { capabilities: { experimental: {} } },
    } as unknown as LanguageClient;
    const ctrl = new StreamingCompletionController(withoutCapability);

    // Routing an invocation to an unsupported method would drop it entirely,
    // since the owner takes exactly one route.
    expect(ctrl.isStreamReady()).toBe(false);
    ctrl.dispose();
  });

  test('an absent initializeResult fails closed to not stream-ready', () => {
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
      get: jest.fn(() => true),
    }));

    const notInitialized = {
      onProgress: jest.fn(() => ({ dispose: jest.fn() })),
      sendRequest: jest.fn(async () => ({})),
      sendNotification: jest.fn(),
    } as unknown as LanguageClient;
    const ctrl = new StreamingCompletionController(notInitialized);

    expect(ctrl.isStreamReady()).toBe(false);
    ctrl.dispose();
  });

  test('an already-cancelled token dispatches no stream request', () => {
    // VS Code defers a listener registered after cancellation to a later
    // event-loop turn, so subscribing alone would let the request go out and be
    // cancelled a tick later. Nothing should be dispatched at all.
    const preCancelledToken = {
      isCancellationRequested: true,
      onCancellationRequested: () => ({ dispose: jest.fn() }),
    } as unknown as vscode.CancellationToken;

    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 1),
      makeMockPos(5, 10),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      preCancelledToken,
    );

    expect(mockClient.sendRequest as jest.Mock).not.toHaveBeenCalled();
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(0);
  });

  test('a live token after a cancelled one still dispatches', () => {
    const preCancelledToken = {
      isCancellationRequested: true,
      onCancellationRequested: () => ({ dispose: jest.fn() }),
    } as unknown as vscode.CancellationToken;

    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 1),
      makeMockPos(5, 10),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      preCancelledToken,
    );

    // Opposite-direction control: the refusal must be scoped to the cancelled
    // token, not latch the adapter off for subsequent live requests.
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 1),
      makeMockPos(5, 10),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );

    expect((mockClient.sendRequest as jest.Mock).mock.calls).toHaveLength(1);
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);
  });

  test('a token that fires cancellation synchronously on subscribe does not throw', () => {
    // Not flagged cancelled on entry, so it passes the pre-cancelled refusal
    // and reaches the subscribe path. Its listener fires synchronously, which
    // clears activeTokenSource — reading the token after subscribing would
    // throw a TypeError here and take the provider down.
    const synchronouslyCancelledToken = {
      isCancellationRequested: false,
      onCancellationRequested: (listener: () => void) => {
        listener();
        return { dispose: jest.fn() };
      },
    } as unknown as vscode.CancellationToken;

    expect(() =>
      controller.provideInlineCompletionItems(
        makeMockDoc('file:///a.pl', 1),
        makeMockPos(5, 10),
        { triggerKind: 0 } as vscode.InlineCompletionContext,
        synchronouslyCancelledToken,
      ),
    ).not.toThrow();

    // The request still went out with the generation's own token, and the
    // synchronous cancellation settled it rather than leaving it in flight.
    expect((mockClient.sendRequest as jest.Mock).mock.calls).toHaveLength(1);
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(1);
  });

  test('editor cancellation cancels the in-flight stream', () => {
    let fireCancellation: (() => void) | undefined;
    const token = {
      onCancellationRequested: (listener: () => void) => {
        fireCancellation = listener;
        return { dispose: jest.fn() };
      },
    } as unknown as vscode.CancellationToken;

    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 1),
      makeMockPos(5, 10),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      token,
    );
    expect(fireCancellation).toBeDefined();

    fireCancellation?.();

    // After cancellation the identity is released, so the next provider call
    // is a fresh generation rather than a deduplicated re-query.
    controller.provideInlineCompletionItems(
      makeMockDoc('file:///a.pl', 1),
      makeMockPos(5, 10),
      { triggerKind: 0 } as vscode.InlineCompletionContext,
      {} as vscode.CancellationToken,
    );
    expect(controller.snapshotStreamCounters().streamGenerationsStarted).toBe(2);
  });
});
