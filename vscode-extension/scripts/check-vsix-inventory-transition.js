#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const {
  baselineForPlatform,
  bundleTargetForPackagedFile,
  collectPackagedFiles,
  compareInventory,
  currentSourceBundleFile,
  summarizeInventory,
} = require('./check-vsix-inventory');

const extensionRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(extensionRoot, '..');
const baselinePath = path.join(__dirname, 'vsix-inventory-baseline.json');
const baselineRepoPath = 'vscode-extension/scripts/vsix-inventory-baseline.json';
const declarationPath = path.join(__dirname, 'vsix-inventory-transition.json');

function parseArgs(argv) {
  const result = { base: '', receipt: '' };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--base' || argument === '--receipt') {
      const value = argv[index + 1];
      if (!value) {
        throw new Error(`${argument} requires a value`);
      }
      result[argument.slice(2)] = value;
      index += 1;
    } else {
      throw new Error(`Unknown argument: ${argument}`);
    }
  }
  return result;
}

function runGit(args, allowFailure = false) {
  const result = spawnSync('git', args, {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
  if (result.error) {
    if (allowFailure) {
      return null;
    }
    throw result.error;
  }
  if (result.status !== 0) {
    if (allowFailure) {
      return null;
    }
    throw new Error(`git ${args.join(' ')} failed: ${(result.stderr || result.stdout || '').trim()}`);
  }
  return result.stdout.trim();
}

function resolveRevision(revision) {
  return runGit(['rev-parse', revision]);
}

function resolveBaseRevision(explicitBase = '') {
  const requested = explicitBase.trim() || (process.env.PERL_LSP_PACKAGE_BASE_SHA || '').trim();
  if (requested && !/^0+$/.test(requested)) {
    return resolveRevision(requested);
  }

  const mergeBase = runGit(['merge-base', 'HEAD', 'origin/main'], true);
  if (mergeBase) {
    return mergeBase;
  }

  return runGit(['rev-parse', 'HEAD^'], true);
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function readBaselineAtRevision(revision) {
  const content = runGit(['show', `${revision}:${baselineRepoPath}`]);
  return JSON.parse(content);
}

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function canonicalInventory(inventory) {
  const files = Object.fromEntries(
    Object.entries(inventory.files || {})
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([file, bytes]) => [file, Number(bytes)]),
  );
  return summarizeInventory(Object.entries(files).map(([file, bytes]) => ({ file, bytes })));
}

function sha256Inventory(inventory) {
  return sha256Bytes(Buffer.from(JSON.stringify(canonicalInventory(inventory))));
}

function fileDigest(relativePath) {
  const absolute = path.join(repoRoot, relativePath);
  return fs.existsSync(absolute) && fs.statSync(absolute).isFile()
    ? sha256Bytes(fs.readFileSync(absolute))
    : null;
}

function projectInventory(inventory, platform, arch, ignoredFiles = []) {
  const target = `${platform}-${arch}`;
  const ignored = new Set(ignoredFiles);
  const entries = Object.entries(inventory.files || {})
    .filter(([file]) => {
      if (ignored.has(file)) {
        return false;
      }
      const fileTarget = bundleTargetForPackagedFile(file);
      return fileTarget === null || fileTarget === target;
    })
    .map(([file, bytes]) => ({ file, bytes }));
  return canonicalInventory(summarizeInventory(entries));
}

function inventoriesEqual(left, right) {
  return JSON.stringify(canonicalInventory(left)) === JSON.stringify(canonicalInventory(right));
}

function inventoryDelta(before, after) {
  const oldFiles = canonicalInventory(before).files;
  const newFiles = canonicalInventory(after).files;
  const additions = [];
  const removals = [];
  const changed = [];

  for (const [file, bytes] of Object.entries(newFiles)) {
    if (!Object.hasOwn(oldFiles, file)) {
      additions.push({ file, bytes });
    } else if (oldFiles[file] !== bytes) {
      changed.push({ file, before: oldFiles[file], after: bytes, delta: bytes - oldFiles[file] });
    }
  }
  for (const [file, bytes] of Object.entries(oldFiles)) {
    if (!Object.hasOwn(newFiles, file)) {
      removals.push({ file, bytes });
    }
  }
  return { additions, removals, changed };
}

function readDeclaration() {
  return fs.existsSync(declarationPath) ? readJson(declarationPath) : null;
}

function validateDeclaration(declaration, baseDigest, candidateDigest) {
  const violations = [];
  if (!declaration || typeof declaration !== 'object') {
    return ['baseline changed without scripts/vsix-inventory-transition.json'];
  }
  if (declaration.schema_version !== 1) {
    violations.push('transition declaration schema_version must be 1');
  }
  if (!Number.isInteger(declaration.owner_issue) || declaration.owner_issue <= 0) {
    violations.push('transition declaration owner_issue must be a positive integer');
  }
  if (typeof declaration.reason !== 'string' || declaration.reason.trim().length < 12) {
    violations.push('transition declaration reason must be a specific non-empty explanation');
  }
  if (declaration.base_baseline_sha256 !== baseDigest) {
    violations.push('transition declaration base_baseline_sha256 does not match the selected base');
  }
  if (declaration.candidate_baseline_sha256 !== candidateDigest) {
    violations.push('transition declaration candidate_baseline_sha256 does not match this candidate');
  }
  return violations;
}

function evaluateTransition({
  actual,
  baseBaseline,
  candidateBaseline,
  declaration,
  platform,
  arch,
  ignoredFiles = [],
}) {
  const projectedActual = projectInventory(actual, platform, arch, ignoredFiles);
  const projectedCandidate = projectInventory(candidateBaseline, platform, arch, ignoredFiles);
  const fullBaselineChanged = !inventoriesEqual(baseBaseline, candidateBaseline);
  const actualMatchesCandidate = inventoriesEqual(projectedActual, projectedCandidate);
  const baseDigest = sha256Inventory(baseBaseline);
  const candidateDigest = sha256Inventory(candidateBaseline);
  const declarationViolations = fullBaselineChanged
    ? validateDeclaration(declaration, baseDigest, candidateDigest)
    : [];
  const policyViolations = compareInventory(actual, candidateBaseline, platform, {
    allowedFiles: ignoredFiles,
    arch,
  });

  let state;
  let passed = false;
  if (!fullBaselineChanged && actualMatchesCandidate) {
    state = 'no_change';
    passed = policyViolations.length === 0;
  } else if (!fullBaselineChanged) {
    state = 'transition_required';
  } else if (!actualMatchesCandidate) {
    state = 'invalid_baseline_update';
  } else if (declarationViolations.length > 0) {
    state = 'undeclared_transition';
  } else {
    state = 'transition_candidate';
    passed = policyViolations.length === 0;
  }

  return {
    state,
    passed,
    baseline_changed: fullBaselineChanged,
    actual_matches_candidate_baseline: actualMatchesCandidate,
    base_baseline_sha256: baseDigest,
    candidate_baseline_sha256: candidateDigest,
    actual_inventory_sha256: sha256Inventory(projectedActual),
    policy_violations: policyViolations,
    declaration_violations: declarationViolations,
    delta: inventoryDelta(baseBaseline, candidateBaseline),
    projected_actual: projectedActual,
    projected_candidate_baseline: projectedCandidate,
  };
}

function writeJsonAtomic(destination, value) {
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(temporary, destination);
}

function defaultReceiptPath(candidateSha) {
  const root = (process.env.PERL_LSP_SMOKE_RECEIPTS_DIR || '').trim()
    ? path.resolve(process.env.PERL_LSP_SMOKE_RECEIPTS_DIR)
    : path.join(repoRoot, 'target', 'receipts', 'vscode-smoke');
  return path.join(root, `vsix-inventory-transition-${candidateSha}.json`);
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const candidateSha = resolveRevision('HEAD');
  const baseSha = resolveBaseRevision(args.base);
  const receiptPath = args.receipt
    ? path.resolve(args.receipt)
    : defaultReceiptPath(candidateSha);

  if (!baseSha) {
    const receipt = {
      schema_version: 'vsix_inventory_transition.v1',
      receipt_kind: 'vsix_inventory_transition',
      candidate_sha: candidateSha,
      base_sha: null,
      state: 'not_proven',
      passed: false,
      reason: 'unable to resolve a base revision',
    };
    writeJsonAtomic(receiptPath, receipt);
    process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
    return 2;
  }

  const actual = summarizeInventory(collectPackagedFiles());
  const baseBaseline = readBaselineAtRevision(baseSha);
  const candidateBaseline = readJson(baselinePath);
  const ignoredFiles =
    process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1'
      ? [currentSourceBundleFile(process.platform, process.arch)]
      : [];
  const evaluation = evaluateTransition({
    actual,
    baseBaseline,
    candidateBaseline,
    declaration: readDeclaration(),
    platform: process.platform,
    arch: process.arch,
    ignoredFiles,
  });
  const receipt = {
    schema_version: 'vsix_inventory_transition.v1',
    receipt_kind: 'vsix_inventory_transition',
    candidate_sha: candidateSha,
    base_sha: baseSha,
    platform: process.platform,
    architecture: process.arch,
    inputs: {
      package_json_sha256: fileDigest('vscode-extension/package.json'),
      package_lock_sha256: fileDigest('vscode-extension/package-lock.json'),
      rolldown_config_sha256: fileDigest('vscode-extension/rolldown.config.mjs'),
      extension_bundle_sha256: fileDigest('vscode-extension/out/extension.js'),
    },
    declaration: readDeclaration(),
    claim_boundary:
      'Compares the candidate package inventory with the accepted base/candidate baselines on this platform; it does not prove extension behavior.',
    ...evaluation,
  };
  writeJsonAtomic(receiptPath, receipt);
  process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
  return receipt.passed ? 0 : 1;
}

if (require.main === module) {
  try {
    process.exit(main());
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.stack || error.message : String(error)}\n`);
    process.exit(2);
  }
}

module.exports = {
  canonicalInventory,
  evaluateTransition,
  inventoriesEqual,
  inventoryDelta,
  projectInventory,
  sha256Inventory,
  validateDeclaration,
  writeJsonAtomic,
};
