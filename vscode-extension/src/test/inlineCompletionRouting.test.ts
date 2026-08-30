import type * as vscode from 'vscode';

import {
  InlineCompletionOwner,
  LSP_INLINE_TRIGGER_AUTOMATIC,
  LSP_INLINE_TRIGGER_INVOKED,
  VSCODE_INLINE_TRIGGER_AUTOMATIC,
  VSCODE_INLINE_TRIGGER_INVOKE,
  decideInlineCompletionRoute,
  toLspInlineTriggerKind,
  toLspSelectedCompletionInfo,
  type InlineStreamAdapter,
} from '../inlineCompletionRouting';

describe('inline completion trigger-kind projection', () => {
  test('the VS Code and LSP enumerations are genuinely different', () => {
    // The whole defect in #8282 is that these two numberings were conflated.
    // If they were equal the projection would be pointless, so pin them.
    //
    // These assertions compare the constants against literals; `vscode` is
    // mocked here, so they cannot themselves detect an upstream renumbering.
    // That binding is enforced at compile time by the `_InvokeIsZero` /
    // `_AutomaticIsOne` guards in `inlineCompletionRouting.ts`, which fail
    // `npm run typecheck` against the real `@types/vscode` declarations.
    expect(VSCODE_INLINE_TRIGGER_INVOKE).toBe(0);
    expect(VSCODE_INLINE_TRIGGER_AUTOMATIC).toBe(1);
    expect(LSP_INLINE_TRIGGER_INVOKED).toBe(1);
    expect(LSP_INLINE_TRIGGER_AUTOMATIC).toBe(2);
    expect(VSCODE_INLINE_TRIGGER_INVOKE).not.toBe(LSP_INLINE_TRIGGER_INVOKED);
  });

  test('Invoke maps to LSP Invoked', () => {
    expect(toLspInlineTriggerKind(VSCODE_INLINE_TRIGGER_INVOKE)).toBe(LSP_INLINE_TRIGGER_INVOKED);
  });

  test('Automatic maps to LSP Automatic', () => {
    expect(toLspInlineTriggerKind(VSCODE_INLINE_TRIGGER_AUTOMATIC)).toBe(
      LSP_INLINE_TRIGGER_AUTOMATIC,
    );
  });

  test('an absent or unknown trigger fails closed to Automatic', () => {
    // Failing open would let an unknown context authorize a remote call.
    expect(toLspInlineTriggerKind(undefined)).toBe(LSP_INLINE_TRIGGER_AUTOMATIC);
    expect(toLspInlineTriggerKind(99)).toBe(LSP_INLINE_TRIGGER_AUTOMATIC);
  });
});

describe('selectedCompletionInfo projection', () => {
  test('a complete value is projected verbatim', () => {
    expect(
      toLspSelectedCompletionInfo({
        range: { start: { line: 1, character: 2 }, end: { line: 1, character: 6 } },
        text: 'find_user',
      } as unknown as vscode.SelectedCompletionInfo),
    ).toEqual({
      range: { start: { line: 1, character: 2 }, end: { line: 1, character: 6 } },
      text: 'find_user',
    });
  });

  test('an absent value is undefined', () => {
    expect(toLspSelectedCompletionInfo(undefined)).toBeUndefined();
  });

  test('a structurally incomplete value is omitted rather than half-sent', () => {
    // A partially-filled constraint would be applied by the server against the
    // wrong range, which is worse than sending none.
    expect(
      toLspSelectedCompletionInfo({
        range: { start: { line: 1 }, end: { line: 1, character: 6 } },
        text: 'find_user',
      } as unknown as vscode.SelectedCompletionInfo),
    ).toBeUndefined();

    expect(
      toLspSelectedCompletionInfo({
        range: { start: { line: 1, character: 2 }, end: { line: 1, character: 6 } },
      } as unknown as vscode.SelectedCompletionInfo),
    ).toBeUndefined();
  });
});

describe('decideInlineCompletionRoute', () => {
  test('an automatic trigger never reaches the stream', () => {
    expect(
      decideInlineCompletionRoute({
        triggerKind: VSCODE_INLINE_TRIGGER_AUTOMATIC,
        streamReady: true,
      }),
    ).toBe('standard');
  });

  test('an explicit invocation with a ready stream takes the stream route', () => {
    expect(
      decideInlineCompletionRoute({ triggerKind: VSCODE_INLINE_TRIGGER_INVOKE, streamReady: true }),
    ).toBe('stream');
  });

  test('an explicit invocation without a ready stream falls back to standard', () => {
    expect(
      decideInlineCompletionRoute({
        triggerKind: VSCODE_INLINE_TRIGGER_INVOKE,
        streamReady: false,
      }),
    ).toBe('standard');
  });

  test('an absent trigger kind fails closed to standard', () => {
    expect(decideInlineCompletionRoute({ triggerKind: undefined, streamReady: true })).toBe(
      'standard',
    );
  });
});

describe('InlineCompletionOwner', () => {
  const doc = {} as vscode.TextDocument;
  const pos = {} as vscode.Position;
  const token = {} as vscode.CancellationToken;

  function makeAdapter(ready: boolean): InlineStreamAdapter & {
    provideInlineCompletionItems: jest.Mock;
  } {
    return {
      isStreamReady: () => ready,
      provideInlineCompletionItems: jest.fn(() => []),
    };
  }

  test('an automatic trigger goes to the standard route only', () => {
    const adapter = makeAdapter(true);
    const owner = new InlineCompletionOwner(() => adapter);
    const next = jest.fn(() => []);

    owner.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: VSCODE_INLINE_TRIGGER_AUTOMATIC } as vscode.InlineCompletionContext,
      token,
      next,
    );

    expect(next).toHaveBeenCalledTimes(1);
    expect(adapter.provideInlineCompletionItems).not.toHaveBeenCalled();
    expect(owner.snapshotCounters()).toEqual({
      providerInvocations: 1,
      standardRoute: 1,
      streamRoute: 0,
    });
  });

  test('an explicit invocation with a ready stream goes to the stream route only', () => {
    const adapter = makeAdapter(true);
    const owner = new InlineCompletionOwner(() => adapter);
    const next = jest.fn(() => []);

    owner.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: VSCODE_INLINE_TRIGGER_INVOKE } as vscode.InlineCompletionContext,
      token,
      next,
    );

    // One trigger, one route: the standard request must not also be dispatched.
    expect(adapter.provideInlineCompletionItems).toHaveBeenCalledTimes(1);
    expect(next).not.toHaveBeenCalled();
    expect(owner.snapshotCounters()).toEqual({
      providerInvocations: 1,
      standardRoute: 0,
      streamRoute: 1,
    });
  });

  test('an unready adapter falls back to the standard route', () => {
    const adapter = makeAdapter(false);
    const owner = new InlineCompletionOwner(() => adapter);
    const next = jest.fn(() => []);

    owner.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: VSCODE_INLINE_TRIGGER_INVOKE } as vscode.InlineCompletionContext,
      token,
      next,
    );

    expect(next).toHaveBeenCalledTimes(1);
    expect(adapter.provideInlineCompletionItems).not.toHaveBeenCalled();
  });

  test('an absent adapter falls back to the standard route', () => {
    const owner = new InlineCompletionOwner(() => undefined);
    const next = jest.fn(() => []);

    owner.provideInlineCompletionItems(
      doc,
      pos,
      { triggerKind: VSCODE_INLINE_TRIGGER_INVOKE } as vscode.InlineCompletionContext,
      token,
      next,
    );

    expect(next).toHaveBeenCalledTimes(1);
  });

  test('a disposed adapter stops being routed to without rebuilding the owner', () => {
    let ready = true;
    const adapter: InlineStreamAdapter = {
      isStreamReady: () => ready,
      provideInlineCompletionItems: jest.fn(() => []),
    };
    const owner = new InlineCompletionOwner(() => adapter);
    const next = jest.fn(() => []);
    const invoked = {
      triggerKind: VSCODE_INLINE_TRIGGER_INVOKE,
    } as vscode.InlineCompletionContext;

    owner.provideInlineCompletionItems(doc, pos, invoked, token, next);
    expect(next).not.toHaveBeenCalled();

    // Simulate configuration reconstruction / client restart disposing it.
    ready = false;
    owner.provideInlineCompletionItems(doc, pos, invoked, token, next);

    expect(next).toHaveBeenCalledTimes(1);
    expect(owner.snapshotCounters()).toEqual({
      providerInvocations: 2,
      standardRoute: 1,
      streamRoute: 1,
    });
  });

  test('the actual document, position, context, and token reach the stream route', () => {
    const adapter = makeAdapter(true);
    const owner = new InlineCompletionOwner(() => adapter);
    const realDoc = { uri: { toString: () => 'file:///x.pl' } } as unknown as vscode.TextDocument;
    const realPos = { line: 9, character: 4 } as unknown as vscode.Position;
    const realToken = { isCancellationRequested: false } as vscode.CancellationToken;
    const realContext = {
      triggerKind: VSCODE_INLINE_TRIGGER_INVOKE,
      selectedCompletionInfo: { text: 'x' },
    } as unknown as vscode.InlineCompletionContext;

    owner.provideInlineCompletionItems(
      realDoc,
      realPos,
      realContext,
      realToken,
      jest.fn(() => []),
    );

    expect(adapter.provideInlineCompletionItems).toHaveBeenCalledWith(
      realDoc,
      realPos,
      realContext,
      realToken,
    );
  });

  test('the actual context and token reach the standard route unchanged', () => {
    const owner = new InlineCompletionOwner(() => undefined);
    const next = jest.fn(() => []);
    const realContext = {
      triggerKind: VSCODE_INLINE_TRIGGER_AUTOMATIC,
    } as vscode.InlineCompletionContext;
    const realToken = { isCancellationRequested: false } as vscode.CancellationToken;

    owner.provideInlineCompletionItems(doc, pos, realContext, realToken, next);

    expect(next).toHaveBeenCalledWith(doc, pos, realContext, realToken);
  });
});
