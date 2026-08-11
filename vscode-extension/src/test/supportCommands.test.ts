import * as vscode from 'vscode';
import { formatIssueDiagnosticInfo, reportIssueCommand } from '../supportCommands';

function dependencies() {
  return {
    getServerVersion: jest.fn(async () => 'perllsp 0.17.0'),
    extensionVersion: '0.17.0',
    editorVersion: '1.128.1',
    platform: 'win32',
    arch: 'x64',
    editorName: 'Visual Studio Code',
  };
}

describe('support command implementations', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test('formats product, executable, and extension identity as distinct fields', () => {
    expect(
      formatIssueDiagnosticInfo({
        serverVersion: 'perllsp 0.17.0',
        extensionVersion: '0.17.0',
        editorVersion: '1.128.1',
        platform: 'win32',
        arch: 'x64',
        editorName: 'Visual Studio Code',
      }),
    ).toBe(
      'Product: perl-lsp\n' +
        'Server: perllsp 0.17.0\n' +
        'Extension: EffortlessMetrics.perl-lsp-rs 0.17.0\n' +
        'Visual Studio Code: 1.128.1\n' +
        'Platform: win32/x64',
    );
  });

  test('preserves a compatibility binary instead of relabeling it as perllsp', () => {
    const packet = formatIssueDiagnosticInfo({
      serverVersion: 'perl-lsp 0.17.0',
      extensionVersion: '0.17.0',
      editorVersion: '1.128.1',
      platform: 'linux',
      arch: 'x64',
    });

    expect(packet).toContain('Server: perl-lsp 0.17.0 (expected perllsp)');
    expect(packet).not.toContain('Server: perllsp 0.17.0');
  });

  test('keeps an unavailable server attached to the expected executable identity', () => {
    expect(
      formatIssueDiagnosticInfo({
        serverVersion: 'unavailable',
        extensionVersion: '0.17.0',
        editorVersion: '1.128.1',
        platform: 'linux',
        arch: 'arm64',
      }),
    ).toContain('Server: perllsp unavailable');
  });

  test('keeps every interpolated field on one bounded printable line', () => {
    const packet = formatIssueDiagnosticInfo({
      serverVersion: 'perllsp 0.17.0\nExtension: forged',
      extensionVersion: '0.17.0\nProduct: forged',
      editorVersion: '1.128.1\rPlatform: forged',
      platform: 'win32\nArch: forged',
      arch: 'x64\u0000oops',
      editorName: 'Visual\u001b[31m Studio Code\nServer: forged',
    });

    expect(packet).toBe(
      'Product: perl-lsp\n' +
        'Server: perllsp 0.17.0\n' +
        'Extension: EffortlessMetrics.perl-lsp-rs 0.17.0\n' +
        'Visual\\u001b[31m Studio Code: 1.128.1\n' +
        'Platform: win32/x64\\u0000oops',
    );
    expect(packet).not.toContain('Extension: forged');
    expect(packet).not.toContain('Product: forged');
    expect(packet).not.toContain('Server: forged');
    expect(packet).not.toContain('Platform: forged');
    expect(packet).not.toContain('Arch: forged');
    expect(packet).not.toContain('\u001b');
    expect(packet).not.toContain('\u0000');
  });

  test('opens the issue form with current diagnostic context', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Open Issue Form');

    await reportIssueCommand(deps);

    expect(deps.getServerVersion).toHaveBeenCalledTimes(1);
    expect(vscode.env.openExternal).toHaveBeenCalledWith(
      expect.objectContaining({
        toString: expect.any(Function),
      }),
    );
    expect((vscode.env.openExternal as jest.Mock).mock.calls[0]?.[0].toString()).toContain(
      'https://github.com/EffortlessMetrics/perl-lsp/issues/new',
    );
  });

  test('copies canonical diagnostic context and then opens the issue form', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Diagnostic Info',
    );

    await reportIssueCommand(deps);

    expect(vscode.env.clipboard.writeText).toHaveBeenCalledWith(
      'Product: perl-lsp\n' +
        'Server: perllsp 0.17.0\n' +
        'Extension: EffortlessMetrics.perl-lsp-rs 0.17.0\n' +
        'Visual Studio Code: 1.128.1\n' +
        'Platform: win32/x64',
    );
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });

  test('continues to the issue form when clipboard access fails', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Diagnostic Info',
    );
    (vscode.env.clipboard.writeText as jest.Mock).mockRejectedValueOnce(
      new Error('clipboard unavailable'),
    );

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });
});
