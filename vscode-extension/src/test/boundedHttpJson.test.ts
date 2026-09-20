import * as http from 'http';
import type { AddressInfo } from 'net';
import { fetchBoundedJson } from '../boundedHttpJson';
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
    return await run(`http://127.0.0.1:${address.port}/release`);
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

function fetchFrom<T>(
  url: string,
  overrides: Partial<Parameters<typeof fetchBoundedJson<T>>[0]> = {},
): Promise<T> {
  return fetchBoundedJson<T>({
    requestFactory: (listener) => http.get(url, listener),
    timeoutMs: 1000,
    maxBytes: 1024,
    operationName: 'Release metadata fetch',
    ...overrides,
  });
}

describe('fetchBoundedJson', () => {
  test('returns a successful bounded JSON document', async () => {
    const result = await withServer(
      (_request, response) => {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end('{"tag_name":"v1.2.3","assets":[]}');
      },
      (url) => fetchFrom<{ tag_name: string }>(url),
    );

    expect(result.tag_name).toBe('v1.2.3');
  });

  test('rejects non-success status before parsing the body', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(404, { 'content-type': 'application/json' });
          response.end('{"tag_name":"must-not-be-used"}');
        },
        (url) => fetchFrom(url),
      ),
    ).rejects.toThrow('HTTP 404');
  });

  test('rejects a declared response larger than the byte envelope', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, {
            'content-type': 'application/json',
            'content-length': '4096',
          });
          response.end('{}');
        },
        (url) => fetchFrom(url, { maxBytes: 64 }),
      ),
    ).rejects.toThrow('exceeded 64 bytes');
  });

  test('rejects a streaming response after it crosses the byte envelope', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.write('{"value":"');
          response.end('x'.repeat(256));
        },
        (url) => fetchFrom(url, { maxBytes: 32 }),
      ),
    ).rejects.toThrow('exceeded 32 bytes');
  });

  test('terminates a stalled response at the deadline', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.write('{');
        },
        (url) => fetchFrom(url, { timeoutMs: 25 }),
      ),
    ).rejects.toThrow('timeout after 0.025 seconds');
  });

  test('enforces the wall-clock deadline while bytes keep arriving', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.write('{"value":"');
          const interval = setInterval(() => {
            response.write('x');
          }, 5);
          response.once('close', () => clearInterval(interval));
        },
        (url) => fetchFrom(url, { timeoutMs: 30 }),
      ),
    ).rejects.toThrow('timeout after 0.03 seconds');
  });

  test('destroys the request when cancellation is signalled', async () => {
    const token = new TestCancellationToken();
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.write('{');
          setImmediate(() => token.cancel());
        },
        (url) => fetchFrom(url, { cancellationToken: token }),
      ),
    ).rejects.toThrow('cancelled');
  });

  test('rejects malformed JSON', async () => {
    await expect(
      withServer(
        (_request, response) => {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.end('{not json');
        },
        (url) => fetchFrom(url),
      ),
    ).rejects.toThrow('returned invalid JSON');
  });
});
