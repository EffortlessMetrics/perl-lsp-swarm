/**
 * Unit tests for the managed HTTP transport (#7804/#7851).
 *
 * Every fixture is a local deterministic server: a canned proxy fixture that
 * records absolute-form requests and CONNECT tunnels, and a dead port that
 * makes the direct path unavailable. No live external network is used.
 */

import * as fs from 'fs';
import * as http from 'http';
import * as net from 'net';
import * as path from 'path';
import type { CancellationTokenLike } from '../boundedHttpJson';
import { fetchBoundedJson } from '../boundedHttpJson';
import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import { isNetworkErrorMessage } from '../downloader';
import {
  createManagedHttpRequest,
  MANAGED_PROXY_ERROR_PREFIX,
  redactCredentialUrl,
  resolveManagedProxy,
} from '../managedHttpTransport';

const PROXIED_BODY = 'PROXY_FIXTURE_BODY_v1';

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

interface ProxyFixtureOptions {
  /** Exact `Proxy-Authorization` value required on CONNECT; mismatch is a 407. */
  requiredConnectCredentials?: string | undefined;
  /** Accept a CONNECT, record it, and never answer. */
  hangConnect?: boolean | undefined;
  /** Absolute-form GET response body. */
  body?: string | undefined;
  /** Absolute-form GET response content type. */
  contentType?: string | undefined;
  /** Absolute-form GET response status. */
  status?: number | undefined;
}

interface RecordedProxyRequest {
  url: string | undefined;
  method: string | undefined;
  headers: http.IncomingHttpHeaders;
}

interface RecordedProxyConnect {
  authority: string | undefined;
  proxyAuthorization: string | undefined;
  clientBytes: Buffer[];
  socket: net.Socket;
  /** True once the peer disconnected (FIN observed). */
  ended: boolean;
}

interface ProxyFixture {
  readonly port: number;
  readonly requests: RecordedProxyRequest[];
  readonly connects: RecordedProxyConnect[];
  close(): Promise<void>;
}

function trackSockets(server: net.Server): Set<net.Socket> {
  const sockets = new Set<net.Socket>();
  server.on('connection', (socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
  });
  return sockets;
}

async function startProxyFixture(options: ProxyFixtureOptions = {}): Promise<ProxyFixture> {
  const requests: RecordedProxyRequest[] = [];
  const connects: RecordedProxyConnect[] = [];

  const server = http.createServer((req, res) => {
    requests.push({ url: req.url, method: req.method, headers: req.headers });
    res.writeHead(options.status ?? 200, {
      'content-type': options.contentType ?? 'application/json',
    });
    res.end(options.body ?? PROXIED_BODY);
  });
  server.on('connect', (req, rawSocket, _head) => {
    const socket = rawSocket as net.Socket;
    const record: RecordedProxyConnect = {
      authority: req.url,
      proxyAuthorization: req.headers['proxy-authorization'],
      clientBytes: [],
      socket,
      ended: false,
    };
    connects.push(record);
    socket.on('end', () => {
      record.ended = true;
    });
    if (options.hangConnect) {
      return;
    }
    if (
      options.requiredConnectCredentials !== undefined &&
      req.headers['proxy-authorization'] !== options.requiredConnectCredentials
    ) {
      socket.write('HTTP/1.1 407 Proxy Authentication Required\r\n\r\n');
      socket.destroy();
      return;
    }
    socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
    socket.on('data', (chunk: Buffer) => {
      record.clientBytes.push(chunk);
      // The fixture is not a TLS origin; after observing the tunneled bytes
      // (the ClientHello) tear the tunnel down so the client settles fast.
      socket.destroy();
    });
    socket.on('error', () => {
      // Client-side TLS teardown after the observation above is expected.
    });
  });
  const sockets = trackSockets(server);
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const address = server.address() as net.AddressInfo;

  return {
    port: address.port,
    requests,
    connects,
    close: () =>
      new Promise<void>((resolve) => {
        for (const socket of sockets) {
          socket.destroy();
        }
        server.close(() => resolve());
      }),
  };
}

/** A closed local port: the direct route to this address is unavailable. */
async function closedLocalPort(): Promise<number> {
  const server = net.createServer();
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const address = server.address() as net.AddressInfo;
  await new Promise<void>((resolve) => {
    server.close(() => resolve());
  });
  return address.port;
}

/** One manual GET through the transport; resolves with the response body. */
function requestViaTransport(params: {
  url: string;
  proxySetting?: string | undefined;
  env?: NodeJS.ProcessEnv | undefined;
}): Promise<{ body: string; error?: Error }> {
  return new Promise((resolve, reject) => {
    let request: http.ClientRequest;
    const finish = (error: Error | undefined, body?: string): void => {
      if (error) {
        resolve({ body: '', error });
      } else {
        resolve({ body: body ?? '' });
      }
    };
    try {
      request = createManagedHttpRequest({
        isHttps: params.url.startsWith('https:'),
        url: params.url,
        options: { headers: { 'User-Agent': 'managed-http-transport-test' } },
        callback: (incoming) => {
          const chunks: Buffer[] = [];
          incoming.on('data', (chunk: Buffer) => chunks.push(chunk));
          incoming.once('error', (error) => finish(error));
          incoming.once('end', () => finish(undefined, Buffer.concat(chunks).toString('utf8')));
        },
        proxySources: {
          settingProxyUrl: params.proxySetting,
          env: params.env ?? {},
        },
      });
      request.once('error', (error) => finish(error));
    } catch (error) {
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

function makeCancellation(): { token: CancellationTokenLike; cancel(): void } {
  const listeners: Array<() => void> = [];
  return {
    token: {
      isCancellationRequested: false,
      onCancellationRequested: (listener) => {
        listeners.push(listener);
        return { dispose: () => undefined };
      },
    },
    cancel: () => {
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

async function waitFor(predicate: () => boolean, what: string): Promise<void> {
  const deadline = Date.now() + 4000;
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
  }
}

// ---------------------------------------------------------------------------
// Proxy resolution precedence
// ---------------------------------------------------------------------------
describe('resolveManagedProxy', () => {
  const target = new URL('http://managed-artifacts.example.com/path');

  test('resolves direct when nothing is configured', () => {
    expect(resolveManagedProxy(target, { env: {} })).toEqual({
      kind: 'direct',
      reason: 'no-proxy-configured',
    });
  });

  test('prefers the http.proxy setting over the environment', () => {
    const resolution = resolveManagedProxy(target, {
      settingProxyUrl: 'http://setting-proxy:3128',
      env: { HTTPS_PROXY: 'http://env-proxy:1080' },
    });
    expect(resolution.kind).toBe('proxy');
    if (resolution.kind === 'proxy') {
      expect(resolution.source).toBe('setting');
      expect(resolution.proxyUrl.host).toBe('setting-proxy:3128');
    }
  });

  test('falls back to the proxy environment with the VS Code variable order', () => {
    const resolution = resolveManagedProxy(target, {
      env: { http_proxy: 'http://lower-http:1', https_proxy: 'http://lower-https:2' },
    });
    expect(resolution.kind).toBe('proxy');
    if (resolution.kind === 'proxy') {
      expect(resolution.source).toBe('env');
      expect(resolution.proxyUrl.host).toBe('lower-https:2');
    }
    const onlyUpper = resolveManagedProxy(target, { env: { HTTP_PROXY: 'http://upper:3' } });
    expect(onlyUpper.kind).toBe('proxy');
    if (onlyUpper.kind === 'proxy') {
      expect(onlyUpper.proxyUrl.host).toBe('upper:3');
    }
  });

  test('http.noProxy entries exclude a matching target and suppress the env list', () => {
    const excluded = resolveManagedProxy(new URL('http://mirror.example.com:8080/a'), {
      settingNoProxy: ['example.com', 'other.test:443'],
      env: { HTTPS_PROXY: 'http://env-proxy:1080', NO_PROXY: 'nothing.relevant' },
    });
    expect(excluded).toEqual({ kind: 'direct', reason: 'no-proxy-excluded' });

    // Config entries present: the env no_proxy list is ignored entirely, so a
    // target excluded only by the env list still routes through the proxy.
    const envSuppressed = resolveManagedProxy(new URL('http://envonly.example.com/a'), {
      settingNoProxy: ['unrelated.test'],
      env: { HTTPS_PROXY: 'http://env-proxy:1080', NO_PROXY: 'envonly.example.com' },
    });
    expect(envSuppressed.kind).toBe('proxy');
  });

  test('port-scoped noProxy entries only exclude that port', () => {
    const excluded = resolveManagedProxy(new URL('http://mirror.example.com:8443/a'), {
      settingNoProxy: ['mirror.example.com:8443'],
      env: {},
    });
    expect(excluded).toEqual({ kind: 'direct', reason: 'no-proxy-excluded' });
    const kept = resolveManagedProxy(new URL('http://mirror.example.com:80/a'), {
      settingNoProxy: ['mirror.example.com:8443'],
      env: {},
    });
    expect(kept).toEqual({ kind: 'direct', reason: 'no-proxy-configured' });
  });

  test('NO_PROXY excludes when no config entries exist', () => {
    const excluded = resolveManagedProxy(new URL('https://Internal.Mirror.Example.COM/a'), {
      env: { NO_PROXY: '.internal.mirror.example.com' },
    });
    expect(excluded).toEqual({ kind: 'direct', reason: 'no-proxy-excluded' });
  });

  test('SOCKS and unparseable proxy configurations fail closed as unsupported', () => {
    const socks = resolveManagedProxy(target, {
      settingProxyUrl: 'socks5h://corporate-gateway:1080',
      env: {},
    });
    expect(socks.kind).toBe('unsupported');
    if (socks.kind === 'unsupported') {
      expect(socks.reason).toContain('SOCKS');
    }

    const unparseable = resolveManagedProxy(target, {
      settingProxyUrl: 'not-a-proxy-url',
      env: {},
    });
    expect(unparseable.kind).toBe('unsupported');
    if (unparseable.kind === 'unsupported') {
      expect(unparseable.reason).toContain('unparseable');
    }
  });

  test('redacts credentials in the resolved proxy URL', () => {
    const resolution = resolveManagedProxy(target, {
      settingProxyUrl: 'http://proxyuser:proxypass@gateway.corp:3128',
      env: {},
    });
    expect(resolution.kind).toBe('proxy');
    if (resolution.kind === 'proxy') {
      expect(resolution.redactedUrl).toBe('http://***@gateway.corp:3128/');
      expect(resolution.redactedUrl).not.toContain('proxypass');
    }
  });
});

// ---------------------------------------------------------------------------
// Credential redaction
// ---------------------------------------------------------------------------
describe('redactCredentialUrl', () => {
  test('replaces userinfo with *** and keeps scheme, host, and port', () => {
    expect(redactCredentialUrl('http://user:secret@proxy.corp:3128')).toBe(
      'http://***@proxy.corp:3128/',
    );
  });

  test('passes credential-free URLs through unchanged', () => {
    expect(redactCredentialUrl('https://proxy.corp:3128/route')).toBe(
      'https://proxy.corp:3128/route',
    );
  });

  test('strips userinfo from an unparseable string without throwing', () => {
    expect(redactCredentialUrl('http://user:secret@')).toBe('http://***@');
  });
});

// ---------------------------------------------------------------------------
// Proxied plain HTTP: traversal with the direct path unavailable
// ---------------------------------------------------------------------------
describe('createManagedHttpRequest over a proxy', () => {
  let proxy: ProxyFixture;
  let deadTargetPort: number;

  beforeEach(async () => {
    proxy = await startProxyFixture();
    deadTargetPort = await closedLocalPort();
  });

  afterEach(async () => {
    await proxy.close();
  });

  test('traverses the proxy while the direct path is unavailable', async () => {
    // Negative control: the target origin refuses direct connections.
    await expect(
      new Promise<void>((resolve, reject) => {
        const request = http.get({ host: '127.0.0.1', port: deadTargetPort, path: '/' }, () => {
          reject(new Error('direct connection unexpectedly succeeded'));
        });
        request.once('error', (error) => {
          expect((error as NodeJS.ErrnoException).code).toBe('ECONNREFUSED');
          resolve();
        });
      }),
    ).resolves.toBeUndefined();

    const { body } = await requestViaTransport({
      url: `http://127.0.0.1:${deadTargetPort}/asset.bin?x=1`,
      proxySetting: `http://127.0.0.1:${proxy.port}`,
    });

    expect(body).toBe(PROXIED_BODY);
    // The proxy received an absolute-form request target for the dead origin.
    expect(proxy.requests).toHaveLength(1);
    expect(proxy.requests[0]?.method).toBe('GET');
    expect(proxy.requests[0]?.url).toBe(`http://127.0.0.1:${deadTargetPort}/asset.bin?x=1`);
    expect(proxy.requests[0]?.headers.host).toBe(`127.0.0.1:${deadTargetPort}`);
  });

  test('sends Proxy-Authorization derived from proxy URL credentials', async () => {
    const { body } = await requestViaTransport({
      url: `http://127.0.0.1:${deadTargetPort}/asset.bin`,
      proxySetting: `http://proxyuser:proxypass@127.0.0.1:${proxy.port}`,
    });
    expect(body).toBe(PROXIED_BODY);
    expect(proxy.requests[0]?.headers['proxy-authorization']).toBe(
      `Basic ${Buffer.from('proxyuser:proxypass', 'utf8').toString('base64')}`,
    );
  });

  test('surfaces a proxy connect failure as a bounded network-class error', async () => {
    const unreachableProxyPort = await closedLocalPort();
    const { error } = await requestViaTransport({
      url: `http://127.0.0.1:${deadTargetPort}/asset.bin`,
      proxySetting: `http://proxyuser:proxypass@127.0.0.1:${unreachableProxyPort}`,
    });
    expect(error?.message).toMatch(/ECONNREFUSED/);
    expect(isNetworkErrorMessage(error?.message ?? '')).toBe(true);
    // The unreachable route carried credentials in its URL; they must not
    // reach the failure message.
    expect(error?.message).not.toContain('proxypass');
  });

  test('fails closed on an unsupported proxy configuration without connecting', async () => {
    await expect(
      requestViaTransport({
        url: `http://127.0.0.1:${deadTargetPort}/asset.bin`,
        proxySetting: 'socks5h://corporate-gateway:1080',
      }),
    ).rejects.toThrow(`${MANAGED_PROXY_ERROR_PREFIX} configuration is unsupported`);
    expect(proxy.requests).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// CONNECT tunnels for HTTPS targets
// ---------------------------------------------------------------------------
describe('createManagedHttpRequest CONNECT tunnels', () => {
  let proxy: ProxyFixture | undefined;

  async function restartProxyFixture(options: ProxyFixtureOptions = {}): Promise<void> {
    await proxy?.close();
    proxy = await startProxyFixture(options);
  }

  afterEach(async () => {
    await proxy?.close();
    proxy = undefined;
  });

  function httpsTunnelError(proxySetting: string): Promise<Error> {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('test deadline exceeded')), 4000);
      let request: http.ClientRequest;
      try {
        request = createManagedHttpRequest({
          isHttps: true,
          url: 'https://tls-origin.example.com/asset.bin',
          options: { headers: { 'User-Agent': 'managed-http-transport-test' } },
          callback: () => reject(new Error('unexpected response')),
          proxySources: { settingProxyUrl: proxySetting, env: {} },
        });
      } catch (error) {
        clearTimeout(timeout);
        reject(error instanceof Error ? error : new Error(String(error)));
        return;
      }
      request.once('error', (error) => {
        clearTimeout(timeout);
        resolve(error);
      });
    });
  }

  test('issues CONNECT with the target authority and attempts TLS over the tunnel', async () => {
    await restartProxyFixture();
    const error = await httpsTunnelError(`http://127.0.0.1:${proxy?.port}`);
    // The fixture is not a TLS origin, so the tunnel attempt is torn down
    // after the ClientHello; the CONNECT traversal itself is what this proves.
    expect(error.message).toMatch(
      /before secure tls connection|ssl|econnreset|handshake|socket hang up|premature close/i,
    );
    expect(proxy?.connects).toHaveLength(1);
    expect(proxy?.connects[0]?.authority).toBe('tls-origin.example.com:443');
    expect(proxy?.connects[0]?.proxyAuthorization).toBeUndefined();
    expect(proxy?.connects[0]?.clientBytes.length).toBeGreaterThan(0);
    // TLS handshake record type 22 (0x16) — the tunnel carries a TLS attempt.
    expect(proxy?.connects[0]?.clientBytes[0]?.[0]).toBe(0x16);
  }, 10000);

  test('carries Proxy-Authorization on CONNECT from proxy URL credentials', async () => {
    const expectedToken = `Basic ${Buffer.from('proxyuser:proxypass', 'utf8').toString('base64')}`;
    await restartProxyFixture({ requiredConnectCredentials: expectedToken });

    const error = await httpsTunnelError(`http://proxyuser:proxypass@127.0.0.1:${proxy?.port}`);
    expect(error.message).toMatch(
      /before secure tls connection|ssl|econnreset|handshake|socket hang up|premature close/i,
    );
    expect(proxy?.connects[0]?.proxyAuthorization).toBe(expectedToken);
  }, 10000);

  test('classifies a denied CONNECT (HTTP 407) as a bounded network failure without leaking credentials', async () => {
    await restartProxyFixture({ requiredConnectCredentials: 'Basic expected-token' });

    const error = await httpsTunnelError(`http://proxyuser:proxypass@127.0.0.1:${proxy?.port}`);
    expect(error.message).toContain('connect refused: HTTP 407');
    expect(error.message).toContain('proxy credentials or access denied');
    expect(isNetworkErrorMessage(error.message)).toBe(true);
    expect(error.message).not.toContain('proxypass');
    expect(error.message).not.toContain('expected-token');
  }, 10000);

  test('destroys an in-flight CONNECT when the bounded deadline expires', async () => {
    await restartProxyFixture({ hangConnect: true });

    let settled: Error | undefined;
    const cancellation = makeCancellation();
    const pending = fetchBoundedJson<unknown>({
      requestFactory: (listener) =>
        createManagedHttpRequest({
          isHttps: true,
          url: 'https://tls-origin.example.com/asset.bin',
          options: {},
          callback: listener,
          proxySources: { settingProxyUrl: `http://127.0.0.1:${proxy?.port}`, env: {} },
        }),
      timeoutMs: 250,
      maxBytes: 1024,
      cancellationToken: cancellation.token,
      operationName: 'Release fetch',
    }).then(
      () => undefined,
      (error: unknown) => {
        settled = error instanceof Error ? error : new Error(String(error));
      },
    );

    await waitFor(() => proxy?.connects.length === 1, 'the CONNECT to reach the fixture');
    cancellation.cancel();
    await pending;
    expect(settled?.message).toMatch(/cancelled|timeout/);
    // The tunnel teardown contract: destroying the bounded request must not
    // leave the in-flight CONNECT connection open (peer observes FIN/close).
    await waitFor(
      () => proxy?.connects[0]?.socket.destroyed === true || proxy?.connects[0]?.ended === true,
      'the CONNECT teardown',
    );
  }, 10000);

  test('destroys an in-flight CONNECT on cancellation', async () => {
    await restartProxyFixture({ hangConnect: true });

    let settled: Error | undefined;
    const cancellation = makeCancellation();
    const pending = fetchBoundedJson<unknown>({
      requestFactory: (listener) =>
        createManagedHttpRequest({
          isHttps: true,
          url: 'https://tls-origin.example.com/asset.bin',
          options: {},
          callback: listener,
          proxySources: { settingProxyUrl: `http://127.0.0.1:${proxy?.port}`, env: {} },
        }),
      timeoutMs: 3000,
      maxBytes: 1024,
      cancellationToken: cancellation.token,
      operationName: 'Release fetch',
    }).then(
      () => undefined,
      (error: unknown) => {
        settled = error instanceof Error ? error : new Error(String(error));
      },
    );

    await waitFor(() => proxy?.connects.length === 1, 'the CONNECT to reach the fixture');
    cancellation.cancel();
    await pending;
    expect(settled?.message).toContain('cancelled');
    await waitFor(
      () => proxy?.connects[0]?.socket.destroyed === true || proxy?.connects[0]?.ended === true,
      'the CONNECT teardown',
    );
  }, 10000);
});

// ---------------------------------------------------------------------------
// Bounded JSON bounds still apply on the proxied route
// ---------------------------------------------------------------------------
describe('fetchBoundedJson over the proxied transport', () => {
  let proxy: ProxyFixture | undefined;
  let deadTargetPort: number;

  beforeEach(async () => {
    deadTargetPort = await closedLocalPort();
  });

  afterEach(async () => {
    await proxy?.close();
    proxy = undefined;
  });

  function proxiedJsonFetch(proxyOptions: ProxyFixtureOptions, maxBytes = 1024): Promise<unknown> {
    return (async () => {
      proxy = await startProxyFixture(proxyOptions);
      return fetchBoundedJson<unknown>({
        requestFactory: (listener) =>
          createManagedHttpRequest({
            isHttps: false,
            url: `http://127.0.0.1:${deadTargetPort}/release.json`,
            options: { headers: { 'User-Agent': 'managed-http-transport-test' } },
            callback: listener,
            proxySources: { settingProxyUrl: `http://127.0.0.1:${proxy?.port}`, env: {} },
          }),
        timeoutMs: 4000,
        maxBytes,
        operationName: 'Release fetch',
      });
    })();
  }

  test('decodes a successful bounded JSON document through the proxy', async () => {
    await expect(proxiedJsonFetch({ body: '{"tag_name":"v1.2.3"}' })).resolves.toEqual({
      tag_name: 'v1.2.3',
    });
    expect(proxy?.requests).toHaveLength(1);
  }, 10000);

  test('rejects a 200 HTML error page where JSON is expected', async () => {
    await expect(
      proxiedJsonFetch({
        body: '<!doctype html><html><body>blocked</body></html>',
        contentType: 'text/html',
      }),
    ).rejects.toThrow(/invalid JSON/);
  }, 10000);

  test('enforces the byte envelope through the proxy', async () => {
    await expect(
      proxiedJsonFetch({ body: JSON.stringify({ big: 'x'.repeat(4096) }) }, 1024),
    ).rejects.toThrow(/exceeded 1024 bytes/);
  }, 10000);

  test('rejects a non-success status through the proxy', async () => {
    await expect(proxiedJsonFetch({ status: 404, body: 'missing' })).rejects.toThrow(/HTTP 404/);
  }, 10000);
});

// ---------------------------------------------------------------------------
// Negative control: the transport never weakens certificate validation
// ---------------------------------------------------------------------------
describe('transport negative controls', () => {
  test('no rejectUnauthorized:false literal and no TLS env bypass in managed transport sources', () => {
    const sources = ['src/managedHttpTransport.ts', 'src/downloader.ts'].map((relative) =>
      fs.readFileSync(path.join(__dirname, '..', '..', relative), 'utf8'),
    );
    for (const source of sources) {
      expect(source).not.toMatch(/rejectUnauthorized\s*:\s*false/);
      expect(source).not.toContain('NODE_TLS_REJECT_UNAUTHORIZED');
    }
  });
});
