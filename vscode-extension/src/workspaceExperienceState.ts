/**
 * Provider-neutral presentation contract for the installed Perl workspace.
 *
 * This module does not decide readiness or provider semantics. It projects the
 * canonical lifecycle/readiness facts supplied by their existing owners into a
 * small, user-legible state vocabulary for VS Code surfaces.
 *
 * Provider outcomes are intentionally not part of the workspace snapshot.
 * They describe one operation, while this module renders workspace-scoped
 * health. Recent-result identity and explanation remain separate concerns.
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

/** User-visible semantic result classes retained outside workspace health. */
export type ProviderOutcome =
  | 'exact_current'
  | 'bounded_fallback'
  | 'not_ready'
  | 'unsupported_or_dynamic'
  | 'safe_refusal'
  | 'legitimate_empty'
  | 'product_or_instrument_error';

/** One current workspace presentation snapshot. */
export interface WorkspaceExperienceSnapshot {
  readonly lifecycle: WorkspaceLifecycleState;
  readonly detail?: string | undefined;
  readonly action?: string | undefined;
  readonly reasonCode?: string | undefined;
}

/** Optional status-bar telemetry that is additive to the workspace state. */
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

/**
 * Trailing click affordance.
 *
 * States that have one obvious repair name it, so a user in a broken state is
 * told what clicking does rather than being offered generic "options".
 */
type ClickAffordance = 'click for options' | 'click for restart options';

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

  const outerVariant = /^\s*([A-Za-z][A-Za-z0-9_]*)\b/.exec(reason)?.[1];
  switch (outerVariant) {
    case 'Cancelled':
      return 'Workspace indexing was cancelled';
    case 'ParseStorm':
    case 'parse_storm':
      return 'Frequent changes limited workspace coverage';
    case 'IoError':
    case 'io_error':
      return 'Some workspace files could not be read';
    case 'ScanTimeout':
    case 'scan_timeout':
      return 'Workspace indexing reached its time budget';
    case 'ResourceLimit': {
      const kind = /\bkind\s*:\s*([A-Za-z][A-Za-z0-9_]*)\b/.exec(reason)?.[1];
      switch (kind) {
        case 'MaxFiles':
        case 'max_files':
          return 'Workspace file limit reached';
        case 'MaxSymbols':
        case 'max_symbols':
          return 'Workspace symbol limit reached';
        case 'MaxCacheBytes':
        case 'max_cache_bytes':
          return 'Workspace cache limit reached';
        default:
          return 'Limited workspace coverage';
      }
    }
    default:
      return 'Limited workspace coverage';
  }
}

/**
 * Render one canonical workspace snapshot for a compact status surface.
 *
 * Operation-scoped provider outcomes are deliberately excluded. A completion,
 * hover, rename, or formatting result cannot reclassify the entire workspace
 * as ready, limited, or failed.
 */
export function presentWorkspaceExperience(
  snapshot: WorkspaceExperienceSnapshot,
  telemetry: WorkspaceExperienceTelemetry = {},
): WorkspaceExperiencePresentation {
  switch (snapshot.lifecycle) {
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
          'Perl Language Server is ready with bounded workspace coverage',
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
        tooltip: detailTooltip(
          snapshot,
          'Perl Language Server has stopped; choose Restart Server from the status menu',
          'click for restart options',
        ),
        background: 'error',
      };
  }
}
