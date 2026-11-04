import * as fs from 'fs';
import * as path from 'path';

export const HOST_RESOLUTION_FAILURE_RECEIPT_NAME = 'vscode_host_resolution_failure.json';

export type HostResolutionDisposition = 'unavailable' | 'network' | 'cache' | 'runner';

export interface HostResolutionFailureReceipt {
  schema_version: 1;
  outcome: 'blocked';
  stage: 'vscode_host_resolution';
  requested_version: string;
  platform: NodeJS.Platform;
  arch: string;
  disposition: HostResolutionDisposition;
  error: string;
}

export type VsCodeHostResolver = (options: { version: string }) => Promise<string>;

export interface ResolvedVsCodeHost {
  executablePath: string;
  requestedVersion: string;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function classifyHostResolutionError(error: unknown): HostResolutionDisposition {
  const code =
    typeof error === 'object' && error !== null && 'code' in error
      ? String((error as { code?: unknown }).code ?? '')
      : '';
  const message = `${errorText(error)} ${code}`.toLowerCase();
  if (message.includes('cache')) {
    return 'cache';
  }
  // Catalog misses must win over broad network tokens: `@vscode/test-electron`
  // reports `Invalid version <id>` while still showing "Resolving version...",
  // and archive/CDN misses often look like `Failed to download ... HTTP 404`.
  if (
    message.includes('404') ||
    message.includes('invalid version') ||
    message.includes('not found') ||
    message.includes('release')
  ) {
    return 'unavailable';
  }
  if (
    message.includes('network') ||
    message.includes('econn') ||
    message.includes('enotfound') ||
    message.includes('http') ||
    message.includes('timeout') ||
    message.includes('timed out') ||
    message.includes('failed to download')
  ) {
    return 'network';
  }
  if (message.includes('runner') || message.includes('spawn')) {
    return 'runner';
  }
  if (message.includes('version')) {
    return 'unavailable';
  }
  return 'runner';
}

export function buildHostResolutionFailureReceipt(
  requestedVersion: string,
  error: unknown,
): HostResolutionFailureReceipt {
  return {
    schema_version: 1,
    outcome: 'blocked',
    stage: 'vscode_host_resolution',
    requested_version: requestedVersion,
    platform: process.platform,
    arch: process.arch,
    disposition: classifyHostResolutionError(error),
    error: errorText(error),
  };
}

export function writeHostResolutionFailureReceipt(
  receiptsRoot: string,
  requestedVersion: string,
  error: unknown,
): string {
  fs.mkdirSync(receiptsRoot, { recursive: true });
  const receiptPath = path.join(receiptsRoot, HOST_RESOLUTION_FAILURE_RECEIPT_NAME);
  fs.writeFileSync(
    receiptPath,
    `${JSON.stringify(buildHostResolutionFailureReceipt(requestedVersion, error), null, 2)}\n`,
  );
  return receiptPath;
}

/**
 * Resolve a VS Code host through the injected downloader. On resolver
 * rejection, write the structured failure packet first, then rethrow the
 * original error. Never retries a different version (including stable).
 */
export async function downloadVsCodeHostOrWriteFailureReceipt(
  receiptsRoot: string,
  requestedVersion: string,
  resolver: VsCodeHostResolver,
): Promise<ResolvedVsCodeHost> {
  try {
    const executablePath = await resolver({ version: requestedVersion });
    return { executablePath, requestedVersion };
  } catch (error: unknown) {
    try {
      const receiptPath = writeHostResolutionFailureReceipt(receiptsRoot, requestedVersion, error);
      process.stderr.write(
        `VS Code host resolution blocked (${classifyHostResolutionError(error)}) for requested ${requestedVersion}; receipt ${receiptPath}\n`,
      );
    } catch (receiptError: unknown) {
      const detail = receiptError instanceof Error ? receiptError.message : String(receiptError);
      process.stderr.write(`Unable to write VS Code host-resolution receipt: ${detail}\n`);
    }
    throw error;
  }
}
