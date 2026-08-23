export type SupportEvidenceState =
  | 'known'
  | 'known_absent'
  | 'unsupported'
  | 'unknown'
  | 'not_proven';
export type SupportHostKind = 'local' | 'remote' | 'unknown';
export type SupportWorkspaceMode = 'single_root' | 'multi_root' | 'none' | 'unknown';
export type SupportTrustDisposition = 'trusted_supported' | 'untrusted_unsupported' | 'unknown';
export type SupportBinaryRole = 'managed' | 'user_supplied' | 'bundled' | 'ambient' | 'unknown';
export type SupportCompatibilityState =
  | 'ready_exact'
  | 'ready_compatible'
  | 'ready_partial'
  | 'action_required'
  | 'missing'
  | 'unsupported'
  | 'unknown'
  | 'not_proven';

export type SupportAtom = string & { readonly __supportAtom: unique symbol };
export type SupportDigest = string & { readonly __supportDigest: unique symbol };

export interface SupportValue<T> {
  state: SupportEvidenceState;
  value: T | null;
}

export interface SupportBinaryIdentity {
  state: SupportEvidenceState;
  role: SupportBinaryRole;
  version: SupportValue<SupportAtom>;
  target: SupportValue<SupportAtom>;
  digest: SupportValue<SupportDigest>;
  compatibility: SupportCompatibilityState;
}

export interface SupportProtocolFamily {
  family: SupportAtom;
  version: SupportValue<SupportAtom>;
  state: SupportCompatibilityState;
}

export interface SupportMigrationSummary {
  registry: SupportValue<SupportAtom>;
  encountered: SupportAtom[];
  status: 'clean' | 'compatible' | 'action_required' | 'inert' | 'invalid' | 'unknown';
}

export interface SupportPacketV1 {
  schema_version: 'perl_lsp_support_packet.v1';
  product: {
    name: SupportAtom;
    version: SupportValue<SupportAtom>;
    track: SupportValue<SupportAtom>;
  };
  extension: {
    id: SupportAtom;
    version: SupportValue<SupportAtom>;
    artifact_digest: SupportValue<SupportDigest>;
  };
  host: {
    editor: SupportAtom;
    editor_version: SupportValue<SupportAtom>;
    platform: SupportAtom;
    architecture: SupportAtom;
    extension_host: SupportHostKind;
    workspace_mode: SupportWorkspaceMode;
    trust: SupportTrustDisposition;
  };
  perllsp: SupportBinaryIdentity;
  perl_dap: SupportBinaryIdentity;
  lifecycle: {
    generation: SupportValue<SupportAtom>;
    readiness: SupportAtom;
    startup_disposition: SupportAtom;
    crash_disposition: SupportAtom;
    activation_disposition: SupportAtom;
    managed_install_disposition: SupportAtom;
  };
  protocol: {
    families: SupportProtocolFamily[];
  };
  configuration: {
    user_present: SupportValue<boolean>;
    workspace_present: SupportValue<boolean>;
    folder_present: SupportValue<boolean>;
    project_config_present: SupportValue<boolean>;
    formatter_mode: SupportAtom;
    critic_mode: SupportAtom;
    migration: SupportMigrationSummary;
  };
  failure: {
    startup_reason: SupportAtom;
    network_reason: SupportAtom;
    provider_state: SupportAtom;
    cache_state: SupportAtom;
  };
}

const MAX_ATOM_LENGTH = 160;
const MAX_LIST_LENGTH = 32;
const SAFE_ATOM = /^[A-Za-z0-9][A-Za-z0-9 ._:+@#()\[\]-]*$/;
const HEX_DIGEST = /^(?:sha256:)?[0-9a-fA-F]{64}$/;
const WINDOWS_ABSOLUTE_PATH = /^[A-Za-z]:[\\/]/;
const LINE_BREAK = /\r\n|[\n\r\u2028\u2029]/;

export function supportAtom(value: string): SupportAtom {
  if (LINE_BREAK.test(value)) {
    throw new Error('support atom must be single-line');
  }
  const normalized = value.trim();
  if (normalized.length === 0 || normalized.length > MAX_ATOM_LENGTH) {
    throw new Error('support atom must be non-empty and bounded');
  }
  if (
    normalized.startsWith('/') ||
    normalized.startsWith('\\\\') ||
    WINDOWS_ABSOLUTE_PATH.test(normalized) ||
    normalized.includes('://') ||
    normalized.includes('=') ||
    normalized.includes('\\') ||
    !SAFE_ATOM.test(normalized)
  ) {
    throw new Error('support atom contains path, URL, secret-like, or unsafe characters');
  }
  return normalized as SupportAtom;
}

export function supportDigest(value: string): SupportDigest {
  if (LINE_BREAK.test(value)) {
    throw new Error('support digest must be single-line');
  }
  const normalized = value.trim();
  if (!HEX_DIGEST.test(normalized)) {
    throw new Error('support digest must be an exact sha256 digest');
  }
  return normalized.toLowerCase() as SupportDigest;
}

export function supportKnown<T>(value: T): SupportValue<T> {
  return { state: 'known', value };
}

export function supportState<T>(state: Exclude<SupportEvidenceState, 'known'>): SupportValue<T> {
  return { state, value: null };
}

function validateEvidenceValue<T>(name: string, value: SupportValue<T>, errors: string[]): void {
  if (value.state === 'known' && value.value === null) {
    errors.push(`${name} is known but has no value`);
  }
  if (value.state !== 'known' && value.value !== null) {
    errors.push(`${name} carries a value without known evidence`);
  }
}

export function validateSupportPacket(packet: SupportPacketV1): string[] {
  const errors: string[] = [];

  if (packet.protocol.families.length > MAX_LIST_LENGTH) {
    errors.push('protocol family list exceeds bounded support-packet limit');
  }
  if (packet.configuration.migration.encountered.length > MAX_LIST_LENGTH) {
    errors.push('migration list exceeds bounded support-packet limit');
  }

  for (const [name, value] of [
    ['product.version', packet.product.version],
    ['product.track', packet.product.track],
    ['extension.version', packet.extension.version],
    ['host.editor_version', packet.host.editor_version],
    ['lifecycle.generation', packet.lifecycle.generation],
  ] as const) {
    validateEvidenceValue(name, value, errors);
  }

  for (const [name, value] of [
    ['extension.artifact_digest', packet.extension.artifact_digest],
    ['perllsp.digest', packet.perllsp.digest],
    ['perl_dap.digest', packet.perl_dap.digest],
  ] as const) {
    if (value.state === 'known' && value.value === null) {
      errors.push(`${name} is known but has no digest`);
    }
    if (value.state !== 'known' && value.value !== null) {
      errors.push(`${name} carries a digest without known evidence`);
    }
  }

  validateEvidenceValue('configuration.user_present', packet.configuration.user_present, errors);
  validateEvidenceValue(
    'configuration.workspace_present',
    packet.configuration.workspace_present,
    errors,
  );
  validateEvidenceValue(
    'configuration.folder_present',
    packet.configuration.folder_present,
    errors,
  );
  validateEvidenceValue(
    'configuration.project_config_present',
    packet.configuration.project_config_present,
    errors,
  );

  return errors;
}

export function normalizeSupportPacket(packet: SupportPacketV1): SupportPacketV1 {
  return {
    ...packet,
    protocol: {
      families: [...packet.protocol.families]
        .sort((left, right) => left.family.localeCompare(right.family))
        .slice(0, MAX_LIST_LENGTH),
    },
    configuration: {
      ...packet.configuration,
      migration: {
        ...packet.configuration.migration,
        encountered: [...packet.configuration.migration.encountered]
          .sort((left, right) => left.localeCompare(right))
          .slice(0, MAX_LIST_LENGTH),
      },
    },
  };
}

export function serializeSupportPacketJson(packet: SupportPacketV1): string {
  const errors = validateSupportPacket(packet);
  if (errors.length > 0) {
    throw new Error(`invalid support packet: ${errors.join('; ')}`);
  }
  return `${JSON.stringify(normalizeSupportPacket(packet), null, 2)}\n`;
}

function supportValueText<T>(value: SupportValue<T>): string {
  return value.state === 'known' && value.value !== null ? String(value.value) : value.state;
}

export function formatSupportPacketHuman(packet: SupportPacketV1): string {
  const errors = validateSupportPacket(packet);
  if (errors.length > 0) {
    throw new Error(`invalid support packet: ${errors.join('; ')}`);
  }
  const normalized = normalizeSupportPacket(packet);
  const lines = [
    `Support packet: ${normalized.schema_version}`,
    `Product: ${normalized.product.name} ${supportValueText(normalized.product.version)} (${supportValueText(normalized.product.track)})`,
    `Extension: ${normalized.extension.id} ${supportValueText(normalized.extension.version)}`,
    `Editor: ${normalized.host.editor} ${supportValueText(normalized.host.editor_version)}`,
    `Host: ${normalized.host.platform}/${normalized.host.architecture} ${normalized.host.extension_host}`,
    `Workspace: ${normalized.host.workspace_mode} ${normalized.host.trust}`,
    `perllsp: ${normalized.perllsp.role} ${supportValueText(normalized.perllsp.version)} ${normalized.perllsp.compatibility}`,
    `perl-dap: ${normalized.perl_dap.role} ${supportValueText(normalized.perl_dap.version)} ${normalized.perl_dap.compatibility}`,
    `Readiness: ${normalized.lifecycle.readiness}`,
    `Startup: ${normalized.lifecycle.startup_disposition}`,
    `Crash: ${normalized.lifecycle.crash_disposition}`,
    `Activation: ${normalized.lifecycle.activation_disposition}`,
    `Managed install: ${normalized.lifecycle.managed_install_disposition}`,
    `Config sources: user=${supportValueText(normalized.configuration.user_present)} workspace=${supportValueText(normalized.configuration.workspace_present)} folder=${supportValueText(normalized.configuration.folder_present)} project=${supportValueText(normalized.configuration.project_config_present)}`,
    `Migration: ${normalized.configuration.migration.status}`,
    `Startup reason: ${normalized.failure.startup_reason}`,
    `Network reason: ${normalized.failure.network_reason}`,
    `Provider state: ${normalized.failure.provider_state}`,
    `Cache state: ${normalized.failure.cache_state}`,
  ];
  return `${lines.join('\n')}\n`;
}
