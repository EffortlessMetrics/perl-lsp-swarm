/**
 * HealthWidgetDataSource — wires `HealthWidget` setters to client-visible
 * first-party diagnostic and workspace-file observations.
 *
 * Counts remain explicitly bounded:
 *
 * - error count includes only diagnostics whose source is the canonical
 *   `perl-lsp` source;
 * - file count de-duplicates URI subjects across globs;
 * - a capped scan is rendered as a lower bound rather than an exact count;
 * - failed or superseded scans clear the current count instead of retaining a
 *   stale exact-looking value.
 */

import type { HealthWidget } from './healthWidget';
import type { Diagnostic, DiagnosticSeverity, Disposable, Uri } from 'vscode';

interface FileCreateDeleteEvent {
  readonly files: readonly Uri[];
}

interface FileRenameEvent {
  readonly files: readonly { readonly oldUri: Uri; readonly newUri: Uri }[];
}

/** Telemetry subset of `vscode.languages` used by the data source. */
export interface LanguagesTelemetry {
  onDidChangeDiagnostics(listener: (event: { uris: readonly Uri[] }) => void): Disposable;
  getDiagnostics(): Array<[Uri, Diagnostic[]]>;
}

/** Telemetry subset of `vscode.workspace` used by the data source. */
export interface WorkspaceTelemetry {
  findFiles(include: string, exclude?: string | null, maxResults?: number): Thenable<Uri[]>;
  onDidChangeWorkspaceFolders?(listener: () => void): Disposable;
  onDidCreateFiles?(listener: (event: FileCreateDeleteEvent) => void): Disposable;
  onDidDeleteFiles?(listener: (event: FileCreateDeleteEvent) => void): Disposable;
  onDidRenameFiles?(listener: (event: FileRenameEvent) => void): Disposable;
}

/** Perl source-file globs scanned for the current bounded file-count claim. */
const PERL_FILE_GLOBS = ['**/*.pl', '**/*.pm', '**/*.t', '**/*.pod', '**/*.psgi'];

/** Perl source-file extensions used to scope diagnostic aggregation. */
const PERL_EXTENSIONS = new Set(['.pl', '.pm', '.t', '.pod', '.psgi']);

/** Canonical source emitted by current `perllsp` diagnostics. */
const PERL_LSP_DIAGNOSTIC_SOURCE = 'perl-lsp';

/** Upper bound on a single `findFiles` scan to keep the count cheap. */
const FILE_SCAN_CAP = 50_000;

/** Folders excluded from the file-count scan. */
const FILE_SCAN_EXCLUDE = '{**/node_modules/**,**/.git/**,**/target/**,**/.vscode/**}';

/**
 * Build a `HealthWidgetDataSource` from the real VS Code surfaces.
 *
 * The optional scan cap is an explicit test seam; production uses the retained
 * 50,000-subject cap.
 */
export interface HealthWidgetDataSourceDeps {
  languages: LanguagesTelemetry;
  workspace: WorkspaceTelemetry;
  fileScanCap?: number;
}

function isPerlFile(uri: Uri): boolean {
  const uriPath = (uri.fsPath ?? uri.toString()) as string;
  const dot = uriPath.lastIndexOf('.');
  if (dot < 0) {
    return false;
  }
  return PERL_EXTENSIONS.has(uriPath.slice(dot).toLowerCase());
}

function isPerlLspErrorDiagnostic(diagnostic: Diagnostic): boolean {
  // `DiagnosticSeverity.Error === 0`; compare numerically so a fake enum
  // value from tests still matches.
  return (
    (diagnostic.severity as DiagnosticSeverity | undefined) === 0 &&
    diagnostic.source === PERL_LSP_DIAGNOSTIC_SOURCE
  );
}

function uriIdentity(uri: Uri): string {
  return uri.toString();
}

/**
 * Wires first-party error and bounded file counts into `HealthWidget`.
 *
 * Call `start()` once the widget and status bar item are owned by the
 * extension; call `dispose()` on shutdown.
 */
export class HealthWidgetDataSource {
  private readonly disposables: Disposable[] = [];
  private fileCountPromise: Promise<void> | undefined;
  private fileCountGeneration = 0;
  private disposed = false;
  private readonly fileScanCap: number;

  constructor(
    private readonly widget: HealthWidget,
    private readonly languages: LanguagesTelemetry,
    private readonly workspace: WorkspaceTelemetry,
    fileScanCap = FILE_SCAN_CAP,
  ) {
    if (!Number.isInteger(fileScanCap) || fileScanCap < 1) {
      throw new Error('HealthWidgetDataSource file scan cap must be a positive integer');
    }
    this.fileScanCap = fileScanCap;
  }

  static fromDeps(widget: HealthWidget, deps: HealthWidgetDataSourceDeps): HealthWidgetDataSource {
    return new HealthWidgetDataSource(
      widget,
      deps.languages,
      deps.workspace,
      deps.fileScanCap ?? FILE_SCAN_CAP,
    );
  }

  /** Register listeners and push the first file/error counts into the widget. */
  start(): void {
    this.disposables.push(
      this.languages.onDidChangeDiagnostics(() => {
        this.refreshErrorCount();
      }),
    );

    const invalidate = (): void => {
      this.invalidateFileCount();
    };
    const folderListener = this.workspace.onDidChangeWorkspaceFolders?.(invalidate);
    if (folderListener) {
      this.disposables.push(folderListener);
    }
    const createListener = this.workspace.onDidCreateFiles?.(invalidate);
    if (createListener) {
      this.disposables.push(createListener);
    }
    const deleteListener = this.workspace.onDidDeleteFiles?.(invalidate);
    if (deleteListener) {
      this.disposables.push(deleteListener);
    }
    const renameListener = this.workspace.onDidRenameFiles?.(invalidate);
    if (renameListener) {
      this.disposables.push(renameListener);
    }

    this.refreshErrorCount();
    void this.refreshFileCount();
  }

  /** Recompute the current first-party Perl LSP error count. */
  refreshErrorCount(): void {
    if (this.disposed) {
      return;
    }

    let errors = 0;
    for (const [uri, diagnostics] of this.languages.getDiagnostics()) {
      if (!isPerlFile(uri)) {
        continue;
      }
      for (const diagnostic of diagnostics) {
        if (isPerlLspErrorDiagnostic(diagnostic)) {
          errors += 1;
        }
      }
    }
    this.widget.setErrorCount(errors);
  }

  /** Invalidate the prior file subject and schedule one current replacement scan. */
  private invalidateFileCount(): void {
    if (this.disposed) {
      return;
    }
    this.fileCountGeneration += 1;
    this.fileCountPromise = undefined;
    this.widget.setFileCount(undefined);
    void this.refreshFileCount();
  }

  /**
   * Recompute the bounded workspace Perl file count via `findFiles`.
   *
   * Concurrent callers share the current-generation scan. A file/root event
   * invalidates the generation; an older scan cannot publish afterward.
   */
  refreshFileCount(): Promise<void> {
    if (this.disposed) {
      return Promise.resolve();
    }
    if (this.fileCountPromise) {
      return this.fileCountPromise;
    }
    const generation = this.fileCountGeneration;
    const promise = this.runFileCountScan(generation);
    this.fileCountPromise = promise;
    return promise;
  }

  private async runFileCountScan(generation: number): Promise<void> {
    try {
      const identities = new Set<string>();
      let lowerBound = false;
      for (const glob of PERL_FILE_GLOBS) {
        const uris = await this.workspace.findFiles(
          glob,
          FILE_SCAN_EXCLUDE,
          this.fileScanCap + 1,
        );
        if (uris.length > this.fileScanCap) {
          lowerBound = true;
        }
        for (const uri of uris.slice(0, this.fileScanCap)) {
          identities.add(uriIdentity(uri));
        }
      }

      if (this.disposed || generation !== this.fileCountGeneration) {
        return;
      }
      this.widget.setFileCount(identities.size, lowerBound);
    } catch {
      if (this.disposed || generation !== this.fileCountGeneration) {
        return;
      }
      // A failed replacement scan is unavailable, not the prior exact count.
      this.widget.setFileCount(undefined);
    }
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.fileCountGeneration += 1;
    this.fileCountPromise = undefined;
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables.length = 0;
  }
}
