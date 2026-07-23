/**
 * HealthWidgetDataSource — wires `HealthWidget` setters to the client-side
 * telemetry events that carry file/error counts.
 *
 * The perl-lsp server does not (as of the Index Lifecycle v1 spec) emit a
 * custom notification carrying workspace file/error counts, so the data source
 * derives them from VSCode's own surfaces:
 *
 *   - **error count**: aggregated from `vscode.languages.getDiagnostics()`,
 *     scoped to Perl documents, on every `onDidChangeDiagnostics` event.
 *   - **file count**: the number of Perl source files in the workspace, via
 *     `vscode.workspace.findFiles`, refreshed once after activation.
 *
 * This closes the gap described in #4620: `HealthWidget.setFileCount` /
 * `setErrorCount` were implemented and unit-tested but never called from
 * production code, so the running-state status bar never showed the
 * `perl-lsp v<x>: <N> files | <M> errors` indicator the widget promises.
 *
 * The data source is best-effort: telemetry failures never throw into the
 * extension host. Counts are derived purely from client-visible state, so they
 * reflect what the user actually sees rather than server-internal counters.
 */

import type { HealthWidget } from './healthWidget';
import type { Diagnostic, DiagnosticSeverity, Disposable, Uri } from 'vscode';

/** Telemetry subset of `vscode.languages` used by the data source. */
export interface LanguagesTelemetry {
  onDidChangeDiagnostics(listener: (event: { uris: readonly Uri[] }) => void): Disposable;
  getDiagnostics(): Array<[Uri, Diagnostic[]]>;
}

/** Telemetry subset of `vscode.workspace` used by the data source. */
export interface WorkspaceTelemetry {
  findFiles(include: string, exclude?: string | null, maxResults?: number): Thenable<Uri[]>;
}

/** Perl source-file globs scanned for the workspace file count. */
const PERL_FILE_GLOBS = ['**/*.pl', '**/*.pm', '**/*.t', '**/*.pod', '**/*.psgi'];

/** Perl source-file extensions used to scope diagnostic aggregation. */
const PERL_EXTENSIONS = new Set(['.pl', '.pm', '.t', '.pod', '.psgi']);

/** Upper bound on a single `findFiles` scan to keep the count cheap. */
const FILE_SCAN_CAP = 50_000;

/** Folders excluded from the file-count scan. */
const FILE_SCAN_EXCLUDE = '{**/node_modules/**,**/.git/**,**/target/**,**/.vscode/**}';

/**
 * Build a `HealthWidgetDataSource` from the real `vscode` surfaces.
 *
 * Kept as a factory so production wiring passes `vscode.languages` /
 * `vscode.workspace` directly while tests inject fakes.
 */
export interface HealthWidgetDataSourceDeps {
  languages: LanguagesTelemetry;
  workspace: WorkspaceTelemetry;
}

function isPerlFile(uri: Uri): boolean {
  const path = (uri.fsPath ?? uri.toString()) as string;
  const dot = path.lastIndexOf('.');
  if (dot < 0) {
    return false;
  }
  return PERL_EXTENSIONS.has(path.slice(dot).toLowerCase());
}

function isErrorDiagnostic(diagnostic: Diagnostic): boolean {
  // `DiagnosticSeverity.Error === 0`; compare numerically so a fake enum
  // value from tests still matches.
  return (diagnostic.severity as DiagnosticSeverity | undefined) === 0;
}

/**
 * Wires `HealthWidget.setFileCount` / `setErrorCount` to client-side telemetry.
 *
 * Call `start()` once the widget and status bar item are owned by the
 * extension; call `dispose()` on shutdown. The data source registers a
 * diagnostics-change listener and performs an initial refresh of both counts.
 */
export class HealthWidgetDataSource {
  private readonly disposables: Disposable[] = [];
  private fileCountPromise: Promise<void> | undefined;

  constructor(
    private readonly widget: HealthWidget,
    private readonly languages: LanguagesTelemetry,
    private readonly workspace: WorkspaceTelemetry,
  ) {}

  static fromDeps(widget: HealthWidget, deps: HealthWidgetDataSourceDeps): HealthWidgetDataSource {
    return new HealthWidgetDataSource(widget, deps.languages, deps.workspace);
  }

  /** Register listeners and push the first file/error counts into the widget. */
  start(): void {
    this.disposables.push(
      this.languages.onDidChangeDiagnostics(() => {
        this.refreshErrorCount();
      }),
    );
    this.refreshErrorCount();
    void this.refreshFileCount();
  }

  /** Recompute the workspace-wide Perl error count from live diagnostics. */
  refreshErrorCount(): void {
    let errors = 0;
    for (const [uri, diagnostics] of this.languages.getDiagnostics()) {
      if (!isPerlFile(uri)) {
        continue;
      }
      for (const diagnostic of diagnostics) {
        if (isErrorDiagnostic(diagnostic)) {
          errors += 1;
        }
      }
    }
    this.widget.setErrorCount(errors);
  }

  /**
   * Recompute the workspace Perl file count via `findFiles`.
   *
   * Runs at most once per data-source lifetime: the count is stable until the
   * workspace folders change, and a fresh activation rebuilds the data source.
   * Concurrent callers share the in-flight scan promise so an `await` after
   * `start()` waits for the real scan rather than returning as a no-op.
   * Failures are swallowed — telemetry must never throw into the host.
   */
  refreshFileCount(): Promise<void> {
    if (this.fileCountPromise) {
      return this.fileCountPromise;
    }
    this.fileCountPromise = this.runFileCountScan();
    return this.fileCountPromise;
  }

  private async runFileCountScan(): Promise<void> {
    try {
      let total = 0;
      for (const glob of PERL_FILE_GLOBS) {
        const uris = await this.workspace.findFiles(glob, FILE_SCAN_EXCLUDE, FILE_SCAN_CAP);
        total += uris.length;
      }
      this.widget.setFileCount(total);
    } catch {
      // Best-effort telemetry: a failed scan leaves the count at its prior
      // value (undefined → omitted from the status bar) rather than throwing.
    }
  }

  dispose(): void {
    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables.length = 0;
  }
}
