import type * as AsyncHooks from 'async_hooks';
import { EventEmitter } from 'events';
import * as fs from 'fs';
import * as http from 'http';
import * as os from 'os';
import * as path from 'path';
import type { AddressInfo } from 'net';
import {
  cleanupPartialDownloadDest,
  downloadBoundedFile,
  unlinkPartialDownloadDest,
} from '../boundedFileDownload';
import type { CancellationTokenLike, DisposableLike } from '../boundedHttpJson';

class TestCancellationToken implements CancellationTokenLike {
  isCancellationRequested = false;
  private readonly listeners = new Set<() => void>();

  onCancellationRequested(listener: () => void): DisposableLike {
    this.listeners.add(listener);
    return {
      dispose: () => {
        this.listeners.delete(listener);
      },
    };
  }

  cancel(): void {
    this.isCancellationRequested = true;
    for (const listener of [...this.listeners]) {
      listener();
    }
  }
}

async function withServer<T>(
  handler: http.RequestListener,
  run: (url: string) => Promise<T>,
): Promise<T> {
  const server = http.createServer(handler);
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address() as AddressInfo;
  try {
    return await run(`http://127.0.0.1:${address.port}/archive`);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

function withTempDir(): { destPath: () => string } {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bounded-dl-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  return {
    destPath: () => path.join(tmpDir, 'payload.bin'),
  };
}

/** Directory-entry oracle. `existsSync` follows symlinks and hides dangling dests. */
function destEntryExists(dest: string): boolean {
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

function observeDestEntryWhenRejected(dest: string): {
  destEntryWhenRejected: () => boolean | undefined;
  run: <T>(work: () => Promise<T>) => Promise<T>;
} {
  let destEntryWhenRejected: boolean | undefined;
  return {
    destEntryWhenRejected: () => destEntryWhenRejected,
    run: async (work) => {
      try {
        return await work();
      } catch (error) {
        destEntryWhenRejected = destEntryExists(dest);
        throw error;
      }
    },
  };
}

describe('unlinkPartialDownloadDest', () => {
  const { destPath } = withTempDir();

  test('is a no-op when the path is already missing', () => {
    const dest = destPath();
    expect(() => unlinkPartialDownloadDest(dest)).not.toThrow();
    expect(destEntryExists(dest)).toBe(false);
  });

  test('removes a regular file synchronously', () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    unlinkPartialDownloadDest(dest);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('throws when the path is a directory', () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    expect(() => unlinkPartialDownloadDest(dest)).toThrow(/EISDIR|EPERM|ENOTEMPTY/);
    expect(destEntryExists(dest)).toBe(true);
  });
});

describe('cleanupPartialDownloadDest', () => {
  const { destPath } = withTempDir();

  test('native fallback removes a dangling symlink after a no-op remover', async () => {
    if (process.platform === 'win32') {
      return;
    }
    const dest = destPath();
    fs.symlinkSync(path.join(path.dirname(dest), 'missing-target'), dest);
    expect(fs.existsSync(dest)).toBe(false);
    expect(destEntryExists(dest)).toBe(true);

    await cleanupPartialDownloadDest(dest, async () => {});

    expect(destEntryExists(dest)).toBe(false);
  });

  test('does not resolve until a delayed remover finishes, and dest is gone afterward', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let destDuringCleanup: boolean | undefined;

    const cleanup = cleanupPartialDownloadDest(dest, async (filePath) => {
      destDuringCleanup = destEntryExists(filePath);
      await new Promise<void>((resolve) => setTimeout(resolve, 25));
      fs.unlinkSync(filePath);
    });

    expect(destEntryExists(dest)).toBe(true);
    await cleanup;
    expect(destDuringCleanup).toBe(true);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('reports when both removal attempts leave a directory dest', async () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    await expect(
      cleanupPartialDownloadDest(dest, async () => {
        throw new Error('injected cleanup failure');
      }),
    ).rejects.toThrow(/EISDIR|EPERM|ENOTEMPTY|destination remains/);
    expect(destEntryExists(dest)).toBe(true);
  });

  test('is a no-op success when dest never existed', async () => {
    const dest = destPath();
    await expect(cleanupPartialDownloadDest(dest)).resolves.toBeUndefined();
    expect(destEntryExists(dest)).toBe(false);
  });

  test('native fallback still removes dest when the injected remover is fire-and-forget', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    await cleanupPartialDownloadDest(dest, async (filePath) => {
      fs.unlink(filePath, () => {});
    });
    expect(destEntryExists(dest)).toBe(false);
  });

  test('joins a started callback unlink before cleanup settles', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let callbackCompleted = false;
    let callbackError: NodeJS.ErrnoException | null = null;
    await cleanupPartialDownloadDest(dest, async (filePath) => {
      fs.unlink(filePath, (error) => {
        callbackError = error;
        callbackCompleted = true;
      });
    });
    expect(callbackCompleted).toBe(true);
    expect(callbackError).toBeNull();
    expect(destEntryExists(dest)).toBe(false);
  });

  test('joins a started promise unlink even when the remover does not await it', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let unlink: Promise<void> | undefined;
    await cleanupPartialDownloadDest(dest, async (filePath) => {
      unlink = fs.promises.unlink(filePath);
    });
    await expect(unlink).resolves.toBeUndefined();
    expect(destEntryExists(dest)).toBe(false);
  });

  test('joins started removal when the injected remover subsequently rejects', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let callbackCompleted = false;
    await cleanupPartialDownloadDest(dest, async (filePath) => {
      fs.unlink(filePath, () => {
        callbackCompleted = true;
      });
      throw new Error('injected failure after starting removal');
    });
    expect(callbackCompleted).toBe(true);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('joins follow-on filesystem removal started by a callback', async () => {
    const dest = destPath();
    const firstDest = path.join(path.dirname(dest), 'first.bin');
    fs.writeFileSync(dest, 'partial');
    fs.writeFileSync(firstDest, 'partial');
    let finalCallbackCompleted = false;
    let finalError: NodeJS.ErrnoException | null = null;
    await cleanupPartialDownloadDest(dest, async () => {
      fs.unlink(firstDest, () => {
        fs.unlink(dest, (error) => {
          finalError = error;
          finalCallbackCompleted = true;
        });
      });
    });
    expect(finalCallbackCompleted).toBe(true);
    expect(finalError).toBeNull();
    expect(destEntryExists(dest)).toBe(false);
    expect(destEntryExists(firstDest)).toBe(false);
  });

  test('does not retain the completion scope after native fallback fails', async () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    await expect(cleanupPartialDownloadDest(dest, async () => {})).rejects.toThrow(
      /EISDIR|EPERM|ENOTEMPTY|destination remains/,
    );
    const followingDest = path.join(path.dirname(dest), 'following.bin');
    fs.writeFileSync(followingDest, 'partial');
    let completed = false;
    await cleanupPartialDownloadDest(followingDest, async (filePath) => {
      fs.unlink(filePath, () => {
        completed = true;
      });
    });
    expect(completed).toBe(true);
    expect(destEntryExists(followingDest)).toBe(false);
    expect(destEntryExists(dest)).toBe(true);
  });

  test('does not wait for future fire-and-forget immediate work', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let scheduled: NodeJS.Immediate | undefined;
    let laterWorkStarted = false;
    try {
      await cleanupPartialDownloadDest(dest, async () => {
        scheduled = setImmediate(() => {
          laterWorkStarted = true;
        });
      });
      expect(laterWorkStarted).toBe(false);
      expect(destEntryExists(dest)).toBe(false);
    } finally {
      if (scheduled) clearImmediate(scheduled);
    }
  });

  test('disables each completion observer on success, remover rejection, and fallback failure', async () => {
    const asyncHooks = jest.requireActual<typeof AsyncHooks>('async_hooks');
    const createHook = asyncHooks.createHook;
    const observers: jest.SpiedFunction<AsyncHooks.AsyncHook['disable']>[] = [];
    const observer = jest.spyOn(asyncHooks, 'createHook').mockImplementation((callbacks) => {
      const hook = createHook(callbacks);
      observers.push(jest.spyOn(hook, 'disable'));
      return hook;
    });
    try {
      const success = destPath();
      fs.writeFileSync(success, 'partial');
      await cleanupPartialDownloadDest(success);
      fs.writeFileSync(success, 'partial');
      await cleanupPartialDownloadDest(success, async () => {
        throw new Error('injected');
      });
      const directory = path.join(path.dirname(success), 'directory');
      fs.mkdirSync(directory);
      await expect(cleanupPartialDownloadDest(directory)).rejects.toThrow();
      expect(observers).toHaveLength(3);
      for (const disable of observers) expect(disable).toHaveBeenCalledTimes(1);
    } finally {
      observer.mockRestore();
      for (const disable of observers) disable.mockRestore();
    }
  });

  test('does not join an unrelated concurrent remover or retain its scope', async () => {
    const slowDest = destPath();
    const fastDest = path.join(path.dirname(slowDest), 'independent.bin');
    fs.writeFileSync(slowDest, 'partial');
    fs.writeFileSync(fastDest, 'partial');
    let releaseSlow: () => void = () => {};
    const slowGate = new Promise<void>((resolve) => {
      releaseSlow = resolve;
    });
    let slowSettled = false;
    const slow = cleanupPartialDownloadDest(slowDest, async (filePath) => {
      await slowGate;
      fs.unlinkSync(filePath);
    }).then(() => {
      slowSettled = true;
    });
    try {
      await cleanupPartialDownloadDest(fastDest, async (filePath) => {
        fs.unlink(filePath, () => {});
      });
      expect(destEntryExists(fastDest)).toBe(false);
      expect(destEntryExists(slowDest)).toBe(true);
      expect(slowSettled).toBe(false);
    } finally {
      releaseSlow();
      await slow;
    }
    expect(destEntryExists(slowDest)).toBe(false);
    const followingDest = destPath();
    fs.writeFileSync(followingDest, 'partial');
    await cleanupPartialDownloadDest(followingDest);
    expect(destEntryExists(followingDest)).toBe(false);
  });
});

describe('downloadBoundedFile', () => {
  const { destPath } = withTempDir();

  test('writes a successful body under the compressed ceiling', async () => {
    const dest = destPath();
    await withServer(
      (_request, response) => {
        response.writeHead(200, { 'content-type': 'application/octet-stream' });
        response.end('ok-bytes');
      },
      (url) =>
        downloadBoundedFile({
          requestFactory: (listener) => http.get(url, listener),
          dest,
          timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
          maxBytes: 64,
          operationName: 'Archive download',
        }),
    );
    expect(fs.readFileSync(dest, 'utf8')).toBe('ok-bytes');
  });

  test('rejects a declared Content-Length one byte over the ceiling before writing the body', async () => {
    const dest = destPath();
    let bodyRead = false;
    await expect(
      withServer(
        (request, response) => {
          request.on('data', () => {
            bodyRead = true;
          });
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': '33',
          });
          response.end('x'.repeat(33));
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
            maxBytes: 32,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('exceeded 32 compressed bytes (declared 33)');
    expect(destEntryExists(dest)).toBe(false);
    expect(bodyRead).toBe(false);
  });

  test('destroys a chunked response at the streaming ceiling and deletes the partial dest', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 12,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('does not delete a pre-existing dest when cancellation is already requested', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'keep-me');
    const token = new TestCancellationToken();
    token.cancel();
    await expect(
      downloadBoundedFile({
        requestFactory: () => {
          throw new Error('must not start a request after pre-cancellation');
        },
        dest,
        timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
        maxBytes: 64,
        cancellationToken: token,
        operationName: 'Archive download',
      }),
    ).rejects.toThrow('cancelled');
    expect(fs.readFileSync(dest, 'utf8')).toBe('keep-me');
  });

  test('native fallback still removes dest when the injected remover is a no-op', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 12,
              operationName: 'Archive download',
              removePartialFile: async () => {},
            }),
          ),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('does not let deferred cleanup delete a replacement destination', async () => {
    const dest = destPath();
    let cleanupStarted!: () => void;
    const cleanupReady = new Promise<void>((resolve) => {
      cleanupStarted = resolve;
    });
    let destDuringCleanup: boolean | undefined;
    let destWhenRejected: boolean | undefined;

    const failure = withServer(
      (_request, response) => {
        response.writeHead(200, { 'content-type': 'application/octet-stream' });
        response.write('0123456789');
        response.end('abcdefghij');
      },
      (url) =>
        downloadBoundedFile({
          requestFactory: (listener) => http.get(url, listener),
          dest,
          timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
          maxBytes: 12,
          operationName: 'Archive download',
          removePartialFile: async (filePath) => {
            destDuringCleanup = destEntryExists(filePath);
            cleanupStarted();
            await new Promise<void>((resolve) => setTimeout(resolve, 25));
            fs.unlinkSync(filePath);
          },
        }),
    );

    await cleanupReady;
    expect(destDuringCleanup).toBe(true);
    expect(destEntryExists(dest)).toBe(true);
    try {
      await failure;
      throw new Error('expected ceiling failure');
    } catch (error) {
      destWhenRejected = destEntryExists(dest);
      expect(error).toEqual(
        expect.objectContaining({
          message: expect.stringContaining('exceeded 12 compressed bytes'),
        }),
      );
    }
    expect(destWhenRejected).toBe(false);
    fs.writeFileSync(dest, 'replacement');
    await new Promise<void>((resolve) => setTimeout(resolve, 40));
    expect(fs.readFileSync(dest, 'utf8')).toBe('replacement');
  });

  test('reports cleanup failure when both removal attempts leave the destination', async () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': 64,
          });
          response.end('payload');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
            maxBytes: 12,
            operationName: 'Archive download',
            removePartialFile: () => {
              throw new Error('injected cleanup failure');
            },
          }),
      ),
    ).rejects.toMatchObject({
      message: expect.stringMatching(
        /exceeded 12 compressed bytes \(declared 64\); partial file cleanup failed:.*(EISDIR|EPERM|ENOTEMPTY|destination remains)/,
      ),
      cause: expect.objectContaining({
        message: 'Archive download exceeded 12 compressed bytes (declared 64)',
      }),
    });
    expect(destEntryExists(dest)).toBe(true);
  });

  test('preserves the original download error when fallback cleanup succeeds', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'stale destination');
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': 64,
          });
          response.end('payload');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
            maxBytes: 12,
            operationName: 'Archive download',
            removePartialFile: async () => {
              throw new Error('injected cleanup failure');
            },
          }),
      ),
    ).rejects.toMatchObject({
      message: 'Archive download exceeded 12 compressed bytes (declared 64)',
    });
    expect(destEntryExists(dest)).toBe(false);
  });

  test('fallback removes a dangling destination symlink', async () => {
    if (process.platform === 'win32') {
      return;
    }
    const dest = destPath();
    fs.symlinkSync(path.join(path.dirname(dest), 'missing-target'), dest);
    expect(fs.existsSync(dest)).toBe(false);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': 64,
          });
          response.end('payload');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
            maxBytes: 12,
            operationName: 'Archive download',
            removePartialFile: async () => {
              throw new Error('injected cleanup failure');
            },
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(destEntryExists(dest)).toBe(false);
  });

  test('no-op remover still unlinks a dangling dest before ceiling rejection', async () => {
    if (process.platform === 'win32') {
      return;
    }
    const dest = destPath();
    fs.symlinkSync(path.join(path.dirname(dest), 'missing-target'), dest);
    expect(fs.existsSync(dest)).toBe(false);
    expect(destEntryExists(dest)).toBe(true);
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': 64,
          });
          response.end('payload');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 12,
              operationName: 'Archive download',
              removePartialFile: async () => {},
            }),
          ),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('deletes a partial dest when cancellation is signalled during transfer', async () => {
    const dest = destPath();
    const token = new TestCancellationToken();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('partial');
          setImmediate(() => token.cancel());
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 1024,
              cancellationToken: token,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('cancelled');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('deletes a partial dest when the response stream errors', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('partial');
          setImmediate(() => response.destroy());
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 1024,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow();
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('timeout is independent of the byte ceiling', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('x');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 25,
              maxBytes: 1024 * 1024,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('Download timeout after 0.025 seconds');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('settles cleanup when a WriteStream does not emit close', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              createWriteStream: (filePath) => fs.createWriteStream(filePath, { emitClose: false }),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 12,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('deletes a pre-existing dest when the response is not 200', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'stale');
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(404, { 'content-type': 'text/plain' });
          response.end('missing');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 64,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('Failed to download: HTTP 404');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });

  test('cleans up dest when createWriteStream returns a non-WriteStream', async () => {
    const dest = destPath();
    const observed = observeDestEntryWhenRejected(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          observed.run(() =>
            downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              createWriteStream: (filePath) => {
                fs.writeFileSync(filePath, '');
                const stream = new EventEmitter() as fs.WriteStream;
                stream.write = ((chunk: string | Buffer) => {
                  fs.appendFileSync(filePath, chunk);
                  return true;
                }) as fs.WriteStream['write'];
                stream.destroy = (() => stream) as fs.WriteStream['destroy'];
                stream.end = (() => stream) as fs.WriteStream['end'];
                stream.close = (() => undefined) as fs.WriteStream['close'];
                return stream;
              },
              dest,
              timeoutMs: 10_000, // #17335: load-tolerant budget; the suite proves completion, not speed
              maxBytes: 12,
              operationName: 'Archive download',
            }),
          ),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(observed.destEntryWhenRejected()).toBe(false);
    expect(destEntryExists(dest)).toBe(false);
  });
});
