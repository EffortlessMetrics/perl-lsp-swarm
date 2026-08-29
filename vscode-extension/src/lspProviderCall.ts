export function isNeutralLspProviderFailure(error: unknown): boolean {
  if (error !== null && typeof error === 'object' && 'code' in error) {
    if ((error as { readonly code?: unknown }).code === -32800) {
      return true;
    }
  }
  const message = error instanceof Error ? error.message : String(error);
  return message.includes('Client got disposed');
}

/** Classify failures where they occur, including disposal while the provider promise is awaited. */
export async function settleLspProviderCall<T>(
  call: () => Promise<T>,
  fallback: T,
  onFailure: (error: unknown) => void,
): Promise<T> {
  try {
    return await call();
  } catch (error: unknown) {
    if (!isNeutralLspProviderFailure(error)) {
      onFailure(error);
    }
    return fallback;
  }
}
