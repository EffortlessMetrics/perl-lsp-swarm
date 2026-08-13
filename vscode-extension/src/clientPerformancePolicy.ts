export type ClientMetricKind =
  | 'correctness_invariant'
  | 'stable_performance_metric'
  | 'informational_metric';
export type ClientMetricEvidenceQuality = 'deterministic' | 'repeatable' | 'noisy' | 'not_proven';
export type ClientMetricPolicyClass = 'blocking' | 'advisory' | 'informational';
export type ClientMetricStatus = 'pass' | 'regressed' | 'not_proven' | 'invalid_subject';

export interface ClientMetricSubject {
  vscode_version: string;
  platform: string;
  architecture: string;
  scenario: string;
  cold_warm: 'cold' | 'warm';
}

export interface ClientMetricObservation {
  metric_id: string;
  subject: ClientMetricSubject;
  availability: 'observed' | 'not_proven';
  value: number | null;
}

export interface ClientMetricBaselineEvidence {
  metric_id: string;
  previous_public: ClientMetricObservation | null;
  current_candidate: ClientMetricObservation;
  evidence_quality: ClientMetricEvidenceQuality;
  observed_range: { min: number; max: number } | null;
}

export interface ClientMetricRatchet {
  metric_id: string;
  kind: ClientMetricKind;
  policy: ClientMetricPolicyClass;
  absolute_ceiling: number | null;
  relative_regression_ratio: number | null;
  rationale: string;
  intentional_update_procedure: string;
}

export interface ClientMetricEvaluation {
  metric_id: string;
  policy: ClientMetricPolicyClass;
  status: ClientMetricStatus;
  reasons: string[];
}

function sameSubject(left: ClientMetricSubject, right: ClientMetricSubject): boolean {
  return (
    left.vscode_version === right.vscode_version &&
    left.platform === right.platform &&
    left.architecture === right.architecture &&
    left.scenario === right.scenario &&
    left.cold_warm === right.cold_warm
  );
}

export function validateClientMetricRatchet(
  evidence: ClientMetricBaselineEvidence,
  ratchet: ClientMetricRatchet,
): string[] {
  const errors: string[] = [];

  if (evidence.metric_id !== ratchet.metric_id) {
    errors.push('ratchet metric does not match baseline evidence metric');
  }
  if (ratchet.rationale.trim().length === 0) {
    errors.push('ratchet requires an evidence-backed rationale');
  }
  if (ratchet.intentional_update_procedure.trim().length === 0) {
    errors.push('ratchet requires an intentional-update procedure');
  }
  if (
    ratchet.absolute_ceiling !== null &&
    (!Number.isFinite(ratchet.absolute_ceiling) || ratchet.absolute_ceiling < 0)
  ) {
    errors.push('absolute ceiling must be a finite non-negative number');
  }
  if (
    ratchet.relative_regression_ratio !== null &&
    (!Number.isFinite(ratchet.relative_regression_ratio) || ratchet.relative_regression_ratio < 1)
  ) {
    errors.push('relative regression ratio must be at least 1');
  }

  if (ratchet.policy === 'informational') {
    if (ratchet.absolute_ceiling !== null || ratchet.relative_regression_ratio !== null) {
      errors.push('informational metrics cannot carry blocking/advisory thresholds');
    }
  }

  if (ratchet.policy === 'blocking') {
    if (evidence.evidence_quality === 'noisy' || evidence.evidence_quality === 'not_proven') {
      errors.push('noisy or not-proven evidence cannot support a blocking ratchet');
    }
    if (ratchet.kind === 'informational_metric') {
      errors.push('informational metrics cannot be blocking');
    }
    if (ratchet.absolute_ceiling === null && ratchet.relative_regression_ratio === null) {
      errors.push('blocking ratchet must name an explicit falsifiable threshold');
    }
  }

  if (
    evidence.previous_public !== null &&
    !sameSubject(evidence.previous_public.subject, evidence.current_candidate.subject)
  ) {
    errors.push('previous-public and current-candidate metric subjects are not comparable');
  }

  if (
    evidence.current_candidate.availability === 'observed' &&
    (evidence.current_candidate.value === null || !Number.isFinite(evidence.current_candidate.value))
  ) {
    errors.push('observed current metric must carry a finite value');
  }
  if (
    evidence.current_candidate.availability === 'not_proven' &&
    evidence.current_candidate.value !== null
  ) {
    errors.push('not-proven current metric cannot carry a value');
  }

  return errors;
}

export function evaluateClientMetricRatchet(
  evidence: ClientMetricBaselineEvidence,
  ratchet: ClientMetricRatchet,
): ClientMetricEvaluation {
  const validation = validateClientMetricRatchet(evidence, ratchet);
  if (validation.length > 0) {
    return {
      metric_id: ratchet.metric_id,
      policy: ratchet.policy,
      status: validation.includes('previous-public and current-candidate metric subjects are not comparable')
        ? 'invalid_subject'
        : 'not_proven',
      reasons: validation,
    };
  }

  const current = evidence.current_candidate;
  if (current.availability !== 'observed' || current.value === null) {
    return {
      metric_id: ratchet.metric_id,
      policy: ratchet.policy,
      status: 'not_proven',
      reasons: ['current candidate metric is not observed'],
    };
  }

  const reasons: string[] = [];
  if (ratchet.absolute_ceiling !== null && current.value > ratchet.absolute_ceiling) {
    reasons.push(`current value ${current.value} exceeds absolute ceiling ${ratchet.absolute_ceiling}`);
  }

  if (
    ratchet.relative_regression_ratio !== null &&
    evidence.previous_public?.availability === 'observed' &&
    evidence.previous_public.value !== null
  ) {
    const baseline = evidence.previous_public.value;
    if (baseline === 0) {
      if (current.value > 0) {
        reasons.push('current value regressed from a zero previous-public baseline');
      }
    } else if (current.value / baseline > ratchet.relative_regression_ratio) {
      reasons.push(
        `current/previous ratio ${(current.value / baseline).toFixed(3)} exceeds ${ratchet.relative_regression_ratio}`,
      );
    }
  }

  return {
    metric_id: ratchet.metric_id,
    policy: ratchet.policy,
    status: reasons.length === 0 ? 'pass' : 'regressed',
    reasons,
  };
}
