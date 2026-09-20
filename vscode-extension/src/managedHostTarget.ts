import {
  RELEASE_TOPOLOGY_MANAGED_TARGET_SET,
  RELEASE_TOPOLOGY_SOURCE,
  type ReleaseTopologyManagedTarget,
} from './releaseTopologyTargets';

export type ManagedHostPlatform = 'linux' | 'darwin' | 'win32' | string;
export type ManagedHostArch = 'x64' | 'arm64' | string;
export type ManagedLinuxLibc = 'gnu' | 'musl' | 'not_proven' | 'not_applicable';
export type ManagedHostEnvironment = 'ordinary' | 'android' | 'termux';
export type ManagedTargetSelectionKind = 'exact' | 'unsupported' | 'not_proven';

export type ManagedTargetDecisionReason =
  | 'exact_topology_target'
  | 'unsupported_platform'
  | 'unsupported_architecture'
  | 'unsupported_android_environment'
  | 'linux_libc_not_proven'
  | 'topology_target_absent';

export interface ManagedWorkspaceHostIdentity {
  readonly platform: ManagedHostPlatform;
  readonly arch: ManagedHostArch;
  readonly linuxLibc: ManagedLinuxLibc;
  readonly environment: ManagedHostEnvironment;
}

export interface ManagedHostTargetDecision {
  readonly hostPlatform: string;
  readonly hostArch: string;
  readonly hostAbi: string;
  readonly selectedTarget: ReleaseTopologyManagedTarget | null;
  readonly selectionKind: ManagedTargetSelectionKind;
  readonly reason: ManagedTargetDecisionReason;
  readonly topologyRef: string;
}

export interface ManagedHostTargetDecisionInput {
  readonly host: ManagedWorkspaceHostIdentity;
  readonly supportedTargets?: ReadonlySet<string> | undefined;
  readonly topologyRef?: string | undefined;
}

function result(
  input: ManagedHostTargetDecisionInput,
  values: Omit<ManagedHostTargetDecision, 'hostPlatform' | 'hostArch' | 'hostAbi' | 'topologyRef'>,
): ManagedHostTargetDecision {
  return {
    hostPlatform: input.host.platform,
    hostArch: input.host.arch,
    hostAbi: input.host.platform === 'linux' ? input.host.linuxLibc : 'not_applicable',
    topologyRef: input.topologyRef ?? RELEASE_TOPOLOGY_SOURCE,
    ...values,
  };
}

function exactCandidate(host: ManagedWorkspaceHostIdentity): string | null {
  switch (host.platform) {
    case 'linux': {
      if (host.environment !== 'ordinary') {
        return null;
      }
      if (host.arch !== 'x64' && host.arch !== 'arm64') {
        return null;
      }
      if (host.linuxLibc !== 'gnu' && host.linuxLibc !== 'musl') {
        return null;
      }
      const architecture = host.arch === 'arm64' ? 'aarch64' : 'x86_64';
      return `${architecture}-unknown-linux-${host.linuxLibc}`;
    }
    case 'darwin':
      if (host.arch === 'arm64') {
        return 'aarch64-apple-darwin';
      }
      if (host.arch === 'x64') {
        return 'x86_64-apple-darwin';
      }
      return null;
    case 'win32':
      if (host.arch === 'arm64') {
        return 'aarch64-pc-windows-msvc';
      }
      if (host.arch === 'x64') {
        return 'x86_64-pc-windows-msvc';
      }
      return null;
    default:
      return null;
  }
}

function noCandidateDecision(input: ManagedHostTargetDecisionInput): ManagedHostTargetDecision {
  if (input.host.platform === 'linux' && input.host.environment !== 'ordinary') {
    return result(input, {
      selectedTarget: null,
      selectionKind: 'unsupported',
      reason: 'unsupported_android_environment',
    });
  }

  if (
    input.host.platform === 'linux' &&
    input.host.environment === 'ordinary' &&
    input.host.linuxLibc !== 'gnu' &&
    input.host.linuxLibc !== 'musl'
  ) {
    return result(input, {
      selectedTarget: null,
      selectionKind: 'not_proven',
      reason: 'linux_libc_not_proven',
    });
  }

  if (!['linux', 'darwin', 'win32'].includes(input.host.platform)) {
    return result(input, {
      selectedTarget: null,
      selectionKind: 'unsupported',
      reason: 'unsupported_platform',
    });
  }

  return result(input, {
    selectedTarget: null,
    selectionKind: 'unsupported',
    reason: 'unsupported_architecture',
  });
}

export function decideManagedHostTarget(
  input: ManagedHostTargetDecisionInput,
): ManagedHostTargetDecision {
  const candidate = exactCandidate(input.host);
  if (!candidate) {
    return noCandidateDecision(input);
  }

  const supportedTargets = input.supportedTargets ?? RELEASE_TOPOLOGY_MANAGED_TARGET_SET;
  if (!supportedTargets.has(candidate)) {
    return result(input, {
      selectedTarget: null,
      selectionKind: 'unsupported',
      reason: 'topology_target_absent',
    });
  }

  return result(input, {
    selectedTarget: candidate as ReleaseTopologyManagedTarget,
    selectionKind: 'exact',
    reason: 'exact_topology_target',
  });
}

export function requireManagedHostTarget(
  decision: ManagedHostTargetDecision,
): ReleaseTopologyManagedTarget {
  if (decision.selectionKind !== 'exact' || !decision.selectedTarget) {
    throw new Error(
      `No managed binary target for workspace host ${decision.hostPlatform}/${decision.hostArch}/${decision.hostAbi}: ${decision.reason} (${decision.topologyRef})`,
    );
  }
  return decision.selectedTarget;
}
