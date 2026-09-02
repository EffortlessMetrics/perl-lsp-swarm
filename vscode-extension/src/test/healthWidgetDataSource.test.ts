/**
 * Unit tests for HealthWidgetDataSource — the production wiring that feeds
 * first-party error and bounded file counts into `HealthWidget`.
 */

import { HealthWidget, ClientState } from '../healthWidget';
import { HealthWidgetDataSource } from '../healthWidgetDataSource';
import type { LanguagesTelemetry, WorkspaceTelemetry } from '../healthWidgetDataSource';
import type { ThemeColor } from 'vscode';
import type { Diagnostic, Disposable, StatusBarItem, Uri } from 'vscode';

interface TestFileSystemWatcher extends Disposable {
  onDidCreate(listener: (uri: Uri) => void): Disposable;
  onDidChange(listener: (uri: Uri) => void): Disposable;
  onDidDelete(listener: (uri: Uri) => void): Disposable;
}

function makeStatusBarItem(): StatusBarItem {
  return {
    text: '',
    tooltip: '' as string | undefined,
    command: '' as string | undefined,
    backgroundColor: undefined as ThemeColor | undefined,
    show(): void {},
    hide(): void {},
    dispose(): void {},
  } as unknown as StatusBarItem;
}

function uri(fsPath: string): Uri {
  return { fsPath, toString: () => `file://${fsPath}` } as unknown as Uri;
}

function diag(severity: number, source = 'perl-lsp'): Diagnostic {
  return { severity, source } as unknown as Diagnostic;
}

interface TestLanguages extends LanguagesTelemetry {
  fire(uris: Uri[]): void;
  setDiagnostics(next: Array<[Uri, Diagnostic[]]>): void;
}

function makeLanguages(initial: Array<[Uri, Diagnostic[]]>): TestLanguages {
  let listener: ((event: { uris: readonly Uri[] }) => void) | undefined;
  let current = initial;
  return {
    onDidChangeDiagnostics(handler: (event: { uris: readonly Uri[] }) => void): Disposable {
      listener = handler;
      return {
        dispose: () => {
          listener = undefined;
        },
      };
    },
    getDiagnostics(): Array<[Uri, Diagnostic[]]> {
      return current;
    },
    fire(uris: Uri[]): void {
      listener?.({ uris });
    },
    setDiagnostics(next: Array<[Uri, Diagnostic[]]>): void {
      current = next;
    },
  };
}

interface TestWorkspace extends WorkspaceTelemetry {
  calls: Array<{ include: string; maxResults: number | undefined }>;
  fireCreate(files?: readonly Uri[]): void;
  fireExternalCreate(file?: Uri): void;
  setFileCounts(next: number[]): void;
}

function makeWorkspace(initialFileCounts: number[]): TestWorkspace {
  const calls: Array<{ include: string; maxResults: number | undefined }> = [];
  let fileCounts = [...initialFileCounts];
  let createListener: ((event: { files: readonly Uri[] }) => void) | undefined;
  let externalCreateListener: ((uri: Uri) => void) | undefined;
  return {
    findFiles(include: string, _exclude?: string | null, maxResults?: number): Thenable<Uri[]> {
      calls.push({ include, maxResults });
      const count = fileCounts.shift() ?? 0;
      const uris: Uri[] = [];
      for (let index = 0; index < count; index += 1) {
        uris.push(uri(`/ws/${encodeURIComponent(include)}/file-${index}`));
      }
      return Promise.resolve(uris);
    },
    onDidCreateFiles(listener): Disposable {
      createListener = listener;
      return {
        dispose: () => {
          createListener = undefined;
        },
      };
    },
    createFileSystemWatcher(): TestFileSystemWatcher {
      return {
        onDidCreate(listener): Disposable {
          externalCreateListener = listener;
          return {
            dispose: () => {
              externalCreateListener = undefined;
            },
          };
        },
        onDidChange(): Disposable {
          return { dispose: () => {} };
        },
        onDidDelete(): Disposable {
          return { dispose: () => {} };
        },
        dispose(): void {},
      };
    },
    calls,
    fireCreate(files = [uri('/ws/new.pm')]): void {
      createListener?.({ files });
    },
    fireExternalCreate(file = uri('/ws/external.pm')): void {
      externalCreateListener?.(file);
    },
    setFileCounts(next: number[]): void {
      fileCounts = [...next];
    },
  };
}

describe('HealthWidgetDataSource — first-party error count', () => {
  test('initial refresh pushes only perl-lsp errors into the status bar', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([
      [uri('/ws/lib.pm'), [diag(0), diag(1), diag(0)]],
      [uri('/ws/t/test.t'), [diag(0)]],
      [uri('/ws/readme.md'), [diag(0)]],
      [uri('/ws/other.pm'), [diag(0, 'other-linter'), diag(0, 'perlcritic')]],
    ]);
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();

    expect(widget.errorCount).toBe(3);
    expect(item.text).toContain('3 errors');
    source.dispose();
  });

  test('another extension cannot increment the perl-lsp error count', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const languages = makeLanguages([
      [uri('/ws/a.pl'), [diag(0, 'perl-lsp'), diag(0, 'other-extension')]],
    ]);
    const source = new HealthWidgetDataSource(
      widget,
      languages,
      makeWorkspace([0, 0, 0, 0, 0]),
    );

    source.start();

    expect(widget.errorCount).toBe(1);
    expect(item.text).toContain('1 error');
    source.dispose();
  });

  test('onDidChangeDiagnostics updates the current first-party count', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.setFileCount(100);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([[uri('/ws/a.pl'), [diag(0), diag(0)]]]);
    const source = new HealthWidgetDataSource(
      widget,
      languages,
      makeWorkspace([0, 0, 0, 0, 0]),
    );
    source.start();
    expect(item.text).toBe('$(check) perl-lsp: 100 files | 2 errors');

    languages.setDiagnostics([
      [uri('/ws/a.pl'), [diag(0), diag(0, 'other-extension')]],
    ]);
    languages.fire([uri('/ws/a.pl')]);

    expect(widget.errorCount).toBe(1);
    expect(item.text).toBe('$(check) perl-lsp: 100 files | 1 error');
    source.dispose();
  });

  test('zero first-party errors omit the error segment', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.setFileCount(42);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([
      [uri('/ws/a.pl'), [diag(1), diag(0, 'other-extension')]],
    ]);
    const source = new HealthWidgetDataSource(
      widget,
      languages,
      makeWorkspace([0, 0, 0, 0, 0]),
    );
    source.start();

    expect(widget.errorCount).toBe(0);
    expect(item.text).toBe('$(check) perl-lsp: 42 files');
    source.dispose();
  });
});

describe('HealthWidgetDataSource — bounded file count', () => {
  test('initial refresh de-duplicates Perl file identities across globs', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const duplicate = uri('/ws/shared.pm');
    let call = 0;
    const workspace: WorkspaceTelemetry = {
      findFiles: async () => {
        call += 1;
        return call <= 2 ? [duplicate] : [];
      },
    };

    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace);
    source.start();
    await source.refreshFileCount();

    expect(widget.fileCount).toBe(1);
    expect(widget.fileCountLowerBound).toBe(false);
    expect(item.text).toContain('1 file');
    source.dispose();
  });

  test('renders a capped scan as a lower bound', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const workspace = makeWorkspace([3, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace, 2);
    source.start();
    await source.refreshFileCount();

    expect(workspace.calls[0]?.maxResults).toBe(3);
    expect(widget.fileCount).toBe(2);
    expect(widget.fileCountLowerBound).toBe(true);
    expect(item.text).toContain('2+ files');
    source.dispose();
  });

  test('file creation invalidates the old count and publishes one replacement', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const workspace = makeWorkspace([1, 0, 0, 0, 0]);
    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace);

    source.start();
    await source.refreshFileCount();
    expect(widget.fileCount).toBe(1);

    workspace.setFileCounts([2, 0, 0, 0, 0]);
    workspace.fireCreate();
    expect(widget.fileCount).toBeUndefined();
    await source.refreshFileCount();

    expect(widget.fileCount).toBe(2);
    expect(workspace.calls).toHaveLength(10);
    source.dispose();
  });

  test('an external filesystem create invalidates the cached count', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const workspace = makeWorkspace([1, 0, 0, 0, 0]);
    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace);

    source.start();
    await source.refreshFileCount();
    expect(widget.fileCount).toBe(1);

    workspace.setFileCounts([2, 0, 0, 0, 0]);
    workspace.fireExternalCreate();
    await source.refreshFileCount();

    expect(widget.fileCount).toBe(2);
    expect(workspace.calls).toHaveLength(10);
    source.dispose();
  });

  test('repeated external events keep at most one scan active and queue one replacement', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    const pending: Array<(uris: Uri[]) => void> = [];
    let active = 0;
    let maxActive = 0;
    let calls = 0;
    const workspace: TestWorkspace = {
      calls: [],
      findFiles: () => {
        calls += 1;
        active += 1;
        maxActive = Math.max(maxActive, active);
        return new Promise<Uri[]>((resolve) => {
          pending.push((uris) => {
            active -= 1;
            resolve(uris);
          });
        });
      },
      createFileSystemWatcher(): TestFileSystemWatcher {
        return {
          onDidCreate(listener): Disposable {
            (
              workspace as TestWorkspace & { fireExternalCreate: (file?: Uri) => void }
            ).fireExternalCreate = (file = uri('/ws/external.pm')) => listener(file);
            return { dispose: () => {} };
          },
          onDidChange: () => ({ dispose: () => {} }),
          onDidDelete: () => ({ dispose: () => {} }),
          dispose: () => {},
        };
      },
      fireCreate: () => {},
      fireExternalCreate: () => {},
      setFileCounts: () => {},
    };
    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace);

    source.start();
    expect(calls).toBe(1);
    for (let index = 0; index < 20; index += 1) {
      workspace.fireExternalCreate();
    }
    expect(calls).toBe(1);
    expect(maxActive).toBe(1);

    pending.shift()?.([]);
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(2);
    expect(maxActive).toBe(1);

    for (let index = 0; index < 5; index += 1) {
      pending.shift()?.([]);
      await Promise.resolve();
    }
    await source.refreshFileCount();
    expect(maxActive).toBe(1);
    source.dispose();
  });

  test('a failed replacement scan clears the prior exact-looking count', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);
    let fail = false;
    let createListener: (() => void) | undefined;
    const workspace: WorkspaceTelemetry = {
      findFiles: async () => {
        if (fail) {
          throw new Error('boom');
        }
        return [];
      },
      onDidCreateFiles: (listener) => {
        createListener = () => listener({ files: [uri('/ws/new.pm')] });
        return { dispose: () => (createListener = undefined) };
      },
    };
    const source = new HealthWidgetDataSource(widget, makeLanguages([]), workspace);

    source.start();
    await source.refreshFileCount();
    expect(widget.fileCount).toBe(0);

    fail = true;
    await source.refreshFileCount();

    expect(widget.fileCount).toBeUndefined();
    expect(item.text).toBe('$(check) perl-lsp');
    source.dispose();
  });
});

describe('HealthWidgetDataSource — dispose', () => {
  test('dispose is idempotent and rejects later diagnostic/file events', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    const languages = makeLanguages([]);
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);
    const source = new HealthWidgetDataSource(widget, languages, workspace);

    source.start();
    await source.refreshFileCount();
    source.dispose();
    source.dispose();

    languages.setDiagnostics([[uri('/x.pl'), [diag(0)]]]);
    languages.fire([uri('/x.pl')]);
    workspace.setFileCounts([3, 0, 0, 0, 0]);
    workspace.fireCreate();

    expect(widget.errorCount).toBe(0);
    expect(widget.fileCount).toBe(0);
  });
});
