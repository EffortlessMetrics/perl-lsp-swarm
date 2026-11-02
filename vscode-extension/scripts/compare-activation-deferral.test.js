const test = require('node:test');
const assert = require('node:assert/strict');
const {
  compareActivationDeferral,
  IMMATERIAL_ACTIVATION_DELTA_MS,
} = require('./compare-activation-deferral.js');

function metric(value) {
  return { count: 3, min: value, max: value, p50: value, p95: value };
}

function summary(activateReturnedMs, mcpRegistrationMs) {
  return {
    sample_count: 3,
    metrics: {
      activate_returned_ms: metric(activateReturnedMs),
      commands_registered_ms: metric(100),
      initialize_completed_ms: metric(500),
      first_useful_request_ms: metric(700),
      warm_request_ms: metric(900),
      restart_milestone_ms: metric(1100),
      shutdown_milestone_ms: metric(1300),
      feature_mcp_registration_ms: metric(mcpRegistrationMs),
    },
  };
}

void test('keep_eager when activation delta is immaterial', () => {
  const comparison = compareActivationDeferral({
    baseline: summary(400, 12),
    deferred: summary(398, 0),
    deferredDomains: ['mcp'],
  });
  assert.equal(comparison.decision, 'keep_eager');
  assert.equal(comparison.violations.length, 0);
  assert.equal(comparison.performance_budget_promoted, false);
});

void test('defer_construction when activation delta exceeds immaterial floor', () => {
  const baseline = summary(400, 20);
  const deferred = {
    sample_count: 3,
    metrics: {
      ...baseline.metrics,
      activate_returned_ms: metric(380),
      commands_registered_ms: metric(80),
      initialize_completed_ms: metric(480),
      first_useful_request_ms: metric(680),
      warm_request_ms: metric(880),
      restart_milestone_ms: metric(1080),
      shutdown_milestone_ms: metric(1280),
      feature_mcp_registration_ms: metric(0),
    },
  };
  const comparison = compareActivationDeferral({
    baseline,
    deferred,
    deferredDomains: ['mcp'],
  });
  assert.equal(comparison.decision, 'defer_construction');
  assert.equal(comparison.activation_delta_ms, 20);
  assert.equal(comparison.immaterial_activation_delta_ms, IMMATERIAL_ACTIVATION_DELTA_MS);
});

void test('defer_construction when milestones shift with faster activation', () => {
  const baseline = summary(400, 20);
  const deferred = {
    sample_count: 3,
    metrics: {
      activate_returned_ms: metric(380),
      commands_registered_ms: metric(80),
      initialize_completed_ms: metric(480),
      first_useful_request_ms: metric(680),
      warm_request_ms: metric(880),
      restart_milestone_ms: metric(1080),
      shutdown_milestone_ms: metric(1280),
      feature_mcp_registration_ms: metric(0),
    },
  };
  const comparison = compareActivationDeferral({
    baseline,
    deferred,
    deferredDomains: ['mcp'],
  });
  assert.equal(comparison.violations.length, 0);
  assert.equal(comparison.decision, 'defer_construction');
});

void test('reports violations when deferred arm did not defer registration', () => {
  const comparison = compareActivationDeferral({
    baseline: summary(400, 12),
    deferred: summary(380, 8),
    deferredDomains: ['mcp'],
  });
  assert.ok(comparison.violations.some((entry) => entry.includes('zero-cost deferral')));
});
