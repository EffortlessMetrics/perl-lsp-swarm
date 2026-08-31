export type LspProviderNeutralFailureKind = 'cancelled' | 'client_disposed';

export type LspProviderCallSettlement<T> =
  | {
      readonly kind: 'returned';
      readonly value: T;
      readonly wireValue: T;
    }
  | {
      readonly kind: LspProviderNeutralFailureKind;
      readonly error: unknown;
      readonly wireValue: T;
    }
  | {
      readonly kind: 'failed';
      readonly error: unknown;
      readonly wireValue: T;
    };

/** Distinguish expected teardown/cancellation from a provider product failure. */
export function classifyNeutralLspProviderFailure(
  error: unknown,
): LspProviderNeutralFailureKind | undefined {
  if (error !== null && typeof error === 'object' && 'code' in error) {
    if ((error as { readonly code?: unknown }).code === -32800) {
      return 'cancelled';
    }
  }
  const message = error instanceof Error ? error.message : String(error);
  return message.includes('Client got disposed') ? 'client_disposed' : undefined;
}

/** Compatibility predicate retained for existing call sites and tests. */
export function isNeutralLspProviderFailure(error: unknown): boolean {
  return classifyNeutralLspProviderFailure(error) !== undefined;
}

/**
 * Settle one provider call without losing the difference between a returned
 * value, cancellation/disposal, and an actual provider failure.
 *
 * `wireValue` is only the smallest editor-compatible projection used when the
 * method cannot propagate the original failure. It is never itself evidence
 * that the provider legitimately returned an empty result or safely refused.
 */
export async function settleLspProviderCallWithDisposition<T>(
  call: () => Promise<T>,
  wireFallback: T,
): Promise<LspProviderCallSettlement<T>> {
  try {
    const value = await call();
    return { kind: 'returned', value, wireValue: value };
  } catch (error: unknown) {
    const neutralKind = classifyNeutralLspProviderFailure(error);
    if (neutralKind) {
      return { kind: neutralKind, error, wireValue: wireFallback };
    }
    return { kind: 'failed', error, wireValue: wireFallback };
  }
}

/**
 * Backward-compatible adapter while middleware call sites migrate to the typed
 * settlement. The optional observer lets a caller retain terminal disposition
 * even when VS Code still requires an ordinary fallback return value.
 */
export async function settleLspProviderCall<T>(
  call: () => Promise<T>,
  fallback: T,
  onFailure: (error: unknown) => void,
  onSettlement?: (settlement: LspProviderCallSettlement<T>) => void,
): Promise<T> {
  const settlement = await settleLspProviderCallWithDisposition(call, fallback);
  onSettlement?.(settlement);
  if (settlement.kind === 'failed') {
    onFailure(settlement.error);
  }
  return settlement.wireValue;
}
