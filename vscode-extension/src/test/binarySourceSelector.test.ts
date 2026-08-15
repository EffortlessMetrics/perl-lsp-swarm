import {
  selectBinarySource,
  type BinaryCandidateFact,
  type BinarySourceSelectionInput,
} from '../binarySourceSelector';

const target = {
  target: 'x86_64-unknown-linux-gnu',
  state: 'supported' as const,
  evidenceRef: 'topology:linux-gnu-x64',
};

function candidate(
  role: BinaryCandidateFact['role'],
  overrides: Partial<BinaryCandidateFact> = {},
): BinaryCandidateFact {
  const authorities: Record<BinaryCandidateFact['role'], BinaryCandidateFact['authority']> = {
    configured_user_binary: 'configured_observation',
    packaged_candidate: 'package_manifest',
    managed_candidate: 'managed_selection',
    external_path_legacy: 'external_path_observation',
  };
  return {
    role,
    path: `/candidate/${role}/perllsp`,
    target: target.target,
    availability: 'available',
    compatibility: 'exact_match',
    authority: authorities[role],
    identityEvidence: 'canonical',
    evidenceRef: `evidence:${role}`,
    candidateId: `candidate:${role}`,
    ...overrides,
  };
}

function select(overrides: Partial<BinarySourceSelectionInput> = {}) {
  return selectBinarySource({
    target,
    allowManagedInstall: true,
    ...overrides,
  });
}

test('valid configured source is a hard user choice', () => {
  const configured = candidate('configured_user_binary');
  const result = select({
    configuredPath: configured.path,
    configuredCandidate: configured,
    packagedCandidate: candidate('packaged_candidate'),
    managedCandidate: candidate('managed_candidate'),
  });

  expect(result).toMatchObject({
    kind: 'selected',
    sourceRole: 'configured_user_binary',
    reason: 'configured_selected',
  });
});

test('missing configured source fails without selecting another candidate', () => {
  const result = select({
    configuredPath: '/configured/missing/perllsp',
    configuredCandidate: candidate('configured_user_binary', {
      path: '/configured/missing/perllsp',
      availability: 'missing',
    }),
    packagedCandidate: candidate('packaged_candidate'),
    managedCandidate: candidate('managed_candidate'),
    externalPathCandidate: candidate('external_path_legacy'),
  });

  expect(result).toMatchObject({
    kind: 'action_required',
    sourceRole: 'configured_user_binary',
    reason: 'configured_candidate_invalid',
    compatibility: 'mismatch',
  });
});

test('non-file and non-executable configured sources remain configured failures', () => {
  for (const availability of ['not_regular_file', 'not_executable'] as const) {
    const configured = candidate('configured_user_binary', { availability });
    expect(
      select({
        configuredPath: configured.path,
        configuredCandidate: configured,
        packagedCandidate: candidate('packaged_candidate'),
      }),
    ).toMatchObject({
      kind: 'action_required',
      sourceRole: 'configured_user_binary',
      reason: 'configured_candidate_invalid',
    });
  }
});

test('a configured wrapper is accepted when canonical identity proves compatibility', () => {
  const configured = candidate('configured_user_binary', {
    path: '/wrappers/perllsp-wrapper',
    compatibility: 'compatible_partial',
  });

  expect(
    select({ configuredPath: configured.path, configuredCandidate: configured }),
  ).toMatchObject({
    kind: 'selected',
    sourceRole: 'configured_user_binary',
    compatibility: 'compatible_partial',
  });
});

test('an exact manifested package wins ordinary discovery', () => {
  expect(
    select({
      packagedCandidate: candidate('packaged_candidate'),
      managedCandidate: candidate('managed_candidate'),
      externalPathCandidate: candidate('external_path_legacy'),
    }),
  ).toMatchObject({
    kind: 'selected',
    sourceRole: 'packaged_candidate',
    reason: 'packaged_candidate_selected',
  });
});

test('an unmanifested package cannot hide a verified managed candidate', () => {
  expect(
    select({
      packagedCandidate: candidate('packaged_candidate', {
        authority: 'external_path_observation',
      }),
      managedCandidate: candidate('managed_candidate'),
    }),
  ).toMatchObject({
    kind: 'selected',
    sourceRole: 'managed_candidate',
  });
});

test('a stale package cannot suppress a verified managed candidate', () => {
  expect(
    select({
      packagedCandidate: candidate('packaged_candidate', { compatibility: 'stale' }),
      managedCandidate: candidate('managed_candidate'),
    }),
  ).toMatchObject({
    kind: 'selected',
    sourceRole: 'managed_candidate',
  });
});

test('an invalid durable selected source blocks fallback', () => {
  expect(
    select({
      selectedSource: 'managed_candidate',
      managedCandidate: candidate('managed_candidate', { compatibility: 'mismatch' }),
      packagedCandidate: candidate('packaged_candidate'),
      externalPathCandidate: candidate('external_path_legacy'),
    }),
  ).toMatchObject({
    kind: 'action_required',
    sourceRole: 'managed_candidate',
    reason: 'durable_source_invalid',
  });
});

test('a stale PATH candidate cannot defeat packaged or managed authority', () => {
  const stalePath = candidate('external_path_legacy', { compatibility: 'stale' });

  expect(
    select({ managedCandidate: candidate('managed_candidate'), externalPathCandidate: stalePath }),
  ).toMatchObject({ sourceRole: 'managed_candidate' });
  expect(
    select({ packagedCandidate: candidate('packaged_candidate'), externalPathCandidate: stalePath }),
  ).toMatchObject({ sourceRole: 'packaged_candidate' });
});

test('PATH cannot become exact from existence or heuristic identity', () => {
  expect(
    select({
      allowManagedInstall: false,
      externalPathCandidate: candidate('external_path_legacy', {
        identityEvidence: 'heuristic',
        evidenceRef: 'filename:perllsp',
      }),
    }),
  ).toMatchObject({
    kind: 'not_proven',
    sourceRole: 'not_proven',
    reason: 'candidate_facts_not_proven',
  });
});

test('canonical external PATH compatibility remains visible as an external role', () => {
  expect(
    select({
      allowManagedInstall: false,
      externalPathCandidate: candidate('external_path_legacy', {
        compatibility: 'compatible_partial',
      }),
    }),
  ).toMatchObject({
    kind: 'selected',
    sourceRole: 'external_path_legacy',
    compatibility: 'compatible_partial',
  });
});

test('unsupported and not-proven targets terminate before candidate discovery', () => {
  expect(
    select({
      target: { target: 'powerpc-unknown-linux-gnu', state: 'unsupported', evidenceRef: 'topology' },
      packagedCandidate: candidate('packaged_candidate'),
    }),
  ).toMatchObject({
    kind: 'unsupported',
    sourceRole: 'unsupported',
    compatibility: 'unsupported',
  });

  expect(
    select({
      target: { target: 'unknown-host', state: 'not_proven' },
      packagedCandidate: candidate('packaged_candidate'),
    }),
  ).toMatchObject({
    kind: 'not_proven',
    sourceRole: 'not_proven',
    reason: 'target_not_proven',
  });
});

test('a candidate from another target cannot be selected', () => {
  expect(
    select({
      allowManagedInstall: false,
      packagedCandidate: candidate('packaged_candidate', {
        target: 'aarch64-unknown-linux-gnu',
      }),
    }),
  ).toMatchObject({
    kind: 'not_proven',
    sourceRole: 'not_proven',
  });
});

test('managed installation is explicit when no local candidate is admissible', () => {
  expect(select()).toMatchObject({
    kind: 'install_required',
    sourceRole: 'managed_install_required',
    reason: 'managed_install_required',
  });
});

test('fixed facts produce deterministic decision content', () => {
  const input: BinarySourceSelectionInput = {
    target,
    packagedCandidate: candidate('packaged_candidate'),
    allowManagedInstall: true,
  };

  expect(JSON.stringify(selectBinarySource(input))).toBe(JSON.stringify(selectBinarySource(input)));
});
