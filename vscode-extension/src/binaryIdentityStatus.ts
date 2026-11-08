import type {
  BinaryCompatibilityState,
  BinaryIdentityResponseV1,
  ExecutableIdentity,
} from './binaryIdentityProtocol.generated';

export type BinaryIdentityUiState =
  | 'ready_exact'
  | 'ready_partial'
  | 'update_or_repair_required'
  | 'configured_binary_incompatible'
  | 'managed_binary_missing'
  | 'unsupported'
  | 'not_proven';

export type BinaryIdentityAction =
  | 'none'
  | 'refresh_identity'
  | 'repair_managed_pair'
  | 'inspect_configured_binary'
  | 'copy_support_packet';

/**
 * The role of the binary the host actually resolved. `unknown` covers a
 * resolved binary the host cannot attribute to the managed install or to an
 * explicit `perl-lsp.serverPath` (for example a PATH or bundled fallback);
 * the host must not claim `managed` for it (#6854, #9096).
 */
export type SelectedBinaryRole = 'managed' | 'user_supplied' | 'unknown';

export interface BinaryIdentityPresentation {
  state: BinaryIdentityUiState;
  action: BinaryIdentityAction;
  quiet: boolean;
  label: string;
  detail: string;
  /** Stable reason codes from the server verdict, or a client-side evidence code. */
  reasons: readonly string[];
  supportPacket: string;
  noticeKey?: string;
}

const COMPATIBILITY_STATES: ReadonlySet<string> = new Set<BinaryCompatibilityState>([
  'exact_match',
  'compatible_partial',
  'mismatch',
  'unsupported',
  'stale',
  'not_proven',
]);

/** Reason codes are bounded snake_case tokens; anything else is not echoed. */
const REASON_CODE = /^[a-z][a-z0-9_]{0,63}$/;
const MAX_REASONS = 16;
const MAX_LIMITATIONS = 8;
const MAX_STRING = 200;
const MAX_DETAIL_REASONS = 4;

/** Client-side evidence codes; they never come from the server. */
export const CLIENT_REASON_RESPONSE_MALFORMED = 'client_response_malformed';
export const CLIENT_REASON_COMPATIBILITY_UNKNOWN = 'client_compatibility_unknown';
export const CLIENT_REASON_SERVER_NOT_RUNNING = 'client_server_not_running';
export const CLIENT_REASON_MANAGED_BINARY_ABSENT = 'client_managed_binary_absent';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function boundedReasons(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const reasons = value
    .filter((reason): reason is string => typeof reason === 'string' && REASON_CODE.test(reason))
    .slice(0, MAX_REASONS);
  return [...new Set(reasons)].sort();
}

/**
 * Path-, URL-, and secret-shaped strings are replaced before anything reaches
 * the clipboard. The identity schema carries no path fields, so a value that
 * looks like one is either a server defect or a field the client does not
 * know; neither may be copied verbatim (#10307).
 */
const UNSAFE_STRING = [
  /[\\/]/, // any path separator, including URLs
  /^[A-Za-z]:/, // drive letter
  /^~/, // home-relative path
  /(token|secret|password|passwd|bearer|api[_-]?key)/i,
];

interface Redactor {
  redactions: number;
}

function safeString(value: unknown, redactor: Redactor): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value !== 'string') {
    redactor.redactions += 1;
    return '<redacted>';
  }
  if (value.length > MAX_STRING || UNSAFE_STRING.some((pattern) => pattern.test(value))) {
    redactor.redactions += 1;
    return '<redacted>';
  }
  return value;
}

/**
 * Repository slugs are the one schema-sanctioned value with a `/`; they are
 * kept only when they are a bare `owner/name` slug.
 */
function safeRepositorySlug(value: unknown, redactor: Redactor): string | undefined {
  if (typeof value === 'string' && /^[A-Za-z0-9_.-]{1,64}\/[A-Za-z0-9_.-]{1,64}$/.test(value)) {
    return value;
  }
  return safeString(value, redactor);
}

function definedEntries(record: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(record).filter(([, value]) => value !== undefined));
}

function projectPacket(value: unknown, redactor: Redactor): Record<string, unknown> | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!isRecord(value)) {
    redactor.redactions += 1;
    return { malformed: true };
  }
  const product = isRecord(value.product) ? value.product : {};
  const binary = isRecord(value.binary) ? value.binary : {};
  const build = isRecord(value.build) ? value.build : {};
  const artifact = isRecord(value.artifact) ? value.artifact : {};
  const compatibility = isRecord(value.compatibility) ? value.compatibility : {};
  // Closed projection: only schema fields are copied, so an unknown field that
  // happens to carry a local path never reaches the clipboard.
  return definedEntries({
    schema_version: safeString(value.schema_version, redactor),
    product: definedEntries({
      name: safeString(product.name, redactor),
      public_repository: safeRepositorySlug(product.public_repository, redactor),
      development_repository: safeRepositorySlug(product.development_repository, redactor),
    }),
    binary: definedEntries({
      executable: safeString(binary.executable, redactor),
      cargo_package: safeString(binary.cargo_package, redactor),
      role: safeString(binary.role, redactor),
      version: safeString(binary.version, redactor),
    }),
    build: definedEntries({
      source_revision: safeString(build.source_revision, redactor),
      source_tree_digest: safeString(build.source_tree_digest, redactor),
      target: safeString(build.target, redactor),
      profile: safeString(build.profile, redactor),
      identity_state: safeString(build.identity_state, redactor),
    }),
    artifact: definedEntries({
      role: safeString(artifact.role, redactor),
      digest: safeString(artifact.digest, redactor),
      candidate_identity: safeString(artifact.candidate_identity, redactor),
    }),
    compatibility: definedEntries({
      expected_product_identity_version:
        typeof compatibility.expected_product_identity_version === 'number'
          ? compatibility.expected_product_identity_version
          : undefined,
      dap_posture: safeString(compatibility.dap_posture, redactor),
    }),
  });
}

function projectExpectedExtension(value: unknown, redactor: Redactor): Record<string, unknown> {
  if (!isRecord(value)) {
    return {};
  }
  return definedEntries({
    id: safeString(value.id, redactor),
    version: safeString(value.version, redactor),
    candidate_identity: safeString(value.candidate_identity, redactor),
    target: safeString(value.target, redactor),
    binary_artifact_role: safeString(value.binary_artifact_role, redactor),
    authority_identity: safeString(value.authority_identity, redactor),
  });
}

function projectLimitations(value: unknown, redactor: Redactor): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .slice(0, MAX_LIMITATIONS)
    .map((limitation) => safeString(limitation, redactor) ?? '<redacted>');
}

/**
 * Builds the copyable identity packet from a closed projection of the
 * response. `redacted_fields` counts every value the client replaced, so a
 * reader can tell a clean packet from a scrubbed one.
 */
function supportPacket(
  response: unknown,
  state: BinaryIdentityUiState,
  reasons: readonly string[],
): string {
  const redactor: Redactor = { redactions: 0 };
  const source = isRecord(response) ? response : {};
  const packet = definedEntries({
    feature_version:
      typeof source.feature_version === 'number' ? source.feature_version : undefined,
    ui_state: state,
    compatibility:
      typeof source.compatibility === 'string' && COMPATIBILITY_STATES.has(source.compatibility)
        ? source.compatibility
        : undefined,
    reasons: [...reasons],
    server: projectPacket(source.server, redactor),
    dap: projectPacket(source.dap, redactor),
    expected_extension: projectExpectedExtension(source.expected_extension, redactor),
    server_instance_id: safeString(source.server_instance_id, redactor),
    environment_snapshot_id: safeString(source.environment_snapshot_id, redactor),
    server_reported_copy_safe: typeof source.redacted === 'boolean' ? source.redacted : undefined,
    limitations: projectLimitations(source.limitations, redactor),
  });
  return JSON.stringify(
    { ...packet, redacted: true, redacted_fields: redactor.redactions },
    null,
    2,
  );
}

/**
 * The notice key names the unresolved condition, not the process: a restart
 * or extension-host reload that reproduces the same mismatch must not raise a
 * second notice, while a changed reason set is a new condition.
 */
function noticeKey(state: BinaryIdentityUiState, reasons: readonly string[]): string {
  return JSON.stringify({ state, reasons });
}

function displayName(value: string): string {
  return safeString(value, { redactions: 0 }) ?? '<redacted>';
}

function reasonSuffix(reasons: readonly string[]): string {
  if (reasons.length === 0) {
    return '';
  }
  const shown = reasons.slice(0, MAX_DETAIL_REASONS).join(', ');
  const more =
    reasons.length > MAX_DETAIL_REASONS ? ` (+${reasons.length - MAX_DETAIL_REASONS} more)` : '';
  return ` Reason: ${shown}${more}.`;
}

interface ValidResponse {
  compatibility: BinaryCompatibilityState;
  server: { binary: Pick<ExecutableIdentity, 'executable' | 'version'> };
}

/**
 * Returns the response only when the fields the projection relies on are
 * present and well-typed. Anything else is evidence failure, not a verdict.
 */
function validResponse(response: unknown): ValidResponse | 'malformed' | 'unknown_state' {
  if (!isRecord(response) || response.feature_version !== 1) {
    return 'malformed';
  }
  const server = response.server;
  if (
    !isRecord(server) ||
    !isRecord(server.binary) ||
    typeof server.binary.executable !== 'string' ||
    typeof server.binary.version !== 'string' ||
    typeof response.server_instance_id !== 'string' ||
    typeof response.environment_snapshot_id !== 'string' ||
    !Array.isArray(response.reasons)
  ) {
    return 'malformed';
  }
  if (
    typeof response.compatibility !== 'string' ||
    !COMPATIBILITY_STATES.has(response.compatibility)
  ) {
    return 'unknown_state';
  }
  return response as unknown as ValidResponse;
}

function notProven(
  response: unknown,
  reasons: readonly string[],
  detail: string,
  action: BinaryIdentityAction = 'copy_support_packet',
  label = 'Perl LSP binary identity is not proven',
): BinaryIdentityPresentation {
  return {
    state: 'not_proven',
    action,
    quiet: true,
    label,
    detail: `${detail}${reasonSuffix(reasons)}`,
    reasons,
    supportPacket: supportPacket(response, 'not_proven', reasons),
  };
}

/**
 * Projects the server's canonical compatibility verdict into the VS Code
 * vocabulary. The server owns the verdict: the client never upgrades it,
 * never reconstructs identity from paths or filenames, and maps anything it
 * cannot read to `not_proven` rather than to a fabricated diagnosis.
 */
export function projectBinaryIdentityStatus(
  response: BinaryIdentityResponseV1 | Record<string, unknown> | null | undefined,
  selectedRole: SelectedBinaryRole,
): BinaryIdentityPresentation {
  const valid = validResponse(response);
  if (valid === 'malformed') {
    return notProven(
      response,
      [CLIENT_REASON_RESPONSE_MALFORMED],
      'The server identity response was missing required fields. No compatibility conclusion was drawn.',
    );
  }
  const serverReasons = boundedReasons((response as { reasons: unknown }).reasons);
  if (valid === 'unknown_state') {
    return notProven(
      response,
      [CLIENT_REASON_COMPATIBILITY_UNKNOWN, ...serverReasons],
      'The server reported a compatibility state this extension does not recognize. No compatibility conclusion was drawn.',
    );
  }
  const reasons = serverReasons;
  const copied = (state: BinaryIdentityUiState): string => supportPacket(response, state, reasons);

  switch (valid.compatibility) {
    case 'exact_match':
      return {
        state: 'ready_exact',
        action: 'none',
        quiet: true,
        label: 'Perl LSP server identity verified',
        detail: `${displayName(valid.server.binary.executable)} ${displayName(valid.server.binary.version)} matches the extension candidate.`,
        reasons,
        supportPacket: copied('ready_exact'),
      };

    case 'compatible_partial':
      return {
        state: 'ready_partial',
        action: 'copy_support_packet',
        quiet: true,
        label: 'Perl LSP server identity is compatible but partial',
        detail: `The server is usable, but source, target, candidate, or DAP identity is not fully proven.${reasonSuffix(reasons)}`,
        reasons,
        supportPacket: copied('ready_partial'),
      };

    case 'stale':
      return notProven(
        response,
        reasons,
        'The server process or workspace snapshot changed. Refresh identity before drawing a compatibility conclusion.',
        'refresh_identity',
        'Perl LSP identity status is stale',
      );

    case 'unsupported':
      return {
        state: 'unsupported',
        action: 'copy_support_packet',
        quiet: true,
        label: 'This server does not support binary identity reporting',
        detail: `Ordinary LSP remains available; exact extension/server parity is not proven.${reasonSuffix(reasons)}`,
        reasons,
        supportPacket: copied('unsupported'),
      };

    case 'not_proven':
      return notProven(
        response,
        reasons,
        'Identity evidence was unavailable or contradictory. This is not a product-failure verdict.',
      );

    case 'mismatch': {
      // The server's mismatch verdict is authoritative whatever its reason
      // codes are, including codes newer than this extension.
      const key = (state: BinaryIdentityUiState): string => noticeKey(state, reasons);
      if (selectedRole === 'managed') {
        const state: BinaryIdentityUiState = 'update_or_repair_required';
        return {
          state,
          action: 'repair_managed_pair',
          quiet: false,
          label: 'Perl LSP managed binary repair required',
          detail: `The managed server or DAP does not match this extension's expected version, target, source, or candidate.${reasonSuffix(reasons)}`,
          reasons,
          supportPacket: copied(state),
          noticeKey: key(state),
        };
      }
      const state: BinaryIdentityUiState = 'configured_binary_incompatible';
      const configured = selectedRole === 'user_supplied';
      return {
        state,
        action: 'inspect_configured_binary',
        quiet: false,
        label: configured
          ? 'Configured Perl LSP binary is incompatible'
          : 'Resolved Perl LSP binary is incompatible',
        detail: `${
          configured
            ? 'Review the configured user-supplied binary.'
            : 'The running server was not installed by this extension.'
        } It will not be replaced automatically.${reasonSuffix(reasons)}`,
        reasons,
        supportPacket: copied(state),
        noticeKey: key(state),
      };
    }
  }
}

/**
 * Projection used when no server is running, so no identity request can be
 * sent. A managed selection with no resolved binary is the one case the host
 * can attribute without the server: the managed artifact is absent. Every
 * other no-server case is `not_proven`, never healthy and never a mismatch.
 */
export function projectUnavailableBinaryIdentity(
  selectedRole: SelectedBinaryRole,
  serverBinaryResolved: boolean | undefined,
): BinaryIdentityPresentation {
  if (selectedRole === 'managed' && serverBinaryResolved === false) {
    const state: BinaryIdentityUiState = 'managed_binary_missing';
    const reasons = [CLIENT_REASON_MANAGED_BINARY_ABSENT];
    return {
      state,
      action: 'repair_managed_pair',
      quiet: false,
      label: 'Perl LSP managed binary is not installed',
      detail: 'No managed server binary is available. Install it to start the language server.',
      reasons,
      supportPacket: supportPacket(undefined, state, reasons),
      noticeKey: noticeKey(state, reasons),
    };
  }
  return notProven(
    undefined,
    [CLIENT_REASON_SERVER_NOT_RUNNING],
    'The language server is not running, so its identity cannot be read yet. Start the server and try again.',
    'none',
  );
}

export class BinaryIdentityNoticeTracker {
  private lastNoticeKey: string | undefined;

  shouldNotify(presentation: BinaryIdentityPresentation): boolean {
    if (presentation.quiet || presentation.noticeKey === undefined) {
      if (presentation.state === 'ready_exact') {
        this.lastNoticeKey = undefined;
      }
      return false;
    }
    if (presentation.noticeKey === this.lastNoticeKey) {
      return false;
    }
    this.lastNoticeKey = presentation.noticeKey;
    return true;
  }
}
