import type * as vscode from 'vscode';

/**
 * Single-owner routing for VS Code inline completion (#8282).
 *
 * One editor trigger must reach exactly one route. Before this seam existed the
 * language client's standard inline-completion provider and
 * `StreamingCompletionController`'s own `registerInlineCompletionItemProvider`
 * were both live for `{ scheme: 'file', language: 'perl' }`, so a single
 * keystroke could dispatch a buffered request and a custom streamed request at
 * the same time.
 *
 * This module owns only the route decision and the trigger-kind projection. It
 * performs no registration, holds no session state, and never sees completion
 * or source text.
 */

/**
 * `vscode.InlineCompletionTriggerKind` values, restated as constants.
 *
 * The enum is unavailable when the `vscode` module is mocked, and the numeric
 * values are load-bearing here, so they are pinned explicitly and bound to the
 * real enum by the compile-time guard below.
 *
 * Source: `@types/vscode` — `Invoke = 0`, `Automatic = 1`.
 */
export const VSCODE_INLINE_TRIGGER_INVOKE = 0;
export const VSCODE_INLINE_TRIGGER_AUTOMATIC = 1;

/**
 * LSP `InlineCompletionTriggerKind` values.
 *
 * These deliberately differ from the VS Code enum: LSP numbers `Invoked = 1`
 * and `Automatic = 2`, so the projection is not the identity function. Sending
 * the VS Code value unchanged would relabel an explicit invocation as automatic.
 *
 * Source: LSP 3.18 `InlineCompletionTriggerKind`, and the server-side parser at
 * `crates/perl-lsp-rs/src/runtime/language/misc.rs:171`.
 */
export const LSP_INLINE_TRIGGER_INVOKED = 1;
export const LSP_INLINE_TRIGGER_AUTOMATIC = 2;

/**
 * Compile-time guard binding the constants above to the real VS Code enum.
 *
 * `vscode` has no runtime presence under Jest — the module is mocked — so a
 * unit test can only compare these constants against themselves, which would
 * not notice an upstream renumbering. These aliases are checked by
 * `npm run typecheck` against the actual `@types/vscode` declarations: if
 * either member changes value, the conditional yields `false`, fails the
 * `extends true` constraint, and the build breaks here rather than silently
 * mislabelling every request's trigger kind.
 */
export type AssertTrue<T extends true> = T;
export type _InvokeIsZero = AssertTrue<
  vscode.InlineCompletionTriggerKind.Invoke extends 0 ? true : false
>;
export type _AutomaticIsOne = AssertTrue<
  vscode.InlineCompletionTriggerKind.Automatic extends 1 ? true : false
>;

/** The route selected for one inline-completion invocation. */
export type InlineCompletionRoute = 'standard' | 'stream';

/** Inputs to the route decision. Deliberately free of VS Code object types. */
export interface InlineRouteInputs {
  /** `vscode.InlineCompletionContext.triggerKind`, or undefined when absent. */
  triggerKind: number | undefined;
  /** Whether a stream adapter exists and AI streaming is configured on. */
  streamReady: boolean;
}

/**
 * Choose the one route for this invocation.
 *
 * ```text
 * automatic                     -> standard (deterministic-only)
 * invoked + stream ready        -> custom streamed request
 * invoked + stream not ready    -> standard (buffered AI or deterministic)
 * ```
 *
 * Automatic never reaches the external stream. That mirrors the server, which
 * refuses an automatic custom-stream request before any backend dispatch
 * (`external_completion_permitted`, `runtime/language/misc.rs:306`), and keeps
 * unbidden external output off the screen.
 *
 * A missing `triggerKind` is treated as automatic. Fail-closed: an unknown
 * trigger must not authorize a remote call.
 */
export function decideInlineCompletionRoute(inputs: InlineRouteInputs): InlineCompletionRoute {
  if (!inputs.streamReady) {
    return 'standard';
  }
  if (inputs.triggerKind !== VSCODE_INLINE_TRIGGER_INVOKE) {
    return 'standard';
  }
  return 'stream';
}

/**
 * Project a VS Code trigger kind onto the LSP wire value.
 *
 * Only an explicit `Invoke` becomes LSP `Invoked`. Everything else — including
 * an absent trigger kind — becomes `Automatic`, so an unknown context cannot
 * gain the stronger permission that `Invoked` carries on the server.
 */
export function toLspInlineTriggerKind(triggerKind: number | undefined): number {
  return triggerKind === VSCODE_INLINE_TRIGGER_INVOKE
    ? LSP_INLINE_TRIGGER_INVOKED
    : LSP_INLINE_TRIGGER_AUTOMATIC;
}

/** The `context.selectedCompletionInfo` projection sent on the wire. */
export interface LspSelectedCompletionInfo {
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  text: string;
}

/**
 * Project `vscode.InlineCompletionContext.selectedCompletionInfo` for the wire.
 *
 * Returns undefined when absent or structurally incomplete, so a malformed
 * value is omitted rather than sent as a partially-filled constraint the server
 * would apply against the wrong range.
 */
export function toLspSelectedCompletionInfo(
  selected: vscode.SelectedCompletionInfo | undefined,
): LspSelectedCompletionInfo | undefined {
  if (!selected || typeof selected.text !== 'string') {
    return undefined;
  }
  const range = selected.range as unknown as
    | {
        start?: { line?: unknown; character?: unknown };
        end?: { line?: unknown; character?: unknown };
      }
    | undefined;
  const start = range?.start;
  const end = range?.end;
  if (
    typeof start?.line !== 'number' ||
    typeof start?.character !== 'number' ||
    typeof end?.line !== 'number' ||
    typeof end?.character !== 'number'
  ) {
    return undefined;
  }
  return {
    range: {
      start: { line: start.line, character: start.character },
      end: { line: end.line, character: end.character },
    },
    text: selected.text,
  };
}

/**
 * Bounded counters for tests and diagnostics.
 *
 * Counts only. No URI, source, prompt, or completion text is recorded here.
 */
export interface InlineCompletionRouteCounters {
  /** Invocations that reached the single owner. */
  providerInvocations: number;
  /** Invocations delegated to the language client's standard route. */
  standardRoute: number;
  /** Invocations handed to the custom stream adapter. */
  streamRoute: number;
}

/** The subset of the stream controller the owner depends on. */
export interface InlineStreamAdapter {
  /** True when AI streaming is enabled and this adapter may take a route. */
  isStreamReady(): boolean;
  provideInlineCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    context: vscode.InlineCompletionContext,
    token: vscode.CancellationToken,
  ): vscode.InlineCompletionItem[] | undefined;
}

/**
 * The one authoritative owner for Perl inline completion in VS Code.
 *
 * Installed as `middleware.provideInlineCompletionItems` on the language
 * client, so it sits on the client's own provider rather than adding a second
 * registration. The stream controller is consulted through
 * `getStreamAdapter`, which returns undefined whenever streaming is disabled or
 * the adapter belongs to a superseded client generation.
 */
export class InlineCompletionOwner {
  private readonly getStreamAdapter: () => InlineStreamAdapter | undefined;
  private counters: InlineCompletionRouteCounters = {
    providerInvocations: 0,
    standardRoute: 0,
    streamRoute: 0,
  };

  constructor(getStreamAdapter: () => InlineStreamAdapter | undefined) {
    this.getStreamAdapter = getStreamAdapter;
  }

  /** Snapshot of the bounded counters. */
  public snapshotCounters(): InlineCompletionRouteCounters {
    return { ...this.counters };
  }

  /** Zero the counters. Test-support only; the owner never resets itself. */
  public resetCounters(): void {
    this.counters = { providerInvocations: 0, standardRoute: 0, streamRoute: 0 };
  }

  /**
   * Route one invocation. `next` is the language client's standard path.
   *
   * The actual document, position, context, and cancellation token are passed
   * through to whichever route is selected; nothing is reconstructed or
   * defaulted along the way.
   */
  public provideInlineCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    context: vscode.InlineCompletionContext,
    token: vscode.CancellationToken,
    next: (
      document: vscode.TextDocument,
      position: vscode.Position,
      context: vscode.InlineCompletionContext,
      token: vscode.CancellationToken,
    ) => vscode.ProviderResult<vscode.InlineCompletionItem[] | vscode.InlineCompletionList>,
  ): vscode.ProviderResult<vscode.InlineCompletionItem[] | vscode.InlineCompletionList> {
    this.counters.providerInvocations += 1;

    const adapter = this.getStreamAdapter();
    const streamReady = adapter?.isStreamReady() ?? false;
    const route = decideInlineCompletionRoute({
      triggerKind: context?.triggerKind,
      streamReady,
    });

    if (route === 'stream' && adapter) {
      this.counters.streamRoute += 1;
      return adapter.provideInlineCompletionItems(document, position, context, token);
    }

    this.counters.standardRoute += 1;
    return next(document, position, context, token);
  }
}
