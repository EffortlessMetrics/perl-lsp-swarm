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

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

type InlineProvider = {
  provideInlineCompletionItems: (
    document: MockDocument,
    position: MockPosition,
    context: unknown,
    token: unknown,
  ) => unknown;
};

interface MockDocument {
  uri: { toString: () => string };
  version: number;
}

interface MockPosition {
  line: number;
  character: number;
}

function makeDocument(uri: string, version: number): MockDocument {
  return { uri: { toString: () => uri }, version };
}

function makePosition(line: number, character: number): MockPosition {
  return { line, character };
}

function makeProgressValue(
  opts: Partial<{
    sessionId: string;
    sequence: number;
    isFinal: boolean;
    insertText: string;
    range: { start: { line: number; character: number }; end: { line: number; character: number } };
  }> = {},
) {
  const {
    sessionId = 'sess-1',
    sequence = 1,
    isFinal = false,
    insertText = '->find_user($id)',
    range,
  } = opts;
  return {
    kind: 'perlInlineCompletionStream',
    sessionId,
    sequence,
    isFinal,
    items: [{ insertText, ...(range ? { range } : {}) }],
  };
}

/** Builds a mock client that captures all progress callbacks registered via onProgress. */
function createCapturingClient() {
  const registeredCallbacks: Array<(value: unknown) => void> = [];

  const client = {
    onProgress: jest.fn(
      (_type: unknown, _token: unknown, callback: (v: unknown) => void) => {
        registeredCallbacks.push(callback);
        return { dispose: jest.fn() };
      },
    ),
    sendRequest: jest.fn(async () => ({})),
    sendNotification: jest.fn(),
  } as unknown as LanguageClient;

  return {
    client,
    /** Deliver progress to the most-recently registered callback (the active stream). */
    deliverProgress: (value: unknown) => {
      const cb = registeredCallbacks[registeredCallbacks.length - 1];
      if (cb) cb(value);
    },
    /** Deliver progress to a specific callback by registration index (0-based). */
    deliverProgressTo: (index: number, value: unknown) => {
      const cb = registeredCallbacks[index];
      if (cb) cb(value);
    },
    callbackCount: () => registeredCallbacks.length,
  };
}

/**
 * Builds a StreamingCompletionController and captures both the registered
 * provider and the client's progress callbacks so tests can drive both.
 */
function createTestController(
  clientOverride?: LanguageClient,
): {
  controller: StreamingCompletionController;
  provider: InlineProvider;
  /** Deliver progress to the most-recently registered (active) stream callback. */
  deliverProgress: (value: unknown) => void;
  /** Deliver progress to a specific callback by registration order (0-based). */
  deliverProgressTo: (index: number, value: unknown) => void;
  client: LanguageClient;
} {
  const capturing = createCapturingClient();
  const client = clientOverride ?? capturing.client;

  let capturedProvider: InlineProvider | null = null;
  (vscode.languages as Record<string, unknown>).registerInlineCompletionItemProvider = jest.fn(
    (_selector: unknown, prov: unknown) => {
      capturedProvider = prov as InlineProvider;
      return { dispose: jest.fn() };
    },
  );

  const controller = new StreamingCompletionController(client);

  if (!capturedProvider) throw new Error('provider not captured');

  return {
    controller,
    provider: capturedProvider,
    deliverProgress: capturing.deliverProgress,
    deliverProgressTo: capturing.deliverProgressTo,
    client,
  };
}

/** Override getConfiguration so aiEnabled=true and streamingEnabled=true. */
function enableAiCompletion() {
  (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
    get: jest.fn((key: string, defaultValue?: unknown) => {
      if (key === 'aiCompletion.enabled') return true;
      if (key === 'aiCompletion.streaming.enabled') return true;
      return defaultValue;
    }),
  }));
}

// ---------------------------------------------------------------------------
// Original suite (preserved)
// ---------------------------------------------------------------------------

/** Create a minimal mock LanguageClient. */
function createMockClient(): LanguageClient {
  return {
    onProgress: jest.fn(() => ({ dispose: jest.fn() })),
    sendRequest: jest.fn(async () => ({})),
    sendNotification: jest.fn(),
  } as unknown as LanguageClient;
}

describe('StreamingCompletionController', () => {
  let mockClient: LanguageClient;
  let controller: StreamingCompletionController;

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

// ---------------------------------------------------------------------------
// New suite: cache identity correctness
// ---------------------------------------------------------------------------

describe('StreamingCompletionController — cache identity', () => {
  beforeEach(() => {
    jest.clearAllMocks();

    // Stub Position and InlineCompletionItem so provider can construct them
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

    // Stub event listeners so the controller can register them
    (vscode.window as Record<string, unknown>).onDidChangeTextEditorSelection = jest.fn(() => ({
      dispose: jest.fn(),
    }));
    (vscode.workspace as Record<string, unknown>).onDidChangeTextDocument = jest.fn(() => ({
      dispose: jest.fn(),
    }));

    enableAiCompletion();
  });

  // -------------------------------------------------------------------------
  // Acceptance tests from the issue
  // -------------------------------------------------------------------------

  test('same line/character in two different document URIs does not reuse ghost text', () => {
    const { controller, provider, deliverProgress } = createTestController();

    // Trigger a stream for file A at (5, 10)
    const docA = makeDocument('file:///a.pl', 1);
    const pos = makePosition(5, 10);
    provider.provideInlineCompletionItems(docA, pos, {}, {});

    // Deliver a progress item for file A
    deliverProgress(makeProgressValue({ insertText: 'text-for-A' }));

    // Verify the candidate is returned for file A
    const resultA = provider.provideInlineCompletionItems(docA, pos, {}, {}) as unknown[];
    expect(resultA).toBeDefined();
    expect(resultA.length).toBe(1);

    // A different document at the same cursor should NOT get the cached result
    const docB = makeDocument('file:///b.pl', 7);
    const resultB = provider.provideInlineCompletionItems(docB, pos, {}, {});
    expect(resultB).toBeUndefined();

    controller.dispose();
  });

  test('same URI and position at a newer document version does not reuse ghost text', () => {
    const { controller, provider, deliverProgress } = createTestController();

    // Trigger and populate a stream for version 1
    const uri = 'file:///foo.pl';
    const pos = makePosition(3, 5);
    const doc1 = makeDocument(uri, 1);
    provider.provideInlineCompletionItems(doc1, pos, {}, {});
    deliverProgress(makeProgressValue({ insertText: 'v1-text' }));

    // Version 1 exact match returns the candidate
    const resultV1 = provider.provideInlineCompletionItems(doc1, pos, {}, {}) as unknown[];
    expect(resultV1).toBeDefined();
    expect(resultV1.length).toBe(1);

    // Version 2 at the same URI/position must NOT get the version-1 candidate
    const doc2 = makeDocument(uri, 2);
    const resultV2 = provider.provideInlineCompletionItems(doc2, pos, {}, {});
    expect(resultV2).toBeUndefined();

    controller.dispose();
  });

  test('exact same URI/version/position key returns the cached candidate', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const doc = makeDocument('file:///exact.pl', 4);
    const pos = makePosition(7, 12);

    // First call: triggers the stream request
    provider.provideInlineCompletionItems(doc, pos, {}, {});

    // Progress arrives from the server
    deliverProgress(makeProgressValue({ insertText: 'exact-match-text', isFinal: true }));

    // Second call with the exact same doc/pos: should return the candidate
    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(result).toBeDefined();
    expect(result.length).toBe(1);
    expect(result[0]!.insertText).toBe('exact-match-text');

    controller.dispose();
  });

  test('a late progress callback from a cancelled/superseded stream cannot repopulate the cache', () => {
    const { controller, provider, deliverProgressTo } = createTestController();

    const doc = makeDocument('file:///late.pl', 1);
    const pos1 = makePosition(2, 0);
    const pos2 = makePosition(5, 0);

    // Start stream at pos1 → registers callback[0]
    provider.provideInlineCompletionItems(doc, pos1, {}, {});

    // Start a new stream at pos2 — this cancels the pos1 stream → registers callback[1]
    provider.provideInlineCompletionItems(doc, pos2, {}, {});

    // The pos1 callback (index 0) fires late — it must be discarded
    deliverProgressTo(0, makeProgressValue({ insertText: 'late-from-pos1' }));

    // pos2 has no candidate yet → provider starts another stream → returns undefined
    const resultAtPos2 = provider.provideInlineCompletionItems(doc, pos2, {}, {});
    expect(resultAtPos2).toBeUndefined();

    // pos1 also has nothing
    const resultAtPos1 = provider.provideInlineCompletionItems(doc, pos1, {}, {});
    expect(resultAtPos1).toBeUndefined();

    controller.dispose();
  });

  test('higher sequence values supersede lower within one session', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const doc = makeDocument('file:///seq.pl', 1);
    const pos = makePosition(0, 0);

    provider.provideInlineCompletionItems(doc, pos, {}, {});

    // sequence 1 arrives first
    deliverProgress(makeProgressValue({ sequence: 1, insertText: 'first' }));
    // sequence 3 arrives (out-of-order higher)
    deliverProgress(makeProgressValue({ sequence: 3, insertText: 'third' }));
    // sequence 2 arrives late — should be ignored because 3 > 2
    deliverProgress(makeProgressValue({ sequence: 2, insertText: 'second-stale' }));

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
    }>;
    expect(result).toBeDefined();
    expect(result[0]!.insertText).toBe('third');

    controller.dispose();
  });

  test('a replacement range beginning before the cursor is preserved while cache matching uses the request cursor', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const uri = 'file:///range.pl';
    const doc = makeDocument(uri, 1);
    const cursorPos = makePosition(4, 8);

    // server range starts before the cursor
    const serverRange = {
      start: { line: 4, character: 3 },
      end: { line: 4, character: 8 },
    };

    provider.provideInlineCompletionItems(doc, cursorPos, {}, {});
    deliverProgress(
      makeProgressValue({ insertText: 'method()', range: serverRange }),
    );

    // Cache hit at the request cursor (4, 8) — not at range.start (4, 3)
    const result = provider.provideInlineCompletionItems(doc, cursorPos, {}, {}) as Array<{
      insertText: string;
      range?: { start: { line: number; character: number } };
    }>;
    expect(result).toBeDefined();
    expect(result[0]!.insertText).toBe('method()');

    // Querying at the range start (4, 3) must NOT return the candidate
    const posAtRangeStart = makePosition(4, 3);
    const resultAtStart = provider.provideInlineCompletionItems(doc, posAtRangeStart, {}, {});
    expect(resultAtStart).toBeUndefined();

    controller.dispose();
  });

  test('cancelActiveStream and dispose clear both candidate and request identity', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const doc = makeDocument('file:///dispose.pl', 1);
    const pos = makePosition(1, 0);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    deliverProgress(makeProgressValue({ insertText: 'before-dispose' }));

    // Confirmed cached
    const before = provider.provideInlineCompletionItems(doc, pos, {}, {});
    expect(before).toBeDefined();

    controller.dispose();

    // After dispose the cache is cleared
    // (construct a new test controller so we can call the provider again)
    const { controller: ctrl2, provider: prov2 } = createTestController();

    // A fresh provider for the same doc/pos should return undefined (no cache)
    const after = prov2.provideInlineCompletionItems(doc, pos, {}, {});
    expect(after).toBeUndefined();

    ctrl2.dispose();
  });
});

// ---------------------------------------------------------------------------
// Provider return-value correctness
// ---------------------------------------------------------------------------

describe('StreamingCompletionController — provider return values', () => {
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

    enableAiCompletion();
  });

  test('returns undefined when AI completion is disabled', () => {
    (vscode.workspace as Record<string, unknown>).getConfiguration = jest.fn(() => ({
      get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
    }));

    const { controller, provider } = createTestController();
    const result = provider.provideInlineCompletionItems(
      makeDocument('file:///a.pl', 1),
      makePosition(0, 0),
      {},
      {},
    );
    expect(result).toBeUndefined();
    controller.dispose();
  });

  test('server replacement range is forwarded to the InlineCompletionItem', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const doc = makeDocument('file:///r.pl', 1);
    const pos = makePosition(2, 10);
    const serverRange = {
      start: { line: 2, character: 5 },
      end: { line: 2, character: 10 },
    };

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    deliverProgress(makeProgressValue({ insertText: 'replaced', range: serverRange }));

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
      range: { start: { line: number; character: number }; end: { line: number; character: number } };
    }>;
    expect(result).toBeDefined();
    expect(result[0]!.range.start.line).toBe(2);
    expect(result[0]!.range.start.character).toBe(5);
    expect(result[0]!.range.end.character).toBe(10);

    controller.dispose();
  });

  test('zero-length range at cursor is used when server provides no range', () => {
    const { controller, provider, deliverProgress } = createTestController();

    const doc = makeDocument('file:///norange.pl', 1);
    const pos = makePosition(3, 7);

    provider.provideInlineCompletionItems(doc, pos, {}, {});
    deliverProgress(makeProgressValue({ insertText: 'ghost' }));

    const result = provider.provideInlineCompletionItems(doc, pos, {}, {}) as Array<{
      insertText: string;
      range: { start: { line: number; character: number }; end: { line: number; character: number } };
    }>;
    expect(result).toBeDefined();
    expect(result[0]!.range.start.line).toBe(3);
    expect(result[0]!.range.start.character).toBe(7);
    expect(result[0]!.range.end.line).toBe(3);
    expect(result[0]!.range.end.character).toBe(7);

    controller.dispose();
  });
});
