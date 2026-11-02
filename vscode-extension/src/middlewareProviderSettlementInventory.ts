/**
 * Guard against reintroducing raw catch/substitute middleware that erases typed
 * provider settlement before the editor-compatible wire fallback.
 */

export function evaluateMiddlewareProviderSettlementInventory(source: string): {
  ok: boolean;
  failures: string[];
} {
  const middlewareStart = source.indexOf('middleware:');
  const middlewareEnd = source.indexOf('handleWorkDoneProgress:', middlewareStart);
  if (middlewareStart < 0 || middlewareEnd < 0) {
    return {
      ok: false,
      failures: ['Could not locate LanguageClient middleware block in extension source'],
    };
  }

  const block = source.slice(middlewareStart, middlewareEnd);
  const failures: string[] = [];

  if (/catch\s*\(\s*error\s*:\s*unknown\s*\)/.test(block)) {
    failures.push(
      'LanguageClient middleware contains raw catch (error: unknown) handlers; use typed provider settlement helpers instead',
    );
  }

  const requiredSettlementMarkers = [
    'settleMiddlewareProviderCall',
    'settleDocumentListMiddlewareProviderCall',
    'settleNullableDocumentMiddlewareProviderCall',
    'settleResolveMiddlewareProviderCall',
    'settleFormattingProviderCall',
  ];
  const providerHooks = block.match(/(?:provide|resolve)[A-Z][A-Za-z]+:/g) ?? [];
  const exemptHooks = new Set(['provideInlineCompletionItems:']);
  for (const hook of providerHooks) {
    if (exemptHooks.has(hook)) {
      continue;
    }
    const hookIndex = block.indexOf(hook);
    const nextHook = providerHooks
      .map((candidate) => block.indexOf(candidate, hookIndex + hook.length))
      .filter((index) => index >= 0)
      .sort((left, right) => left - right)[0];
    const hookBody = block.slice(hookIndex, nextHook ?? block.length);
    if (!requiredSettlementMarkers.some((marker) => hookBody.includes(marker))) {
      failures.push(`${hook.trim()} does not route through typed provider settlement`);
    }
  }

  return { ok: failures.length === 0, failures };
}
