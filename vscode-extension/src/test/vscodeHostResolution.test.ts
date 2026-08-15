import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import {
  buildHostResolutionFailureReceipt,
  classifyHostResolutionError,
  writeHostResolutionFailureReceipt,
} from './vscodeHostResolution';

describe('VS Code host resolution receipts', () => {
  test('classifies an unavailable requested release without fallback', () => {
    expect(classifyHostResolutionError(new Error('VS Code release 1.125.0 was not found'))).toBe(
      'unavailable',
    );
    expect(
      buildHostResolutionFailureReceipt('1.125.0', new Error('release not found')),
    ).toMatchObject({
      outcome: 'blocked',
      stage: 'vscode_host_resolution',
      requested_version: '1.125.0',
      disposition: 'unavailable',
    });
  });

  test('classifies DNS, timeout, and download failures as network blocks', () => {
    const dnsError = Object.assign(
      new Error('getaddrinfo ENOTFOUND update.code.visualstudio.com'),
      {
        code: 'ENOTFOUND',
      },
    );
    expect(classifyHostResolutionError(dnsError)).toBe('network');
    expect(classifyHostResolutionError(new Error('request timeout while resolving host'))).toBe(
      'network',
    );
    expect(classifyHostResolutionError(new Error('Failed to download and unzip VS Code'))).toBe(
      'network',
    );
  });

  test('writes the resolver error and environment identity to a receipt', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-host-resolution-test-'));
    const receiptPath = writeHostResolutionFailureReceipt(
      root,
      '1.125.0',
      new Error('network request failed with HTTP 503'),
    );

    const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8')) as Record<string, unknown>;
    expect(receipt).toMatchObject({
      schema_version: 1,
      outcome: 'blocked',
      stage: 'vscode_host_resolution',
      requested_version: '1.125.0',
      disposition: 'network',
      error: 'network request failed with HTTP 503',
    });
    expect(receipt.platform).toBe(process.platform);
    expect(receipt.arch).toBe(process.arch);
  });
});
