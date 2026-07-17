import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { ProgressType } from 'vscode-jsonrpc';

/** Shape of each candidate item inside a stream progress value. */
interface StreamCandidateItem {
  insertText: string;
  filterText?: string;
  range?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

/** Payload sent by the server via $/progress for streaming inline completions. */
interface StreamProgressValue {
  kind: string;
  sessionId: string;
  sequence: number;
  isFinal: boolean;
  items: StreamCandidateItem[];
}

/**
 * Identity of a single stream request: the document URI, version, and cursor
 * position at the moment the request was made. Used as the cache key so that
 * candidates from one document or version are never served for another.
 */
interface RequestIdentity {
  uri: string;
  version: number;
  line: number;
  character: number;
}

/**
 * Snapshot of the most recent candidate received from the server for a
 * specific request identity.
 *
 * `uri`, `version`, `line`, and `character` are populated from the
 * originating request, NOT from item.range, so cache lookups always use
 * the request cursor as the key. `serverRange` stores the replacement range
 * the server supplied; it is used when building the returned
 * InlineCompletionItem but is never part of the cache-hit predicate.
 */
interface CachedCandidate {
  uri: string;
  version: number;
  line: number;
  character: number;
  text: string;
  sessionId: string;
  sequence: number;
  isFinal: boolean;
  serverRange?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}

/**
 * Progress type marker used with `LanguageClient.onProgress`.
 *
 * `ProgressType<T>` is purely a type-level marker in vscode-jsonrpc —
 * the constructor takes no arguments and the generic parameter is
 * only used for TypeScript inference.
 */
const streamProgressType = new ProgressType<StreamProgressValue>();

/**
 * Streaming inline completion controller.
 *
 * Manages the lifecycle of progressive AI inline completions:
 * 1. Triggers custom stream requests to the server
 * 2. Caches cumulative candidates from $/progress, keyed to the request
 *    identity (URI + document version + cursor line + cursor character)
 * 3. Feeds cached candidates through the inline completion provider only
 *    when all four key fields match the current editor state
 * 4. Cancels streams on cursor movement or document changes
 */
export class StreamingCompletionController implements vscode.Disposable {
  private client: LanguageClient;
  private cachedCandidate: CachedCandidate | null = null;
  private activeRequestIdentity: RequestIdentity | null = null;
  private activeTokenSource: vscode.CancellationTokenSource | null = null;
  private activeProgressToken: string | null = null;
  private activeProgressDisposable: vscode.Disposable | null = null;
  private disposables: vscode.Disposable[] = [];

  constructor(client: LanguageClient) {
    this.client = client;

    // Register inline completion provider
    const provider = vscode.languages.registerInlineCompletionItemProvider(
      { scheme: 'file', language: 'perl' },
      {
        provideInlineCompletionItems: (
          document: vscode.TextDocument,
          position: vscode.Position,
          context: vscode.InlineCompletionContext,
          token: vscode.CancellationToken,
        ) => {
          return this.provideInlineCompletionItems(document, position, context, token);
        },
      },
    );
    this.disposables.push(provider);

    // Cancel on cursor movement
    const cursorDisposable = vscode.window.onDidChangeTextEditorSelection(() => {
      this.cancelActiveStream();
    });
    this.disposables.push(cursorDisposable);

    // Cancel on document change
    const changeDisposable = vscode.workspace.onDidChangeTextDocument(() => {
      this.cancelActiveStream();
    });
    this.disposables.push(changeDisposable);
  }

  /**
   * Check whether a progress value matches our streaming completion protocol.
   */
  private isStreamProgressValue(value: unknown): value is StreamProgressValue {
    if (typeof value !== 'object' || value === null) {
      return false;
    }
    const obj = value as Record<string, unknown>;
    return (
      obj.kind === 'perlInlineCompletionStream' &&
      typeof obj.sessionId === 'string' &&
      typeof obj.sequence === 'number' &&
      typeof obj.isFinal === 'boolean' &&
      Array.isArray(obj.items)
    );
  }

  /**
   * Process a progress notification for an active stream.
   *
   * `capturedIdentity` is the request identity snapshotted when the stream
   * was started (captured in the closure by `startStreamRequest`). If it no
   * longer matches `activeRequestIdentity`, the stream was cancelled or
   * superseded and any progress from it is ignored, preventing stale or
   * out-of-order candidates from populating the cache.
   */
  private handleProgress(value: unknown, capturedIdentity: RequestIdentity): void {
    // Ignore progress from a cancelled or superseded stream
    if (capturedIdentity !== this.activeRequestIdentity) {
      return;
    }

    if (!this.isStreamProgressValue(value)) {
      return;
    }

    if (value.items.length === 0) {
      return;
    }

    const item = value.items[0];
    if (!item || typeof item.insertText !== 'string') {
      return;
    }

    // Update cached candidate if it's newer (within the same session)
    if (
      this.cachedCandidate &&
      this.cachedCandidate.sessionId === value.sessionId &&
      value.sequence <= this.cachedCandidate.sequence
    ) {
      return; // Stale update — a higher-sequence candidate is already cached
    }

    // Populate the candidate from the request identity, not from item.range.
    // The server-supplied replacement range is stored separately and used when
    // building the returned InlineCompletionItem, but it is never the cache key.
    const candidate: CachedCandidate = {
      uri: capturedIdentity.uri,
      version: capturedIdentity.version,
      line: capturedIdentity.line,
      character: capturedIdentity.character,
      text: item.insertText,
      sessionId: value.sessionId,
      sequence: value.sequence,
      isFinal: value.isFinal,
    };
    if (item.range !== undefined) {
      candidate.serverRange = item.range;
    }
    this.cachedCandidate = candidate;

    // Trigger re-evaluation of inline completions
    void vscode.commands.executeCommand('editor.action.inlineSuggest.trigger');
  }

  private provideInlineCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    _context: vscode.InlineCompletionContext,
    _token: vscode.CancellationToken,
  ): vscode.InlineCompletionItem[] | undefined {
    // Check if AI completion is enabled
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const aiEnabled = config.get<boolean>('aiCompletion.enabled', false);
    const streamingEnabled = config.get<boolean>('aiCompletion.streaming.enabled', true);

    if (!aiEnabled || !streamingEnabled) {
      return undefined; // Let the server handle via standard path
    }

    const docUri = document.uri.toString();
    const docVersion = document.version;

    // Require an exact match on all four key fields before serving the cache.
    // A candidate produced for file:///a.pl v1 must not be returned for
    // file:///b.pl, a newer version of the same file, or a different cursor.
    if (
      this.cachedCandidate &&
      this.cachedCandidate.uri === docUri &&
      this.cachedCandidate.version === docVersion &&
      this.cachedCandidate.line === position.line &&
      this.cachedCandidate.character === position.character
    ) {
      const requestPos = new vscode.Position(
        this.cachedCandidate.line,
        this.cachedCandidate.character,
      );
      // Use the server-supplied replacement range when present; otherwise a
      // zero-length range at the request cursor so VS Code replaces nothing.
      const range = this.cachedCandidate.serverRange
        ? new vscode.Range(
            new vscode.Position(
              this.cachedCandidate.serverRange.start.line,
              this.cachedCandidate.serverRange.start.character,
            ),
            new vscode.Position(
              this.cachedCandidate.serverRange.end.line,
              this.cachedCandidate.serverRange.end.character,
            ),
          )
        : new vscode.Range(requestPos, requestPos);
      return [new vscode.InlineCompletionItem(this.cachedCandidate.text, range)];
    }

    // Start a new stream request
    this.startStreamRequest(document, position);

    return undefined; // No immediate result -- will come via progress
  }

  private startStreamRequest(document: vscode.TextDocument, position: vscode.Position): void {
    // Cancel any existing stream (also sets activeRequestIdentity = null)
    this.cancelActiveStream();

    // Create cancellation token
    this.activeTokenSource = new vscode.CancellationTokenSource();

    const partialResultToken = `stream-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    this.activeProgressToken = partialResultToken;

    // Capture the request identity for this stream. The progress handler
    // closure holds a reference to this object; we also store it on the
    // instance so `handleProgress` can verify the stream is still active.
    const requestIdentity: RequestIdentity = {
      uri: document.uri.toString(),
      version: document.version,
      line: position.line,
      character: position.character,
    };
    this.activeRequestIdentity = requestIdentity;

    // Register progress handler for this specific token
    this.activeProgressDisposable = this.client.onProgress(
      streamProgressType,
      partialResultToken,
      (value: unknown) => {
        this.handleProgress(value, requestIdentity);
      },
    );

    // Send custom request
    const params = {
      textDocument: {
        uri: document.uri.toString(),
        version: document.version,
      },
      position: {
        line: position.line,
        character: position.character,
      },
      context: {
        triggerKind: 2, // Automatic
      },
      partialResultToken,
    };

    this.client
      .sendRequest('textDocument/perlInlineCompletionStream', params, this.activeTokenSource.token)
      .catch((err: unknown) => {
        // Silently ignore cancellation errors (expected on cursor movement)
        if (err instanceof Error && err.message.includes('cancelled')) {
          return;
        }
        // Non-cancellation errors are suppressed — the stream will be
        // retried on the next inline completion trigger.
      });
  }

  private cancelActiveStream(): void {
    // Null out the active identity first so any in-flight progress callbacks
    // that fire after this point see a mismatched identity and bail out.
    this.activeRequestIdentity = null;
    this.cachedCandidate = null;
    if (this.activeProgressDisposable) {
      this.activeProgressDisposable.dispose();
      this.activeProgressDisposable = null;
    }
    this.activeProgressToken = null;
    if (this.activeTokenSource) {
      this.activeTokenSource.cancel();
      this.activeTokenSource.dispose();
      this.activeTokenSource = null;
    }
  }

  /**
   * Send telemetry notification when a completion is accepted.
   */
  public notifyAccepted(sessionId: string, candidateIndex: number): void {
    void this.client.sendNotification('perl/didAcceptInlineCompletion', {
      sessionId,
      candidate: candidateIndex,
    });
  }

  /**
   * Send telemetry notification when a completion is shown to the user.
   */
  public notifyShown(sessionId: string): void {
    void this.client.sendNotification('perl/didShowInlineCompletion', {
      sessionId,
    });
  }

  dispose(): void {
    this.cancelActiveStream();
    for (const d of this.disposables) {
      d.dispose();
    }
    this.disposables = [];
  }
}
