import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { BinaryDownloader } from './downloader';

const SERVER_DEBUG_TEST_COMMAND = 'perl.debugTest';
export const VSCODE_DEBUG_TEST_COMMAND = 'perl-lsp.debugTest';
const DEBUGGING_GUIDE_URL =
  'https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/tutorials/DAP_USER_GUIDE.md';

export interface DebugTestLaunchTarget {
  label: string;
  program: string;
  args: string[];
}

// ---------------------------------------------------------------------------
// Debug configuration wizard helpers (exported for unit testing)
// ---------------------------------------------------------------------------

/** Template names available in the debug config wizard. */
export type DebugConfigTemplate =
  | 'launch-script'
  | 'attach-process'
  | 'remote-ssh'
  | 'external-peer'
  | 'all';

/**
 * Build the content of a `.vscode/launch.json` file for the given template.
 * Falls back to `launch-script` for unrecognised template names.
 */
export function buildLaunchJsonContent(template: DebugConfigTemplate | string): string {
  const launchScript = {
    type: 'perl',
    request: 'launch',
    name: 'Perl: Launch Script',
    program: '${workspaceFolder}/script.pl',
    stopOnEntry: true,
    args: [],
    perlPath: 'perl',
    includePaths: [],
  };

  const attachProcess = {
    type: 'perl',
    request: 'attach',
    name: 'Perl: Attach to Process',
    host: 'localhost',
    port: 13603,
    timeout: 5000,
  };

  const remoteSSH = {
    type: 'perl',
    request: 'attach',
    name: 'Perl: Remote (SSH)',
    host: 'remote-host',
    port: 13603,
    timeout: 10000,
  };

  const externalPeer = {
    type: 'perl',
    request: 'attach',
    name: 'Perl: External Debugger Peer (experimental)',
    // This host path is proven against repository fake/reference peers. A stock
    // Devel::ptkdb build still needs the partner-side protocol patch and receipt.
    externalPeer: '127.0.0.1:13604',
  };

  let configurations: object[];

  switch (template) {
    case 'attach-process':
      configurations = [attachProcess];
      break;
    case 'remote-ssh':
      configurations = [remoteSSH];
      break;
    case 'external-peer':
      configurations = [externalPeer];
      break;
    case 'all':
      configurations = [launchScript, attachProcess, remoteSSH, externalPeer];
      break;
    case 'launch-script':
    default:
      configurations = [launchScript];
      break;
  }

  return JSON.stringify({ version: '0.2.0', configurations }, null, 4);
}

/**
 * Return true if a `.vscode/launch.json` exists under the given workspace root.
 */
export function hasLaunchJson(workspaceRoot: string): boolean {
  try {
    const launchPath = path.join(workspaceRoot, '.vscode', 'launch.json');
    return fs.existsSync(launchPath);
  } catch {
    return false;
  }
}

/**
 * Interactively create a `.vscode/launch.json` via a QuickPick wizard.
 * Called by the `perl-lsp.createDebugConfig` command.
 */
export async function createDebugConfigWizard(): Promise<void> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    vscode.window.showErrorMessage(
      'Open a workspace folder first before creating a debug configuration.',
    );
    return;
  }

  // If multiple workspace folders exist, let the user pick one.
  let workspaceRoot: string;
  if (workspaceFolders.length === 1) {
    const onlyFolder = workspaceFolders[0];
    if (!onlyFolder) {
      return;
    }
    workspaceRoot = onlyFolder.uri.fsPath;
  } else {
    interface WorkspaceFolderItem extends vscode.QuickPickItem {
      fsPath: string;
    }

    const items: WorkspaceFolderItem[] = workspaceFolders.map((f) => ({
      label: f.name,
      description: f.uri.fsPath,
      fsPath: f.uri.fsPath,
    }));

    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: 'Select workspace folder',
    });
    if (!picked) {
      return;
    }
    workspaceRoot = picked.fsPath;
  }

  const launchPath = path.join(workspaceRoot, '.vscode', 'launch.json');

  if (hasLaunchJson(workspaceRoot)) {
    const choice = await vscode.window.showWarningMessage(
      'A `.vscode/launch.json` already exists. Overwrite it?',
      'Overwrite',
      'Open Existing',
      'Cancel',
    );
    if (choice === 'Open Existing') {
      const doc = await vscode.workspace.openTextDocument(launchPath);
      await vscode.window.showTextDocument(doc);
      return;
    }
    if (choice !== 'Overwrite') {
      return;
    }
  }

  interface TemplateItem extends vscode.QuickPickItem {
    template: DebugConfigTemplate;
    label: string;
    description: string;
    detail: string;
  }

  const templateItems: TemplateItem[] = [
    {
      label: '$(play) Launch Script',
      description: 'Run the active Perl file under the debugger',
      detail: 'Adds a "Launch Script" configuration — the most common starting point.',
      template: 'launch-script',
    },
    {
      label: '$(plug) Attach to Process',
      description: 'Connect to a running Perl process over TCP',
      detail: 'Adds an "Attach" configuration targeting localhost:13603.',
      template: 'attach-process',
    },
    {
      label: '$(remote) Remote (SSH)',
      description: 'Attach to a remote Perl process via SSH tunnel',
      detail: 'Adds a remote attach configuration — edit the host to match your SSH target.',
      template: 'remote-ssh',
    },
    {
      label: '$(beaker) External Debugger Peer (experimental)',
      description: 'Connect to a peer implementation that already speaks the peer protocol',
      detail:
        'Developer preview. Host behavior is proven against repository peers; stock Devel::ptkdb compatibility is not yet proven.',
      template: 'external-peer',
    },
    {
      label: '$(list-flat) All Templates',
      description: 'Include native, attach, remote, and experimental peer configurations',
      detail:
        'Launch Script + Attach to Process + Remote (SSH) + External Debugger Peer (experimental).',
      template: 'all',
    },
  ];

  const selected = await vscode.window.showQuickPick(templateItems, {
    placeHolder: 'Choose a debug configuration template',
    title: 'Perl: Create Debug Configuration',
  });

  if (!selected) {
    return;
  }

  const content = buildLaunchJsonContent(selected.template);

  try {
    const vscodDir = path.join(workspaceRoot, '.vscode');
    if (!fs.existsSync(vscodDir)) {
      fs.mkdirSync(vscodDir, { recursive: true });
    }
    fs.writeFileSync(launchPath, content, 'utf8');
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    vscode.window.showErrorMessage(`Failed to write launch.json: ${message}`);
    return;
  }

  const friendlyName = selected.label.replace(/\$\([^)]+\)\s*/g, '');
  const choice = await vscode.window.showInformationMessage(
    `Created .vscode/launch.json with "${friendlyName}" template.`,
    'Open File',
  );

  if (choice === 'Open File') {
    const doc = await vscode.workspace.openTextDocument(launchPath);
    await vscode.window.showTextDocument(doc);
  }
}

/**
 * Check for missing debug configuration on first Perl file open and offer
 * a one-time prompt to create it.
 *
 * The prompt is only shown once per VS Code session (tracked via the
 * `_debugConfigPromptShown` module-level flag).
 */
let _debugConfigPromptShown = false;

export async function offerDebugConfigOnFirstPerlOpen(
  document: vscode.TextDocument,
): Promise<void> {
  if (_debugConfigPromptShown) {
    return;
  }
  if (document.languageId !== 'perl') {
    return;
  }

  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    return;
  }

  const firstFolder = workspaceFolders[0];
  if (!firstFolder) {
    return;
  }

  const workspaceRoot = firstFolder.uri.fsPath;
  if (hasLaunchJson(workspaceRoot)) {
    return;
  }

  _debugConfigPromptShown = true;

  const choice = await vscode.window.showInformationMessage(
    'No Perl debug configuration found. Would you like to set up debugging?',
    'Create Debug Config',
    'Not Now',
  );

  if (choice === 'Create Debug Config') {
    await createDebugConfigWizard();
  }
}

/** Reset the per-session prompt flag (used in tests). */
export function resetDebugConfigPromptFlag(): void {
  _debugConfigPromptShown = false;
}

const SERVER_RUN_TEST_COMMAND = 'perl.runTest';
export const VSCODE_RUN_TEST_COMMAND = 'perl-lsp.runTests';

export function rewriteTestLensCommand<T extends { command?: { command?: string } }>(lens: T): T {
  if (!lens.command) {
    return lens;
  }

  if (lens.command.command === SERVER_DEBUG_TEST_COMMAND) {
    return {
      ...lens,
      command: {
        ...lens.command,
        command: VSCODE_DEBUG_TEST_COMMAND,
      },
    };
  }

  if (lens.command.command === SERVER_RUN_TEST_COMMAND) {
    return {
      ...lens,
      command: {
        ...lens.command,
        command: VSCODE_RUN_TEST_COMMAND,
      },
    };
  }

  return lens;
}

export function parseDebugTestLaunchTarget(test: unknown): DebugTestLaunchTarget | undefined {
  if (typeof test === 'string') {
    return parseDebugTestId(test);
  }

  if (!test || typeof test !== 'object') {
    return undefined;
  }

  const candidate = test as {
    id?: unknown;
    label?: unknown;
    uri?: { fsPath?: unknown };
    args?: unknown;
  };

  if (typeof candidate.uri?.fsPath === 'string' && candidate.uri.fsPath.trim()) {
    return {
      label:
        typeof candidate.label === 'string' && candidate.label.trim()
          ? candidate.label
          : path.basename(candidate.uri.fsPath),
      program: candidate.uri.fsPath,
      args: normalizeDebugArgs(candidate.args),
    };
  }

  if (typeof candidate.id === 'string') {
    return parseDebugTestId(candidate.id);
  }

  return undefined;
}

function parseDebugTestId(testId: string): DebugTestLaunchTarget | undefined {
  const trimmed = testId.trim();
  if (!trimmed) {
    return undefined;
  }

  const splitIndex = trimmed.lastIndexOf('::');
  const fileOrUri = splitIndex >= 0 ? trimmed.slice(0, splitIndex) : trimmed;
  const label = splitIndex >= 0 ? trimmed.slice(splitIndex + 2) : path.basename(fileOrUri);
  const program = toFsPath(fileOrUri);

  if (!program) {
    return undefined;
  }

  return { label, program, args: [] };
}

function toFsPath(fileOrUri: string): string | undefined {
  const trimmed = fileOrUri.trim();
  if (!trimmed) {
    return undefined;
  }

  if (!trimmed.startsWith('file:')) {
    return trimmed;
  }

  try {
    const url = new URL(trimmed);
    const decodedPath = decodeURIComponent(url.pathname);
    if (process.platform === 'win32') {
      return decodedPath.replace(/^\/([A-Za-z]:)/, '$1').replace(/\//g, path.sep);
    }
    return decodedPath;
  } catch {
    const parsedUri = vscode.Uri.parse(trimmed);
    return typeof parsedUri.fsPath === 'string' && parsedUri.fsPath.trim()
      ? parsedUri.fsPath
      : undefined;
  }
}

function normalizeDebugArgs(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((entry): entry is string => typeof entry === 'string');
}

/** Regex for the shape of a `host:port` peer address (IPv4/hostname, not IPv6). */
const PEER_ADDR_RE = /^([^\s:]+):(\d+)$/;

/** A connectable TCP port: an integer in 1..=65535 (0 = "allocate" is not connectable). */
function isConnectablePort(port: unknown): port is number {
  return typeof port === 'number' && Number.isInteger(port) && port > 0 && port <= 65535;
}

function hasOwn(object: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(object, key);
}

/**
 * Return the reason an explicitly selected non-native debugger configuration
 * cannot be honored, or `undefined` when it is valid or native.
 *
 * The descriptor factory uses this before spawning `perl-dap`. An invalid or
 * unsupported external selection must stop the launch; it must not become an
 * empty argv list that silently starts the native debugger instead.
 */
export function externalDebuggerConfigurationError(
  config: vscode.DebugConfiguration | undefined,
): string | undefined {
  if (!config) {
    return undefined;
  }

  const backend = config.debuggerBackend;
  const hasFlatPeer = hasOwn(config, 'externalPeer');

  if (backend === 'ptkdb-bootstrap') {
    return (
      'The VS Code adapter does not yet wire debuggerBackend="ptkdb-bootstrap". ' +
      'Generate the bootstrap explicitly with perl-dap --ptkdb-bootstrap-rc.'
    );
  }

  if (backend !== undefined && !['native', 'external'].includes(String(backend))) {
    return `Unsupported debuggerBackend value: ${String(backend)}`;
  }

  if (hasFlatPeer && backend === 'native') {
    return 'externalPeer conflicts with debuggerBackend="native"';
  }

  const requestsExternal = hasFlatPeer || backend === 'external';
  if (!requestsExternal) {
    return undefined;
  }

  if (hasFlatPeer && backend === 'external') {
    return 'Use either externalPeer or debuggerBackend="external" with externalDebugger, not both';
  }

  if (hasFlatPeer) {
    if (typeof config.externalPeer !== 'string') {
      return 'externalPeer must be a HOST:PORT string';
    }
    const match = PEER_ADDR_RE.exec(config.externalPeer.trim());
    if (!match || !isConnectablePort(Number(match[2]))) {
      return 'externalPeer must use a hostname or IPv4 address and a port in 1..=65535';
    }
    return undefined;
  }

  if (
    !config.externalDebugger ||
    typeof config.externalDebugger !== 'object' ||
    Array.isArray(config.externalDebugger)
  ) {
    return 'debuggerBackend="external" requires an externalDebugger object';
  }

  const external = config.externalDebugger as {
    control?: unknown;
    host?: unknown;
    mode?: unknown;
    port?: unknown;
  };
  const mode = typeof external.mode === 'string' ? external.mode : 'connect';
  const control = typeof external.control === 'string' ? external.control : 'mirror';
  const host =
    typeof external.host === 'string' && external.host.trim() ? external.host.trim() : '127.0.0.1';

  if (host.includes(':') || /\s/.test(host)) {
    return 'externalDebugger.host must be a hostname or IPv4 address without whitespace';
  }
  if (control !== 'mirror') {
    return 'Only mirror control is currently behavior-backed for external debugger peers';
  }

  if (mode === 'connect') {
    if (!isConnectablePort(external.port)) {
      return 'externalDebugger connect mode requires a port in 1..=65535';
    }
    return undefined;
  }

  if (mode === 'listen') {
    if (external.port !== undefined && external.port !== 0 && !isConnectablePort(external.port)) {
      return 'externalDebugger listen mode requires port 0 or a port in 1..=65535';
    }
    return undefined;
  }

  if (mode === 'launchPeer') {
    return 'externalDebugger mode="launchPeer" is not implemented';
  }

  return `Unsupported externalDebugger mode: ${String(mode)}`;
}

/**
 * Resolve the external-peer `HOST:PORT` a valid debug config asks perl-dap to
 * connect to, or `undefined` when the config does not request connect mode.
 */
function resolveExternalPeerAddress(
  config: vscode.DebugConfiguration | undefined,
): string | undefined {
  if (!config) {
    return undefined;
  }

  const flat = config.externalPeer;
  if (typeof flat === 'string') {
    const match = PEER_ADDR_RE.exec(flat.trim());
    if (match && isConnectablePort(Number(match[2]))) {
      return `${match[1]}:${Number(match[2])}`;
    }
  }

  if (
    config.debuggerBackend === 'external' &&
    config.externalDebugger &&
    typeof config.externalDebugger === 'object'
  ) {
    const external = config.externalDebugger as { host?: unknown; port?: unknown; mode?: unknown };
    const mode = typeof external.mode === 'string' ? external.mode : 'connect';
    const host =
      typeof external.host === 'string' && external.host.trim()
        ? external.host.trim()
        : '127.0.0.1';
    if (
      mode === 'connect' &&
      isConnectablePort(external.port) &&
      !host.includes(':') &&
      !/\s/.test(host)
    ) {
      return `${host}:${external.port}`;
    }
  }

  return undefined;
}

/**
 * Resolve the `HOST[:PORT]` bind spec for a valid `mode: "listen"`
 * external-peer config, or `undefined` for another mode.
 */
function resolveExternalPeerListenBind(
  config: vscode.DebugConfiguration | undefined,
): string | undefined {
  if (
    !config ||
    config.debuggerBackend !== 'external' ||
    !config.externalDebugger ||
    typeof config.externalDebugger !== 'object'
  ) {
    return undefined;
  }
  const external = config.externalDebugger as { host?: unknown; port?: unknown; mode?: unknown };
  if (external.mode !== 'listen') {
    return undefined;
  }
  const host =
    typeof external.host === 'string' && external.host.trim() ? external.host.trim() : '127.0.0.1';
  if (host.includes(':') || /\s/.test(host)) {
    return undefined;
  }
  // Port 0 or an absent port asks perl-dap to allocate an ephemeral port.
  return isConnectablePort(external.port) ? `${host}:${external.port}` : host;
}

/**
 * Build the argv `perl-dap` is spawned with from a validated debug config.
 *
 * The descriptor factory calls [`externalDebuggerConfigurationError`] first.
 * This function intentionally remains a pure argv projection for unit tests and
 * callers that have already validated the explicit backend selection.
 */
export function buildDapExecutableArgs(config: vscode.DebugConfiguration | undefined): string[] {
  const peer = resolveExternalPeerAddress(config);
  if (peer) {
    return ['--external-peer', peer];
  }
  const listen = resolveExternalPeerListenBind(config);
  if (listen) {
    return ['--external-peer-listen', listen];
  }
  return [];
}

export class PerlDebugAdapterDescriptorFactory implements vscode.DebugAdapterDescriptorFactory {
  constructor(private readonly context: vscode.ExtensionContext) {}

  createDebugAdapterDescriptor(
    session: vscode.DebugSession,
    _executable: vscode.DebugAdapterExecutable | undefined,
  ): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
    const configurationError = externalDebuggerConfigurationError(session?.configuration);
    if (configurationError) {
      void vscode.window.showErrorMessage(
        `Perl debugger configuration error: ${configurationError} Native debugging was not started.`,
      );
      return undefined;
    }

    // Try to find perl-dap in PATH or use bundled version.
    const dapPath = this.findDebugAdapter();

    if (!dapPath) {
      vscode.window
        .showErrorMessage(
          'Perl Debug Adapter (perl-dap) not found. Debugging requires perl-dap. ' +
            'Use "Perl LSP: Reinstall" from the Command Palette to re-download it, ' +
            'or install it manually with: cargo install perl-dap.',
          'Reinstall',
          'Open Debugging Guide',
        )
        .then((sel) => {
          if (sel === 'Reinstall') {
            void vscode.commands.executeCommand('perl-lsp.reinstall');
          } else if (sel === 'Open Debugging Guide') {
            void vscode.env.openExternal(vscode.Uri.parse(DEBUGGING_GUIDE_URL));
          }
        });
      return undefined;
    }

    const args = buildDapExecutableArgs(session?.configuration);
    return new vscode.DebugAdapterExecutable(dapPath, args, {
      env: { ...process.env, RUST_LOG: 'debug' },
    });
  }

  private findDebugAdapter(): string | undefined {
    // First, check the auto-download directory (ships with perl-lsp)
    const downloadedDap = BinaryDownloader.getLocalDapPath(this.context);
    if (this.isExecutable(downloadedDap)) {
      return downloadedDap;
    }

    // Next, try to find perl-dap in PATH
    const pathDap = this.findExecutable('perl-dap');
    if (pathDap) {
      return pathDap;
    }

    // Otherwise, check common installation locations
    const binary = process.platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
    const possiblePaths: string[] = [
      path.join(process.env.HOME || '', '.cargo', 'bin', binary),
      path.join(process.env.CARGO_HOME || '', 'bin', binary),
    ];
    if (process.platform !== 'win32') {
      possiblePaths.push('/usr/local/bin/perl-dap', '/usr/bin/perl-dap');
    }

    for (const candidate of possiblePaths) {
      if (this.isExecutable(candidate)) {
        return candidate;
      }
    }

    return undefined;
  }

  private findExecutable(command: string): string | undefined {
    // If it's already an absolute path, check it
    if (path.isAbsolute(command)) {
      return this.isExecutable(command) ? command : undefined;
    }

    const pathEnv = process.env.PATH || '';
    const pathDirs = pathEnv.split(path.delimiter);

    // On Windows, we need to check extensions
    const isWindows = process.platform === 'win32';
    const extensions = isWindows
      ? process.env.PATHEXT
        ? process.env.PATHEXT.split(';')
        : ['.EXE', '.CMD', '.BAT', '.COM']
      : [''];

    for (const dir of pathDirs) {
      if (!dir) continue;

      for (const ext of extensions) {
        const fullPath = path.join(dir, command + ext);
        if (this.isExecutable(fullPath)) {
          return fullPath;
        }
      }
    }

    return undefined;
  }

  private isExecutable(filePath: string): boolean {
    try {
      const fsModule = require('fs');
      // Check if file exists and is a file
      const stats = fsModule.statSync(filePath);
      if (!stats.isFile()) return false;

      // On Windows, existence is enough (permissions are complex)
      // On Unix, check for execute permission
      if (process.platform !== 'win32') {
        fsModule.accessSync(filePath, fsModule.constants.X_OK);
      }
      return true;
    } catch {
      return false;
    }
  }
}

export class PerlDebugConfigurationProvider implements vscode.DebugConfigurationProvider {
  resolveDebugConfiguration(
    _folder: vscode.WorkspaceFolder | undefined,
    config: vscode.DebugConfiguration,
    _token?: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.DebugConfiguration> {
    // If launch.json is missing or empty
    if (!config.type && !config.request && !config.name) {
      const editor = vscode.window.activeTextEditor;
      if (editor && editor.document.languageId === 'perl') {
        config.type = 'perl';
        config.name = 'Launch Perl';
        config.request = 'launch';
        config.program = '${file}';
      }
    }

    if (config.request === 'attach') {
      // Attach supports either processId or host/port. External-peer fields are
      // validated separately by the descriptor factory.
      if (config.processId === undefined || config.processId === null) {
        if (!config.host) {
          config.host = 'localhost';
        }
        if (config.port === undefined || config.port === null) {
          config.port = 13603;
        }
      }
      return config;
    }

    if (!config.program) {
      return vscode.window.showInformationMessage('Cannot find a Perl file to debug').then(() => {
        return undefined;
      });
    }

    return config;
  }

  provideDebugConfigurations(
    _folder: vscode.WorkspaceFolder | undefined,
    _token?: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.DebugConfiguration[]> {
    return [
      {
        type: 'perl',
        request: 'launch',
        name: 'Launch Perl Script',
        program: '${file}',
        stopOnEntry: true,
        args: [],
      },
      {
        type: 'perl',
        request: 'launch',
        name: 'Launch Perl Test',
        program: '${file}',
        stopOnEntry: false,
        args: [],
        env: {
          PERL_TEST_HARNESS_DUMP_TAP: '1',
        },
      },
      {
        type: 'perl',
        request: 'attach',
        name: 'Attach by TCP',
        host: 'localhost',
        port: 13603,
        timeout: 5000,
      },
      {
        type: 'perl',
        request: 'attach',
        name: 'Attach by Process ID',
        processId: 12345,
      },
    ];
  }
}

export function activateDebugger(context: vscode.ExtensionContext) {
  // Register the debug adapter
  const provider = new PerlDebugConfigurationProvider();
  context.subscriptions.push(vscode.debug.registerDebugConfigurationProvider('perl', provider));

  const factory = new PerlDebugAdapterDescriptorFactory(context);
  context.subscriptions.push(vscode.debug.registerDebugAdapterDescriptorFactory('perl', factory));

  // Register debug commands
  context.subscriptions.push(
    vscode.commands.registerCommand(VSCODE_DEBUG_TEST_COMMAND, (test: unknown) => {
      const target = parseDebugTestLaunchTarget(test);
      if (!target) {
        void vscode.window.showErrorMessage(
          'Cannot debug this test: the test location could not be resolved. ' +
            'Save the file and try again, or launch the debugger manually via the Run and Debug panel.',
        );
        return undefined;
      }

      const config: vscode.DebugConfiguration = {
        type: 'perl',
        name: `Debug ${target.label}`,
        request: 'launch',
        program: target.program,
        stopOnEntry: false,
        args: target.args,
      };

      return vscode.debug.startDebugging(undefined, config);
    }),
  );

  // Register the debug configuration wizard command
  context.subscriptions.push(
    vscode.commands.registerCommand('perl-lsp.createDebugConfig', () => {
      return createDebugConfigWizard();
    }),
  );
}
