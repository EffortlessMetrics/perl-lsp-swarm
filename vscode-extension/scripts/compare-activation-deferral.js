#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

/** @typedef {'keep_eager' | 'defer_construction' | 'dynamic_import_experiment'} DeferralDecision */

/** @typedef {{ sample_count: number; metrics: Record<string, { p50: number|null; p95: number|null; min: number|null; max: number|null; count: number }> }} ReceiptSummary */

const IMMATERIAL_ACTIVATION_DELTA_MS = 5;

/**
 * @param {unknown} summary
 * @returns {summary is ReceiptSummary}
 */
function isSummary(summary) {
  if (!summary || typeof summary !== 'object') {
    return false;
  }
  const candidate = /** @type {Record<string, unknown>} */ (summary);
  return (
    typeof candidate.sample_count === 'number' &&
    candidate.metrics !== null &&
    candidate.metrics !== undefined &&
    typeof candidate.metrics === 'object'
  );
}

/**
 * @param {Record<string, { p50: number|null; p95: number|null; min: number|null; max: number|null; count: number }>} metrics
 * @param {string} name
 */
function metricP50(metrics, name) {
  return metrics[name]?.p50 ?? null;
}

/**
 * @param {Record<string, { p50: number|null; p95: number|null; min: number|null; max: number|null; count: number }>} metrics
 * @param {string} name
 */
function metricP95(metrics, name) {
  return metrics[name]?.p95 ?? null;
}

/**
 * @param {{
 *   baseline: ReceiptSummary;
 *   deferred: ReceiptSummary;
 *   deferredDomains: string[];
 *   minimumSamples?: number;
 * }} input
 */
function compareActivationDeferral(input) {
  const minimumSamples = input.minimumSamples ?? 3;
  const violations = [];
  if (input.baseline.sample_count < minimumSamples) {
    violations.push(
      `baseline sample_count ${input.baseline.sample_count} is below required ${minimumSamples}`,
    );
  }
  if (input.deferred.sample_count < minimumSamples) {
    violations.push(
      `deferred sample_count ${input.deferred.sample_count} is below required ${minimumSamples}`,
    );
  }
  for (const domain of input.deferredDomains) {
    const registrationKey = `feature_${domain}_registration_ms`;
    const baselineRegistration = input.baseline.metrics[registrationKey];
    if (!baselineRegistration || baselineRegistration.count < minimumSamples) {
      violations.push(`baseline is missing ${registrationKey} measurements for ${domain}`);
    }
    const deferredRegistration = input.deferred.metrics[registrationKey];
    if (!deferredRegistration || deferredRegistration.p50 !== 0) {
      violations.push(`deferred arm did not record zero-cost deferral for ${domain}`);
    }
  }

  const baselineActivation = metricP50(input.baseline.metrics, 'activate_returned_ms');
  const deferredActivation = metricP50(input.deferred.metrics, 'activate_returned_ms');
  const activationDelta =
    baselineActivation !== null && deferredActivation !== null
      ? baselineActivation - deferredActivation
      : null;

  const behaviorChecks = [
    'commands_registered_ms',
    'initialize_completed_ms',
    'first_useful_request_ms',
    'warm_request_ms',
    'restart_milestone_ms',
    'shutdown_milestone_ms',
  ];
  for (const metricName of behaviorChecks) {
    const baseline = metricP50(input.baseline.metrics, metricName);
    const deferred = metricP50(input.deferred.metrics, metricName);
    if (baseline === null || deferred === null) {
      violations.push(`missing paired ${metricName} for behavior comparison`);
      continue;
    }
    if (Math.abs(baseline - deferred) > IMMATERIAL_ACTIVATION_DELTA_MS) {
      violations.push(
        `${metricName} p50 diverged beyond immaterial noise (${baseline} vs ${deferred})`,
      );
    }
  }

  /** @type {DeferralDecision} */
  let decision = 'keep_eager';
  let rationale =
    'Deferral did not produce a material activation benefit or behavior parity failed.';
  if (violations.length === 0) {
    if (activationDelta !== null && activationDelta >= IMMATERIAL_ACTIVATION_DELTA_MS) {
      decision = 'defer_construction';
      rationale =
        'Matched same-host samples show a material activate_returned_ms improvement when deferring non-critical registration without behavior divergence.';
    } else {
      decision = 'keep_eager';
      rationale =
        'Registration deferral did not materially improve activate_returned_ms; eager registration remains justified for the selected seam.';
    }
  }

  return {
    schema_version: 'feature_activation_deferral_comparison.v1',
    generated_at: new Date().toISOString(),
    deferred_domains: input.deferredDomains,
    minimum_samples: minimumSamples,
    immaterial_activation_delta_ms: IMMATERIAL_ACTIVATION_DELTA_MS,
    baseline: {
      sample_count: input.baseline.sample_count,
      activate_returned_ms: {
        p50: baselineActivation,
        p95: metricP95(input.baseline.metrics, 'activate_returned_ms'),
      },
    },
    deferred: {
      sample_count: input.deferred.sample_count,
      activate_returned_ms: {
        p50: deferredActivation,
        p95: metricP95(input.deferred.metrics, 'activate_returned_ms'),
      },
    },
    activation_delta_ms: activationDelta,
    decision,
    rationale,
    violations,
    performance_budget_promoted: false,
    claim_boundary:
      'Comparison summarizes matched same-host current-source receipts only. It does not promote a performance budget or justify dynamic imports unless decision=dynamic_import_experiment with separate chunking proof.',
  };
}

function readSummary(summaryPath) {
  const summary = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
  if (!isSummary(summary)) {
    throw new Error(`Summary at ${summaryPath} is not a vscode receipt roll-up`);
  }
  return summary;
}

function main() {
  const baselinePath = path.resolve(process.argv[2] ?? '');
  const deferredPath = path.resolve(process.argv[3] ?? '');
  const outputPath = path.resolve(
    process.argv[4] ??
      process.env.PERL_LSP_ACTIVATION_DEFERRAL_COMPARISON ??
      path.join(path.dirname(baselinePath), 'activation-deferral-comparison.json'),
  );
  const deferredDomains = (process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS ?? 'mcp')
    .split(/[,;\s]+/)
    .map((token) => token.trim())
    .filter(Boolean);
  if (!baselinePath || !deferredPath) {
    throw new Error(
      'Usage: compare-activation-deferral.js <baseline-summary.json> <deferred-summary.json> [output.json]',
    );
  }
  const baseline = readSummary(baselinePath);
  const deferred = readSummary(deferredPath);
  const comparison = compareActivationDeferral({ baseline, deferred, deferredDomains });
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(comparison, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(comparison, null, 2)}\n`);
  if (comparison.violations.length > 0) {
    process.exitCode = 2;
  }
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

module.exports = { compareActivationDeferral, IMMATERIAL_ACTIVATION_DELTA_MS };
