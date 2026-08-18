import {
  type ManagedStartupPolicyInput,
  decideManagedStartup,
  networkFailureChangesBinaryReadiness,
} from '../managedStartupPolicy';

function input(overrides: Partial<ManagedStartupPolicyInput> = {}): ManagedStartupPolicyInput {
  return {
    selected_source: 'managed',
    action: 'startup',
    local_subject_present: true,
    local_verification: 'verified',
    local_compatibility: 'exact',
    update_metadata: 'current',
    network_failure: 'none',
    ...overrides,
  };
}

describe('managed startup policy', () => {
  test('starts a verified exact managed binary without requiring update network', () => {
    expect(
      decideManagedStartup(
        input({
          update_metadata: 'unavailable',
          network_failure: 'network_unavailable',
        }),
      ),
    ).toMatchObject({
      selected_source: 'managed',
      binary_readiness: 'ready_exact',
      update_availability: 'unavailable',
      network_requirement: 'none',
      may_start_local_subject: true,
      should_attempt_network: false,
      reason: 'verified_exact_local',
    });
  });

  test('keeps a verified compatible managed binary usable while update freshness is unknown', () => {
    expect(
      decideManagedStartup(
        input({
          local_compatibility: 'compatible',
          update_metadata: 'unknown',
          network_failure: 'network_unavailable',
        }),
      ),
    ).toMatchObject({
      binary_readiness: 'ready_compatible',
      update_availability: 'unknown',
      network_requirement: 'none',
      may_start_local_subject: true,
      should_attempt_network: false,
    });
  });

  test('requires managed repair when no local managed subject is usable', () => {
    expect(
      decideManagedStartup(
        input({
          local_subject_present: false,
          local_verification: 'not_proven',
          local_compatibility: 'not_proven',
          update_metadata: 'unavailable',
          network_failure: 'network_unavailable',
        }),
      ),
    ).toMatchObject({
      binary_readiness: 'missing',
      network_requirement: 'required_for_repair',
      should_attempt_network: true,
      may_start_local_subject: false,
    });
  });

  test('never falls back from an explicit user-supplied binary to managed networking', () => {
    expect(
      decideManagedStartup(
        input({
          selected_source: 'user_supplied',
          local_subject_present: true,
          local_verification: 'verified',
          local_compatibility: 'incompatible',
          update_metadata: 'unavailable',
          network_failure: 'network_unavailable',
        }),
      ),
    ).toMatchObject({
      selected_source: 'user_supplied',
      binary_readiness: 'action_required',
      network_requirement: 'none',
      should_attempt_network: false,
      may_start_local_subject: false,
      reason: 'local_incompatible',
    });
  });

  test('does not call a verification-unknown local subject ready because metadata is unavailable', () => {
    expect(
      decideManagedStartup(
        input({
          local_verification: 'not_proven',
          local_compatibility: 'compatible',
          update_metadata: 'unavailable',
          network_failure: 'network_unavailable',
        }),
      ),
    ).toMatchObject({
      binary_readiness: 'not_proven',
      may_start_local_subject: false,
      network_requirement: 'required_for_repair',
    });
  });

  test('explicit update check may use network without changing local readiness', () => {
    const decision = decideManagedStartup(
      input({
        action: 'check_updates',
        update_metadata: 'unknown',
        network_failure: 'none',
      }),
    );
    expect(decision).toMatchObject({
      binary_readiness: 'ready_exact',
      network_requirement: 'advisory_check',
      should_attempt_network: true,
      reason: 'explicit_update_check',
    });
  });

  test('explicit managed repair requires network while user-supplied repair does not switch channels', () => {
    expect(decideManagedStartup(input({ action: 'repair' }))).toMatchObject({
      selected_source: 'managed',
      network_requirement: 'required_for_repair',
      should_attempt_network: true,
      reason: 'explicit_repair',
    });
    expect(
      decideManagedStartup(input({ selected_source: 'user_supplied', action: 'repair' })),
    ).toMatchObject({
      selected_source: 'user_supplied',
      network_requirement: 'none',
      should_attempt_network: false,
      reason: 'explicit_repair',
    });
  });

  test('network failure alone cannot mutate binary source/readiness for an accepted local subject', () => {
    const before = decideManagedStartup(input());
    const after = decideManagedStartup(
      input({
        update_metadata: 'unavailable',
        network_failure: 'proxy_failure',
      }),
    );
    expect(networkFailureChangesBinaryReadiness(before, after)).toBe(false);
  });
});
