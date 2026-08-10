/**
 * HealthWidget — status bar item that reflects LSP health.
 *
 * Displays one of:
 *   $(sync~spin) Perl LSP: Indexing...
 *   $(check) perl-lsp: 847 files | 12 errors
 *   $(error) perl-lsp: stopped
 *
 * The widget is driven by four external events:
 *   - LSP state changes (Starting / Running / Stopped)
 *   - $/progress notifications (begin → report → end)
 *   - workspace diagnostic changes (error count)
 *   - explicit file-count updates from the server
 *
 * Callers own the StatusBarItem lifecycle; this class merely reads and
 * mutates its text / tooltip / backgroundColor properties.
 */

import * as vscode from 'vscode';

/**
 * Mirror of `State` from vscode-languageclient.
 *
 * Using numeric values here avoids importing from vscode-languageclient in the
 * widget so that unit tests can run without instantiating the full LSP client.
 * The numeric values MUST stay in sync with the upstream enum:
 *   Stopped = 1, Running = 2, Starting = 3
 * (vscode-languageclient/lib/common/client.ts)
 */
export const enum ClientState {
  Stopped = 1,
  Running = 2,
  Starting = 3,
}

/** Progress token received from $/progress notifications. */
export type ProgressToken = string | number;

/** Subset of $/progress `begin` payload used by the widget. */
export interface ProgressBeginPayload {
  kind: 'begin';
  title: string;
  message?: string;
}

/** Subset of $/progress `report` payload used by the widget. */
export interface ProgressReportPayload {
  kind: 'report';
  message?: string;
  percentage?: number;
}

/** $/progress `end` payload. */
export interface ProgressEndPayload {
  kind: 'end';
  message?: string;
}

export type ProgressPayload = ProgressBeginPayload | ProgressReportPayload | ProgressEndPayload;

/** Internal display state of the widget. */
export type IndexReadinessState = 'building' | 'ready' | 'ready_limited';

export type WidgetMode = 'starting' | 'indexing' | 'running' | 'ready_limited' | 'stopped';

export class HealthWidget {
  private _mode: WidgetMode = 'starting';
  private _fileCount: number | undefined = undefined;
  private _errorCount = 0;
  private _indexingMessage: string | undefined = undefined;
  private _indexingPercentage: number | undefined = undefined;
  private _activeTokens = new Set<ProgressToken>();
  private _version: string | undefined = undefined;
  private _readinessState: IndexReadinessState = 'ready';
  private _readinessReason: string | undefined = undefined;

  constructor(private readonly item: vscode.StatusBarItem) {
    this._render();
  }

  // -----------------------------------------------------------------------
  // External API
  // -----------------------------------------------------------------------

  /** Called when the LSP client state changes. */
  onStateChange(state: ClientState): void {
    switch (state) {
      case ClientState.Starting:
        this._setMode('starting');
        break;
      case ClientState.Running:
        // Only move to 'running' if no active indexing progress tokens.
        if (this._activeTokens.size === 0) {
          this._setMode(this._modeForReadiness());
        }
        break;
      case ClientState.Stopped:
        this._activeTokens.clear();
        this._indexingMessage = undefined;
        this._indexingPercentage = undefined;
        // The two counts have different owners, so they are handled
        // differently here.
        //
        // `_errorCount` came from the server session that just ended, so it IS
        // stale — clear it. It repopulates on its own: HealthWidgetDataSource
        // subscribes to `onDidChangeDiagnostics` and calls
        // `refreshErrorCount()`, which the restarted server triggers as it
        // republishes diagnostics.
        //
        // `_fileCount` is client-owned — HealthWidgetDataSource computes it
        // from `vscode.workspace.findFiles`, never from the server — so a
        // server restart does not invalidate it; the files on disk did not
        // change. Do NOT clear it. `refreshFileCount()` short-circuits on the
        // cached `fileCountPromise`, which is only invalidated when workspace
        // folders change, and `start()` runs once per data-source lifetime, so
        // nothing would recompute it. Clearing here blanked the count until the
        // workspace changed or the extension host reloaded — worse than the
        // stale value this reset exists to avoid.
        this._errorCount = 0;
        this._setMode('stopped');
        break;
    }
  }

  /** Called with each $/progress notification from the LSP server. */
  onProgress(token: ProgressToken, payload: ProgressPayload): void {
    if (payload.kind === 'begin') {
      this._activeTokens.add(token);
      this._indexingMessage = payload.message ?? payload.title;
      this._setMode('indexing');
    } else if (payload.kind === 'report') {
      if (this._activeTokens.has(token)) {
        if (payload.message !== undefined) {
          this._indexingMessage = payload.message;
        }
        this._indexingPercentage = payload.percentage;
        this._render();
      }
    } else if (payload.kind === 'end') {
      this._activeTokens.delete(token);
      if (this._activeTokens.size === 0) {
        this._indexingMessage = undefined;
        this._indexingPercentage = undefined;
        this._setMode(this._modeForReadiness());
      }
    }
  }

  /** Consume canonical server readiness without inferring it from progress. */
  onIndexReadinessState(state: IndexReadinessState, reason?: string): void {
    this._readinessState = state;
    this._readinessReason = reason;
    if (this._activeTokens.size === 0 && this._mode !== 'stopped') {
      this._setMode(this._modeForReadiness());
    }
  }

  /** Update the workspace-wide file count (from server telemetry). */
  setFileCount(count: number): void {
    this._fileCount = count;
    this._render();
  }

  /** Replace the current workspace-wide error count. */
  setErrorCount(count: number): void {
    this._errorCount = count;
    this._render();
  }

  /** Set the server version string from the initialize handshake. */
  setVersion(version: string): void {
    this._version = version;
    this._render();
  }

  /** Current display mode (useful for testing). */
  get mode(): WidgetMode {
    return this._mode;
  }

  /** Current file count (undefined until first update). */
  get fileCount(): number | undefined {
    return this._fileCount;
  }

  /** Current error count. */
  get errorCount(): number {
    return this._errorCount;
  }

  /** Server version from the initialize handshake (undefined until set). */
  get version(): string | undefined {
    return this._version;
  }

  /** Current canonical index state. */
  get readinessState(): IndexReadinessState {
    return this._readinessState;
  }

  // -----------------------------------------------------------------------
  // Private helpers
  // -----------------------------------------------------------------------

  private _setMode(mode: WidgetMode): void {
    this._mode = mode;
    this._render();
  }

  private _modeForReadiness(): WidgetMode {
    return this._readinessState === 'building'
      ? 'indexing'
      : this._readinessState === 'ready_limited'
        ? 'ready_limited'
        : 'running';
  }

  private _render(): void {
    switch (this._mode) {
      case 'starting':
        this.item.text = '$(sync~spin) Perl LSP';
        this.item.tooltip = 'Perl Language Server is starting\u2026 (click for options)';
        this.item.backgroundColor = undefined;
        break;

      case 'indexing': {
        let msg = this._indexingMessage ?? 'Indexing\u2026';
        // Show file count and percentage when available (UX_GAP_1.1)
        if (this._fileCount !== undefined && this._fileCount > 0) {
          msg = `Indexing\u2026 (${this._fileCount} files)`;
        }
        if (this._indexingPercentage !== undefined && this._indexingPercentage > 0) {
          msg += ` ${Math.round(this._indexingPercentage)}%`;
        }
        this.item.text = `$(sync~spin) perl-lsp: ${msg}`;
        this.item.tooltip = 'Perl Language Server is indexing your workspace (click for options)';
        this.item.backgroundColor = undefined;
        break;
      }

      case 'running': {
        const label = this._version ? `perl-lsp v${this._version}` : 'perl-lsp';
        const parts: string[] = [];
        if (this._fileCount !== undefined) {
          parts.push(`${this._fileCount} files`);
        }
        if (this._errorCount > 0) {
          parts.push(`${this._errorCount} error${this._errorCount === 1 ? '' : 's'}`);
        }
        const detail = parts.length > 0 ? `: ${parts.join(' | ')}` : '';
        this.item.text = `$(check) ${label}${detail}`;
        const versionNote = this._version ? ` v${this._version}` : '';
        this.item.tooltip = `Perl Language Server${versionNote} is running (click for options)`;
        this.item.backgroundColor = undefined;
        break;
      }

      case 'ready_limited': {
        const label = this._version ? `perl-lsp v${this._version}` : 'perl-lsp';
        this.item.text = `$(check) ${label} (limited)`;
        const reason = this._readinessReason ? `: ${this._readinessReason}` : '';
        this.item.tooltip = `Perl Language Server is ready with limited workspace coverage${reason} (click for options)`;
        this.item.backgroundColor = undefined;
        break;
      }

      case 'stopped':
        this.item.text = '$(error) perl-lsp: stopped';
        this.item.tooltip = 'Perl Language Server has stopped (click to restart)';
        this.item.backgroundColor = new vscode.ThemeColor('statusBarItem.errorBackground');
        break;
    }
  }
}
