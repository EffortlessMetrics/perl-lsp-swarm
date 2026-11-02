#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');

function parseRunCount(env = process.env) {
  const runs = Number(env.PERL_LSP_VSCODE_SAMPLE_RUNS || '3');
  if (!Number.isSafeInteger(runs) || runs < 1) {
    throw new Error(
      `Sample count must be a positive integer, received: ${env.PERL_LSP_VSCODE_SAMPLE_RUNS}`,
    );
  }
  return runs;
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

function main() {
  const runs = parseRunCount();
  const experimentRoot = path.resolve(
    process.env.PERL_LSP_ACTIVATION_DEFERRAL_ROOT ??
      path.join(root, 'target', 'receipts', 'activation-deferral', `run-${Date.now()}`),
  );
  const baselineRoot = path.join(experimentRoot, 'baseline');
  const deferredRoot = path.join(experimentRoot, 'deferred');
  fs.mkdirSync(baselineRoot, { recursive: true });
  fs.mkdirSync(deferredRoot, { recursive: true });

  const sharedEnv = {
    ...process.env,
    PERL_LSP_VSCODE_SAMPLE_RUNS: String(runs),
  };
  const deferredDomains = (process.env.PERL_LSP_ACTIVATION_DEFER_DOMAINS ?? 'mcp').trim();

  process.stdout.write(`\n=== activation deferral baseline (${runs} samples) ===\n`);
  if (runSampleScript(baselineRoot, sharedEnv) !== 0) {
    process.exitCode = 1;
    return;
  }
  const baselineSummary = path.join(baselineRoot, 'summary.json');
  if (summarize(baselineRoot, baselineSummary) !== 0) {
    process.exitCode = 1;
    return;
  }

  process.stdout.write(
    `\n=== activation deferral experiment (${runs} samples, defer ${deferredDomains}) ===\n`,
  );
  const deferredEnv = {
    ...sharedEnv,
    PERL_LSP_ACTIVATION_DEFERRAL_EXPERIMENT: '1',
    PERL_LSP_ACTIVATION_DEFER_DOMAINS: deferredDomains,
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
