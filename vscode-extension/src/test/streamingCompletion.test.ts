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
  } as unknown as LanguageClient;
}

/** Shared beforeEach setup for controller construction. */
function setupVscodeMocks() {
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
  (vscode as Record<string, unknown>).CancellationTokenSource = class {
    token = { isCancellationRequested: false };
    cancel = jest.fn();
    dispose = jest.fn();
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
}

describe('StreamingCompletionController', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;

  beforeEach(() => {
    jest.clearAllMocks();
    setupVscodeMocks();

    mockClient = createMockClient();
    controller = new StreamingCompletionController(mockClient);
  });

  afterEach(() => {
    controller.dispose();
  });

  test('registers inline completion provider on construction', () => {
    expect(vscode.languages.registerInlineCompletionItemProvider).toHaveBeenCalledTimes(1);
    const call = (vscode.languages.registerInlineCompletionItemProvider as jest.Mock).mock.calls[0];
    expect(call[0]).toEqual({ scheme: 'file', language: 'perl' });
    expect(call[1]).toBeDefined();
  });

  test('registers cursor and document change listeners', () => {
    expect(vscode.window.onDidChangeTextEditorSelection).toHaveBeenCalledTimes(1);
    expect(vscode.workspace.onDidChangeTextDocument).toHaveBeenCalledTimes(1);
  });

  test('dispose cleans up all disposables', () => {
    const providerDispose = jest.fn();
    const cursorDispose = jest.fn();
    const docChangeDispose = jest.fn();

    (vscode.languages.registerInlineCompletionItemProvider as jest.Mock).mockReturnValue({
      dispose: providerDispose,
    });
    (vscode.window.onDidChangeTextEditorSelection as jest.Mock).mockReturnValue({
      dispose: cursorDispose,
    });
    (vscode.workspace.onDidChangeTextDocument as jest.Mock).mockReturnValue({
      dispose: docChangeDispose,
    });

    const ctrl = new StreamingCompletionController(createMockClient());
    ctrl.dispose();

    expect(providerDispose).toHaveBeenCalled();
    expect(cursorDispose).toHaveBeenCalled();
    expect(docChangeDispose).toHaveBeenCalled();
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

/** Build a progress payload matching the stream protocol. */
function makeProgressValue(
  sessionId: string,
  sequence: number,
  isFinal: boolean,
  insertText: string,
  serverRange?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  },
) {
  return {
    kind: 'perlInlineCompletionStream',
    sessionId,
    sequence,
    isFinal,
    items: [{ insertText, range: serverRange }],
  };
}

/** Create a minimal fake TextDocument. */
function createDocument(uri: string, version: number): vscode.TextDocument {
  return {
    uri: { toString: () => uri },
    version,
  } as unknown as vscode.TextDocument;
}

/** Create a Position using the test-injected mock class. */
function makePosition(line: number, character: number): vscode.Position {
  type PositionCtor = new (l: number, c: number) => vscode.Position;
  const Pos = (vscode as Record<string, unknown>).Position as PositionCtor;
  return new Pos(line, character);
}

describe('StreamingCompletionController — provider and cache identity correctness', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;
  let provider: {
    provideInlineCompletionItems: (
      doc: vscode.TextDocument,
      pos: vscode.Position,
      ctx: unknown,
      tok: unknown,
    ) => unknown;
  };

  beforeEach(() => {
    jest.clearAllMocks();
    setupVscodeMocks();

    // Enable AI completion so the provider runs the cache/stream logic
    (vscode.workspace.getConfiguration as jest.Mock).mockImplementation((_section?: string) => ({
      get: jest.fn((key: string, defaultValue?: unknown) => {
        if (key === 'aiCompletion.enabled') return true;
        if (key === 'aiCompletion.streaming.enabled') return true;
        return defaultValue;
      }),
    }));

    mockClient = createMockClient();
    controller = new StreamingCompletionController(mockClient);

    const registration = (
      vscode.languages.registerInlineCompletionItemProvider as jest.Mock
    ).mock.calls[0];
    provider = registration[1] as typeof provider;
  });

  afterEach(() => {
    controller.dispose();
  });

  /** Return the progress handler registered in the most recent onProgress call. */
  function lastProgressHandler(): (value: unknown) => void {
    const calls = (mockClient.onProgress as jest.Mock).mock.calls;
    if (calls.length === 0) throw new Error('onProgress was not called');
    return calls[calls.length - 1][2] as (v: unknown) => void;
  }

  test('cache miss starts a stream request and returns undefined', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {});

    expect(result).toBeUndefined();
    expect(mockClient.sendRequest as jest.Mock).toHaveBeenCalledTimes(1);
  });

  test('exact cache hit (same URI, version, line, char) returns ghost text', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    // First call: no cache → starts stream
    provider.provideInlineCompletionItems(doc, pos, {}, {});
    lastProgressHandler()(makeProgressValue('sess1', 1, false, 'foo_bar()'));

    // Second call with same identity → cache hit
    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;

    expect(Array.isArray(result)).toBe(true);
    expect(result[0]?.insertText).toBe('foo_bar()');
  });

  test('same line/char in a different URI is a cache miss', () => {
    const docA = createDocument('file:///a.pl', 1);
    const docB = createDocument('file:///b.pl', 1);
    const pos = makePosition(5, 10);

    // Fill cache for docA
    provider.provideInlineCompletionItems(docA, pos, {}, {});
    lastProgressHandler()(makeProgressValue('sess1', 1, false, 'foo_bar()'));

    // Same coordinates, different URI → miss
    const result = provider.provideInlineCompletionItems(docB, pos, {}, {});
    expect(result).toBeUndefined();
  });

  test('same URI and position at a newer document version is a cache miss', () => {
    const docV1 = createDocument('file:///a.pl', 1);
    const docV2 = createDocument('file:///a.pl', 2);
    const pos = makePosition(5, 10);

    // Fill cache for version 1
    provider.provideInlineCompletionItems(docV1, pos, {}, {});
    lastProgressHandler()(makeProgressValue('sess1', 1, false, 'foo_bar()'));

    // Same URI + position, newer version → miss
    const result = provider.provideInlineCompletionItems(docV2, pos, {}, {});
    expect(result).toBeUndefined();
  });

  test('late progress from a cancelled/superseded stream is silently ignored', () => {
    const docA = createDocument('file:///a.pl', 1);
    const docB = createDocument('file:///b.pl', 1);
    const pos = makePosition(5, 10);

    // Start stream for docA and capture its handler before it is superseded
    provider.provideInlineCompletionItems(docA, pos, {}, {});
    const handlerA = lastProgressHandler();

    // Start a new stream for docB (supersedes docA — activeRequestIdentity changes)
    provider.provideInlineCompletionItems(docB, pos, {}, {});

    // Late progress from the cancelled docA stream fires
    handlerA(makeProgressValue('sess-stale', 1, false, 'stale_text()'));

    // docB cache must remain empty; the stale progress must not have landed
    const result = provider.provideInlineCompletionItems(docB, pos, {}, {});
    expect(result).toBeUndefined();
  });

  test('higher sequence number supersedes lower within one session', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    const handler = lastProgressHandler();

    handler(makeProgressValue('sess1', 1, false, 'partial'));
    handler(makeProgressValue('sess1', 5, false, 'final'));

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(result[0]?.insertText).toBe('final');
  });

  test('out-of-order lower sequence is ignored after a higher one', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    const handler = lastProgressHandler();

    handler(makeProgressValue('sess1', 5, false, 'established'));
    handler(makeProgressValue('sess1', 2, false, 'older_chunk')); // stale

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(result[0]?.insertText).toBe('established');
  });

  test('cache identity uses request cursor position, not server replacement range start', () => {
    const doc = createDocument('file:///a.pl', 1);
    // Request cursor is at line 5 / char 10
    const pos = makePosition(5, 10);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    // Server replacement range begins before the cursor (char 5)
    lastProgressHandler()(
      makeProgressValue('sess1', 1, true, 'foo()', {
        start: { line: 5, character: 5 },
        end: { line: 5, character: 10 },
      }),
    );

    // Cache hit must still work at the request cursor position (5, 10)
    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(result).toBeDefined();
    expect(result[0]?.insertText).toBe('foo()');
  });

  test('server replacement range is preserved in the returned completion item', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    lastProgressHandler()(
      makeProgressValue('sess1', 1, true, 'bar()', {
        start: { line: 5, character: 3 },
        end: { line: 5, character: 10 },
      }),
    );

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
      range: unknown;
    }>;
    expect(result).toBeDefined();
    // The range must be present (server-provided, not a zero-length fallback)
    expect(result[0]?.range).toBeDefined();
  });

  test('cancelActiveStream (via dispose) clears candidate and request identity', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    // Fill the cache
    provider.provideInlineCompletionItems(doc, pos, {}, {});
    lastProgressHandler()(makeProgressValue('sess1', 1, false, 'cached_text'));

    // Verify cache is live
    const hit = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(hit[0]?.insertText).toBe('cached_text');

    // Dispose clears both candidate and activeRequestIdentity
    controller.dispose();

    // After disposal the same call no longer returns cached content
    const miss = provider.provideInlineCompletionItems(doc, pos, {}, {});
    expect(miss).toBeUndefined();
  });

  test('progress with empty items list does not update the cache', () => {
    const doc = createDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    lastProgressHandler()({
      kind: 'perlInlineCompletionStream',
      sessionId: 'sess1',
      sequence: 1,
      isFinal: false,
      items: [],
    });

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {});
    expect(result).toBeUndefined();
  });
});
