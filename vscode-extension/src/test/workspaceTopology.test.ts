import packageJson from '../../package.json';
import { describeWorkspaceTopology, WORKSPACE_CAPABILITY_MATRIX } from '../workspaceTopology';

function folder(scheme: string, fsPath?: string) {
  return { uri: { scheme, fsPath } };
}

function document(scheme: string, fsPath?: string) {
  return { uri: { scheme, fsPath } };
}

describe('workspace topology capability contract', () => {
  test('keeps manifest capability claims tied to the tested contract', () => {
    expect(packageJson.extensionKind).toEqual(['workspace']);
    expect(packageJson.capabilities?.untrustedWorkspaces).toEqual({ supported: false });
  });

  test('claims limited virtual-workspace support, matching the capability matrix', () => {
    // The matrix classifies file-backed operations as unsupported and language
    // server documents as no better than degraded in virtual mode, so a bare
    // `true` here would overclaim: VS Code reads `true` as full support and
    // suppresses the limitation banner users need.
    const virtualWorkspaces = packageJson.capabilities?.virtualWorkspaces;
    expect(virtualWorkspaces).not.toBe(true);
    expect(virtualWorkspaces).toMatchObject({ supported: 'limited' });
    expect((virtualWorkspaces as { description?: string } | undefined)?.description ?? '').toMatch(
      /file-backed folder/,
    );
    expect(WORKSPACE_CAPABILITY_MATRIX.virtual.file_backed_operations).toBe('unsupported');
    expect(WORKSPACE_CAPABILITY_MATRIX.virtual.language_server_documents).not.toBe('supported');
  });

  test('classifies trusted single-root and multi-root file workspaces', () => {
    const singleRoot = describeWorkspaceTopology({
      folders: [folder('file', '/workspace')],
      documents: [document('file', '/workspace/main.pl')],
      isTrusted: true,
    });
    const multiRoot = describeWorkspaceTopology({
      folders: [folder('file', '/workspace/a'), folder('file', '/workspace/b')],
      documents: [],
      isTrusted: true,
    });

    expect(singleRoot.mode).toBe('single-root');
    expect(singleRoot.capabilities).toEqual(WORKSPACE_CAPABILITY_MATRIX['single-root']);
    expect(multiRoot.mode).toBe('multi-root');
    expect(multiRoot.folder_count).toBe(2);
    expect(multiRoot.capabilities.workspace_root).toBe('supported');
  });

  test('marks untrusted workspaces without hiding their topology', () => {
    const topology = describeWorkspaceTopology({
      folders: [folder('file', '/workspace')],
      documents: [],
      isTrusted: false,
    });

    expect(topology.mode).toBe('single-root');
    expect(topology.trust).toBe('untrusted');
    expect(topology.folder_uri_schemes).toEqual(['file']);
  });

  test('classifies virtual and untitled documents with explicit limitations', () => {
    const virtual = describeWorkspaceTopology({
      folders: [folder('git', '')],
      documents: [document('git', '')],
      isTrusted: true,
    });
    const untitled = describeWorkspaceTopology({
      folders: [],
      documents: [document('untitled')],
      isTrusted: undefined,
    });

    expect(virtual.mode).toBe('virtual');
    expect(virtual.virtual_document_count).toBe(1);
    expect(virtual.capabilities.file_backed_operations).toBe('unsupported');
    expect(virtual.limitations.join(' ')).toMatch(/file-backed operations/);
    // No file-backed folder is open, so nothing reaches the language server.
    expect(virtual.file_backed_folder_count).toBe(0);
    expect(virtual.virtual_folder_count).toBe(1);
    expect(virtual.capabilities.language_server_documents).toBe('unsupported');
    expect(virtual.limitations.join(' ')).toMatch(/language server startup is deferred/);
    expect(untitled.mode).toBe('empty');
    expect(untitled.untitled_document_count).toBe(1);
    expect(untitled.trust).toBe('unknown');
    expect(untitled.limitations.join(' ')).toMatch(/untitled documents/);
  });

  test('keeps mixed file-and-virtual workspaces served but degraded', () => {
    const mixed = describeWorkspaceTopology({
      folders: [folder('file', '/workspace'), folder('vscode-vfs', '')],
      documents: [document('file', '/workspace/main.pl'), document('vscode-vfs', '')],
      isTrusted: true,
    });

    expect(mixed.mode).toBe('virtual');
    expect(mixed.file_backed_folder_count).toBe(1);
    expect(mixed.virtual_folder_count).toBe(1);
    // The file-backed folder is genuinely served, so this must not be
    // 'unsupported' — but the virtual documents are not attached, so it must
    // not be 'supported' either.
    expect(mixed.capabilities.language_server_documents).toBe('degraded');
    expect(mixed.limitations.join(' ')).toMatch(/file and untitled schemes only/);
    expect(mixed.limitations.join(' ')).not.toMatch(/startup is deferred/);
  });

  test('identifies remote hosts without claiming local-host proof', () => {
    const topology = describeWorkspaceTopology({
      folders: [folder('file', '/remote/workspace')],
      documents: [],
      isTrusted: true,
      remoteName: 'ssh-remote',
    });

    expect(topology.host_kind).toBe('remote');
    expect(topology.remote_name).toBe('ssh-remote');
    expect(topology.limitations.join(' ')).toMatch(/remote-host receipt/);
  });
});
