import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import {
  HOST_RESOLUTION_FAILURE_RECEIPT_NAME,
  buildHostResolutionFailureReceipt,
  classifyHostResolutionError,
  downloadVsCodeHostOrWriteFailureReceipt,
  writeHostResolutionFailureReceipt,
} from './vscodeHostResolution';

function tempReceiptRoot(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-host-resolution-test-'));
}

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

  test('classifies cache and runner failures separately from unavailable', () => {
    expect(
      classifyHostResolutionError(new Error('Could not read the VS Code download cache')),
    ).toBe('cache');
    const spawnError = Object.assign(new Error('spawn vscode ENOENT'), { code: 'ENOENT' });
    expect(classifyHostResolutionError(spawnError)).toBe('runner');
    expect(classifyHostResolutionError(new Error('unexpected resolver abort'))).toBe('runner');
  });

  test('treats HTTP 404 and Invalid version catalog misses as unavailable, not network', () => {
    expect(
      classifyHostResolutionError(
        new Error(
          'Failed to download vscode 1.125.0 from https://update.code.visualstudio.com/1.125.0/linux-x64/stable HTTP 404',
        ),
      ),
    ).toBe('unavailable');
    expect(classifyHostResolutionError(new Error('Invalid version 1.125.0'))).toBe('unavailable');
    expect(classifyHostResolutionError(new Error('network request failed with HTTP 503'))).toBe(
      'network',
    );
  });

  test('writes the resolver error and environment identity to a receipt', () => {
    const root = tempReceiptRoot();
    const receiptPath = writeHostResolutionFailureReceipt(
      root,
      '1.125.0',
      new Error('network request failed with HTTP 503'),
    );

    const receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8')) as Record<string, unknown>;
    expect(receiptPath).toBe(path.join(root, HOST_RESOLUTION_FAILURE_RECEIPT_NAME));
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

  test('mocked resolver rejection writes a receipt then rethrows without falling back to stable', async () => {
    const root = tempReceiptRoot();
    const resolver = jest.fn(async ({ version }: { version: string }) => {
      if (version === 'stable') {
        return '/fake/code-stable';
      }
      throw new Error(`VS Code release ${version} was not found`);
    });

    await expect(
      downloadVsCodeHostOrWriteFailureReceipt(root, '1.125.0', resolver),
    ).rejects.toThrow(/1\.125\.0 was not found/);

    expect(resolver).toHaveBeenCalledTimes(1);
    expect(resolver).toHaveBeenCalledWith({ version: '1.125.0' });
    expect(resolver.mock.calls.map(([argument]) => argument.version)).not.toContain('stable');

    const receipt = JSON.parse(
      fs.readFileSync(path.join(root, HOST_RESOLUTION_FAILURE_RECEIPT_NAME), 'utf8'),
    ) as Record<string, unknown>;
    expect(receipt).toMatchObject({
      outcome: 'blocked',
      stage: 'vscode_host_resolution',
      requested_version: '1.125.0',
      disposition: 'unavailable',
      error: 'VS Code release 1.125.0 was not found',
    });
    expect(receipt.requested_version).not.toBe('stable');
    expect(receipt.platform).toBe(process.platform);
    expect(receipt.arch).toBe(process.arch);
  });

  test('mocked resolver success preserves independent 1.125.0 and stable identities', async () => {
    const root = tempReceiptRoot();
    const resolver = jest.fn(async ({ version }: { version: string }) => `/fake/code-${version}`);

    await expect(
      downloadVsCodeHostOrWriteFailureReceipt(root, '1.125.0', resolver),
    ).resolves.toEqual({
      executablePath: '/fake/code-1.125.0',
      requestedVersion: '1.125.0',
    });
    await expect(
      downloadVsCodeHostOrWriteFailureReceipt(root, 'stable', resolver),
    ).resolves.toEqual({
      executablePath: '/fake/code-stable',
      requestedVersion: 'stable',
    });

    expect(fs.existsSync(path.join(root, HOST_RESOLUTION_FAILURE_RECEIPT_NAME))).toBe(false);
    expect(resolver.mock.calls.map(([argument]) => argument.version)).toEqual([
      '1.125.0',
      'stable',
    ]);
  });

  test('published and integration runners resolve hosts through the shared wrapper', () => {
    const srcRoot = path.resolve(__dirname, '..', '..', 'src', 'test');
    const sources = [
      fs.readFileSync(path.join(srcRoot, 'published/runPublishedSmoke.ts'), 'utf8'),
      fs.readFileSync(path.join(srcRoot, 'integration/runTest.ts'), 'utf8'),
    ];
    for (const source of sources) {
      expect(source).toContain('downloadVsCodeHostOrWriteFailureReceipt');
      expect(source).toMatch(
        /downloadVsCodeHostOrWriteFailureReceipt\([\s\S]*downloadAndUnzipVSCode/,
      );
      expect(source).not.toMatch(/await downloadAndUnzipVSCode\(\{[^}]*version:/);
    }
  });
});
