import {
  type ClientMetricBaselineEvidence,
  type ClientMetricObservation,
  type ClientMetricRatchet,
  evaluateClientMetricRatchet,
  validateClientMetricRatchet,
} from '../clientPerformancePolicy';

function observation(value: number | null, availability: 'observed' | 'not_proven' = 'observed'): ClientMetricObservation {
  return {
    metric_id: 'pretrigger_network_requests',
    subject: {
      vscode_version: '1.125.1',
      platform: 'linux',
      architecture: 'x64',
      scenario: 'unrelated-workspace',
      cold_warm: 'cold',
    },
    availability,
    value,
  };
}

function evidence(): ClientMetricBaselineEvidence {
  return {
    metric_id: 'pretrigger_network_requests',
    previous_public: observation(0),
    current_candidate: observation(0),
    evidence_quality: 'deterministic',
    observed_range: { min: 0, max: 0 },
  };
}

function ratchet(): ClientMetricRatchet {
  return {
    metric_id: 'pretrigger_network_requests',
    kind: 'correctness_invariant',
    policy: 'blocking',
    absolute_ceiling: 0,
    relative_regression_ratio: null,
    rationale: 'No network request is justified before a supported activation need.',
    intentional_update_procedure: 'Update only with a reviewed activation/network contract and new baseline evidence.',
  };
}

describe('client performance ratchet policy', () => {
  test('supports deterministic zero-budget correctness-style resource invariants', () => {
    expect(validateClientMetricRatchet(evidence(), ratchet())).toEqual([]);
    expect(evaluateClientMetricRatchet(evidence(), ratchet())).toMatchObject({
      status: 'pass',
      reasons: [],
    });
  });

  test('detects a regression from a zero previous-public baseline', () => {
    const current = evidence();
    current.current_candidate = observation(1);

    expect(evaluateClientMetricRatchet(current, ratchet())).toMatchObject({
      status: 'regressed',
    });
  });

  test('refuses to turn noisy evidence into a blocking performance gate', () => {
    const current = evidence();
    current.evidence_quality = 'noisy';
    const policy: ClientMetricRatchet = {
      ...ratchet(),
      kind: 'stable_performance_metric',
      absolute_ceiling: 100,
    };

    expect(validateClientMetricRatchet(current, policy)).toContain(
      'noisy or not-proven evidence cannot support a blocking ratchet',
    );
    expect(evaluateClientMetricRatchet(current, policy).status).toBe('not_proven');
  });

  test('keeps unavailable current instrumentation not-proven rather than zero', () => {
    const current = evidence();
    current.current_candidate = observation(null, 'not_proven');
    current.evidence_quality = 'not_proven';
    const policy: ClientMetricRatchet = {
      ...ratchet(),
      policy: 'advisory',
    };

    expect(evaluateClientMetricRatchet(current, policy)).toEqual({
      metric_id: 'pretrigger_network_requests',
      policy: 'advisory',
      status: 'not_proven',
      reasons: ['current candidate metric is not observed'],
    });
  });

  test('rejects comparison between different host/scenario subjects', () => {
    const current = evidence();
    current.current_candidate = {
      ...observation(0),
      subject: {
        ...observation(0).subject,
        vscode_version: 'current-stable-other-version',
      },
    };

    expect(evaluateClientMetricRatchet(current, ratchet())).toMatchObject({
      status: 'invalid_subject',
      reasons: expect.arrayContaining([
        'previous-public and current-candidate metric subjects are not comparable',
      ]),
    });
  });

  test('requires explicit rationale and update procedure for every ratchet', () => {
    const policy = {
      ...ratchet(),
      rationale: '',
      intentional_update_procedure: '',
    };
    expect(validateClientMetricRatchet(evidence(), policy)).toEqual(
      expect.arrayContaining([
        'ratchet requires an evidence-backed rationale',
        'ratchet requires an intentional-update procedure',
      ]),
    );
  });

  test('does not permit informational metrics to masquerade as blocking thresholds', () => {
    const policy: ClientMetricRatchet = {
      ...ratchet(),
      kind: 'informational_metric',
    };
    expect(validateClientMetricRatchet(evidence(), policy)).toContain(
      'informational metrics cannot be blocking',
    );
  });
});
