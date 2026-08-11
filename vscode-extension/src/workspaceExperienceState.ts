/**
 * Provider-neutral presentation contract for the installed Perl workspace.
 *
 * This module does not decide readiness or provider semantics. It projects the
 * canonical lifecycle/readiness/result facts supplied by their existing owners
 * into a small, user-legible state vocabulary for VS Code surfaces.
 */

/** User-visible workspace lifecycle states owned by the v0.18 experience contract. */
export type WorkspaceLifecycleState =
  | 'starting'
  | 'resolving_environment'
  | 'indexing_active_context'
  | 'indexing_workspace'
  | 'ready'
  | 'ready_limited'
  | 'configuration_action_required'
  | 'failed';

/** Lifecycle states emitted by the language-client composition owner. */
export type WorkspaceLifecycleInput =
  | 'stopped'
  | 'resolving'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'failed';

/** Preserve environment resolution as a distinct user-facing state. */
export function projectWorkspaceLifecycle(state: WorkspaceLifecycleInput): WorkspaceLifecycleState {
  switch (state) {
    case 'resolving':
      return 'resolving_environment';
    case 'running':
      return 'ready';
    case 'failed':
      return 'failed';
    case 'stopped':
    case 'starting':
    case 'stopping':
      return 'starting';
  }
}

/** User-visible semantic result classes. */
export type ProviderOutcome =
  | 'exact_current'
  | 'bounded_fallback'
  | 'not_ready'
  | 'unsupported_or_dynamic'
  | 'safe_refusal'
  | 'legitimate_empty'
  | 'product_or_instrument_error';

/** One current presentation snapshot. Machine decisions remain in their source receipts. */
export interface WorkspaceExperienceSnapshot {
  readonly lifecycle: WorkspaceLifecycleState;
  readonly providerOutcome?: ProviderOutcome | undefined;
  readonly detail?: string | undefined;
  readonly action?: string | undefined;
  readonly reasonCode?: string | undefined;
}

/** Optional status-bar telemetry that is additive to the semantic state. */
export interface WorkspaceExperienceTelemetry {
  readonly version?: string | undefined;
  readonly fileCount?: number | undefined;
  readonly errorCount?: number | undefined;
  readonly indexingMessage?: string | undefined;
  readonly indexingPercentage?: number | undefined;
}

/** Compatibility mode consumed by the existing HealthWidget API and tests. */
export type LegacyWidgetMode = 'starting' | 'indexing' | 'running' | 'stopped';

/** Rendered state independent of VS Code classes. */
export interface WorkspaceExperiencePresentation {
  readonly mode: LegacyWidgetMode;
  readonly text: string;
  readonly tooltip: string;
  readonly background: 'warning' | 'error' | undefined;
}

function effectiveLifecycle(snapshot: WorkspaceExperienceSnapshot): WorkspaceLifecycleState {
  switch (snapshot.providerOutcome) {
    case 'product_or_instrument_error':
      return 'failed';
    case 'not_ready':
      return snapshot.lifecycle === 'ready' || snapshot.lifecycle === 'ready_limited'
        ? 'indexing_active_context'
        : snapshot.lifecycle;
    case 'bounded_fallback':
    case 'unsupported_or_dynamic':
    case 'safe_refusal':
      return snapshot.lifecycle === 'ready' ? 'ready_limited' : snapshot.lifecycle;
    case 'exact_current':
    case 'legitimate_empty':
    case undefined:
      return snapshot.lifecycle;
  }
}

/**
 * Trailing click affordance.
 *
 * States that have one obvious repair name it, so a user in a broken state is
 * told what clicking does rather than being offered generic "options".
 */
type ClickAffordance = 'click for options' | 'click to restart';

function detailTooltip(
  snapshot: WorkspaceExperienceSnapshot,
  fallback: string,
  affordance: ClickAffordance = 'click for options',
): string {
  const details = [snapshot.detail ?? fallback];
  if (snapshot.reasonCode) {
    details.push(`Reason: ${snapshot.reasonCode}`);
  }
  if (snapshot.action) {
    details.push(`Next: ${snapshot.action}`);
  }
  return `${details.join(' — ')} (${affordance})`;
}

function readyLabel(telemetry: WorkspaceExperienceTelemetry): string {
  const label = telemetry.version ? `perl-lsp v${telemetry.version}` : 'perl-lsp';
  const parts: string[] = [];
  if (telemetry.fileCount !== undefined) {
    parts.push(`${telemetry.fileCount} files`);
  }
  if ((telemetry.errorCount ?? 0) > 0) {
    const errorCount = telemetry.errorCount ?? 0;
    parts.push(`${errorCount} error${errorCount === 1 ? '' : 's'}`);
  }
  return parts.length > 0 ? `${label}: ${parts.join(' | ')}` : label;
}

function indexingLabel(telemetry: WorkspaceExperienceTelemetry): string {
  let message = telemetry.indexingMessage ?? 'Indexing…';
  if ((telemetry.fileCount ?? 0) > 0) {
    message = `Indexing… (${telemetry.fileCount} files)`;
  }
  if ((telemetry.indexingPercentage ?? 0) > 0) {
    message += ` ${Math.round(telemetry.indexingPercentage ?? 0)}%`;
  }
  return message;
}

/**
 * Convert the readiness transport's internal reason into bounded UI text.
 *
 * The current server serializes its Rust enum with Debug formatting for
 * diagnostics. That representation is intentionally not part of the VS Code
 * presentation contract: it is unstable, implementation-shaped, and can
 * contain raw I/O text. Keep this mapper provider-neutral and fail closed for
 * unknown/future reasons.
 */
export function presentIndexReadinessReason(reason?: string): string {
  if (!reason) {
    return 'Limited workspace coverage';
  }

  const normalized = reason.toLowerCase();
  if (normalized.includes('parsestorm') || normalized.includes('parse_storm')) {
    return 'Frequent changes limited workspace coverage';
  }
  if (normalized.includes('ioerror') || normalized.includes('io_error')) {
    return 'Some workspace files could not be read';
  }
  if (normalized.includes('scantimeout') || normalized.includes('scan_timeout')) {
    return 'Workspace indexing reached its time budget';
  }
  if (normalized.includes('maxfiles') || normalized.includes('max_files')) {
    return 'Workspace file limit reached';
  }
  if (normalized.includes('maxsymbols') || normalized.includes('max_symbols')) {
    return 'Workspace symbol limit reached';
  }
  if (normalized.includes('maxcachebytes') || normalized.includes('max_cache_bytes')) {
    return 'Workspace cache limit reached';
  }
  return 'Limited workspace coverage';
}

/**
 * Render one canonical experience snapshot for a compact status surface.
 *
 * Normal exact/current and legitimate-empty outcomes remain quiet. Limited,
 * action-required, and failed states remain visually distinct without exposing
 * raw receipt internals in the first-line label.
 */
export function presentWorkspaceExperience(
  snapshot: WorkspaceExperienceSnapshot,
  telemetry: WorkspaceExperienceTelemetry = {},
): WorkspaceExperiencePresentation {
  const lifecycle = effectiveLifecycle(snapshot);

  switch (lifecycle) {
    case 'starting':
      return {
        mode: 'starting',
        text: '$(sync~spin) Perl LSP',
        tooltip: detailTooltip(snapshot, 'Perl Language Server is starting…'),
        background: undefined,
      };
    case 'resolving_environment':
      return {
        mode: 'starting',
        text: '$(sync~spin) perl-lsp: resolving environment',
        tooltip: detailTooltip(
          snapshot,
          'Perl Language Server is resolving workspace configuration',
        ),
        background: undefined,
      };
    case 'indexing_active_context':
      return {
        mode: 'indexing',
        text: '$(sync~spin) perl-lsp: preparing active file',
        tooltip: detailTooltip(snapshot, 'The active Perl document is still becoming ready'),
        background: undefined,
      };
    case 'indexing_workspace':
      return {
        mode: 'indexing',
        text: `$(sync~spin) perl-lsp: ${indexingLabel(telemetry)}`,
        tooltip: detailTooltip(snapshot, 'Perl Language Server is indexing your workspace'),
        background: undefined,
      };
    case 'ready': {
      const label = readyLabel(telemetry);
      const versionNote = telemetry.version ? ` v${telemetry.version}` : '';
      return {
        mode: 'running',
        text: `$(check) ${label}`,
        tooltip: detailTooltip(snapshot, `Perl Language Server${versionNote} is running`),
        background: undefined,
      };
    }
    case 'ready_limited': {
      const version = telemetry.version ? ` v${telemetry.version}` : '';
      return {
        mode: 'running',
        text: `$(warning) perl-lsp${version}: ready (limited)`,
        tooltip: detailTooltip(
          snapshot,
          'Perl Language Server is ready with a bounded fallback or limitation',
        ),
        background: undefined,
      };
    }
    case 'configuration_action_required':
      return {
        mode: 'stopped',
        text: '$(warning) perl-lsp: action required',
        tooltip: detailTooltip(
          snapshot,
          'Perl Language Server needs configuration or trust action',
        ),
        background: 'warning',
      };
    case 'failed':
      return {
        mode: 'stopped',
        text: snapshot.detail ? '$(error) perl-lsp: failed' : '$(error) perl-lsp: stopped',
        tooltip: detailTooltip(snapshot, 'Perl Language Server has stopped', 'click to restart'),
        background: 'error',
      };
  }
}
