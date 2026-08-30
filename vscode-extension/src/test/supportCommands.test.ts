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
    // Pin the canonical public repository and template, not just the host: #7862
    // requires Open Issue Form to target the canonical public issue form, and a
    // silent retarget to another repo or template would otherwise pass unnoticed.
    expect(url).toBe(
      'https://github.com/EffortlessMetrics/perl-lsp/issues/new?template=bug_report.yml',
    );
    expect(url).not.toContain('Support%20packet');
    expect(vscode.env.clipboard.writeText).not.toHaveBeenCalled();
  });

  test('copies the typed support packet without opening anything automatically', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Support Packet',
    );

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
    expect(vscode.env.openExternal).not.toHaveBeenCalled();
    expect(vscode.workspace.openTextDocument).not.toHaveBeenCalled();
    // Copy is now a dead end unless the user is told what to do next, so pin the
    // confirmation itself: deleting it must not leave this test green.
    expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
      expect.stringContaining('Support packet copied'),
    );
  });

  test('shows the packet in a native inspectable document without copying or opening it', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Show Support Packet',
    );

    await reportIssueCommand(deps);

    const expectedPacket = formatSupportPacketHuman(
      buildBasicSupportPacket({
        serverVersion: 'perllsp 0.17.0',
        extensionVersion: '0.17.0',
        editorVersion: '1.128.1',
        platform: 'win32',
        arch: 'x64',
        editorName: 'Visual Studio Code',
      }),
    );
    expect(vscode.workspace.openTextDocument).toHaveBeenCalledWith(
      expect.objectContaining({ content: expectedPacket }),
    );
    // Pin that the document actually opened is the one just created, and that it
    // opens as a preview tab — a call count alone would survive showing anything.
    const opened = await (vscode.workspace.openTextDocument as jest.Mock).mock.results[0]?.value;
    expect(vscode.window.showTextDocument).toHaveBeenCalledWith(opened, { preview: true });
    expect(vscode.env.clipboard.writeText).not.toHaveBeenCalled();
    expect(vscode.env.openExternal).not.toHaveBeenCalled();
  });

  test('offers bounded recovery when clipboard access fails, without auto-opening the browser', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Support Packet',
    );
    (vscode.env.clipboard.writeText as jest.Mock).mockRejectedValueOnce(
      new Error('clipboard unavailable'),
    );

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();

    expect(vscode.window.showWarningMessage).toHaveBeenCalledTimes(1);
    const [warning] = (vscode.window.showWarningMessage as jest.Mock).mock.calls[0] as [string];
    expect(warning).toContain('clipboard');
    expect(warning).not.toContain('clipboard unavailable');
    expect(vscode.env.openExternal).not.toHaveBeenCalled();
  });

  test('clipboard failure does not prevent reaching the issue form', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Support Packet',
    );
    (vscode.env.clipboard.writeText as jest.Mock).mockRejectedValueOnce(
      new Error('clipboard unavailable'),
    );
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce('Open Issue Form');

    await reportIssueCommand(deps);

    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });

  test('clipboard-failure recovery can show the packet instead of opening the browser', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Support Packet',
    );
    (vscode.env.clipboard.writeText as jest.Mock).mockRejectedValueOnce(
      new Error('clipboard unavailable'),
    );
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce('Show Support Packet');

    await reportIssueCommand(deps);

    const expectedPacket = formatSupportPacketHuman(
      buildBasicSupportPacket({
        serverVersion: 'perllsp 0.17.0',
        extensionVersion: '0.17.0',
        editorVersion: '1.128.1',
        platform: 'win32',
        arch: 'x64',
        editorName: 'Visual Studio Code',
      }),
    );
    expect(vscode.workspace.openTextDocument).toHaveBeenCalledWith(
      expect.objectContaining({ content: expectedPacket }),
    );
    // The whole point of this recovery branch is a non-browser way to reach the
    // packet: swapping it for the issue form would otherwise pass unnoticed.
    expect(vscode.env.openExternal).not.toHaveBeenCalled();
  });

  test('a failure opening the packet document stays bounded and still reaches the issue form', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Show Support Packet',
    );
    (vscode.workspace.openTextDocument as jest.Mock).mockRejectedValueOnce(
      new Error('editor host unavailable'),
    );
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce('Open Issue Form');

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();

    const [warning] = (vscode.window.showWarningMessage as jest.Mock).mock.calls[0] as [string];
    expect(warning).not.toContain('editor host unavailable');
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
  });

  test('dismissing the prompt copies, shows, and opens nothing', async () => {
    const deps = dependencies();
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(undefined);

    await reportIssueCommand(deps);

    expect(vscode.env.clipboard.writeText).not.toHaveBeenCalled();
    expect(vscode.env.openExternal).not.toHaveBeenCalled();
    expect(vscode.workspace.openTextDocument).not.toHaveBeenCalled();
  });

  test('packet generation failure still allows filing a report and leaks no raw error', async () => {
    const deps = dependencies();
    const packetModule = require('../supportPacket') as {
      formatSupportPacketHuman: (packet: unknown) => string;
    };
    const spy = jest.spyOn(packetModule, 'formatSupportPacketHuman').mockImplementation(() => {
      throw new Error('invalid support packet: forged/private/path');
    });
    (vscode.window.showWarningMessage as jest.Mock).mockResolvedValueOnce('Open Issue Form');

    try {
      await expect(reportIssueCommand(deps)).resolves.toBeUndefined();
    } finally {
      spy.mockRestore();
    }

    expect(vscode.window.showWarningMessage).toHaveBeenCalledTimes(1);
    const [warning] = (vscode.window.showWarningMessage as jest.Mock).mock.calls[0] as [string];
    expect(warning).not.toContain('forged/private/path');
    expect(vscode.env.openExternal).toHaveBeenCalledTimes(1);
    expect(vscode.env.clipboard.writeText).not.toHaveBeenCalled();
  });

  test('server-version probe failure degrades to bounded missing evidence', async () => {
    const deps = dependencies();
    deps.getServerVersion.mockRejectedValueOnce(new Error('probe failed'));
    (vscode.window.showInformationMessage as jest.Mock).mockResolvedValueOnce(
      'Copy Support Packet',
    );

    await expect(reportIssueCommand(deps)).resolves.toBeUndefined();
    const copied = (vscode.env.clipboard.writeText as jest.Mock).mock.calls[0]?.[0] as string;
    expect(copied).toContain('perllsp: unknown known_absent missing');
    expect(copied).not.toContain('probe failed');
  });
});
