import * as fs from 'fs';
import * as crypto from 'crypto';
import type * as http from 'http';
import type { CancellationTokenLike, DisposableLike } from './boundedHttpJson';

export interface BoundedFileDownloadOptions {
  requestFactory: (listener: (response: http.IncomingMessage) => void) => http.ClientRequest;
  dest: string;
  timeoutMs: number;
  maxBytes: number;
  cancellationToken?: CancellationTokenLike;
  operationName?: string;
  maxRedirects?: number;
  followRedirect?: (location: string, remainingRedirects: number) => Promise<void>;
  createWriteStream?: (dest: string) => fs.WriteStream;
  removePartialFile?: (dest: string) => void;
  promoteFile?: (source: string, dest: string) => void;
}

function defaultRemovePartialFile(dest: string): void {
  try {
    fs.unlinkSync(dest);
  } catch {
    // Best effort: the partial file may never have been created.
  }
}

/**
 * Download one HTTP body into `dest` with a hard compressed-byte ceiling that
 * is independent of the wall-clock timeout.
 *
 * Oversized `Content-Length` is rejected before any body is written. Chunked
 * or lying responses are destroyed at the streaming ceiling. Oversize, error,
 * timeout, and cancel paths delete the partial dest.
 */
export function downloadBoundedFile(options: BoundedFileDownloadOptions): Promise<void> {
  const {
    requestFactory,
    dest,
    timeoutMs,
    maxBytes,
    cancellationToken,
    operationName = 'Download',
    maxRedirects = 5,
    followRedirect,
    createWriteStream = (path) => fs.createWriteStream(path),
    removePartialFile = defaultRemovePartialFile,
    promoteFile = (source, target) => fs.renameSync(source, target),
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

  return new Promise<void>((resolve, reject) => {
    let request: http.ClientRequest | undefined;
    let response: http.IncomingMessage | undefined;
    let file: fs.WriteStream | undefined;
    let cancellation: DisposableLike | undefined;
    let timeoutId: NodeJS.Timeout | undefined;
    let settled = false;
    let receivedBytes = 0;
    // The generation owns this unique staging path. It does not claim the
    // caller-owned destination until the stream has completed successfully,
    // so cleanup can never quarantine a replacement that arrived at dest.
    const stagingDest = `${dest}.partial-download-${crypto.randomUUID()}`;

    // Match the previous createWriteStream truncation behavior for an
    // existing destination, while leaving any later replacement independent
    // from this generation's staging path.
    defaultRemovePartialFile(dest);

    const cleanup = (): void => {
      cancellation?.dispose();
      if (timeoutId) {
        clearTimeout(timeoutId);
      }
    };

    const removePartialAndReject = (error: Error): void => {
      try {
        removePartialFile(stagingDest);
      } catch {
        // Best effort: an injected remover must not replace the download error.
      }
      // Callers may inject a callback-based removal seam that returns before
      // deletion. Keep that seam observable, then enforce this helper's own
      // post-rejection cleanup contract before settling the promise.
      defaultRemovePartialFile(stagingDest);
      reject(error);
    };

    const fail = (error: Error): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();

      if (!file || file.closed) {
        removePartialAndReject(error);
        return;
      }

      let closeHandled = false;
      let closePoll: NodeJS.Immediate | undefined;
      const rejectAfterClose = (): void => {
        if (closeHandled) {
          return;
        }
        closeHandled = true;
        if (closePoll) {
          clearImmediate(closePoll);
        }
        file?.removeListener('close', rejectAfterClose);
        removePartialAndReject(error);
      };

      file.once('close', rejectAfterClose);
      try {
        file.destroy();
        if (closeHandled) {
          return;
        }
        // `emitClose: false` is a supported WriteStream option. Such a stream
        // closes its resource but never emits the event above, so observe the
        // documented closed state as a fallback. The undefined case is kept
        // out of this branch for lightweight test doubles without stream
        // lifecycle state.
        if (file.closed === undefined) {
          rejectAfterClose();
          return;
        }
        const pollClosedState = (): void => {
          if (file?.closed) {
            rejectAfterClose();
            return;
          }
          closePoll = setImmediate(pollClosedState);
        };
        closePoll = setImmediate(pollClosedState);
      } catch {
        file.removeListener('close', rejectAfterClose);
        rejectAfterClose();
      }
    };

    const succeed = (): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      resolve();
    };

    const abort = (error: Error): void => {
      fail(error);
      response?.destroy();
      request?.destroy();
    };

    try {
      request = requestFactory((incoming) => {
        response = incoming;
        const statusCode = incoming.statusCode ?? 0;

        if (statusCode === 301 || statusCode === 302) {
          const location = incoming.headers.location;
          incoming.resume();
          if (!location) {
            abort(new Error(`${operationName} redirect missing Location`));
            return;
          }
          if (!followRedirect) {
            abort(new Error(`${operationName} unexpected redirect`));
            return;
          }
          if (maxRedirects <= 0) {
            abort(new Error('Too many redirects'));
            return;
          }
          if (settled) {
            return;
          }
          settled = true;
          cleanup();
          incoming.destroy();
          request?.destroy();
          followRedirect(location, maxRedirects - 1)
            .then(() => resolve())
            .catch((error: unknown) => {
              reject(error instanceof Error ? error : new Error(String(error)));
            });
          return;
        }

        if (statusCode !== 200) {
          abort(new Error(`Failed to download: HTTP ${statusCode}`));
          return;
        }

        const declaredLength = Number(incoming.headers['content-length']);
        if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
          incoming.resume();
          abort(
            new Error(
              `${operationName} exceeded ${maxBytes} compressed bytes (declared ${declaredLength})`,
            ),
          );
          return;
        }

        file = createWriteStream(stagingDest);
        file.once('error', (err: NodeJS.ErrnoException) => {
          abort(err);
        });
        file.once('finish', () => {
          if (!settled) {
            file?.close();
            try {
              promoteFile(stagingDest, dest);
              succeed();
            } catch (error) {
              fail(error instanceof Error ? error : new Error(String(error)));
            }
          }
        });

        incoming.on('data', (chunk: Buffer | string) => {
          if (settled) {
            return;
          }
          const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
          receivedBytes += buffer.length;
          if (receivedBytes > maxBytes) {
            abort(new Error(`${operationName} exceeded ${maxBytes} compressed bytes`));
            return;
          }
          if (file && !file.write(buffer)) {
            incoming.pause();
            file.once('drain', () => incoming.resume());
          }
        });

        incoming.once('aborted', () => {
          abort(new Error(`${operationName} response was aborted`));
        });
        incoming.once('error', (error) => {
          abort(error instanceof Error ? error : new Error(String(error)));
        });
        incoming.once('end', () => {
          if (settled) {
            return;
          }
          file?.end();
        });
      });

      timeoutId = setTimeout(() => {
        abort(new Error(`Download timeout after ${timeoutMs / 1000} seconds`));
      }, timeoutMs);
      timeoutId.unref();

      request.once('error', (error) => {
        abort(error instanceof Error ? error : new Error(String(error)));
      });
      request.once('timeout', () => {
        abort(new Error('Request timeout'));
      });

      cancellation = cancellationToken?.onCancellationRequested(() => {
        abort(new Error(`${operationName} cancelled`));
      });
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
    }
  });
}
