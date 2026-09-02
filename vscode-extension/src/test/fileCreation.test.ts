/**
 * Unit tests for smart file creation with package boilerplate.
 *
 * Tests cover the pure logic exported from fileCreation.ts:
 *   - inferPackageName: derive Perl package name from file path
 *   - generateBoilerplate: produce correct file content for .pm and .t files
 *
 * and the workspace-folder gate the file-creation handler applies before using
 * that content (`populateCreatedFiles`, #14547).
 *
 * Issue: #2056
 */

import * as vscode from 'vscode';

jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  Trace: {
    Off: 'off',
    Messages: 'messages',
    Verbose: 'verbose',
  },
  TransportKind: {
    stdio: 0,
  },
}));

import { inferPackageName, generateBoilerplate, FileKind } from '../fileCreation';
import { populateCreatedFiles } from '../extension';

// ---------------------------------------------------------------------------
// inferPackageName
// ---------------------------------------------------------------------------
describe('inferPackageName', () => {
  test('derives package from lib/ path', () => {
    expect(inferPackageName('/project/lib/Foo/Bar.pm')).toBe('Foo::Bar');
  });

  test('derives package from nested lib/ path', () => {
    expect(inferPackageName('/project/lib/My/Long/Module.pm')).toBe('My::Long::Module');
  });

  test('derives package from top-level lib/ module', () => {
    expect(inferPackageName('/project/lib/Foo.pm')).toBe('Foo');
  });

  test('derives package from lib/ with Windows-style separators', () => {
    // path.sep may be / on Linux; test cross-platform via explicit slashes
    expect(inferPackageName('C:\\project\\lib\\Foo\\Bar.pm')).toBe('Foo::Bar');
  });

  test('falls back to basename without extension when no lib/ anchor', () => {
    expect(inferPackageName('/project/Foo/Bar.pm')).toBe('Bar');
  });

  test('returns null for .t files (no package name)', () => {
    expect(inferPackageName('/project/t/foo.t')).toBeNull();
  });

  test('returns null for non-.pm files', () => {
    expect(inferPackageName('/project/lib/Foo.pl')).toBeNull();
  });

  test('handles deep lib anchor correctly', () => {
    expect(inferPackageName('/home/user/myapp/lib/App/Controller/Root.pm')).toBe(
      'App::Controller::Root',
    );
  });
});

// ---------------------------------------------------------------------------
// generateBoilerplate — .pm files
// ---------------------------------------------------------------------------
describe('generateBoilerplate for .pm files', () => {
  test('returns FileKind.Module for .pm extension', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    expect(result.kind).toBe(FileKind.Module);
  });

  test('includes correct package declaration', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    expect(result.content).toContain('package Foo::Bar;');
  });

  test('includes use strict', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    expect(result.content).toContain('use strict;');
  });

  test('includes use warnings', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    expect(result.content).toContain('use warnings;');
  });

  test('ends with 1;', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    expect(result.content.trimEnd()).toMatch(/1;$/);
  });

  test('package declaration appears before use strict', () => {
    const result = generateBoilerplate('/project/lib/Foo/Bar.pm')!;
    const pkgIdx = result.content.indexOf('package');
    const strictIdx = result.content.indexOf('use strict');
    expect(pkgIdx).toBeLessThan(strictIdx);
  });

  test('uses fallback basename when no lib/ anchor', () => {
    const result = generateBoilerplate('/project/MyModule.pm')!;
    expect(result.content).toContain('package MyModule;');
  });
});

// ---------------------------------------------------------------------------
// generateBoilerplate — .t files
// ---------------------------------------------------------------------------
describe('generateBoilerplate for .t files', () => {
  test('returns FileKind.Test for .t extension', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.kind).toBe(FileKind.Test);
  });

  test('includes use strict', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content).toContain('use strict;');
  });

  test('includes use warnings', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content).toContain('use warnings;');
  });

  test('includes use Test::More', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content).toContain('use Test::More;');
  });

  test('ends with done_testing', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content.trimEnd()).toMatch(/done_testing;$/);
  });

  test('does not include a package declaration', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content).not.toContain('package ');
  });

  test('does not include 1; terminator', () => {
    const result = generateBoilerplate('/project/t/foo.t')!;
    expect(result.content).not.toContain('\n1;');
  });
});

// ---------------------------------------------------------------------------
// generateBoilerplate — unsupported extensions
// ---------------------------------------------------------------------------
describe('generateBoilerplate for unsupported files', () => {
  test('returns null for .pl files', () => {
    expect(generateBoilerplate('/project/script.pl')).toBeNull();
  });

  test('returns null for .pod files', () => {
    expect(generateBoilerplate('/project/doc.pod')).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// populateCreatedFiles — workspace-folder gate (#14547)
// ---------------------------------------------------------------------------

const ENABLED_ROOT = '/ws/enabled';
const DISABLED_ROOT = '/ws/disabled';

/**
 * Install a `getConfiguration` that models the one property of the real API
 * this claim turns on: **only a scoped read can observe a folder value**.
 *
 * An unscoped `getConfiguration('perl-lsp')` has no resource to resolve
 * against, so VS Code answers it from the global/workspace layers alone. The
 * fake reproduces exactly that, which is what makes the contradictory-roots
 * cases below fail if the read is hoisted back out of the loop.
 */
function installScopedConfiguration(
  workspaceValue: boolean | undefined,
  folderValues: ReadonlyArray<readonly [string, boolean]> = [],
): void {
  (vscode.workspace.getConfiguration as jest.Mock).mockImplementation(
    (section?: string, scope?: unknown) => ({
      get: (key: string, defaultValue?: unknown) => {
        if (section !== 'perl-lsp' || key !== 'autoPopulateNewFiles') {
          return defaultValue;
        }
        if (scope !== undefined) {
          // The real `ConfigurationScope` also accepts a `WorkspaceFolder` and a
          // `TextDocument`. This fake answers only a `Uri`, and throws instead
          // of falling through to the workspace value — a silent fall-through
          // would read as a passing test.
          //
          // Be honest about what that buys: passing `getWorkspaceFolder(uri)`
          // would be *behaviourally* equivalent for this setting, so this pins
          // one spelling rather than discriminating between right and wrong
          // implementations. The spelling is worth pinning because the created
          // URI is the scope the claim is about and needs no folder lookup, but
          // a future refactor that legitimately reuses a resolved folder here
          // should update this fake rather than be treated as a regression.
          if (typeof (scope as { fsPath?: unknown }).fsPath !== 'string') {
            throw new Error(
              `getConfiguration scope must be the created file Uri, got ${JSON.stringify(scope)}`,
            );
          }
          const { fsPath } = scope as { fsPath: string };
          // Anchor on a trailing separator so `/ws/enabled-other/x.pm` cannot
          // match the `/ws/enabled` row.
          const folder = folderValues.find(([root]) => fsPath.startsWith(`${root}/`));
          if (folder) {
            return folder[1];
          }
        }
        return workspaceValue ?? defaultValue;
      },
      has: () => false,
      inspect: () => undefined,
      update: () => undefined,
    }),
  );
}

/**
 * Paths the handler *submitted* an edit for.
 *
 * Deliberately not named "populated": `applyEdit` reports whether the editor
 * accepted the edit, and production discards that boolean, so nothing here —
 * or in the extension — can tell an applied edit from a rejected one. These
 * assertions therefore prove which files the gate let through, which is this
 * claim's subject, and not that bytes reached disk.
 */
function stagedPaths(): string[] {
  const applyEdit = vscode.workspace.applyEdit as jest.Mock;
  return applyEdit.mock.calls.flatMap((call) => {
    const edit = call[0] as { inserts: Array<{ uri: { fsPath: string } }> };
    return edit.inserts.map((insert) => insert.uri.fsPath);
  });
}

function creationEvent(...paths: string[]): vscode.FileCreateEvent {
  return { files: paths.map((p) => vscode.Uri.file(p)) } as unknown as vscode.FileCreateEvent;
}

/** Restores the shared mocks' defaults: documents open empty, edits succeed. */
function resetSharedMockImplementations(): void {
  (vscode.workspace.openTextDocument as jest.Mock).mockImplementation(async (value: unknown) => ({
    uri: value,
    getText: () => '',
  }));
  (vscode.workspace.applyEdit as jest.Mock).mockImplementation(async () => true);
}

describe('populateCreatedFiles gates on the folder each file was created in (#14547)', () => {
  beforeEach(() => {
    // `clearAllMocks` clears call records but NOT implementations, and this
    // suite installs persistent ones. Without an explicit reset a later test
    // silently inherits an earlier test's document contents — which disarms a
    // negative assertion rather than failing it.
    jest.clearAllMocks();
    resetSharedMockImplementations();
  });

  test('a folder that disables population is not populated while a sibling folder is', async () => {
    // Hoisting the read out of the loop makes the unscoped answer (true) apply
    // to both files, so the disabled root would be populated too.
    installScopedConfiguration(true, [[DISABLED_ROOT, false]]);

    await populateCreatedFiles(
      creationEvent(`${ENABLED_ROOT}/lib/Kept.pm`, `${DISABLED_ROOT}/lib/Skipped.pm`),
    );

    expect(stagedPaths()).toEqual([`${ENABLED_ROOT}/lib/Kept.pm`]);
  });

  test('a folder that enables population is populated while the workspace default is off', async () => {
    // The mirror case: hoisting makes the unscoped answer (false) suppress both.
    installScopedConfiguration(false, [[ENABLED_ROOT, true]]);

    await populateCreatedFiles(
      creationEvent(`${DISABLED_ROOT}/t/skipped.t`, `${ENABLED_ROOT}/t/kept.t`),
    );

    expect(stagedPaths()).toEqual([`${ENABLED_ROOT}/t/kept.t`]);
  });

  test('the gate is resolved against every created URI, not just the first', async () => {
    installScopedConfiguration(true, [[DISABLED_ROOT, false]]);
    const files = [`${ENABLED_ROOT}/lib/A.pm`, `${DISABLED_ROOT}/lib/B.pm`];

    await populateCreatedFiles(creationEvent(...files));

    const scopes = (vscode.workspace.getConfiguration as jest.Mock).mock.calls.map(
      (call) => (call[1] as { fsPath: string } | undefined)?.fsPath,
    );
    expect(scopes).toEqual(files);
  });

  test('a disabled folder is not opened at all', async () => {
    installScopedConfiguration(true, [[DISABLED_ROOT, false]]);

    await populateCreatedFiles(creationEvent(`${DISABLED_ROOT}/lib/Skipped.pm`));

    expect(vscode.workspace.openTextDocument).not.toHaveBeenCalled();
    expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
  });

  test('an unset value still populates, in a single-root workspace as before', async () => {
    installScopedConfiguration(undefined);

    await populateCreatedFiles(creationEvent('/only-root/lib/Foo/Bar.pm'));

    expect(stagedPaths()).toEqual(['/only-root/lib/Foo/Bar.pm']);
    const applyEdit = vscode.workspace.applyEdit as jest.Mock;
    const edit = applyEdit.mock.calls[0]?.[0] as { inserts: Array<{ newText: string }> };
    expect(edit.inserts[0]?.newText).toContain('package Foo::Bar;');
  });

  test('a file outside every workspace folder falls back to the workspace value', async () => {
    installScopedConfiguration(false, [[ENABLED_ROOT, true]]);

    await populateCreatedFiles(creationEvent('/elsewhere/lib/Foo.pm'));

    expect(stagedPaths()).toEqual([]);
  });

  test('a sibling folder with a shared name prefix does not inherit the override', async () => {
    // `/ws/disabled-legacy` is a different root from `/ws/disabled`; an
    // unanchored prefix match in the fake would silently hand it the override
    // and make the disabled-folder cases above pass for the wrong reason.
    installScopedConfiguration(true, [[DISABLED_ROOT, false]]);

    await populateCreatedFiles(creationEvent(`${DISABLED_ROOT}-legacy/lib/Kept.pm`));

    expect(stagedPaths()).toEqual([`${DISABLED_ROOT}-legacy/lib/Kept.pm`]);
  });

  test('a file with content is skipped without stopping the rest of the event', async () => {
    // Two files, so the skip is proved to be per-file. With a single file a
    // `return` here would be indistinguishable from `continue`, and this claim
    // is precisely that the handler decides per file rather than per event.
    installScopedConfiguration(true);
    (vscode.workspace.openTextDocument as jest.Mock).mockImplementation(async (value: unknown) => ({
      uri: value,
      getText: () =>
        (value as { fsPath: string }).fsPath.includes('Existing') ? 'package X;\n' : '',
    }));

    await populateCreatedFiles(
      creationEvent(`${ENABLED_ROOT}/lib/Existing.pm`, `${ENABLED_ROOT}/lib/Fresh.pm`),
    );

    expect(stagedPaths()).toEqual([`${ENABLED_ROOT}/lib/Fresh.pm`]);
  });

  test('an unsupported extension is skipped without stopping the rest of the event', async () => {
    // Same reasoning: `.pl` gets no boilerplate, but the `.pm` after it must
    // still be reached.
    installScopedConfiguration(true);

    await populateCreatedFiles(
      creationEvent(`${ENABLED_ROOT}/script.pl`, `${ENABLED_ROOT}/lib/After.pm`),
    );

    expect(stagedPaths()).toEqual([`${ENABLED_ROOT}/lib/After.pm`]);
    // The unsupported file is never opened — the boilerplate check precedes I/O.
    expect((vscode.workspace.openTextDocument as jest.Mock).mock.calls).toHaveLength(1);
  });

  test('a rejected edit is currently indistinguishable from an applied one', async () => {
    // `applyEdit` resolves false when the editor refuses the edit, and
    // production discards that result — so no file is populated and nothing is
    // logged. Pinned deliberately: this suite observes edits *submitted*, and
    // that limit should fail loudly if the production seam ever starts caring.
    installScopedConfiguration(true);
    (vscode.workspace.applyEdit as jest.Mock).mockImplementation(async () => false);

    await expect(
      populateCreatedFiles(creationEvent(`${ENABLED_ROOT}/lib/Rejected.pm`)),
    ).resolves.toBeUndefined();

    expect(stagedPaths()).toEqual([`${ENABLED_ROOT}/lib/Rejected.pm`]);
  });
});
