import type * as http from 'http';

export interface DisposableLike {
  dispose(): void;
}

export interface CancellationTokenLike {
  readonly isCancellationRequested: boolean;
  onCancellationRequested(listener: () => void): DisposableLike;
}

/**
 * Raised when the response carries a non-success status. Callers that must
 * preserve their own status-specific messages read `statusCode` instead of
 * matching on the message text.
 */
export class BoundedJsonStatusError extends Error {
  public readonly statusCode: number;

  constructor(operationName: string, statusCode: number) {
    super(`${operationName} failed: HTTP ${statusCode}`);
    this.name = 'BoundedJsonStatusError';
    this.statusCode = statusCode;
  }
}

export interface BoundedJsonRequestOptions {
  requestFactory: (listener: (response: http.IncomingMessage) => void) => http.ClientRequest;
  timeoutMs: number;
  maxBytes: number;
  cancellationToken?: CancellationTokenLike | undefined;
  operationName?: string | undefined;
}

/**
 * Execute one HTTP request and decode a bounded JSON response.
 *
 * The caller owns URL/proxy/TLS policy through requestFactory. This helper owns
 * lifecycle safety: non-success status rejection, a hard wall-clock deadline,
 * response byte accounting, cancellation, and single settlement.
 */
export function fetchBoundedJson<T>(options: BoundedJsonRequestOptions): Promise<T> {
  const {
    requestFactory,
    timeoutMs,
    maxBytes,
    cancellationToken,
    operationName = 'JSON request',
  } = options;

  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    return Promise.reject(new Error(`${operationName} requires a positive timeout`));
  }
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    return Promise.reject(new Error(`${operationName} requires a positive byte limit`));
  }
  if (cancellationToken?.isCancellationRequested) {
    return Promise.reject(new Error(`${operationName} cancelled`));
  }

  return new Promise<T>((resolve, reject) => {
    let request: http.ClientRequest | undefined;
    let response: http.IncomingMessage | undefined;
    let cancellation: DisposableLike | undefined;
    let timeoutId: NodeJS.Timeout | undefined;
    let settled = false;

    const cleanup = (): void => {
      cancellation?.dispose();
      if (timeoutId) {
        clearTimeout(timeoutId);
      }
    };

    const succeed = (value: T): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      resolve(value);
    };

    const fail = (error: Error): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      reject(error);
    };

    // Settle before tearing down the socket. Destroying first lets the
    // transport's own 'aborted'/'error' events settle the promise with a
    // generic transport message, losing the real reason (deadline, byte
    // envelope, cancellation, or non-success status).
    const abort = (error: Error): void => {
      fail(error);
      response?.destroy();
      request?.destroy();
    };

    try {
      request = requestFactory((incoming) => {
        response = incoming;
        const statusCode = incoming.statusCode ?? 0;
        if (statusCode < 200 || statusCode >= 300) {
          abort(new BoundedJsonStatusError(operationName, statusCode));
          return;
        }

        const declaredLength = Number(incoming.headers['content-length']);
        if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
          abort(
            new Error(`${operationName} exceeded ${maxBytes} bytes (declared ${declaredLength})`),
          );
          return;
        }

        const chunks: Buffer[] = [];
        let receivedBytes = 0;

        incoming.on('data', (chunk: Buffer | string) => {
          if (settled) {
            return;
          }
          const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
          receivedBytes += buffer.length;
          if (receivedBytes > maxBytes) {
            abort(new Error(`${operationName} exceeded ${maxBytes} bytes`));
            return;
          }
          chunks.push(buffer);
        });

        incoming.once('aborted', () => {
          fail(new Error(`${operationName} response was aborted`));
        });
        incoming.once('error', (error) => {
          fail(error);
        });
        incoming.once('end', () => {
          if (settled) {
            return;
          }
          try {
            const text = Buffer.concat(chunks, receivedBytes).toString('utf8');
            succeed(JSON.parse(text) as T);
          } catch (error) {
            fail(
              error instanceof Error
                ? new Error(`${operationName} returned invalid JSON: ${error.message}`)
                : new Error(`${operationName} returned invalid JSON`),
            );
          }
        });
      });

      timeoutId = setTimeout(() => {
        abort(new Error(`${operationName} timeout after ${timeoutMs / 1000} seconds`));
      }, timeoutMs);
      timeoutId.unref();

      request.once('error', (error) => {
        fail(error);
      });

      cancellation = cancellationToken?.onCancellationRequested(() => {
        abort(new Error(`${operationName} cancelled`));
      });
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
    }
  });
}
