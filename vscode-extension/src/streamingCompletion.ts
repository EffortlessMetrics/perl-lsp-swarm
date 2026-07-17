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

/** Identity of the specific request that owns a progress stream. */
interface RequestIdentity {
  uri: string;
  version: number;
  line: number;
  character: number;
}

/** Snapshot of the most recent candidate received from the server. */
interface CachedCandidate {
  requestUri: string;
  requestVersion: number;
  requestLine: number;
  requestCharacter: number;
  serverRange?: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  text: string;
  sessionId: string;
  sequence: number;
  isFinal: boolean;
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
 * 2. Caches cumulative candidates from $/progress
 * 3. Feeds cached candidates through the inline completion provider
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

  private handleProgress(value: unknown, requestIdentity: RequestIdentity): void {
    if (!this.isStreamProgressValue(value)) {
      return;
    }

    // Ignore progress from a superseded or cancelled request
    if (
      !this.activeRequestIdentity ||
      this.activeRequestIdentity.uri !== requestIdentity.uri ||
      this.activeRequestIdentity.version !== requestIdentity.version ||
      this.activeRequestIdentity.line !== requestIdentity.line ||
      this.activeRequestIdentity.character !== requestIdentity.character
    ) {
      return;
    }

    if (value.items.length === 0) {
      return;
    }

    const item = value.items[0];
    if (!item || typeof item.insertText !== 'string') {
      return;
    }

    // Update cached candidate if it's newer
    if (
      this.cachedCandidate &&
      this.cachedCandidate.sessionId === value.sessionId &&
      value.sequence <= this.cachedCandidate.sequence
    ) {
      return; // Stale update
    }

    // Populate cache from the captured request identity, not from item.range.start
    const candidate: CachedCandidate = {
      requestUri: requestIdentity.uri,
      requestVersion: requestIdentity.version,
      requestLine: requestIdentity.line,
      requestCharacter: requestIdentity.character,
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

    const uri = document.uri.toString();
    const version = document.version;

    // Return cached candidate only when the full request identity matches
    if (
      this.cachedCandidate &&
      this.cachedCandidate.requestUri === uri &&
      this.cachedCandidate.requestVersion === version &&
      this.cachedCandidate.requestLine === position.line &&
      this.cachedCandidate.requestCharacter === position.character
    ) {
      const candidate = this.cachedCandidate;
      const requestStart = new vscode.Position(candidate.requestLine, candidate.requestCharacter);
      const range = candidate.serverRange
        ? new vscode.Range(
            new vscode.Position(
              candidate.serverRange.start.line,
              candidate.serverRange.start.character,
            ),
            new vscode.Position(
              candidate.serverRange.end.line,
              candidate.serverRange.end.character,
            ),
          )
        : new vscode.Range(requestStart, requestStart);
      return [new vscode.InlineCompletionItem(candidate.text, range)];
    }

    // Start a new stream request
    this.startStreamRequest(document, position);

    return undefined; // No immediate result -- will come via progress
  }

  private startStreamRequest(document: vscode.TextDocument, position: vscode.Position): void {
    // Cancel any existing stream
    this.cancelActiveStream();

    // Capture the full request identity; the progress handler closes over it
    const requestIdentity: RequestIdentity = {
      uri: document.uri.toString(),
      version: document.version,
      line: position.line,
      character: position.character,
    };
    this.activeRequestIdentity = requestIdentity;

    // Create cancellation token
    this.activeTokenSource = new vscode.CancellationTokenSource();

    const partialResultToken = `stream-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    this.activeProgressToken = partialResultToken;

    // Register progress handler for this specific token, capturing the request identity
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
    this.cachedCandidate = null;
    this.activeRequestIdentity = null;
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
