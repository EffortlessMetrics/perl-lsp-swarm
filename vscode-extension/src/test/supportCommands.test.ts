import * as vscode from 'vscode';
import {
  buildBasicSupportPacket,
  formatIssueDiagnosticInfo,
  reportIssueCommand,
} from '../supportCommands';
import { formatSupportPacketHuman, validateSupportPacket } from '../supportPacket';

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

  test('keeps every interpolated legacy diagnostic field on one bounded printable line', () => {
    const packet = formatIssueDiagnosticInfo({
      serverVersion: 'perllsp 0.17.0\u2028Extension: forged',
      extensionVersion: '0.17.0\nProduct: forged',
      editorVersion: '1.128.1\rPlatform: forged',
      platform: 'win32\nArch: forged',
      arch: 'x64\u0000oops',
      editorName: 'Visual\u001b[31m Studio Code\u2029Server: forged',
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

  test('truncates oversized legacy diagnostic fields to the bounded one-line form', () => {
    const packet = formatIssueDiagnosticInfo({
      serverVersion: 'perllsp 0.17.0',
      extensionVersion: '0.17.0',
      editorVersion: '1.128.1',
      platform: 'win32',
      arch: 'x64',
      editorName: 'x'.repeat(300),
    });

    expect(packet.split('\n')).toHaveLength(5);
    expect(packet).toContain(`${'x'.repeat(197)}...: 1.128.1`);
  });

  test('builds a valid basic packet without inventing unavailable live state', () => {
    const packet = buildBasicSupportPacket({
      serverVersion: 'perllsp 0.17.0',
      extensionVersion: '0.17.0',
      editorVersion: '1.128.1',
      platform: 'win32',
      arch: 'x64',
      editorName: 'Visual Studio Code',
    });

    expect(validateSupportPacket(packet)).toEqual([]);
    expect(packet.perllsp.version).toEqual(
      expect.objectContaining({ state: 'known', value: '0.17.0' }),
    );
    expect(packet.extension.artifact_digest.state).toBe('not_proven');
    expect(packet.lifecycle.generation.state).toBe('not_proven');
    expect(packet.configuration.user_present.state).toBe('not_proven');
    expect(packet.product.version.state).toBe('not_proven');
  });

  test('marks a non-perllsp compatibility binary as action required without copying its path', () => {
    const packet = buildBasicSupportPacket({
      serverVersion: 'perl-lsp 0.17.0',
      extensionVersion: '0.17.0',
      editorVersion: '1.128.1',
      platform: 'linux',
      arch: 'x64',
    });

    expect(packet.perllsp).toMatchObject({
      state: 'known',
      role: 'ambient',
      compatibility: 'action_required',
    });
    expect(packet.perllsp.version.state).toBe('unknown');
  });

  test('opens the issue form without embedding the packet in the URL', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Open Issue Form');

    await reportIssueCommand(deps);

    expect(deps.getServerVersion).toHaveBeenCalledTimes(1);
    expect(vscode.env.openExternal).toHaveBeenCalledWith(
      expect.objectContaining({
        toString: expect.any(Function),
      }),
    );
    const url = (vscode.env.openExternal as jest.Mock).mock.calls[0]?.[0].toString();
    expect(url).toContain('https://github.com/EffortlessMetrics/perl-lsp/issues/new');
    expect(url).not.toContain('Support%20packet');
    expect(vscode.env.clipboard.writeText).not.toHaveBeenCalled();
  });

  test('copies the typed support packet and then opens the issue form', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Copy Support Packet');

    await reportIssueCommand(deps);

    const expectedPacket = buildBasicSupportPacket({
      serverVersion: 'perllsp 0.17.0',
      extensionVersion: '0.17.0',
      editorVersion: '1.128.1',
      platform: 'win32',
      arch: 'x64',
      editorName: 'Visual Studio Code',
    });
    expect(vscode.env.clipboard.writeText).toHaveBeenCalledWith(
      formatSupportPacketHuman(expectedPacket),
    );
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });

  test('continues to the issue form when clipboard access fails', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Copy Support Packet');
    (vscode.env.clipboard.writeText as jest.Mock).mockRejectedValueOnce(
      new Error('clipboard unavailable'),
    );

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });

  test('server-version probe failure degrades to bounded missing evidence', async () => {
    const deps = dependencies();
    deps.getServerVersion.mockRejectedValueOnce(new Error('probe failed'));
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce('Copy Support Packet');

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();
    const copied = (vscode.env.clipboard.writeText as jest.Mock).mock.calls[0]?.[0] as string;
    expect(copied).toContain('perllsp: unknown known_absent missing');
    expect(copied).not.toContain('probe failed');
  });
});
