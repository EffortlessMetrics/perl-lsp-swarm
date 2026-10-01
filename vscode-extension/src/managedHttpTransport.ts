import * as http from 'http';
import * as https from 'https';
import * as tls from 'tls';
import type { Duplex } from 'stream';

/**
 * One transport authority for managed release metadata and artifact requests
 * (#7804/#7851).
 *
 * The module owns exactly one decision set for every managed network request:
 *
 * - which proxy route applies, resolved once from the supported VS Code sources
 *   with the same precedence VS Code's own resolver documents
 *   (`http.noProxy`/`NO_PROXY` exclusions, then the `http.proxy` setting, then
 *   the `https_proxy`/`HTTPS_PROXY`/`http_proxy`/`HTTP_PROXY` environment —
 *   verified against microsoft/vscode `src/vs/platform/request/common/request.ts`
 *   and microsoft/vscode-proxy-agent `src/index.ts` at implementation time);
 * - how to traverse that route: absolute-form GET for plain HTTP targets and a
 *   CONNECT tunnel upgraded to TLS for HTTPS targets, preserving the caller's
 *   `rejectUnauthorized` policy (never weakened here, never set to false);
 * - how to fail: proxy dispositions surface as bounded, credential-free errors
 *   prefixed "Managed download proxy" so they classify as network failures;
 * - what may be logged: URLs carrying credentials are redacted before they can
 *   reach any output seam.
 *
 * In the VS Code extension host the patched `http`/`https` modules route
 * requests through VS Code's proxy agent according to `http.proxySupport`
 * (default `override`). Verified patch behavior (`createHttpPatch`): an
 * explicitly provided agent is honored for localhost targets and when support
 * is `off`, and replaced by VS Code's own agent otherwise — the same resolved
 * route either way, so an explicit agent never double-proxies. Plain Node
 * (unit tests, non-patched hosts) uses the explicit agent directly, which is
 * the environment where raw `http.get`/`https.get` ignored `http.proxy` (#7804).
 */

/** Stable error prefix so downloader's network classifier recognizes proxy dispositions. */
export const MANAGED_PROXY_ERROR_PREFIX = 'Managed download proxy';

const PROXY_ERROR_PREFIX = MANAGED_PROXY_ERROR_PREFIX;

const SUPPORTED_PROXY_PROTOCOLS = new Set(['http:', 'https:']);

/** Where a resolved proxy configuration came from. */
export type ProxySourceName = 'setting' | 'env';

/**
 * Injectable proxy configuration sources. Production callers derive these from
 * the VS Code `http.*` settings plus `process.env`; tests supply fixtures.
 */
export interface ManagedProxySources {
  /** The `http.proxy` setting. Empty/whitespace means unset. */
  readonly settingProxyUrl?: string | undefined;
  /** The `http.noProxy` setting entries (domain names, optional `:port`). */
  readonly settingNoProxy?: readonly string[] | undefined;
  /** Environment consulted after the setting; defaults to `process.env`. */
  readonly env?: NodeJS.ProcessEnv | undefined;
}

/**
 * One proxy resolution outcome for a target request.
 *
 * `unsupported` is fail-closed: an unparseable, non-HTTP(S), or SOCKS proxy
 * configuration refuses the request instead of silently bypassing the
 * corporate egress route.
 */
export type ManagedProxyResolution =
  | { readonly kind: 'direct'; readonly reason: 'no-proxy-configured' | 'no-proxy-excluded' }
  | {
      readonly kind: 'proxy';
      readonly source: ProxySourceName;
      readonly proxyUrl: URL;
      readonly redactedUrl: string;
    }
  | {
      readonly kind: 'unsupported';
      readonly source: ProxySourceName;
      readonly redactedUrl: string;
      readonly reason: string;
    };

/**
 * Redact `user:password` userinfo from a URL string, keeping scheme, host, and
 * port. Unparseable input only has the userinfo pattern removed. This is the
 * only form of a proxy URL allowed into errors or logs.
 */
export function redactCredentialUrl(value: string | URL): string {
  const raw = typeof value === 'string' ? value : value.toString();
  let parsed: URL | undefined;
  try {
    parsed = new URL(raw);
  } catch {
    parsed = undefined;
  }
  if (!parsed) {
    return raw.replace(/\/\/[^@/\s]+@/, '//***@');
  }
  if (!parsed.username && !parsed.password) {
    return parsed.toString();
  }
  const redacted = new URL(parsed.toString());
  redacted.username = '***';
  redacted.password = '';
  return redacted.toString();
}

/** Suffix/port no-proxy match used for both `http.noProxy` and `NO_PROXY`. */
function matchesNoProxy(targetUrl: URL, entries: readonly string[]): boolean {
  const hostname = targetUrl.hostname.toLowerCase();
  const port = targetUrl.port || (targetUrl.protocol === 'https:' ? '443' : '80');
  return entries.some((entryRaw) => {
    const entry = entryRaw.trim().toLowerCase();
    if (!entry) {
      return false;
    }
    // `[::1]:8080`-style IPv6 entries carry the host in brackets.
    const bracketed = /^\[(.+)\](?::(\d+))?$/.exec(entry);
    const entryHost = bracketed?.[1] ?? entry.split(':')[0];
    const entryPort = bracketed?.[2] ?? (bracketed ? undefined : entry.split(':')[1]);
    if (!entryHost) {
      return false;
    }
    if (entryPort !== undefined && entryPort !== port) {
      return false;
    }
    const bareHost = entryHost.startsWith('.') ? entryHost.slice(1) : entryHost;
    return hostname === bareHost || hostname.endsWith(`.${bareHost}`);
  });
}

function classifyProxyUrl(raw: string, source: ProxySourceName): ManagedProxyResolution {
  const patternRedacted = raw.replace(/\/\/[^@/\s]+@/, '//***@');
  let parsed: URL | undefined;
  try {
    parsed = new URL(raw);
  } catch {
    return {
      kind: 'unsupported',
      source,
      redactedUrl: patternRedacted,
      reason: 'unparseable proxy URL',
    };
  }
  if (!SUPPORTED_PROXY_PROTOCOLS.has(parsed.protocol)) {
    const reason = parsed.protocol.startsWith('socks')
      ? 'SOCKS proxies are not supported by the managed transport'
      : `unsupported proxy scheme ${parsed.protocol}`;
    return { kind: 'unsupported', source, redactedUrl: redactCredentialUrl(parsed), reason };
  }
  if (!parsed.hostname) {
    return {
      kind: 'unsupported',
      source,
      redactedUrl: redactCredentialUrl(parsed),
      reason: 'proxy URL has no host',
    };
  }
  return { kind: 'proxy', source, proxyUrl: parsed, redactedUrl: redactCredentialUrl(parsed) };
}

/**
 * Resolve the proxy route for one target, honoring the documented VS Code
 * precedence: `http.noProxy` config entries, then `NO_PROXY`/`no_proxy`, then
 * the `http.proxy` setting, then the proxy environment variables.
 */
export function resolveManagedProxy(
  targetUrl: URL,
  sources: ManagedProxySources,
): ManagedProxyResolution {
  const env = sources.env ?? process.env;
  const settingNoProxy = sources.settingNoProxy;
  if (settingNoProxy && settingNoProxy.length > 0) {
    // VS Code's resolver: config noProxy entries suppress the env list entirely.
    if (matchesNoProxy(targetUrl, settingNoProxy)) {
      return { kind: 'direct', reason: 'no-proxy-excluded' };
    }
  } else {
    const envNoProxyRaw = env.no_proxy ?? env.NO_PROXY;
    if (envNoProxyRaw && matchesNoProxy(targetUrl, envNoProxyRaw.split(','))) {
      return { kind: 'direct', reason: 'no-proxy-excluded' };
    }
  }

  const settingProxyUrl = (sources.settingProxyUrl ?? '').trim();
  if (settingProxyUrl) {
    return classifyProxyUrl(settingProxyUrl, 'setting');
  }

  // One combined environment lookup, matching VS Code's resolver: a target's
  // scheme does not select the variable, the first present one wins.
  const envProxyRaw = (
    env.https_proxy ||
    env.HTTPS_PROXY ||
    env.http_proxy ||
    env.HTTP_PROXY ||
    ''
  ).trim();
  if (envProxyRaw) {
    return classifyProxyUrl(envProxyRaw, 'env');
  }

  return { kind: 'direct', reason: 'no-proxy-configured' };
}

interface ResolvedProxy {
  readonly protocol: 'http:' | 'https:';
  readonly hostname: string;
  readonly port: number;
  /** Decoded `user:password` userinfo, or undefined when absent. */
  readonly credentials: string | undefined;
  readonly redactedUrl: string;
}

function safeDecodeURIComponent(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function parseResolvedProxy(proxyUrl: URL): ResolvedProxy {
  const credentials =
    proxyUrl.username || proxyUrl.password
      ? `${safeDecodeURIComponent(proxyUrl.username)}:${safeDecodeURIComponent(proxyUrl.password)}`
      : undefined;
  const defaultPort = proxyUrl.protocol === 'https:' ? 443 : 80;
  return {
    protocol: proxyUrl.protocol === 'https:' ? 'https:' : 'http:',
    hostname: proxyUrl.hostname,
    port: proxyUrl.port ? Number(proxyUrl.port) : defaultPort,
    credentials,
    redactedUrl: redactCredentialUrl(proxyUrl),
  };
}

function proxyAuthorizationHeader(proxy: ResolvedProxy): http.OutgoingHttpHeaders {
  if (!proxy.credentials) {
    return {};
  }
  return {
    'Proxy-Authorization': `Basic ${Buffer.from(proxy.credentials, 'utf8').toString('base64')}`,
  };
}

/**
 * Runtime connect options Node hands to `Agent#createConnection`. `servername`
 * and `rejectUnauthorized` ride along with the target request options but are
 * not part of the declared `http.RequestOptions` surface.
 */
interface AgentConnectOptions extends https.RequestOptions {
  readonly servername?: string | undefined;
}

/**
 * HTTPS targets through an HTTP(S) proxy: one CONNECT tunnel per request,
 * upgraded to TLS with the caller's certificate policy unchanged.
 *
 * A fresh agent is built per request, so `abortPendingConnect` tears down the
 * in-flight CONNECT when the bounded transport destroys the outer request
 * (timeout/cancellation) — no orphaned tunnel outlives its bounded owner.
 */
class ConnectTunnelAgent extends https.Agent {
  private pendingConnect: http.ClientRequest | undefined;

  constructor(
    private readonly proxy: ResolvedProxy,
    private readonly fallbackRejectUnauthorized: boolean,
  ) {
    super({ keepAlive: false, rejectUnauthorized: fallbackRejectUnauthorized });
  }

  abortPendingConnect(): void {
    this.pendingConnect?.destroy();
    this.pendingConnect = undefined;
  }

  override createConnection(
    options: https.RequestOptions,
    callback?: (err: Error | null, stream: Duplex) => void,
  ): Duplex | null | undefined {
    const connectOptions = options as AgentConnectOptions;
    const targetHost = connectOptions.host ?? '';
    const targetPort = Number(connectOptions.port ?? 443);
    if (!targetHost || !callback) {
      callback?.(new Error(`${PROXY_ERROR_PREFIX} CONNECT requires a target host`), null as never);
      return undefined;
    }

    let settled = false;
    // Node's declared callback shape carries a required `stream` even on the
    // error path, where callers ignore it; keep the declared signature and
    // adapt here.
    const settle = (err: Error | null, stream: Duplex | null): void => {
      if (settled) {
        return;
      }
      settled = true;
      callback(err, stream as Duplex);
    };

    const connectRequest = http.request({
      protocol: this.proxy.protocol,
      hostname: this.proxy.hostname,
      port: this.proxy.port,
      method: 'CONNECT',
      path: `${targetHost}:${targetPort}`,
      headers: {
        host: `${targetHost}:${targetPort}`,
        ...proxyAuthorizationHeader(this.proxy),
      },
      // One-shot tunnel setup: never pool through another agent.
      agent: false,
    });
    this.pendingConnect = connectRequest;

    connectRequest.once('connect', (res, socket, head) => {
      this.pendingConnect = undefined;
      const status = res.statusCode ?? 0;
      if (status !== 200) {
        socket.destroy();
        const denied = status === 407 ? ' (proxy credentials or access denied)' : '';
        settle(
          new Error(
            `${PROXY_ERROR_PREFIX} connect refused: HTTP ${status}${denied} for ${this.proxy.redactedUrl}`,
          ),
          null,
        );
        return;
      }
      if (head.length > 0) {
        socket.unshift(head);
      }
      try {
        const tlsOptions: tls.ConnectionOptions = {
          socket,
          host: targetHost,
          port: targetPort,
        };
        if (connectOptions.servername !== undefined) {
          tlsOptions.servername = connectOptions.servername;
        }
        tlsOptions.rejectUnauthorized =
          connectOptions.rejectUnauthorized ?? this.fallbackRejectUnauthorized;
        if (connectOptions.ca !== undefined) {
          tlsOptions.ca = connectOptions.ca;
        }
        settle(null, tls.connect(tlsOptions));
      } catch (error) {
        socket.destroy();
        settle(
          new Error(
            `${PROXY_ERROR_PREFIX} tunnel TLS setup failed: ${
              error instanceof Error ? error.message : String(error)
            }`,
          ),
          null,
        );
      }
    });

    connectRequest.once('error', (error) => {
      this.pendingConnect = undefined;
      settle(
        new Error(
          `${PROXY_ERROR_PREFIX} connect failed: ${error.message} (${this.proxy.redactedUrl})`,
        ),
        null,
      );
    });

    connectRequest.end();
    return undefined;
  }
}

/**
 * Route one managed GET through the resolved proxy, or fall back to the direct
 * Node transport when no proxy applies. Signature-compatible with the raw
 * `http.get`/`https.get` seam it replaces.
 */
export function createManagedHttpRequest(params: {
  readonly isHttps: boolean;
  readonly url: string;
  readonly options: https.RequestOptions;
  readonly callback: (response: http.IncomingMessage) => void;
  readonly proxySources?: ManagedProxySources | undefined;
}): http.ClientRequest {
  const { isHttps, url, options, callback } = params;
  const sources = params.proxySources ?? {};
  const targetUrl = new URL(url);
  const resolution = resolveManagedProxy(targetUrl, sources);

  if (resolution.kind === 'unsupported') {
    throw new Error(
      `${PROXY_ERROR_PREFIX} configuration is unsupported: ${resolution.reason} (${resolution.redactedUrl})`,
    );
  }
  if (resolution.kind === 'proxy') {
    if (targetUrl.protocol === 'https:') {
      return httpsOverProxyRequest(resolution.proxyUrl, url, options, callback);
    }
    return httpOverProxyRequest(resolution.proxyUrl, targetUrl, options, callback);
  }
  return isHttps ? https.get(url, options, callback) : http.get(url, options, callback);
}

/**
 * Plain HTTP target through a proxy: one absolute-form request to the proxy
 * (RFC 7230 §5.3.2). The Host header stays the target's; the proxy forwards.
 */
function httpOverProxyRequest(
  proxyUrl: URL,
  targetUrl: URL,
  options: https.RequestOptions,
  callback: (response: http.IncomingMessage) => void,
): http.ClientRequest {
  const proxy = parseResolvedProxy(proxyUrl);
  const request = http.request(
    {
      protocol: proxy.protocol,
      hostname: proxy.hostname,
      port: proxy.port,
      method: 'GET',
      path: `${targetUrl.protocol}//${targetUrl.host}${targetUrl.pathname}${targetUrl.search}`,
      headers: {
        ...options.headers,
        host: targetUrl.host,
        ...proxyAuthorizationHeader(proxy),
      },
      // One-shot proxied request: never pool through another agent.
      agent: false,
    },
    callback,
  );
  request.end();
  return request;
}

function httpsOverProxyRequest(
  proxyUrl: URL,
  url: string,
  options: https.RequestOptions,
  callback: (response: http.IncomingMessage) => void,
): http.ClientRequest {
  const proxy = parseResolvedProxy(proxyUrl);
  const agent = new ConnectTunnelAgent(proxy, options.rejectUnauthorized ?? true);
  const request = https.get(url, { ...options, agent }, callback);
  // The bounded transports settle timeout/cancellation with
  // `request.destroy()`; that must also tear down the in-flight CONNECT,
  // which Node does not associate with the queued request.
  const originalDestroy = request.destroy.bind(request);
  (request as { destroy: (error?: Error) => http.ClientRequest }).destroy = (error?: Error) => {
    agent.abortPendingConnect();
    return originalDestroy(error);
  };
  return request;
}
