/**
 * Pure managed-binary startup/update decision policy (#7852).
 *
 * NOT YET REACHABLE FROM PRODUCTION. Nothing in `activate()` or
 * `BinaryDownloader` calls `decideManagedStartup` today; the live startup gate
 * is still `BinaryDownloader.runEnsureBinary`'s inline "does the file exist"
 * fast path in `downloader.ts`. Wiring is #7853's slice.
 *
 * Before wiring, note that two of this policy's inputs have no data source yet:
 * `downloader.ts` only checksum-verifies a *freshly downloaded* archive and only
 * version-compares inside the update-check path, so an already-installed binary
 * currently has neither a verification nor a compatibility fact. Whoever
 * integrates this must source `local_verification` and `local_compatibility`
 * from real checks rather than defaulting them to `verified`/`exact`, which
 * would reproduce the "present means usable" gap these type names exist to close.
 */
export type ManagedBinarySourceRole = 'managed' | 'user_supplied';
export type ManagedStartupAction = 'startup' | 'check_updates' | 'repair';
export type LocalBinaryVerification = 'verified' | 'invalid' | 'not_proven';
export type LocalBinaryCompatibility = 'exact' | 'compatible' | 'incompatible' | 'not_proven';
export type UpdateMetadataState = 'current' | 'newer_available' | 'unavailable' | 'unknown';

export type BinaryReadiness =
  | 'ready_exact'
  | 'ready_compatible'
  | 'action_required'
  | 'missing'
  | 'not_proven';
export type NetworkRequirement = 'none' | 'advisory_check' | 'required_for_repair';
export type ManagedNetworkFailure =
  | 'none'
  | 'network_unavailable'
  | 'proxy_failure'
  | 'tls_failure'
  | 'download_failure'
  | 'not_proven';

export interface ManagedStartupPolicyInput {
  selected_source: ManagedBinarySourceRole;
  action: ManagedStartupAction;
  local_subject_present: boolean;
  local_verification: LocalBinaryVerification;
  local_compatibility: LocalBinaryCompatibility;
  update_metadata: UpdateMetadataState;
  network_failure: ManagedNetworkFailure;
}

export interface ManagedStartupDecision {
  selected_source: ManagedBinarySourceRole;
  binary_readiness: BinaryReadiness;
  update_availability: UpdateMetadataState;
  network_requirement: NetworkRequirement;
  network_failure: ManagedNetworkFailure;
  may_start_local_subject: boolean;
  should_attempt_network: boolean;
  reason:
    | 'verified_exact_local'
    | 'verified_compatible_local'
    | 'local_incompatible'
    | 'local_invalid'
    | 'local_not_proven'
    | 'local_missing'
    | 'explicit_update_check'
    | 'explicit_repair';
}

function localReadiness(
  input: ManagedStartupPolicyInput,
): Pick<ManagedStartupDecision, 'binary_readiness' | 'may_start_local_subject' | 'reason'> {
  if (!input.local_subject_present) {
    return {
      binary_readiness: 'missing',
      may_start_local_subject: false,
      reason: 'local_missing',
    };
  }

  if (input.local_verification === 'invalid') {
    return {
      binary_readiness: 'action_required',
      may_start_local_subject: false,
      reason: 'local_invalid',
    };
  }
  if (input.local_verification === 'not_proven') {
    return {
      binary_readiness: 'not_proven',
      may_start_local_subject: false,
      reason: 'local_not_proven',
    };
  }

  switch (input.local_compatibility) {
    case 'exact':
      return {
        binary_readiness: 'ready_exact',
        may_start_local_subject: true,
        reason: 'verified_exact_local',
      };
    case 'compatible':
      return {
        binary_readiness: 'ready_compatible',
        may_start_local_subject: true,
        reason: 'verified_compatible_local',
      };
    case 'incompatible':
      return {
        binary_readiness: 'action_required',
        may_start_local_subject: false,
        reason: 'local_incompatible',
      };
    case 'not_proven':
      return {
        binary_readiness: 'not_proven',
        may_start_local_subject: false,
        reason: 'local_not_proven',
      };
  }
}

function startupNetworkRequirement(
  input: ManagedStartupPolicyInput,
  readiness: BinaryReadiness,
): NetworkRequirement {
  if (readiness === 'ready_exact' || readiness === 'ready_compatible') {
    return 'none';
  }

  // An explicitly configured user-supplied binary never silently falls back to
  // the managed channel just because its local subject is absent/incompatible.
  if (input.selected_source === 'user_supplied') {
    return 'none';
  }

  return 'required_for_repair';
}

export function decideManagedStartup(input: ManagedStartupPolicyInput): ManagedStartupDecision {
  const local = localReadiness(input);

  if (input.action === 'check_updates') {
    return {
      selected_source: input.selected_source,
      ...local,
      update_availability: input.update_metadata,
      network_requirement: 'advisory_check',
      network_failure: input.network_failure,
      should_attempt_network: true,
      reason: 'explicit_update_check',
    };
  }

  if (input.action === 'repair') {
    return {
      selected_source: input.selected_source,
      ...local,
      update_availability: input.update_metadata,
      network_requirement: input.selected_source === 'managed' ? 'required_for_repair' : 'none',
      network_failure: input.network_failure,
      should_attempt_network: input.selected_source === 'managed',
      reason: 'explicit_repair',
    };
  }

  const networkRequirement = startupNetworkRequirement(input, local.binary_readiness);
  return {
    selected_source: input.selected_source,
    ...local,
    update_availability: input.update_metadata,
    network_requirement: networkRequirement,
    network_failure: input.network_failure,
    should_attempt_network: networkRequirement === 'required_for_repair',
  };
}

export function networkFailureChangesBinaryReadiness(
  before: ManagedStartupDecision,
  after: ManagedStartupDecision,
): boolean {
  return (
    before.selected_source !== after.selected_source ||
    before.binary_readiness !== after.binary_readiness ||
    before.may_start_local_subject !== after.may_start_local_subject
  );
}
