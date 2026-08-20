export type BinarySourceRole =
  | 'configured_user_binary'
  | 'packaged_candidate'
  | 'managed_candidate'
  | 'external_path_legacy'
  | 'managed_install_required'
  | 'unsupported'
  | 'not_proven';

export type BinaryCompatibilityState =
  | 'exact_match'
  | 'compatible_partial'
  | 'mismatch'
  | 'stale'
  | 'unsupported'
  | 'not_proven';

export type BinaryCandidateAvailability =
  | 'available'
  | 'missing'
  | 'not_regular_file'
  | 'not_executable'
  | 'not_proven';

export type BinaryCandidateAuthority =
  | 'configured_observation'
  | 'package_manifest'
  | 'managed_selection'
  | 'external_path_observation';

export type BinaryIdentityEvidence = 'canonical' | 'heuristic' | 'none';
export type BinaryTargetState = 'supported' | 'unsupported' | 'not_proven';

export interface BinaryTargetFact {
  readonly target: string;
  readonly state: BinaryTargetState;
  readonly evidenceRef?: string | undefined;
}

export interface BinaryCandidateFact {
  readonly role:
    | 'configured_user_binary'
    | 'packaged_candidate'
    | 'managed_candidate'
    | 'external_path_legacy';
  readonly path: string;
  readonly target: string;
  readonly availability: BinaryCandidateAvailability;
  readonly compatibility: BinaryCompatibilityState;
  readonly authority: BinaryCandidateAuthority;
  readonly identityEvidence: BinaryIdentityEvidence;
  readonly evidenceRef?: string | undefined;
  readonly candidateId?: string | undefined;
}

export interface BinarySourceSelectionInput {
  readonly target: BinaryTargetFact;
  readonly configuredPath?: string | undefined;
  readonly configuredCandidate?: BinaryCandidateFact | undefined;
  readonly selectedSource?: 'packaged_candidate' | 'managed_candidate' | undefined;
  readonly packagedCandidate?: BinaryCandidateFact | undefined;
  readonly managedCandidate?: BinaryCandidateFact | undefined;
  readonly externalPathCandidate?: BinaryCandidateFact | undefined;
  readonly allowManagedInstall: boolean;
}

export type BinarySourceDecisionKind =
  | 'selected'
  | 'action_required'
  | 'install_required'
  | 'unsupported'
  | 'not_proven';

export type BinarySourceDecisionReason =
  | 'configured_selected'
  | 'configured_missing_observation'
  | 'configured_candidate_invalid'
  | 'durable_source_selected'
  | 'durable_source_invalid'
  | 'packaged_candidate_selected'
  | 'managed_candidate_selected'
  | 'external_path_selected'
  | 'managed_install_required'
  | 'unsupported_target'
  | 'target_not_proven'
  | 'candidate_facts_not_proven'
  | 'no_admissible_candidate';

export interface BinarySourceDecision {
  readonly kind: BinarySourceDecisionKind;
  readonly sourceRole: BinarySourceRole;
  readonly compatibility: BinaryCompatibilityState;
  readonly reason: BinarySourceDecisionReason;
  readonly target: BinaryTargetFact;
  readonly candidate?: BinaryCandidateFact | undefined;
  readonly detail: string;
}

interface CandidateAssessment {
  readonly selectable: boolean;
  readonly compatibility: BinaryCompatibilityState;
  readonly detail: string;
}

function nonEmpty(value: string): boolean {
  return value.trim().length > 0;
}

function cloneTarget(target: BinaryTargetFact): BinaryTargetFact {
  return {
    target: target.target,
    state: target.state,
    evidenceRef: target.evidenceRef,
  };
}

function cloneCandidate(candidate: BinaryCandidateFact): BinaryCandidateFact {
  return {
    role: candidate.role,
    path: candidate.path,
    target: candidate.target,
    availability: candidate.availability,
    compatibility: candidate.compatibility,
    authority: candidate.authority,
    identityEvidence: candidate.identityEvidence,
    evidenceRef: candidate.evidenceRef,
    candidateId: candidate.candidateId,
  };
}

function decision(
  input: BinarySourceSelectionInput,
  values: Omit<BinarySourceDecision, 'target'>,
): BinarySourceDecision {
  return {
    ...values,
    target: cloneTarget(input.target),
    candidate: values.candidate ? cloneCandidate(values.candidate) : undefined,
  };
}

function expectedAuthority(role: BinaryCandidateFact['role']): BinaryCandidateAuthority {
  switch (role) {
    case 'configured_user_binary':
      return 'configured_observation';
    case 'packaged_candidate':
      return 'package_manifest';
    case 'managed_candidate':
      return 'managed_selection';
    case 'external_path_legacy':
      return 'external_path_observation';
  }
}

function availabilityFailure(candidate: BinaryCandidateFact): CandidateAssessment | null {
  switch (candidate.availability) {
    case 'available':
      return null;
    case 'missing':
      return {
        selectable: false,
        compatibility: 'mismatch',
        detail: `${candidate.role} is missing at the observed path.`,
      };
    case 'not_regular_file':
      return {
        selectable: false,
        compatibility: 'mismatch',
        detail: `${candidate.role} is not a regular file.`,
      };
    case 'not_executable':
      return {
        selectable: false,
        compatibility: 'mismatch',
        detail: `${candidate.role} is not executable.`,
      };
    case 'not_proven':
      return {
        selectable: false,
        compatibility: 'not_proven',
        detail: `${candidate.role} availability is not proven.`,
      };
  }
}

function assessCandidate(
  candidate: BinaryCandidateFact,
  expectedRole: BinaryCandidateFact['role'],
  expectedTarget: string,
): CandidateAssessment {
  if (candidate.role !== expectedRole) {
    return {
      selectable: false,
      compatibility: 'not_proven',
      detail: `Candidate role ${candidate.role} does not match the ${expectedRole} slot.`,
    };
  }

  if (!nonEmpty(candidate.path) || candidate.target !== expectedTarget) {
    return {
      selectable: false,
      compatibility: 'not_proven',
      detail: `${candidate.role} has no path or belongs to another target.`,
    };
  }

  if (candidate.authority !== expectedAuthority(candidate.role)) {
    return {
      selectable: false,
      compatibility: 'not_proven',
      detail: `${candidate.role} was not observed by its canonical authority.`,
    };
  }

  const availability = availabilityFailure(candidate);
  if (availability) {
    return availability;
  }

  if (
    candidate.identityEvidence !== 'canonical' ||
    !candidate.evidenceRef ||
    !nonEmpty(candidate.evidenceRef)
  ) {
    return {
      selectable: false,
      compatibility: 'not_proven',
      detail: `${candidate.role} lacks canonical identity evidence.`,
    };
  }

  if (!candidate.candidateId || !nonEmpty(candidate.candidateId)) {
    return {
      selectable: false,
      compatibility: 'not_proven',
      detail: `${candidate.role} lacks a canonical candidate identity.`,
    };
  }

  if (
    candidate.compatibility !== 'exact_match' &&
    candidate.compatibility !== 'compatible_partial'
  ) {
    return {
      selectable: false,
      compatibility: candidate.compatibility,
      detail: `${candidate.role} is ${candidate.compatibility}.`,
    };
  }

  return {
    selectable: true,
    compatibility: candidate.compatibility,
    detail: `${candidate.role} is available with canonical ${candidate.compatibility} evidence.`,
  };
}

function strictCandidateDecision(
  input: BinarySourceSelectionInput,
  candidate: BinaryCandidateFact | undefined,
  role: 'packaged_candidate' | 'managed_candidate',
): BinarySourceDecision {
  if (!candidate) {
    return decision(input, {
      kind: 'action_required',
      sourceRole: role,
      compatibility: 'not_proven',
      reason: 'durable_source_invalid',
      detail: `The durable ${role} selection has no matching candidate observation.`,
    });
  }

  const assessment = assessCandidate(candidate, role, input.target.target);
  if (!assessment.selectable) {
    return decision(input, {
      kind: 'action_required',
      sourceRole: role,
      compatibility: assessment.compatibility,
      reason: 'durable_source_invalid',
      candidate,
      detail: assessment.detail,
    });
  }

  return decision(input, {
    kind: 'selected',
    sourceRole: role,
    compatibility: assessment.compatibility,
    reason: 'durable_source_selected',
    candidate,
    detail: assessment.detail,
  });
}

function configuredDecision(input: BinarySourceSelectionInput): BinarySourceDecision | null {
  const configuredPath = input.configuredPath;
  if (!configuredPath || !nonEmpty(configuredPath)) {
    return null;
  }

  const candidate = input.configuredCandidate;
  if (!candidate || candidate.path !== configuredPath) {
    return decision(input, {
      kind: 'action_required',
      sourceRole: 'configured_user_binary',
      compatibility: 'not_proven',
      reason: 'configured_missing_observation',
      candidate,
      detail: `The configured server path ${configuredPath} could not be observed as that exact subject.`,
    });
  }

  const assessment = assessCandidate(candidate, 'configured_user_binary', input.target.target);
  if (!assessment.selectable) {
    return decision(input, {
      kind: 'action_required',
      sourceRole: 'configured_user_binary',
      compatibility: assessment.compatibility,
      reason: 'configured_candidate_invalid',
      candidate,
      detail: assessment.detail,
    });
  }

  return decision(input, {
    kind: 'selected',
    sourceRole: 'configured_user_binary',
    compatibility: assessment.compatibility,
    reason: 'configured_selected',
    candidate,
    detail: assessment.detail,
  });
}

export function selectBinarySource(input: BinarySourceSelectionInput): BinarySourceDecision {
  if (!nonEmpty(input.target.target)) {
    return decision(input, {
      kind: 'not_proven',
      sourceRole: 'not_proven',
      compatibility: 'not_proven',
      reason: 'target_not_proven',
      detail: 'The host target identity is empty.',
    });
  }

  if (input.target.state === 'unsupported') {
    return decision(input, {
      kind: 'unsupported',
      sourceRole: 'unsupported',
      compatibility: 'unsupported',
      reason: 'unsupported_target',
      detail: `Managed execution is unsupported for target ${input.target.target}.`,
    });
  }

  if (
    input.target.state === 'not_proven' ||
    !input.target.evidenceRef ||
    !nonEmpty(input.target.evidenceRef)
  ) {
    return decision(input, {
      kind: 'not_proven',
      sourceRole: 'not_proven',
      compatibility: 'not_proven',
      reason: 'target_not_proven',
      detail: `Target support is not proven for ${input.target.target}.`,
    });
  }

  const configured = configuredDecision(input);
  if (configured) {
    return configured;
  }

  if (input.selectedSource) {
    return strictCandidateDecision(
      input,
      input.selectedSource === 'packaged_candidate'
        ? input.packagedCandidate
        : input.managedCandidate,
      input.selectedSource,
    );
  }

  const candidates: ReadonlyArray<{
    readonly candidate: BinaryCandidateFact | undefined;
    readonly role: 'packaged_candidate' | 'managed_candidate' | 'external_path_legacy';
    readonly reason:
      | 'packaged_candidate_selected'
      | 'managed_candidate_selected'
      | 'external_path_selected';
  }> = [
    {
      candidate: input.packagedCandidate,
      role: 'packaged_candidate',
      reason: 'packaged_candidate_selected',
    },
    {
      candidate: input.managedCandidate,
      role: 'managed_candidate',
      reason: 'managed_candidate_selected',
    },
    {
      candidate: input.externalPathCandidate,
      role: 'external_path_legacy',
      reason: 'external_path_selected',
    },
  ];

  let observedUnproven = false;
  for (const { candidate, role, reason } of candidates) {
    if (!candidate) {
      continue;
    }
    const assessment = assessCandidate(candidate, role, input.target.target);
    if (assessment.selectable) {
      return decision(input, {
        kind: 'selected',
        sourceRole: role,
        compatibility: assessment.compatibility,
        reason,
        candidate,
        detail: assessment.detail,
      });
    }
    observedUnproven ||= assessment.compatibility === 'not_proven';
  }

  if (input.allowManagedInstall) {
    return decision(input, {
      kind: 'install_required',
      sourceRole: 'managed_install_required',
      compatibility: 'not_proven',
      reason: 'managed_install_required',
      detail: 'No admissible local candidate is selected; a managed install is required.',
    });
  }

  if (observedUnproven) {
    return decision(input, {
      kind: 'not_proven',
      sourceRole: 'not_proven',
      compatibility: 'not_proven',
      reason: 'candidate_facts_not_proven',
      detail: 'Candidate facts were observed, but none had canonical selectable identity evidence.',
    });
  }

  return decision(input, {
    kind: 'action_required',
    sourceRole: 'not_proven',
    compatibility: 'not_proven',
    reason: 'no_admissible_candidate',
    detail: 'No configured, packaged, managed, or external candidate is admissible.',
  });
}
