import type * as http from 'http';

export interface DisposableLike {
  dispose(): void;
}

export interface CancellationTokenLike {
  readonly isCancellationRequested: boolean;
  onCancellationRequested(listener: () => void): DisposableLike;
}

export interface BoundedJsonRequestOptions {
  requestFactory: (listener: (response: http.IncomingMessage) => void) => http.ClientRequest;
  timeoutMs: number;
  maxBytes: number;
  cancellationToken?: CancellationTokenLike;
  operationName?: string;
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
    let settled = false;

    const cleanup = (): void => {
      cancellation?.dispose();
      request?.setTimeout(0);
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

    try {
      request = requestFactory((incoming) => {
        response = incoming;
        const statusCode = incoming.statusCode ?? 0;
        if (statusCode < 200 || statusCode >= 300) {
          incoming.resume();
          fail(new Error(`${operationName} failed: HTTP ${statusCode}`));
          return;
        }

        const declaredLength = Number(incoming.headers['content-length']);
        if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
          incoming.resume();
          fail(
            new Error(
              `${operationName} exceeded ${maxBytes} bytes (declared ${declaredLength})`,
            ),
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
            const error = new Error(`${operationName} exceeded ${maxBytes} bytes`);
            incoming.destroy(error);
            request?.destroy(error);
            fail(error);
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

      request.setTimeout(timeoutMs, () => {
        const error = new Error(`${operationName} timeout after ${timeoutMs / 1000} seconds`);
        response?.destroy(error);
        request?.destroy(error);
        fail(error);
      });
      request.once('error', (error) => {
        fail(error);
      });

      cancellation = cancellationToken?.onCancellationRequested(() => {
        const error = new Error(`${operationName} cancelled`);
        response?.destroy(error);
        request?.destroy(error);
        fail(error);
      });
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
    }
  });
}
