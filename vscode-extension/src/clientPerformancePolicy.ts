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
    (evidence.current_candidate.value === null ||
      !Number.isFinite(evidence.current_candidate.value))
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
      status: validation.includes(
        'previous-public and current-candidate metric subjects are not comparable',
      )
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
    reasons.push(
      `current value ${current.value} exceeds absolute ceiling ${ratchet.absolute_ceiling}`,
    );
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

export const CLIENT_PERF_POLICY_SCHEMA_VERSION = 'vscode_client_perf_policy.v1';
export type ClientPerfPolicyReceiptKind = 'vscode_client_perf_policy';

export interface ClientPerfPolicyObservationRecord {
  availability: 'observed' | 'not_proven';
  value: number | null;
}

export interface ClientPerfPolicyMetricRecord {
  metric_id: string;
  kind: ClientMetricKind;
  policy: ClientMetricPolicyClass;
  evidence_quality: ClientMetricEvidenceQuality;
  subject: ClientMetricSubject;
  previous_public: ClientPerfPolicyObservationRecord | null;
  current_candidate: ClientPerfPolicyObservationRecord | null;
  absolute_ceiling: number | null;
  relative_regression_ratio: number | null;
  rationale: string;
  intentional_update_procedure: string;
  status: ClientMetricStatus;
}

export interface ClientPerfPolicyRecord {
  schema_version: typeof CLIENT_PERF_POLICY_SCHEMA_VERSION;
  receipt_kind: ClientPerfPolicyReceiptKind;
  metrics: ClientPerfPolicyMetricRecord[];
}

const METRIC_KINDS: readonly ClientMetricKind[] = [
  'correctness_invariant',
  'stable_performance_metric',
  'informational_metric',
];
const EVIDENCE_QUALITIES: readonly ClientMetricEvidenceQuality[] = [
  'deterministic',
  'repeatable',
  'noisy',
  'not_proven',
];
const POLICY_CLASSES: readonly ClientMetricPolicyClass[] = [
  'blocking',
  'advisory',
  'informational',
];
const STATUSES: readonly ClientMetricStatus[] = [
  'pass',
  'regressed',
  'not_proven',
  'invalid_subject',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isEnumMember<T extends string>(value: unknown, members: readonly T[]): value is T {
  return typeof value === 'string' && (members as readonly string[]).includes(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0;
}

function isSubject(value: unknown): value is ClientMetricSubject {
  return (
    isRecord(value) &&
    typeof value.vscode_version === 'string' &&
    typeof value.platform === 'string' &&
    typeof value.architecture === 'string' &&
    typeof value.scenario === 'string' &&
    (value.cold_warm === 'cold' || value.cold_warm === 'warm')
  );
}

function isObservationRecord(value: unknown): value is ClientPerfPolicyObservationRecord {
  if (!isRecord(value)) {
    return false;
  }
  if (value.availability !== 'observed' && value.availability !== 'not_proven') {
    return false;
  }
  if (value.availability === 'not_proven') {
    return value.value === null;
  }
  return isFiniteNumber(value.value);
}

function observationFromRecord(
  metricId: string,
  subject: ClientMetricSubject,
  record: ClientPerfPolicyObservationRecord | null,
): ClientMetricObservation {
  if (record === null) {
    return { metric_id: metricId, subject, availability: 'not_proven', value: null };
  }
  return {
    metric_id: metricId,
    subject,
    availability: record.availability,
    value: record.value,
  };
}

export function validateClientPerfPolicyRecord(record: unknown): string[] {
  if (!isRecord(record)) {
    return ['policy record must be a JSON object'];
  }
  const errors: string[] = [];
  if (record.schema_version !== CLIENT_PERF_POLICY_SCHEMA_VERSION) {
    errors.push(`schema_version must be ${CLIENT_PERF_POLICY_SCHEMA_VERSION}`);
  }
  if (record.receipt_kind !== 'vscode_client_perf_policy') {
    errors.push('receipt_kind must be vscode_client_perf_policy');
  }
  if (!Array.isArray(record.metrics)) {
    errors.push('metrics must be an array');
    return errors;
  }

  const seenIds = new Set<string>();
  record.metrics.forEach((entry, index) => {
    const label = `metrics[${index}]`;
    if (!isRecord(entry)) {
      errors.push(`${label} must be a JSON object`);
      return;
    }

    const {
      metric_id,
      kind,
      policy,
      evidence_quality,
      subject,
      previous_public,
      current_candidate,
      absolute_ceiling,
      relative_regression_ratio,
      rationale,
      intentional_update_procedure,
      status,
    } = entry;

    if (!isNonEmptyString(metric_id)) {
      errors.push(`${label}.metric_id must be a non-empty string`);
      return;
    }
    if (seenIds.has(metric_id)) {
      errors.push(`${label}.metric_id duplicates ${metric_id}`);
      return;
    }
    seenIds.add(metric_id);

    if (!isEnumMember(kind, METRIC_KINDS)) {
      errors.push(`${metric_id}: kind must be one of ${METRIC_KINDS.join(', ')}`);
    }
    if (!isEnumMember(policy, POLICY_CLASSES)) {
      errors.push(`${metric_id}: policy must be one of ${POLICY_CLASSES.join(', ')}`);
    }
    if (!isEnumMember(evidence_quality, EVIDENCE_QUALITIES)) {
      errors.push(`${metric_id}: evidence_quality must be one of ${EVIDENCE_QUALITIES.join(', ')}`);
    }
    if (!isSubject(subject)) {
      errors.push(`${metric_id}: subject must name host/scenario/cold-warm applicability`);
    }
    if (!isNonEmptyString(rationale)) {
      errors.push(`${metric_id}: rationale must be a non-empty string`);
    }
    if (!isNonEmptyString(intentional_update_procedure)) {
      errors.push(`${metric_id}: intentional_update_procedure must be a non-empty string`);
    }
    const ceilingValid =
      absolute_ceiling === null || (isFiniteNumber(absolute_ceiling) && absolute_ceiling >= 0);
    if (!ceilingValid) {
      errors.push(`${metric_id}: absolute_ceiling must be a finite non-negative number or null`);
    }
    const ratioValid =
      relative_regression_ratio === null ||
      (isFiniteNumber(relative_regression_ratio) && relative_regression_ratio >= 1);
    if (!ratioValid) {
      errors.push(`${metric_id}: relative_regression_ratio must be at least 1 or null`);
    }
    const previousValid = previous_public === null || isObservationRecord(previous_public);
    if (!previousValid) {
      errors.push(
        `${metric_id}: previous_public must be null or {availability, value} with not_proven carrying no value`,
      );
    }
    const currentValid = current_candidate === null || isObservationRecord(current_candidate);
    if (!currentValid) {
      errors.push(
        `${metric_id}: current_candidate must be null or {availability, value} with not_proven carrying no value`,
      );
    }
    const statusValid = isEnumMember(status, STATUSES);
    if (!statusValid) {
      errors.push(`${metric_id}: status must be one of ${STATUSES.join(', ')}`);
    }

    if (
      !isEnumMember(kind, METRIC_KINDS) ||
      !isEnumMember(policy, POLICY_CLASSES) ||
      !isEnumMember(evidence_quality, EVIDENCE_QUALITIES) ||
      !isSubject(subject) ||
      !isNonEmptyString(rationale) ||
      !isNonEmptyString(intentional_update_procedure) ||
      !ceilingValid ||
      !ratioValid ||
      !previousValid ||
      !currentValid ||
      !statusValid
    ) {
      return;
    }

    const evidence: ClientMetricBaselineEvidence = {
      metric_id,
      previous_public:
        previous_public === null
          ? null
          : observationFromRecord(metric_id, subject, previous_public),
      current_candidate:
        current_candidate === null
          ? observationFromRecord(metric_id, subject, null)
          : observationFromRecord(metric_id, subject, current_candidate),
      evidence_quality,
      observed_range: null,
    };
    const ratchet: ClientMetricRatchet = {
      metric_id,
      kind,
      policy,
      absolute_ceiling: absolute_ceiling === null ? null : absolute_ceiling,
      relative_regression_ratio:
        relative_regression_ratio === null ? null : relative_regression_ratio,
      rationale,
      intentional_update_procedure,
    };

    for (const ruleError of validateClientMetricRatchet(evidence, ratchet)) {
      errors.push(`${metric_id}: ${ruleError}`);
    }

    const evaluated = evaluateClientMetricRatchet(evidence, ratchet);
    if (evaluated.status !== status) {
      errors.push(
        `${metric_id}: recorded status ${status} does not match evaluated status ${evaluated.status}`,
      );
    }
  });

  return errors;
}
