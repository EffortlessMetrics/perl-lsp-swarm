import fs from 'fs';
import path from 'path';
import {
  type ClientMetricBaselineEvidence,
  type ClientMetricObservation,
  type ClientMetricRatchet,
  type ClientPerfPolicyMetricRecord,
  type ClientPerfPolicyRecord,
  evaluateClientMetricRatchet,
  validateClientMetricRatchet,
  validateClientPerfPolicyRecord,
} from '../clientPerformancePolicy';

function observation(
  value: number | null,
  availability: 'observed' | 'not_proven' = 'observed',
): ClientMetricObservation {
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
    intentional_update_procedure:
      'Update only with a reviewed activation/network contract and new baseline evidence.',
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

  test('rejects negative observed values before absolute or relative evaluation', () => {
    const negativePrevious = evidence();
    negativePrevious.previous_public = observation(-1);

    expect(validateClientMetricRatchet(negativePrevious, ratchet())).toContain(
      'observed previous-public metric must carry a finite non-negative value',
    );
    expect(evaluateClientMetricRatchet(negativePrevious, ratchet()).status).toBe('not_proven');

    const negativeCurrent = evidence();
    negativeCurrent.current_candidate = observation(-1);

    expect(validateClientMetricRatchet(negativeCurrent, ratchet())).toContain(
      'observed current metric must carry a finite non-negative value',
    );
    expect(evaluateClientMetricRatchet(negativeCurrent, ratchet()).status).toBe('not_proven');
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

describe('committed client perf policy record', () => {
  const extensionRoot = path.resolve(__dirname, '..', '..');
  const recordPath = path.join(extensionRoot, 'scripts', 'client-perf-policy.v1.json');
  const committed = JSON.parse(fs.readFileSync(recordPath, 'utf8')) as unknown;

  function seededMetric(
    record: ClientPerfPolicyRecord,
    metricId: string,
  ): ClientPerfPolicyMetricRecord {
    const metric = record.metrics.find((entry) => entry.metric_id === metricId);
    if (metric === undefined) {
      throw new Error(`seeded record is missing metric ${metricId}`);
    }
    return metric;
  }

  test('committed baseline/policy record validates clean through the model', () => {
    expect(validateClientPerfPolicyRecord(committed)).toEqual([]);
  });

  test('keeps the whole seeded record honestly not-proven with no manufactured thresholds', () => {
    const record = committed as ClientPerfPolicyRecord;
    expect(record.receipt_kind).toBe('vscode_client_perf_policy');
    expect(record.metrics.length).toBeGreaterThan(0);
    for (const metric of record.metrics) {
      expect(metric.evidence_quality).toBe('not_proven');
      expect(metric.absolute_ceiling).toBeNull();
      expect(metric.relative_regression_ratio).toBeNull();
      expect(metric.status).toBe('not_proven');
      expect(metric.current_candidate).toBeNull();
    }
  });

  test('rejects promoting a seeded metric to blocking without evidence or threshold', () => {
    const record = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    seededMetric(record, 'pretrigger_network_requests').policy = 'blocking';

    expect(validateClientPerfPolicyRecord(record)).toEqual([
      'pretrigger_network_requests: noisy or not-proven evidence cannot support a blocking ratchet',
      'pretrigger_network_requests: blocking ratchet must name an explicit falsifiable threshold',
    ]);
  });

  test('rejects a recorded status that contradicts evaluated evidence', () => {
    const record = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    seededMetric(record, 'pretrigger_network_requests').status = 'pass';

    const errors = validateClientPerfPolicyRecord(record);
    expect(errors).toEqual([
      'pretrigger_network_requests: recorded status pass does not match evaluated status not_proven',
    ]);
  });

  test('rejects structural drift in the record envelope and metric entries', () => {
    expect(validateClientPerfPolicyRecord(null)).toEqual(['policy record must be a JSON object']);
    expect(validateClientPerfPolicyRecord({ schema_version: 'other.v1', metrics: [] })).toEqual([
      'schema_version must be vscode_client_perf_policy.v1',
      'receipt_kind must be vscode_client_perf_policy',
    ]);

    const record = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    record.metrics[1] = { ...seededMetric(record, 'pretrigger_network_requests') };
    expect(validateClientPerfPolicyRecord(record)).toEqual([
      'metrics[1].metric_id duplicates pretrigger_network_requests',
    ]);
  });

  test('rejects observed observations that carry non-finite values', () => {
    const record = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    seededMetric(record, 'extension_host_rss_bytes_delta').current_candidate = {
      availability: 'observed',
      value: Number.NaN,
    };

    expect(validateClientPerfPolicyRecord(record)).toEqual([
      'extension_host_rss_bytes_delta: current_candidate must be null or {availability, value} with observed values finite/non-negative and not_proven carrying no value',
    ]);
  });

  test('rejects negative observed current and previous record values', () => {
    const negativeCurrent = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    seededMetric(negativeCurrent, 'extension_host_rss_bytes_delta').current_candidate = {
      availability: 'observed',
      value: -1,
    };

    expect(validateClientPerfPolicyRecord(negativeCurrent)).toEqual([
      'extension_host_rss_bytes_delta: current_candidate must be null or {availability, value} with observed values finite/non-negative and not_proven carrying no value',
    ]);

    const negativePrevious = JSON.parse(JSON.stringify(committed)) as ClientPerfPolicyRecord;
    seededMetric(negativePrevious, 'extension_host_rss_bytes_delta').previous_public = {
      availability: 'observed',
      value: -1,
    };

    expect(validateClientPerfPolicyRecord(negativePrevious)).toEqual([
      'extension_host_rss_bytes_delta: previous_public must be null or {availability, value} with observed values finite/non-negative and not_proven carrying no value',
    ]);
  });
});
