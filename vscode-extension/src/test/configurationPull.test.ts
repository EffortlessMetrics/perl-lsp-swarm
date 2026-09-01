import * as vscode from 'vscode';
import {
  buildPerlSectionValue,
  PERL_CONFIGURATION_SECTION,
  resolvePerlConfiguration,
} from '../configurationPull';

type FolderValues = Record<string, unknown>;

/**
 * Stand in for `workspace.getConfiguration('perl-lsp', scope)` with genuinely
 * different values per folder, so a resolver that ignores `scopeUri` is
 * observably wrong rather than merely unproven.
 */
function installScopedConfiguration(byScope: Record<string, FolderValues>): void {
  const unscoped = byScope[''] ?? {};

  // VS Code accepts either a Uri or a `{ uri, languageId }` scope object; the
  // critic reader uses the language-qualified form, so the harness must resolve
  // both to the same folder.
  const scopeKey = (scope: unknown): string => {
    if (!scope) {
      return '';
    }
    const uri = (scope as { uri?: { toString(): string } }).uri;
    if (uri) {
      return uri.toString();
    }
    return (scope as { toString(): string }).toString();
  };

  (vscode.workspace.getConfiguration as unknown as jest.Mock).mockImplementation(
    (section?: string, scope?: unknown) => {
      if (section !== 'perl-lsp') {
        return {
          get: jest.fn((_key: string, defaultValue?: unknown) => defaultValue),
          has: jest.fn(() => false),
          inspect: jest.fn(),
          update: jest.fn(),
        };
      }

      const key = scopeKey(scope);
      const values = byScope[key] ?? unscoped;

      return {
        get: jest.fn((setting: string, defaultValue?: unknown) =>
          setting in values ? values[setting] : defaultValue,
        ),
        has: jest.fn((setting: string) => setting in values),
        inspect: jest.fn((setting: string) =>
          setting in values ? { workspaceFolderValue: values[setting] } : undefined,
        ),
        update: jest.fn(),
      };
    },
  );
}

const FOLDER_A = 'file:///workspace/a';
const FOLDER_B = 'file:///workspace/b';

describe('workspace/configuration folder ownership (#14447)', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  afterEach(() => {
    (vscode.workspace.getConfiguration as unknown as jest.Mock).mockReset();
  });

  test('answers each requested item from its own scopeUri', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'] },
      [FOLDER_B]: { includePaths: ['b/lib'] },
    });

    const next = jest.fn();
    const result = await resolvePerlConfiguration(
      {
        items: [
          { section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_B, section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      next,
    );

    expect(result).toHaveLength(3);
    expect(result[1]).toEqual({ workspace: { includePaths: ['a/lib'] } });
    expect(result[2]).toEqual({ workspace: { includePaths: ['b/lib'] } });
    expect(next).not.toHaveBeenCalled();
  });

  test("folder A's value never answers folder B", async () => {
    // Negative control for the reported defect: before the middleware existed,
    // the client resolved `section: "perl"` against the unrelated `perl.*`
    // namespace, so no folder received its own values at all. A resolver that
    // ignores scopeUri and reuses one snapshot fails here.
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'], 'critic.severity': 1 },
      [FOLDER_B]: {},
    });

    const result = await resolvePerlConfiguration(
      {
        items: [
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_B, section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      jest.fn(),
    );

    expect(result[0]).toEqual({
      workspace: { includePaths: ['a/lib'] },
      critic: { severity: 1 },
    });
    // Folder B declares nothing, so it must inherit nothing from folder A.
    expect(result[1]).toEqual({});
  });

  test('folder order does not change the answers', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'] },
      [FOLDER_B]: { includePaths: ['b/lib'] },
    });

    const forward = await resolvePerlConfiguration(
      {
        items: [
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_B, section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      jest.fn(),
    );
    const reversed = await resolvePerlConfiguration(
      {
        items: [
          { scopeUri: FOLDER_B, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      jest.fn(),
    );

    expect(forward[0]).toEqual(reversed[1]);
    expect(forward[1]).toEqual(reversed[0]);
  });

  test('the unscoped item is resolved without a resource', async () => {
    installScopedConfiguration({
      '': { includePaths: ['workspace/lib'] },
      [FOLDER_A]: { includePaths: ['a/lib'] },
    });

    const result = await resolvePerlConfiguration(
      { items: [{ section: PERL_CONFIGURATION_SECTION }] },
      undefined,
      jest.fn(),
    );

    // The unscoped slot is applied by the server as a base layer under every
    // folder, so it must never carry one folder's value.
    expect(result[0]).toEqual({ workspace: { includePaths: ['workspace/lib'] } });
  });

  test('an unparseable scopeUri falls back to the unscoped view, not another folder', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'] },
    });
    const parse = jest.spyOn(vscode.Uri, 'parse').mockImplementation(() => {
      throw new Error('not a uri');
    });

    try {
      const result = await resolvePerlConfiguration(
        { items: [{ scopeUri: '::::', section: PERL_CONFIGURATION_SECTION }] },
        undefined,
        jest.fn(),
      );

      expect(result[0]).toEqual({});
    } finally {
      parse.mockRestore();
    }
  });

  test('non-perl sections are delegated and spliced back at their own index', async () => {
    installScopedConfiguration({ '': {}, [FOLDER_A]: { includePaths: ['a/lib'] } });

    const next = jest.fn(() => [null, { editorValue: true }, null]);
    const result = await resolvePerlConfiguration(
      {
        items: [
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_A, section: 'editor' },
          { section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      next,
    );

    expect(next).toHaveBeenCalledTimes(1);
    expect(result[0]).toEqual({ workspace: { includePaths: ['a/lib'] } });
    expect(result[1]).toEqual({ editorValue: true });
    expect(result[2]).toEqual({});
  });

  test('response arity always matches request arity', async () => {
    installScopedConfiguration({ '': {} });

    const result = await resolvePerlConfiguration(
      {
        items: [
          { section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION },
          { scopeUri: FOLDER_B, section: PERL_CONFIGURATION_SECTION },
        ],
      },
      undefined,
      jest.fn(),
    );

    expect(result).toHaveLength(3);
    expect(result.every((entry) => entry !== undefined)).toBe(true);
  });

  test('pull and push describe the same scope identically', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { includePaths: ['a/lib'], 'critic.profile': 'strict' },
    });

    const pulled = await resolvePerlConfiguration(
      { items: [{ scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION }] },
      undefined,
      jest.fn(),
    );

    // `buildPerlSectionValue` is the same builder the didChangeConfiguration
    // push uses, so a divergence between the two transports is unrepresentable.
    expect(pulled[0]).toEqual(buildPerlSectionValue(vscode.Uri.parse(FOLDER_A)));
  });

  test('machine-scoped external roots are not published as folder values', async () => {
    installScopedConfiguration({
      '': {},
      [FOLDER_A]: { externalIncludePaths: ['/opt/perl/lib'] },
    });

    const result = await resolvePerlConfiguration(
      { items: [{ scopeUri: FOLDER_A, section: PERL_CONFIGURATION_SECTION }] },
      undefined,
      jest.fn(),
    );

    // The scoped reader reports it as a workspaceFolderValue, which is exactly
    // the shape `externalIncludePaths` must refuse: it is machine-scoped and
    // only `inspect().globalValue` may authorize it.
    expect(result[0]).toEqual({});
  });
});
