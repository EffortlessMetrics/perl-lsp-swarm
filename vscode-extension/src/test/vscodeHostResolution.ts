import * as fs from 'fs';
import * as path from 'path';

export type HostResolutionDisposition = 'unavailable' | 'network' | 'cache' | 'runner' | 'unknown';

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
  if (message.includes('version') || message.includes('release') || message.includes('404')) {
    return 'unavailable';
  }
  return 'unknown';
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
  const receiptPath = path.join(receiptsRoot, 'vscode_host_resolution_failure.json');
  fs.writeFileSync(
    receiptPath,
    `${JSON.stringify(buildHostResolutionFailureReceipt(requestedVersion, error), null, 2)}\n`,
  );
  return receiptPath;
}
