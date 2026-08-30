import * as vscode from 'vscode';
import type { LanguageClient } from 'vscode-languageclient/node';
import { ProgressType } from 'vscode-jsonrpc';
import {
  toLspInlineTriggerKind,
  toLspSelectedCompletionInfo,
  type InlineStreamAdapter,
} from './inlineCompletionRouting';

/** Shape of each candidate item inside a stream progress value. */
interface StreamCandidateItem {
  insertText: string;
  filterText?: string;
  range?: StreamReplacementRange;
}

interface StreamReplacementRange {
  start: { line: number; character: number };
  end: { line: number; character: number };
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
  serverRange?: StreamReplacementRange;
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
 * Streaming inline completion adapter.
 *
 * This type deliberately does **not** register an inline-completion provider.
 * `InlineCompletionOwner`, installed as language-client middleware, is the one
 * authoritative provider for Perl inline completion (#8282); this class is the
 * custom-stream session and cache adapter it delegates to. Registering here as
 * well would put two providers on the same `{ scheme: 'file', language: 'perl' }`
 * selector and let one editor trigger dispatch two server requests.
 *
 * Responsibilities:
 * 1. Start at most one custom stream request per request identity
 * 2. Cache cumulative candidates from $/progress, keyed to the request
 *    identity (URI + document version + cursor line + cursor character)
 * 3. Serve cached candidates only when all four key fields match
 * 4. Cancel streams on cursor movement or document changes
 */
export class StreamingCompletionController implements vscode.Disposable, InlineStreamAdapter {
  private client: LanguageClient;
  private cachedCandidate: CachedCandidate | null = null;
  private activeRequestIdentity: RequestIdentity | null = null;
  private activeTokenSource: vscode.CancellationTokenSource | null = null;
  private activeProgressToken: string | null = null;
  private activeProgressDisposable: vscode.Disposable | null = null;
  /** Editor cancellation subscription for the in-flight generation. */
  private activeCancellationSubscription: vscode.Disposable | null = null;
  private disposables: vscode.Disposable[] = [];
  /** Backend stream generations actually started. Bounded count, no text. */
  private streamGenerationsStarted = 0;
  /** Display re-queries that reused an in-flight generation instead of restarting it. */
  private duplicateDisplayRequeries = 0;
  /**
   * Set by `dispose`. A disposed adapter belongs to a superseded language-client
   * generation and must never take a route, even if the owner still holds a
   * reference to it during reconstruction.
   */
  private disposed = false;

  constructor(client: LanguageClient) {
    this.client = client;

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
      // `sequence` carries the ordering contract, and the stale-frame guard
      // compares it numerically. `NaN` loses every comparison, so an
      // unvalidated value would defeat that guard rather than be rejected.
      this.isDocumentCoordinate(obj.sequence) &&
      typeof obj.isFinal === 'boolean' &&
      Array.isArray(obj.items)
    );
  }

  /**
   * A document coordinate this client is willing to hand to `vscode.Position`.
   *
   * `typeof x === 'number'` is not enough: it admits `NaN`, `Infinity`, `-1`,
   * and `1.5`. `vscode.Position` rejects negative values by throwing, and this
   * runs inside `provideInlineCompletionItems`, so an unvalidated coordinate
   * from a malformed frame would surface as an exception in the provider
   * rather than as a frame that is quietly ignored.
   */
  private isDocumentCoordinate(value: unknown): value is number {
    return typeof value === 'number' && Number.isInteger(value) && value >= 0;
  }

  private isStreamPosition(value: unknown): value is { line: number; character: number } {
    if (typeof value !== 'object' || value === null) {
      return false;
    }
    const position = value as Record<string, unknown>;
    return (
      this.isDocumentCoordinate(position.line) && this.isDocumentCoordinate(position.character)
    );
  }

  private isStreamReplacementRange(value: unknown): value is StreamReplacementRange {
    if (typeof value !== 'object' || value === null) {
      return false;
    }
    const range = value as Record<string, unknown>;
    if (!this.isStreamPosition(range.start) || !this.isStreamPosition(range.end)) {
      return false;
    }
    // A range whose end precedes its start is not a range. VS Code would
    // silently reorder the two, so an inverted frame would apply an edit the
    // server never described.
    const start = range.start;
    const end = range.end;
    return start.line < end.line || (start.line === end.line && start.character <= end.character);
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
    // Reference identity is intentional: two requests with equal URI/version/
    // cursor values still represent different stream generations.
    if (capturedIdentity !== this.activeRequestIdentity) {
      return;
    }

    if (!this.isStreamProgressValue(value)) {
      return;
    }

    const item = value.items.length > 0 ? value.items[0] : undefined;
    const hasCandidate = !!item && typeof item.insertText === 'string';
    // A higher-sequence candidate for this session is already cached, so this
    // frame is out of order.
    const isStale =
      !!this.cachedCandidate &&
      this.cachedCandidate.sessionId === value.sessionId &&
      value.sequence <= this.cachedCandidate.sequence;

    if (!value.isFinal) {
      // An intermediate frame can only install a candidate. An empty one is a
      // chunk the server skipped as unsafe, not a decision about the stream.
      if (!hasCandidate || isStale) {
        return;
      }
      this.cachedCandidate = this.buildCandidate(capturedIdentity, value, item);
      void vscode.commands.executeCommand('editor.action.inlineSuggest.trigger');
      return;
    }

    // A terminal frame is the server's authoritative decision for this
    // request. An empty one revokes: it is not an ignorable progress frame.
    const accepted = hasCandidate && !isStale;
    if (accepted) {
      this.cachedCandidate = this.buildCandidate(capturedIdentity, value, item);
    } else if (!hasCandidate) {
      this.revokeCandidateFor(capturedIdentity);
    }

    // Settling first releases the in-flight marker and nulls the active
    // identity, so any later frame from this session fails the identity guard
    // above and cannot reopen the stream.
    this.settleActiveStream(capturedIdentity);

    // Only an accepted candidate needs the widget re-queried. Revocation has
    // already dismissed the old suggestion without re-entering this provider.
    if (accepted) {
      void vscode.commands.executeCommand('editor.action.inlineSuggest.trigger');
    }
  }

  /**
   * Build a cached candidate from the request identity, never from `item.range`.
   *
   * The server-supplied replacement range is stored separately and used when
   * building the returned `InlineCompletionItem`, but it is never the cache key.
   */
  private buildCandidate(
    identity: RequestIdentity,
    value: StreamProgressValue,
    item: StreamCandidateItem,
  ): CachedCandidate {
    const candidate: CachedCandidate = {
      uri: identity.uri,
      version: identity.version,
      line: identity.line,
      character: identity.character,
      text: item.insertText,
      sessionId: value.sessionId,
      sequence: value.sequence,
      isFinal: value.isFinal,
    };
    if (this.isStreamReplacementRange(item.range)) {
      candidate.serverRange = item.range;
    }
    return candidate;
  }

  /**
   * Discard any ghost text this request produced, and dismiss it on screen.
   *
   * Used for an empty terminal progress value, a request that completed
   * without one, and a backend failure — including one that arrives after
   * partial cumulative text has already been shown. Partial text from a failed
   * stream must never stay on screen looking like a completed suggestion.
   *
   * Clearing the cache is not enough: the suggestion is already rendered, so
   * the widget must be told. It is told with `hide` rather than `trigger`
   * precisely because `trigger` re-enters this provider at the cursor the
   * server just answered "nothing" for, which would dispatch another backend
   * generation, revoke again, and loop. `hide` dismisses without re-querying,
   * so revocation costs no generation and leaves a deliberate re-invocation at
   * the same cursor free to retry.
   */
  private revokeCandidateFor(identity: RequestIdentity): void {
    const cached = this.cachedCandidate;
    const hadVisibleCandidate =
      !!cached &&
      cached.uri === identity.uri &&
      cached.version === identity.version &&
      cached.line === identity.line &&
      cached.character === identity.character;

    if (!hadVisibleCandidate) {
      // Nothing was on screen for this identity, so there is nothing to clear
      // and nothing to dismiss.
      return;
    }

    this.cachedCandidate = null;
    void vscode.commands.executeCommand('editor.action.inlineSuggest.hide');
  }

  /**
   * Whether this adapter may take an inline-completion route.
   *
   * The owner consults this before selecting the stream route, so a disabled
   * configuration falls through to the standard path instead of being handled
   * here and returning nothing.
   */
  public isStreamReady(): boolean {
    if (this.disposed) {
      return false;
    }
    if (!this.serverAdvertisesStream()) {
      return false;
    }
    const config = vscode.workspace.getConfiguration('perl-lsp');
    const aiEnabled = config.get<boolean>('aiCompletion.enabled', false);
    const streamingEnabled = config.get<boolean>('aiCompletion.streaming.enabled', true);
    return aiEnabled && streamingEnabled;
  }

  /**
   * Whether the connected server actually implements the custom stream method.
   *
   * Local configuration is not sufficient readiness. `perl-lsp.serverPath` can
   * select an older or third-party server that supports standard inline
   * completion but not `textDocument/perlInlineCompletionStream`. Because the
   * owner routes an invocation to exactly one route, routing to an unsupported
   * method would drop the request entirely rather than falling back.
   *
   * Fails closed: an absent or not-yet-populated `initializeResult` reports
   * unready, so the standard route — which always works — is taken.
   *
   * Server side: `runtime/lifecycle/capabilities.rs:869` advertises this only
   * when `features.inline_completion` is on and the client declared support.
   */
  private serverAdvertisesStream(): boolean {
    const experimental = this.client.initializeResult?.capabilities?.experimental as
      | Record<string, unknown>
      | undefined;
    return experimental?.perlInlineCompletionStream === true;
  }

  /** Bounded counters for tests. Counts only — no source or completion text. */
  public snapshotStreamCounters(): {
    streamGenerationsStarted: number;
    duplicateDisplayRequeries: number;
  } {
    return {
      streamGenerationsStarted: this.streamGenerationsStarted,
      duplicateDisplayRequeries: this.duplicateDisplayRequeries,
    };
  }

  /**
   * Serve or start the custom stream for one invocation.
   *
   * Public because `InlineCompletionOwner` calls it directly; it is not
   * registered with VS Code as a provider.
   */
  public provideInlineCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    context: vscode.InlineCompletionContext,
    token: vscode.CancellationToken,
  ): vscode.InlineCompletionItem[] | undefined {
    if (!this.isStreamReady()) {
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

    // A display re-query for a generation that is already in flight must not
    // start a second one. `handleProgress` re-triggers the suggest widget on
    // every chunk, so without this guard each chunk would cancel the stream
    // that produced it and dispatch a fresh backend generation.
    const active = this.activeRequestIdentity;
    if (
      active &&
      active.uri === docUri &&
      active.version === docVersion &&
      active.line === position.line &&
      active.character === position.character
    ) {
      this.duplicateDisplayRequeries += 1;
      return undefined;
    }

    // Start a new stream request
    this.startStreamRequest(document, position, context, token);

    return undefined; // No immediate result -- will come via progress
  }

  /**
   * Begin one backend generation for this request identity.
   *
   * Cancels any prior generation first, then forwards the request's exact
   * document identity, cursor, trigger kind, and `selectedCompletionInfo` to
   * the server. The caller has already established that no generation is in
   * flight for this identity.
   */
  private startStreamRequest(
    document: vscode.TextDocument,
    position: vscode.Position,
    context: vscode.InlineCompletionContext,
    token: vscode.CancellationToken,
  ): void {
    // Cancel any existing stream (also sets activeRequestIdentity = null)
    this.cancelActiveStream();

    // The editor can hand the provider a token that is already cancelled when
    // the request was abandoned before we ran. VS Code schedules a listener
    // registered after cancellation for a later event-loop turn, so merely
    // subscribing below would still let the request go out and be cancelled a
    // tick later — one wasted backend generation. Refuse it outright instead.
    if (token?.isCancellationRequested === true) {
      return;
    }

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

    // Forward the actual editor context. The trigger kind is projected onto the
    // LSP numbering (Invoke 0 -> Invoked 1, Automatic 1 -> Automatic 2); it is
    // not the VS Code value. Sending a hardcoded 2 previously relabelled every
    // explicit invocation as automatic, which the server refuses before backend
    // dispatch, so no streamed candidate could ever be produced.
    const requestContext: {
      triggerKind: number;
      selectedCompletionInfo?: ReturnType<typeof toLspSelectedCompletionInfo>;
    } = {
      triggerKind: toLspInlineTriggerKind(context?.triggerKind),
    };
    const selectedCompletionInfo = toLspSelectedCompletionInfo(context?.selectedCompletionInfo);
    if (selectedCompletionInfo) {
      requestContext.selectedCompletionInfo = selectedCompletionInfo;
    }

    const params = {
      textDocument: {
        uri: document.uri.toString(),
        version: document.version,
      },
      position: {
        line: position.line,
        character: position.character,
      },
      context: requestContext,
      partialResultToken,
    };

    // Read the token before subscribing below. An already-cancelled editor
    // token may invoke its listener synchronously, and that listener clears
    // `activeTokenSource`; reading it afterwards would throw inside the
    // provider and take inline completion down with it.
    const requestToken = this.activeTokenSource.token;

    // Forward editor cancellation to the stream. Bound to this generation, so a
    // token cancelled after the stream was superseded cannot cancel its
    // successor.
    if (typeof token?.onCancellationRequested === 'function') {
      const cancellationSubscription = token.onCancellationRequested(() => {
        if (this.activeRequestIdentity === requestIdentity) {
          this.cancelActiveStream();
        }
      });
      this.activeCancellationSubscription = cancellationSubscription;
    }

    this.streamGenerationsStarted += 1;

    this.client.sendRequest('textDocument/perlInlineCompletionStream', params, requestToken).then(
      (response: unknown) => {
        // A terminal progress value already settled this generation (or it was
        // superseded). The stream owns its own outcome; the response is only
        // the acknowledgement.
        if (this.activeRequestIdentity !== requestIdentity) {
          return;
        }
        // The server answers this custom request with a one-shot inline
        // completion result whenever it declines to stream — its own AI config
        // is off (`streaming.rs:102-104`), or no backend is available and
        // `aiCompletion.fallback` is set (`streaming.rs:146-149`). Those items
        // are the deterministic fallback. Discarding them loses completions
        // outright, because the owner routes an invocation to exactly one route
        // and never calls the standard path afterwards.
        const served = this.cacheOneShotResponse(response, requestIdentity);
        if (!served) {
          // The request resolved with neither a terminal progress value nor a
          // one-shot result. Fail closed: anything this stream showed so far is
          // unconfirmed and must not survive as if it had completed.
          this.revokeCandidateFor(requestIdentity);
        }
        this.settleActiveStream(requestIdentity);
        if (served) {
          void vscode.commands.executeCommand('editor.action.inlineSuggest.trigger');
        }
      },
      // The rejection reason is deliberately unread: every rejection that gets
      // past the identity guard below is handled the same way, so branching on
      // it is what produced the misclassification this arm used to suffer from.
      (_err: unknown) => {
        // A cancelled or superseded generation has already been cleaned up by
        // `cancelActiveStream`, which also discarded its candidate. Reference
        // identity is load-bearing here: two successive requests can carry
        // identical URI/version/cursor values, and `revokeCandidateFor` below
        // compares by value, so without this guard a late rejection from an
        // abandoned generation would revoke its successor's live candidate.
        if (this.activeRequestIdentity !== requestIdentity) {
          return;
        }
        // Every rejection that reaches this point revokes, cancellations
        // included, because reaching this point *means* nothing cleaned up.
        //
        // Local cancellation — cursor movement, a document edit, or the
        // editor's own token — runs through `cancelActiveStream`, which nulls
        // `activeRequestIdentity` and discards the candidate before it cancels
        // the token. Such a request therefore returns at the identity guard
        // above and never arrives here. What does arrive is a rejection while
        // this generation is still active: a backend failure, or a
        // server-initiated cancellation (LSP `ServerCancelled`) that no local
        // cleanup has seen. Both leave a cached partial candidate that is still
        // servable, and leaving it is exactly the stale-ghost-text outcome this
        // module exists to prevent.
        //
        // Revoking dismisses rather than re-queries, so this costs no backend
        // generation and an explicit retry is never swallowed.
        this.revokeCandidateFor(requestIdentity);
        this.settleActiveStream(requestIdentity);
      },
    );
  }

  /**
   * Cache a one-shot inline completion result returned as the request's own
   * response, so the deterministic fallback is still displayed.
   *
   * The payload is the ordinary inline-completion shape, `{ items: [...] }`.
   * Returns true when a candidate was cached and the suggest widget should be
   * retriggered. No-ops for a superseded generation, so a late response cannot
   * install a candidate over its successor's.
   */
  private cacheOneShotResponse(response: unknown, requestIdentity: RequestIdentity): boolean {
    if (this.activeRequestIdentity !== requestIdentity) {
      return false;
    }
    if (typeof response !== 'object' || response === null) {
      return false;
    }
    const items = (response as { items?: unknown }).items;
    if (!Array.isArray(items) || items.length === 0) {
      return false;
    }
    const item = items[0];
    if (typeof item !== 'object' || item === null) {
      return false;
    }
    const insertText = (item as { insertText?: unknown }).insertText;
    if (typeof insertText !== 'string' || insertText.length === 0) {
      return false;
    }
    const candidate: CachedCandidate = {
      uri: requestIdentity.uri,
      version: requestIdentity.version,
      line: requestIdentity.line,
      character: requestIdentity.character,
      text: insertText,
      // A one-shot answer has no stream session. The marker keeps it distinct
      // from any session id, and the request has already completed, so no
      // progress can arrive to be compared against this sequence.
      sessionId: 'one-shot-fallback',
      sequence: 0,
      isFinal: true,
    };
    const range = (item as { range?: unknown }).range;
    if (this.isStreamReplacementRange(range)) {
      candidate.serverRange = range;
    }
    this.cachedCandidate = candidate;
    return true;
  }

  /**
   * Release the in-flight marker for a generation that has finished.
   *
   * The re-query guard suppresses a second backend generation only while one
   * is *in flight*. Without this, a stream that settled without producing a
   * candidate would leave `activeRequestIdentity` set forever, and every later
   * invocation at that cursor — including an explicit re-invocation the user
   * asked for — would be suppressed until they moved or edited.
   *
   * Unlike `cancelActiveStream` this preserves `cachedCandidate`, so a
   * successful final candidate stays servable from the cache while no live
   * request, progress registration, or cancellation resource remains.
   *
   * No-ops when the generation was already superseded, so a late completion
   * cannot settle its successor.
   */
  private settleActiveStream(requestIdentity: RequestIdentity): void {
    if (this.activeRequestIdentity !== requestIdentity) {
      return;
    }
    this.activeRequestIdentity = null;
    if (this.activeCancellationSubscription) {
      this.activeCancellationSubscription.dispose();
      this.activeCancellationSubscription = null;
    }
    if (this.activeProgressDisposable) {
      this.activeProgressDisposable.dispose();
      this.activeProgressDisposable = null;
    }
    this.activeProgressToken = null;
    if (this.activeTokenSource) {
      this.activeTokenSource.dispose();
      this.activeTokenSource = null;
    }
  }

  /**
   * Abandon the in-flight generation and discard its candidate.
   *
   * Used when the candidate is no longer valid for what the user is looking at
   * — cursor move, document change, editor cancellation, supersession, or
   * disposal. Contrast `settleActiveStream`, which releases the same resources
   * for a generation that finished on its own but keeps the cached candidate
   * servable.
   */
  private cancelActiveStream(): void {
    // Null out the active identity first so any in-flight progress callbacks
    // that fire after this point see a mismatched identity and bail out.
    this.activeRequestIdentity = null;
    this.cachedCandidate = null;
    if (this.activeCancellationSubscription) {
      this.activeCancellationSubscription.dispose();
      this.activeCancellationSubscription = null;
    }
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

  /** True when this adapter still belongs to the given language client. */
  public isBoundTo(candidate: LanguageClient): boolean {
    return !this.disposed && this.client === candidate;
  }

  /**
   * Retire this adapter with its language-client generation.
   *
   * Marks it disposed so `isStreamReady` reports false and the owner stops
   * routing to it, then cancels any in-flight stream and releases the editor
   * event subscriptions.
   */
  dispose(): void {
    this.disposed = true;
    this.cancelActiveStream();
    for (const d of this.disposables) {
      d.dispose();
    }
    this.disposables = [];
  }
}
