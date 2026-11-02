#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');

const ACTIVATION_CRITICAL_DEFER_DOMAINS = new Set(['providers', 'debugger']);

function parseRunCount(env = process.env) {
  const runs = Number(env.PERL_LSP_VSCODE_SAMPLE_RUNS || '3');
  if (!Number.isSafeInteger(runs) || runs < 1) {
    throw new Error(
      `Sample count must be a positive integer, received: ${env.PERL_LSP_VSCODE_SAMPLE_RUNS}`,
    );
  }
  return runs;
}

function parseDeferredDomainList(raw) {
  return raw
    .split(/[,;\s]+/)
    .map((token) => token.trim())
    .filter(Boolean);
}

function validateDeferredDomains(domains) {
  if (domains.length === 0) {
    throw new Error('PERL_LSP_ACTIVATION_DEFER_DOMAINS must list at least one domain');
  }
  for (const domain of domains) {
    if (ACTIVATION_CRITICAL_DEFER_DOMAINS.has(domain)) {
      throw new Error(
        `domain ${JSON.stringify(domain)} is activation-critical and cannot be deferred in this experiment`,
      );
    }
  }
}

/**
 * @param {number} runs
 * @returns {NodeJS.ProcessEnv}
 */
function baselineSampleEnv(runs) {
  /** @type {NodeJS.ProcessEnv} */
  const env = {
    ...process.env,
    PERL_LSP_VSCODE_SAMPLE_RUNS: String(runs),
  };
  delete env.PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT;
  delete env.PERL_LSP_ACTIVATION_DEFER_DOMAINS;
  return env;
}

function runSampleScript(sampleRoot, env) {
  const result = spawnSync(
    process.execPath,
    [path.join(root, 'scripts', 'sample-local-vsix-smoke.js')],
    {
      cwd: root,
      env: {
        ...env,
        PERL_LSP_VSCODE_SAMPLES_DIR: sampleRoot,
      },
      stdio: 'inherit',
      windowsHide: true,
    },
  );
  return result.status ?? 1;
}

function summarize(sampleRoot, summaryPath) {
  return (
    spawnSync(
      process.execPath,
      [path.join(root, 'scripts', 'summarize-vscode-receipts.js'), sampleRoot, summaryPath],
      { cwd: root, env: process.env, stdio: 'inherit', windowsHide: true },
    ).status ?? 1
  );
}

function compare(baselineSummary, deferredSummary, outputPath, env) {
  return (
    spawnSync(
      process.execPath,
      [
        path.join(root, 'scripts', 'compare-activation-deferral.js'),
        baselineSummary,
        deferredSummary,
        outputPath,
      ],
      { cwd: root, env, stdio: 'inherit', windowsHide: true },
    ).status ?? 1
  );
}

function assertFreshExperimentRoot(experimentRoot) {
  if (!fs.existsSync(experimentRoot)) {
    return;
  }
  const entries = fs.readdirSync(experimentRoot);
  if (entries.length > 0) {
    throw new Error(
      `experiment root ${experimentRoot} is not empty; set a fresh PERL_LSP_ACTIVATION_DEFERRAL_ROOT`,
    );
  }
}

function main() {
  const runs = parseRunCount();
  const deferredDomains = parseDeferredDomainList(
    process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS ?? 'mcp',
  );
  validateDeferredDomains(deferredDomains);
  const deferredDomainsJoined = deferredDomains.join(',');

  const experimentRoot = path.resolve(
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_ROOT ??
      path.join(root, 'target', 'receipts', 'activation-deferral', `run-${Date.now()}`),
  );
  assertFreshExperimentRoot(experimentRoot);
  const baselineRoot = path.join(experimentRoot, 'baseline');
  const deferredRoot = path.join(experimentRoot, 'deferred');
  fs.mkdirSync(baselineRoot, { recursive: true });
  fs.mkdirSync(deferredRoot, { recursive: true });

  process.stdout.write(`\n=== activation deferral baseline (${runs} samples) ===\n`);
  if (runSampleScript(baselineRoot, baselineSampleEnv(runs)) !== 0) {
    process.exitCode = 1;
    return;
  }
  const baselineSummary = path.join(baselineRoot, 'summary.json');
  if (summarize(baselineRoot, baselineSummary) !== 0) {
    process.exitCode = 1;
    return;
  }

  process.stdout.write(
    `\n=== activation deferral experiment (${runs} samples, defer ${deferredDomainsJoined}) ===\n`,
  );
  const deferredEnv = {
    ...baselineSampleEnv(runs),
    PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT: '1',
    PERL_LSP_ACTIVATION_DEFER_DOMAINS: deferredDomainsJoined,
  };
  if (runSampleScript(deferredRoot, deferredEnv) !== 0) {
    process.exitCode = 1;
    return;
  }
  const deferredSummary = path.join(deferredRoot, 'summary.json');
  if (summarize(deferredRoot, deferredSummary) !== 0) {
    process.exitCode = 1;
    return;
  }

  const comparisonPath =
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_COMPARISON ??
    path.join(experimentRoot, 'activation-deferral-comparison.json');
  const compareStatus = compare(baselineSummary, deferredSummary, comparisonPath, deferredEnv);
  if (compareStatus !== 0) {
    process.exitCode = compareStatus === 2 ? 2 : 1;
  }
  process.stdout.write(`\nWrote comparison to ${comparisonPath}\n`);
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
