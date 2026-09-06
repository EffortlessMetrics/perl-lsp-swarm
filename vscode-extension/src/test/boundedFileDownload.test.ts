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

describe('unlinkPartialDownloadDest', () => {
  const { destPath } = withTempDir();

  test('is a no-op when the path is already missing', () => {
    const dest = destPath();
    expect(() => unlinkPartialDownloadDest(dest)).not.toThrow();
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('removes a regular file synchronously', () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    unlinkPartialDownloadDest(dest);
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('throws when the path is a directory', () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    expect(() => unlinkPartialDownloadDest(dest)).toThrow(/EISDIR|EPERM|ENOTEMPTY/);
    expect(fs.existsSync(dest)).toBe(true);
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
    expect(() => fs.lstatSync(dest)).not.toThrow();

    await cleanupPartialDownloadDest(dest, async () => {});

    expect(fs.existsSync(dest)).toBe(false);
    expect(() => fs.lstatSync(dest)).toThrow();
  });

  test('does not resolve until a delayed remover finishes, and dest is gone afterward', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'partial');
    let destDuringCleanup: boolean | undefined;

    const cleanup = cleanupPartialDownloadDest(dest, async (filePath) => {
      destDuringCleanup = fs.existsSync(filePath);
      await new Promise<void>((resolve) => setTimeout(resolve, 25));
      fs.unlinkSync(filePath);
    });

    expect(fs.existsSync(dest)).toBe(true);
    await cleanup;
    expect(destDuringCleanup).toBe(true);
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('reports when both removal attempts leave a directory dest', async () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    await expect(
      cleanupPartialDownloadDest(dest, async () => {
        throw new Error('injected cleanup failure');
      }),
    ).rejects.toThrow(/EISDIR|EPERM|ENOTEMPTY|destination remains/);
    expect(fs.existsSync(dest)).toBe(true);
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
          timeoutMs: 1000,
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
            timeoutMs: 1000,
            maxBytes: 32,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('exceeded 32 compressed bytes (declared 33)');
    expect(fs.existsSync(dest)).toBe(false);
    expect(bodyRead).toBe(false);
  });

  test('destroys a chunked response at the streaming ceiling and deletes the partial dest', async () => {
    const dest = destPath();
    let destExistedWhenRejected: boolean | undefined;
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        async (url) => {
          try {
            await downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 1000,
              maxBytes: 12,
              operationName: 'Archive download',
            });
          } catch (error) {
            destExistedWhenRejected = fs.existsSync(dest);
            throw error;
          }
        },
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(destExistedWhenRejected).toBe(false);
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('does not reject until an injected partial-file cleanup has completed', async () => {
    const dest = destPath();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 1000,
            maxBytes: 12,
            operationName: 'Archive download',
            removePartialFile: async () => {},
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(fs.existsSync(dest)).toBe(false);
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
          timeoutMs: 1000,
          maxBytes: 12,
          operationName: 'Archive download',
          removePartialFile: async (filePath) => {
            destDuringCleanup = fs.existsSync(filePath);
            cleanupStarted();
            await new Promise<void>((resolve) => setTimeout(resolve, 25));
            fs.unlinkSync(filePath);
          },
        }),
    );

    await cleanupReady;
    expect(destDuringCleanup).toBe(true);
    expect(fs.existsSync(dest)).toBe(true);
    try {
      await failure;
      throw new Error('expected ceiling failure');
    } catch (error) {
      destWhenRejected = fs.existsSync(dest);
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
            timeoutMs: 1000,
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
    expect(fs.existsSync(dest)).toBe(true);
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
            timeoutMs: 1000,
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
    expect(fs.existsSync(dest)).toBe(false);
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
            timeoutMs: 1000,
            maxBytes: 12,
            operationName: 'Archive download',
            removePartialFile: async () => {
              throw new Error('injected cleanup failure');
            },
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(fs.existsSync(dest)).toBe(false);
    expect(() => fs.lstatSync(dest)).toThrow();
  });

  test('no-op remover still unlinks a dangling dest before ceiling rejection', async () => {
    if (process.platform === 'win32') {
      return;
    }
    const dest = destPath();
    fs.symlinkSync(path.join(path.dirname(dest), 'missing-target'), dest);
    expect(fs.existsSync(dest)).toBe(false);
    expect(() => fs.lstatSync(dest)).not.toThrow();
    let destEntryWhenRejected: boolean | undefined;
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/octet-stream',
            'content-length': 64,
          });
          response.end('payload');
        },
        async (url) => {
          try {
            await downloadBoundedFile({
              requestFactory: (listener) => http.get(url, listener),
              dest,
              timeoutMs: 1000,
              maxBytes: 12,
              operationName: 'Archive download',
              removePartialFile: async () => {},
            });
          } catch (error) {
            try {
              fs.lstatSync(dest);
              destEntryWhenRejected = true;
            } catch {
              destEntryWhenRejected = false;
            }
            throw error;
          }
        },
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(destEntryWhenRejected).toBe(false);
    expect(() => fs.lstatSync(dest)).toThrow();
  });

  test('deletes a partial dest when cancellation is signalled during transfer', async () => {
    const dest = destPath();
    const token = new TestCancellationToken();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('partial');
          setImmediate(() => token.cancel());
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 1000,
            maxBytes: 1024,
            cancellationToken: token,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('cancelled');
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('deletes a partial dest when the response stream errors', async () => {
    const dest = destPath();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('partial');
          setImmediate(() => response.destroy());
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 1000,
            maxBytes: 1024,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow();
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('timeout is independent of the byte ceiling', async () => {
    const dest = destPath();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('x');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 25,
            maxBytes: 1024 * 1024,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('Download timeout after 0.025 seconds');
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('settles cleanup when a WriteStream does not emit close', async () => {
    const dest = destPath();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            createWriteStream: (filePath) => fs.createWriteStream(filePath, { emitClose: false }),
            dest,
            timeoutMs: 1000,
            maxBytes: 12,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('deletes a pre-existing dest when the response is not 200', async () => {
    const dest = destPath();
    fs.writeFileSync(dest, 'stale');
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(404, { 'content-type': 'text/plain' });
          response.end('missing');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 1000,
            maxBytes: 64,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('Failed to download: HTTP 404');
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('cleans up dest when createWriteStream returns a non-WriteStream', async () => {
    const dest = destPath();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.write('0123456789');
          response.end('abcdefghij');
        },
        (url) =>
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
            timeoutMs: 1000,
            maxBytes: 12,
            operationName: 'Archive download',
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(fs.existsSync(dest)).toBe(false);
  });
});
