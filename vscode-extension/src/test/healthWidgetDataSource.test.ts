/**
 * Unit tests for HealthWidgetDataSource — the production wiring that feeds
 * file/error counts into `HealthWidget` from client-side telemetry (#4620).
 *
 * These tests assert the full event → widget → status-bar-text chain, not just
 * accessor values, so they cover the externally observable effect the issue
 * requires.
 */

import { HealthWidget, ClientState } from '../healthWidget';
import { HealthWidgetDataSource } from '../healthWidgetDataSource';
import type { LanguagesTelemetry, WorkspaceTelemetry } from '../healthWidgetDataSource';
import type { ThemeColor } from 'vscode';
import type { Diagnostic, Disposable, StatusBarItem, Uri } from 'vscode';

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

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

function diag(severity: number): Diagnostic {
  return { severity } as unknown as Diagnostic;
}

/** Captures the diagnostics-change listener so tests can fire it. */
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
      if (listener) listener({ uris });
    },
    setDiagnostics(next: Array<[Uri, Diagnostic[]]>): void {
      current = next;
    },
  };
}

interface TestWorkspace extends WorkspaceTelemetry {
  calls: string[];
}

function makeWorkspace(fileCounts: number[]): TestWorkspace {
  const calls: string[] = [];
  return {
    findFiles(include: string): Thenable<Uri[]> {
      calls.push(include);
      const count = fileCounts.shift() ?? 0;
      const uris: Uri[] = [];
      for (let i = 0; i < count; i++) {
        uris.push(uri(`/ws/file${i}${include.slice(-3)}`));
      }
      return Promise.resolve(uris);
    },
    calls,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('HealthWidgetDataSource — error count wiring', () => {
  test('initial refresh pushes the Perl error count into the status bar', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([
      [uri('/ws/lib.pm'), [diag(0), diag(1), diag(0)]], // 2 errors
      [uri('/ws/t/test.t'), [diag(0)]], // 1 error
      [uri('/ws/readme.md'), [diag(0)]], // ignored — not Perl
    ]);
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();

    expect(widget.errorCount).toBe(3);
    expect(item.text).toContain('3 errors');
    source.dispose();
  });

  test('onDidChangeDiagnostics event updates the status bar text', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.setFileCount(100);
    widget.onStateChange(ClientState.Running);
    expect(item.text).toBe('$(check) perl-lsp: 100 files');

    const languages = makeLanguages([[uri('/ws/a.pl'), [diag(0), diag(0)]]]);
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    expect(item.text).toBe('$(check) perl-lsp: 100 files | 2 errors');

    // A new diagnostic event arrives with a different count.
    languages.setDiagnostics([[uri('/ws/a.pl'), [diag(0)]]]);
    languages.fire([uri('/ws/a.pl')]);
    expect(widget.errorCount).toBe(1);
    expect(item.text).toBe('$(check) perl-lsp: 100 files | 1 error');

    source.dispose();
  });

  test('zero errors omit the error segment from the status bar', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.setFileCount(42);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([[uri('/ws/a.pl'), [diag(1), diag(2)]]]); // warnings only
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    expect(widget.errorCount).toBe(0);
    expect(item.text).toBe('$(check) perl-lsp: 42 files');
    source.dispose();
  });
});

describe('HealthWidgetDataSource — file count wiring', () => {
  test('initial refresh sums Perl files across all globs into the status bar', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([]);
    // 5 globs scanned: *.pl=10, *.pm=8, *.t=4, *.pod=2, *.psgi=1  → 25
    const workspace = makeWorkspace([10, 8, 4, 2, 1]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    await source.refreshFileCount(); // ensure the async scan settles

    expect(widget.fileCount).toBe(25);
    expect(item.text).toContain('25 files');
    expect(workspace.calls).toHaveLength(5);
    source.dispose();
  });

  test('refreshFileCount runs only once per data-source lifetime', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([]);
    const workspace = makeWorkspace([3, 2, 1, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    await source.refreshFileCount();
    await source.refreshFileCount(); // no-op — already refreshed

    expect(workspace.calls).toHaveLength(5);
    expect(widget.fileCount).toBe(6);
    source.dispose();
  });

  test('a failed findFiles scan leaves the count unset rather than throwing', async () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    widget.onStateChange(ClientState.Running);

    const languages = makeLanguages([]);
    const workspace: WorkspaceTelemetry = {
      findFiles: () => Promise.reject(new Error('boom')),
    };

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    await source.refreshFileCount();

    expect(widget.fileCount).toBeUndefined();
    expect(item.text).toBe('$(check) perl-lsp'); // no counts segment
    source.dispose();
  });
});

describe('HealthWidgetDataSource — dispose', () => {
  test('dispose is idempotent and clears listeners', () => {
    const item = makeStatusBarItem();
    const widget = new HealthWidget(item);
    const languages = makeLanguages([]);
    const workspace = makeWorkspace([0, 0, 0, 0, 0]);

    const source = new HealthWidgetDataSource(widget, languages, workspace);
    source.start();
    source.dispose();
    source.dispose(); // second dispose must not throw
    // Firing after dispose should not update the widget.
    languages.setDiagnostics([[uri('/x.pl'), [diag(0)]]]);
    languages.fire([uri('/x.pl')]);
    expect(widget.errorCount).toBe(0);
  });
});
