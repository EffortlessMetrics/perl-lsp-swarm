import * as fs from 'fs';
import { AsyncResource, createHook } from 'async_hooks';
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
  /** Resolves only after the destination cleanup has completed. */
  removePartialFile?: (dest: string) => Promise<void>;
}

/**
 * Unlink `dest`. Missing paths are success; every other errno is an error.
 *
 * Uses `unlinkSync` so a caller can observe dest absence before a Promise
 * rejects. `existsSync` follows symlinks, so a dangling destination entry
 * would otherwise look absent even though `unlinkSync` can still remove it.
 */
export function unlinkPartialDownloadDest(dest: string): void {
  try {
    fs.unlinkSync(dest);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw error;
    }
  }
}

function destinationDirectoryEntryExists(dest: string): boolean {
  try {
    fs.lstatSync(dest);
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return false;
    }
    throw error;
  }
}

/** Default injected remover. Native `ensurePartialDownloadDestGone` owns unlink. */
async function noopRemovePartialFile(_dest: string): Promise<void> {}

/**
 * Native fail-closed dest removal. No-op when the directory entry is already
 * gone, so a successful injected remover is the only unlink for that dest.
 */
function ensurePartialDownloadDestGone(dest: string): void {
  if (!destinationDirectoryEntryExists(dest)) {
    return;
  }
  unlinkPartialDownloadDest(dest);
  if (destinationDirectoryEntryExists(dest)) {
    throw new Error('destination remains');
  }
}

/** Join filesystem requests started before this remover settles, including callback APIs
 * whose caller returns before the callback. This owns their completion rather
 * than racing a second unlink against a Windows delete-pending handle.
 * Future timer-scheduled work is outside the remover's completion contract.
 */
async function removeAndJoinFilesystemRequests(
  dest: string,
  removePartialFile: (dest: string) => Promise<void>,
): Promise<void> {
  const scope = new AsyncResource('PartialDownloadCleanup', { requireManualDestroy: true });
  const descendants = new Set([scope.asyncId()]);
  const pending = new Set<number>();
  let removerSettled = false;
  let resolveDrained: () => void = () => {};
  const drained = new Promise<void>((resolve) => {
    resolveDrained = resolve;
  });
  const hook = createHook({
    init(id, type, trigger) {
      if (!descendants.has(trigger)) return;
      // Once the remover settles, only a native request's synchronous
      // callback can extend its completion chain. Later Promise/timer work
      // does not own this cleanup operation.
      if (removerSettled && !pending.has(trigger)) return;
      // A returned promise does not own future fire-and-forget scheduled
      // work. Awaited timers remain covered by the remover promise itself.
      if (type === 'Timeout' || type === 'Immediate') return;
      descendants.add(id);
      if (type === 'FSREQCALLBACK' || type === 'FSREQPROMISE') pending.add(id);
    },
    destroy(id) {
      descendants.delete(id);
      pending.delete(id);
      if (removerSettled && pending.size === 0) resolveDrained();
    },
  });
  hook.enable();
  try {
    try {
      await scope.runInAsyncScope(removePartialFile, undefined, dest);
    } finally {
      removerSettled = true;
      if (pending.size === 0) resolveDrained();
      await drained;
    }
  } finally {
    hook.disable();
    scope.emitDestroy();
  }
}

/**
 * Run the injected remover, then native unlink only if dest still exists.
 * Succeeds only when the directory entry is gone. Injected failures are
 * ignored so the native fallback remains authoritative.
 */
export async function cleanupPartialDownloadDest(
  dest: string,
  removePartialFile: (dest: string) => Promise<void> = noopRemovePartialFile,
): Promise<void> {
  try {
    await removeAndJoinFilesystemRequests(dest, removePartialFile);
  } catch {
    // The native fallback below is authoritative for the cleanup result.
  }
  try {
    ensurePartialDownloadDestGone(dest);
  } catch (error) {
    if (error instanceof Error) {
      throw error;
    }
    throw new Error('destination remains');
  }
}

function whenDestinationStreamReleased(stream: fs.WriteStream | undefined, then: () => void): void {
  // Only the native WriteStream has a close lifecycle that guarantees the
  // file handle is gone. Test/injected streams may expose EventEmitter's
  // `once` without ever emitting `close`, so they must not block failure
  // settlement indefinitely.
  const waitsForClose =
    stream instanceof fs.WriteStream && stream.closed === false && stream.destroyed !== true;
  if (!waitsForClose) {
    stream?.destroy();
    then();
    return;
  }
  // Use the destroy callback rather than only `close`: callers may provide
  // a WriteStream with emitClose:false, which legitimately never emits it.
  const destroyWithCallback = stream.destroy as unknown as (
    error: Error | undefined,
    callback: () => void,
  ) => void;
  destroyWithCallback.call(stream, undefined, then);
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
    removePartialFile = noopRemovePartialFile,
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
    let failureRejected = false;
    let receivedBytes = 0;

    const cleanup = (): void => {
      cancellation?.dispose();
      if (timeoutId) {
        clearTimeout(timeoutId);
      }
    };

    const rejectAfterPartialCleanup = async (error: Error): Promise<void> => {
      if (failureRejected) {
        return;
      }
      failureRejected = true;
      try {
        await cleanupPartialDownloadDest(dest, removePartialFile);
      } catch (cleanupError) {
        const reason = cleanupError instanceof Error ? cleanupError.message : 'destination remains';
        reject(
          new Error(`${error.message}; partial file cleanup failed: ${reason}`, { cause: error }),
        );
        return;
      }
      reject(error);
    };

    const fail = (error: Error): void => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      whenDestinationStreamReleased(file, () => {
        void rejectAfterPartialCleanup(error);
      });
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

        file = createWriteStream(dest);
        file.once('error', (err: NodeJS.ErrnoException) => {
          abort(err);
        });
        file.once('finish', () => {
          if (!settled) {
            file?.close();
            succeed();
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
