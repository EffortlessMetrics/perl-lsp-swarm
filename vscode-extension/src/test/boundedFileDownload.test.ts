import * as fs from 'fs';
import * as http from 'http';
import * as os from 'os';
import * as path from 'path';
import type { AddressInfo } from 'net';
import { downloadBoundedFile } from '../boundedFileDownload';
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

describe('downloadBoundedFile', () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bounded-dl-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  function destPath(): string {
    return path.join(tmpDir, 'payload.bin');
  }

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
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
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
            removePartialFile: () => {},
          }),
      ),
    ).rejects.toThrow('exceeded 12 compressed bytes');
    expect(fs.existsSync(dest)).toBe(false);
  });

  test('reports cleanup failure when both removal attempts leave the destination', async () => {
    const dest = destPath();
    fs.mkdirSync(dest);
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/octet-stream' });
          response.end('payload');
        },
        (url) =>
          downloadBoundedFile({
            requestFactory: (listener) => http.get(url, listener),
            dest,
            timeoutMs: 1000,
            maxBytes: 64,
            operationName: 'Archive download',
            removePartialFile: () => {
              throw new Error('injected cleanup failure');
            },
          }),
      ),
    ).rejects.toThrow('partial file cleanup failed:');
    expect(fs.existsSync(dest)).toBe(true);
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
});
