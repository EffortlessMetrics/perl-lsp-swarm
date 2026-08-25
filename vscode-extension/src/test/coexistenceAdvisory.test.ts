import * as vscode from 'vscode';
import { coexistenceConflictKey } from '../coexistenceRegistry';
import {
  collectCoexistenceFindings,
  renderCoexistenceStatusReport,
  runCoexistenceAdvisory,
} from '../coexistenceAdvisory';

const workspaceMock = vscode.workspace as unknown as {
  workspaceFolders: unknown;
  getConfiguration: jest.Mock;
  findFiles: jest.Mock;
  openTextDocument: jest.Mock;
};
const extensionsMock = vscode.extensions as unknown as { all: unknown[] };
const showWarningMessage = vscode.window.showWarningMessage as jest.Mock;
const clipboardWrite = vscode.env.clipboard.writeText as jest.Mock;

type StateStore = Map<string, unknown>;

function makeState(): {
  get: jest.Mock;
  update: jest.Mock;
  keys: jest.Mock;
  store: StateStore;
} {
  const store: StateStore = new Map();
  return {
    store,
    get: jest.fn((key: string, defaultValue?: unknown) =>
      store.has(key) ? store.get(key) : defaultValue,
    ),
    update: jest.fn(async (key: string, value: unknown) => {
      if (value === undefined) {
        store.delete(key);
      } else {
        store.set(key, value);
      }
    }),
    keys: jest.fn(() => [...store.keys()]),
  };
}

function makeContext(state: ReturnType<typeof makeState>): vscode.ExtensionContext {
  return {
    extension: {
      id: 'EffortlessMetrics.perl-lsp-rs',
      packageJSON: { publisher: 'EffortlessMetrics', name: 'perl-lsp-rs', version: '0.17.0' },
    },
    globalState: state,
  } as unknown as vscode.ExtensionContext;
}

function configure(overrides: Record<string, unknown> = {}): void {
  workspaceMock.getConfiguration.mockImplementation((section?: string) => ({
    get: jest.fn((key: string, defaultValue?: unknown) => {
      if (section === 'perl-lsp' && key in overrides) {
        return overrides[key];
      }
      if (section === 'editor' && key === 'formatOnSave') {
        return overrides['editor.formatOnSave'] ?? defaultValue;
      }
      return defaultValue;
    }),
    inspect: jest.fn((key: string) => {
      if (section === 'perl-lsp' && key === 'critic.engine') {
        return { globalValue: overrides['critic.engine'] };
      }
      if (section === 'editor' && key === 'defaultFormatter') {
        return { globalValue: overrides['editor.defaultFormatter'] };
      }
      return undefined;
    }),
    update: jest.fn(),
  }));
}

const NAVIGATOR = {
  id: 'bscan.perlnavigator',
  isActive: true,
  packageJSON: {},
};

// fractalboy.pls produces exactly one finding class (a retired-runtime LSP
// client), which keeps single-conflict flows unambiguous.
const PLS_CLIENT = {
  id: 'fractalboy.pls',
  isActive: true,
  packageJSON: {},
};

beforeEach(() => {
  extensionsMock.all = [];
  workspaceMock.workspaceFolders = [];
  workspaceMock.findFiles.mockResolvedValue([]);
  workspaceMock.openTextDocument.mockImplementation(
    async (value: string | { content: string }) => ({
      uri: { fsPath: typeof value === 'string' ? value : 'report.md' },
      getText: () => (typeof value === 'string' ? value : value.content),
    }),
  );
  configure();
  showWarningMessage.mockResolvedValue(undefined);
});

afterEach(() => {
  jest.clearAllMocks();
  extensionsMock.all = [];
  workspaceMock.workspaceFolders = undefined;
});

describe('coexistence advisory flow (#7214)', () => {
  test('a PATH-perlcritic-only style environment never notifies', async () => {
    // Nothing installed except the native extension; residue inputs do not
    // exist on this host and cannot be fabricated into findings.
    extensionsMock.all = [{ id: 'EffortlessMetrics.perl-lsp-rs', isActive: true, packageJSON: {} }];
    const context = makeContext(makeState());

    const findings = await runCoexistenceAdvisory(context);

    expect(findings).toEqual([]);
    expect(showWarningMessage).not.toHaveBeenCalled();
  });

  test('notifies once for a detected conflict and does not replay unchanged state', async () => {
    extensionsMock.all = [PLS_CLIENT];
    const context = makeContext(makeState());

    await runCoexistenceAdvisory(context);
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
    expect(showWarningMessage).toHaveBeenCalledWith(
      expect.stringContaining('multiple_language_servers'),
      'Show coexistence status',
      'Disable for this exact conflict',
      'Copy redacted support packet',
    );

    await runCoexistenceAdvisory(context);
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
  });

  test('suppressing the exact conflict silences it without touching other conflicts', async () => {
    extensionsMock.all = [PLS_CLIENT];
    const state = makeState();
    const context = makeContext(state);

    // The user clicks "Disable for this exact conflict" in the popup itself.
    showWarningMessage.mockResolvedValue('Disable for this exact conflict');
    const findings = await runCoexistenceAdvisory(context);
    expect(findings).toHaveLength(1);

    const suppressedKeys = [...state.store.keys()].filter((key) =>
      key.startsWith('perl-lsp.coexistence.suppressed.'),
    );
    expect(suppressedKeys).toHaveLength(1);
    const primary = findings[0];
    if (!primary) {
      throw new Error('expected the single conflict finding to exist');
    }
    expect(suppressedKeys[0]).toBe(
      `perl-lsp.coexistence.suppressed.${coexistenceConflictKey(primary)}`,
    );

    showWarningMessage.mockClear();
    await runCoexistenceAdvisory(context);
    expect(showWarningMessage).not.toHaveBeenCalled();
  });

  test('clearing the condition prunes the suppression so recurrence is restored', async () => {
    extensionsMock.all = [PLS_CLIENT];
    const state = makeState();
    const context = makeContext(state);

    showWarningMessage.mockResolvedValue('Disable for this exact conflict');
    await runCoexistenceAdvisory(context);
    const suppressedKeys = [...state.store.keys()].filter((key) =>
      key.startsWith('perl-lsp.coexistence.suppressed.'),
    );
    expect(suppressedKeys.length).toBeGreaterThan(0);

    // The competing provider is uninstalled.
    extensionsMock.all = [];
    showWarningMessage.mockClear();
    await runCoexistenceAdvisory(context);
    expect(
      [...state.store.keys()].some((key) => key.startsWith('perl-lsp.coexistence.suppressed.')),
    ).toBe(false);

    // ...and a later reinstall is reported again.
    extensionsMock.all = [NAVIGATOR];
    showWarningMessage.mockResolvedValue(undefined);
    await runCoexistenceAdvisory(context);
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
  });

  test('multi-root folders report distinct subjects per root', async () => {
    extensionsMock.all = [];
    workspaceMock.workspaceFolders = [
      { name: 'root-a', uri: { fsPath: '/w/a', toString: () => 'file:///w/a' } },
      { name: 'root-b', uri: { fsPath: '/w/b', toString: () => 'file:///w/b' } },
    ];
    configure({ formatOnSave: false });
    workspaceMock.findFiles.mockImplementation(
      async (_pattern: unknown, _excludes: unknown, limit: number) => {
        // Only root A carries a .perltidyrc candidate; findFiles is scoped by the
        // RelativePattern, which the mock approximates by call order.
        const calls = workspaceMock.findFiles.mock.calls.length;
        return calls === 1 && limit >= 1 ? ['/w/a/.perltidyrc'] : [];
      },
    );

    const findings = await collectCoexistenceFindings(makeContext(makeState()));
    const candidates = findings.filter(
      (finding) => finding.conflictClass === 'external_tool_candidate_not_selected',
    );
    expect(candidates).toHaveLength(1);
    expect(candidates[0]?.folderName).toBe('root-a');
    expect(candidates[0]?.subject).toBe('.perltidyrc');
  });

  test('the redacted support packet omits private paths and inventory', async () => {
    extensionsMock.all = [NAVIGATOR];
    workspaceMock.workspaceFolders = [
      {
        name: '/home/dev/secret-project',
        uri: { fsPath: '/home/dev/secret-project', toString: () => 'x' },
      },
    ];
    configure({ 'critic.enabled': true, formatOnSave: false });
    const findings = await collectCoexistenceFindings(makeContext(makeState()));
    expect(findings.length).toBeGreaterThan(0);

    // A fresh state so the finding set is treated as new and the packet
    // action becomes reachable.
    showWarningMessage.mockResolvedValue('Copy redacted support packet');
    await runCoexistenceAdvisory(makeContext(makeState()));
    expect(clipboardWrite).toHaveBeenCalled();

    const packet = clipboardWrite.mock.calls.at(-1)?.[0] as string;
    expect(packet).toContain('Perl Navigator');
    expect(packet).toContain('"conflictClass"');
    for (const finding of findings) {
      if (finding.folderName !== undefined) {
        expect(packet).not.toContain(finding.folderName);
      }
    }
    expect(packet).not.toContain('/home/dev');
    // Only involved owners appear — never the full extension inventory.
    expect(packet.match(/extensionId/g) ?? []).toHaveLength(0);
  });

  test('the status command explains evidence sources and the clean boundary', async () => {
    extensionsMock.all = [NAVIGATOR, PLS_CLIENT];
    configure({ 'critic.enabled': true, formatOnSave: false });
    const findings = await collectCoexistenceFindings(makeContext(makeState()));
    const report = renderCoexistenceStatusReport(findings);

    expect(report).toContain('# Perl LSP coexistence status');
    expect(report).toContain('Evidence source:');
    expect(report).toContain('Claim boundary:');
    expect(report).toContain('Registry reason code (#7209/#7212): runtime_enablement_forbidden');

    const cleanReport = renderCoexistenceStatusReport([]);
    expect(cleanReport).toContain(
      'No conflicts detected among the facts this product can observe.',
    );
    expect(cleanReport).toContain('reviewed extension identities');
    expect(cleanReport).toContain('are not providers and are never reported as conflicts');
  });

  test('self extension is never its own conflict', async () => {
    extensionsMock.all = [{ id: 'effortlessmetrics.perl-lsp-rs', isActive: true, packageJSON: {} }];
    const findings = await collectCoexistenceFindings(makeContext(makeState()));
    expect(findings).toEqual([]);
  });
});
