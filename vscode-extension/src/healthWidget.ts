/**
 * HealthWidget — status bar item that reflects LSP health.
 *
 * The widget consumes the existing language-client lifecycle and progress
 * owners. Provider outcomes are retained separately for on-demand explanation;
 * one operation cannot reclassify workspace-scoped health.
 *
 * Callers own the StatusBarItem lifecycle; this class merely reads and mutates
 * its text / tooltip / backgroundColor properties.
 */

import * as vscode from 'vscode';
import type { IndexReadinessState } from './activeDocumentReadiness';
import {
  presentIndexReadinessReason,
  presentWorkspaceExperience,
  type LegacyWidgetMode,
  type ProviderOutcome,
  type WorkspaceExperienceSnapshot,
  type WorkspaceLifecycleState,
} from './workspaceExperienceState';

export type { IndexReadinessState };

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

/** Compatibility display mode retained for existing callers and tests. */
export type WidgetMode = LegacyWidgetMode;

export interface WorkspaceExperienceUpdate {
  readonly detail?: string;
  readonly action?: string;
  readonly reasonCode?: string;
}

export class HealthWidget {
  private _mode: WidgetMode = 'starting';
  private _experience: WorkspaceExperienceSnapshot = { lifecycle: 'starting' };
  private _providerOutcome: ProviderOutcome | undefined;
  private _providerDetail: string | undefined;
  private _providerAction: string | undefined;
  private _providerReasonCode: string | undefined;
  private _fileCount: number | undefined = undefined;
  private _errorCount = 0;
  private _indexingMessage: string | undefined = undefined;
  private _indexingPercentage: number | undefined = undefined;
  private _activeTokens = new Set<ProgressToken>();
  private _version: string | undefined = undefined;
  private _readinessState: IndexReadinessState = 'ready';
  private _readinessReason: string | undefined = undefined;
  private _enhancedReadinessAvailable = false;

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
        this._enhancedReadinessAvailable = false;
        this.setWorkspaceLifecycleState('starting');
        break;
      case ClientState.Running:
        // Only move to ready if no active indexing progress tokens.
        if (this._activeTokens.size === 0) {
          this._applyReadinessLifecycle();
        }
        break;
      case ClientState.Stopped:
        this._activeTokens.clear();
        this._indexingMessage = undefined;
        this._indexingPercentage = undefined;
        // The error count belonged to the server session that just ended, so
        // clear it. The file count is derived from the client-visible workspace
        // and remains current across a server restart.
        this._errorCount = 0;
        // A client stop is not, by itself, a product failure: normal restart
        // and extension shutdown use the same language-client state. The
        // authoritative crash/diagnosis path promotes an unexpected stop to
        // `failed` explicitly after it has established that meaning.
        this.setWorkspaceLifecycleState('starting', {
          detail: 'Perl Language Server is restarting or shutting down.',
          action: 'Wait for the server to reconnect, or start it again.',
          reasonCode: 'client_stopped',
        });
        break;
    }
  }

  /** Called with each $/progress notification from the LSP server. */
  onProgress(token: ProgressToken, payload: ProgressPayload): void {
    if (payload.kind === 'begin') {
      this._activeTokens.add(token);
      this._indexingMessage = payload.message ?? payload.title;
      this.setWorkspaceLifecycleState('indexing_workspace');
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
        this._applyReadinessLifecycle();
      }
    }
  }

  /** Seed the visible readiness state while waiting for the server contract. */
  seedIndexReadinessState(state: IndexReadinessState): void {
    this._readinessState = state;
    this._readinessReason = undefined;
    if (this._activeTokens.size === 0) {
      this._applyReadinessLifecycle();
    }
  }

  /** Consume canonical server readiness without inferring it from progress. */
  onIndexReadinessState(state: IndexReadinessState, reason?: string): void {
    this._enhancedReadinessAvailable = true;
    this._readinessState = state;
    this._readinessReason = reason;
    if (this._activeTokens.size === 0) {
      this._applyReadinessLifecycle();
    }
  }

  /**
   * Present one canonical lifecycle state without making the widget its owner.
   *
   * A lifecycle transition clears the prior operation-scoped provider outcome,
   * preventing an old fallback, refusal, or failure from surviving restart or
   * a later readiness generation.
   */
  setWorkspaceLifecycleState(
    lifecycle: WorkspaceLifecycleState,
    update: WorkspaceExperienceUpdate = {},
  ): void {
    this._experience = {
      lifecycle,
      detail: update.detail,
      action: update.action,
      reasonCode: update.reasonCode,
    };
    this._clearProviderOutcome();
    this._render();
  }

  /**
   * Retain the latest operation-scoped provider result without reclassifying
   * workspace health. A future recent-result surface may consume these fields.
   */
  setProviderOutcome(
    providerOutcome: ProviderOutcome | undefined,
    update: WorkspaceExperienceUpdate = {},
  ): void {
    this._providerOutcome = providerOutcome;
    this._providerDetail = update.detail;
    this._providerAction = update.action;
    this._providerReasonCode = update.reasonCode;
  }

  /** Update the workspace-wide file count. */
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

  /** Current compatibility display mode. */
  get mode(): WidgetMode {
    return this._mode;
  }

  /** Current canonical user-facing lifecycle state. */
  get lifecycleState(): WorkspaceLifecycleState {
    return this._experience.lifecycle;
  }

  /** Latest operation-scoped provider outcome, when one has been recorded. */
  get providerOutcome(): ProviderOutcome | undefined {
    return this._providerOutcome;
  }

  get providerDetail(): string | undefined {
    return this._providerDetail;
  }

  get providerAction(): string | undefined {
    return this._providerAction;
  }

  get providerReasonCode(): string | undefined {
    return this._providerReasonCode;
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

  /** Current canonical index readiness state from the server. */
  get readinessState(): IndexReadinessState {
    return this._readinessState;
  }

  /** Whether this server has emitted the enhanced readiness notification. */
  get enhancedReadinessAvailable(): boolean {
    return this._enhancedReadinessAvailable;
  }

  /** Bounded user-facing readiness detail for status surfaces. */
  get readinessReason(): string | undefined {
    return this._readinessState === 'ready_limited'
      ? presentIndexReadinessReason(this._readinessReason)
      : undefined;
  }

  /** Current presentation detail/action supplied by the owning lifecycle path. */
  get experienceDetail(): string | undefined {
    return this._experience.detail;
  }

  get experienceAction(): string | undefined {
    return this._experience.action;
  }

  // -----------------------------------------------------------------------
  // Private helpers
  // -----------------------------------------------------------------------

  private _clearProviderOutcome(): void {
    this._providerOutcome = undefined;
    this._providerDetail = undefined;
    this._providerAction = undefined;
    this._providerReasonCode = undefined;
  }

  private _applyReadinessLifecycle(): void {
    switch (this._readinessState) {
      case 'building':
        this.setWorkspaceLifecycleState('indexing_workspace');
        break;
      case 'ready_limited': {
        const detail = presentIndexReadinessReason(this._readinessReason);
        this.setWorkspaceLifecycleState('ready_limited', {
          detail,
          reasonCode: 'index_ready_limited',
        });
        break;
      }
      case 'ready':
        this.setWorkspaceLifecycleState('ready');
        break;
    }
  }

  private _render(): void {
    const presentation = presentWorkspaceExperience(this._experience, {
      version: this._version,
      fileCount: this._fileCount,
      errorCount: this._errorCount,
      indexingMessage: this._indexingMessage,
      indexingPercentage: this._indexingPercentage,
    });

    this._mode = presentation.mode;
    this.item.text = presentation.text;
    this.item.tooltip = presentation.tooltip;
    this.item.backgroundColor =
      presentation.background === 'warning'
        ? new vscode.ThemeColor('statusBarItem.warningBackground')
        : presentation.background === 'error'
          ? new vscode.ThemeColor('statusBarItem.errorBackground')
          : undefined;
  }
}
