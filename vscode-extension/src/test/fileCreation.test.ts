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
    (section?: string, scope?: { fsPath: string }) => ({
      get: (key: string, defaultValue?: unknown) => {
        if (section !== 'perl-lsp' || key !== 'autoPopulateNewFiles') {
          return defaultValue;
        }
        if (scope) {
          const folder = folderValues.find(([root]) => scope.fsPath.startsWith(`${root}/`));
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

/** Paths the handler actually staged boilerplate for. */
function populatedPaths(): string[] {
  const applyEdit = vscode.workspace.applyEdit as jest.Mock;
  return applyEdit.mock.calls.flatMap((call) => {
    const edit = call[0] as { inserts: Array<{ uri: { fsPath: string } }> };
    return edit.inserts.map((insert) => insert.uri.fsPath);
  });
}

function creationEvent(...paths: string[]): vscode.FileCreateEvent {
  return { files: paths.map((p) => vscode.Uri.file(p)) } as unknown as vscode.FileCreateEvent;
}

describe('populateCreatedFiles gates on the folder each file was created in (#14547)', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test('a folder that disables population is not populated while a sibling folder is', async () => {
    // Hoisting the read out of the loop makes the unscoped answer (true) apply
    // to both files, so the disabled root would be populated too.
    installScopedConfiguration(true, [[DISABLED_ROOT, false]]);

    await populateCreatedFiles(
      creationEvent(`${ENABLED_ROOT}/lib/Kept.pm`, `${DISABLED_ROOT}/lib/Skipped.pm`),
    );

    expect(populatedPaths()).toEqual([`${ENABLED_ROOT}/lib/Kept.pm`]);
  });

  test('a folder that enables population is populated while the workspace default is off', async () => {
    // The mirror case: hoisting makes the unscoped answer (false) suppress both.
    installScopedConfiguration(false, [[ENABLED_ROOT, true]]);

    await populateCreatedFiles(
      creationEvent(`${DISABLED_ROOT}/t/skipped.t`, `${ENABLED_ROOT}/t/kept.t`),
    );

    expect(populatedPaths()).toEqual([`${ENABLED_ROOT}/t/kept.t`]);
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

    expect(populatedPaths()).toEqual(['/only-root/lib/Foo/Bar.pm']);
    const applyEdit = vscode.workspace.applyEdit as jest.Mock;
    const edit = applyEdit.mock.calls[0]?.[0] as { inserts: Array<{ newText: string }> };
    expect(edit.inserts[0]?.newText).toContain('package Foo::Bar;');
  });

  test('a file outside every workspace folder falls back to the workspace value', async () => {
    installScopedConfiguration(false, [[ENABLED_ROOT, true]]);

    await populateCreatedFiles(creationEvent('/elsewhere/lib/Foo.pm'));

    expect(populatedPaths()).toEqual([]);
  });

  test('an enabled folder still skips files that already have content', async () => {
    installScopedConfiguration(true);
    (vscode.workspace.openTextDocument as jest.Mock).mockImplementation(async (value: unknown) => ({
      uri: value,
      getText: () => 'package Existing;\n',
    }));

    await populateCreatedFiles(creationEvent(`${ENABLED_ROOT}/lib/Existing.pm`));

    expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
  });

  test('an enabled folder still skips extensions that get no boilerplate', async () => {
    installScopedConfiguration(true);

    await populateCreatedFiles(creationEvent(`${ENABLED_ROOT}/script.pl`));

    expect(vscode.workspace.openTextDocument).not.toHaveBeenCalled();
    expect(vscode.workspace.applyEdit).not.toHaveBeenCalled();
  });
});
