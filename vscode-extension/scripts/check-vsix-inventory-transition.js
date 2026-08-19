#!/usr/bin/env node

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const AdmZip = require('adm-zip');
const {
  bundleTargetForPackagedFile,
  classifyInventoryViolations,
  compareInventory,
  currentSourceBundleFile,
  summarizeInventory,
} = require('./check-vsix-inventory');

const extensionRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(extensionRoot, '..');
const baselinePath = path.join(__dirname, 'vsix-inventory-baseline.json');
const baselineRepoPath = 'vscode-extension/scripts/vsix-inventory-baseline.json';
const declarationPath = path.join(__dirname, 'vsix-inventory-transition.json');
const INVENTORY_KEYS = ['files', 'schema_version', 'total_bytes', 'total_files'];
const DECLARATION_KEYS = [
  'base_baseline_file_sha256',
  'base_inventory_sha256',
  'candidate_baseline_file_sha256',
  'candidate_inventory_sha256',
  'owner_issue',
  'reason',
  'schema_version',
];
const SHA256_PATTERN = /^[0-9a-f]{64}$/;

function parseArgs(argv) {
  const result = { base: '', receipt: '', vsix: '' };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--base' || argument === '--receipt' || argument === '--vsix') {
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

function runGitTextResult(args) {
  return spawnSync('git', args, {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

function runGitBytesResult(args) {
  return spawnSync('git', args, {
    cwd: repoRoot,
    windowsHide: true,
  });
}

function runGit(args) {
  const result = runGitTextResult(args);
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `git ${args.join(' ')} failed: ${(result.stderr || result.stdout || '').trim()}`,
    );
  }
  return result.stdout.trim();
}

function runGitOptional(args) {
  const result = runGitTextResult(args);
  if (result.error || result.status !== 0) {
    return null;
  }
  return result.stdout.trim();
}

function runGitRaw(args) {
  const result = runGitBytesResult(args);
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    const message = Buffer.concat([
      result.stderr || Buffer.alloc(0),
      result.stdout || Buffer.alloc(0),
    ])
      .toString('utf8')
      .trim();
    throw new Error(`git ${args.join(' ')} failed: ${message}`);
  }
  return result.stdout;
}

function resolveRevision(revision) {
  return runGit(['rev-parse', `${revision}^{commit}`]);
}

function ensureDistinctBase(candidateSha, baseSha, source = 'base revision') {
  if (!baseSha) {
    throw new Error(`unable to resolve ${source}`);
  }
  if (baseSha === candidateSha) {
    throw new Error(`${source} resolves to the candidate itself; provide the accepted base SHA`);
  }
  return baseSha;
}

function resolveBaseRevision(candidateSha, explicitBase = '') {
  const requested = explicitBase.trim() || (process.env.PERL_LSP_PACKAGE_BASE_SHA || '').trim();
  if (requested && !/^0+$/.test(requested)) {
    return ensureDistinctBase(candidateSha, resolveRevision(requested), 'requested base revision');
  }

  const mergeBase = runGitOptional(['merge-base', 'HEAD', 'origin/main']);
  if (mergeBase && mergeBase !== candidateSha) {
    return mergeBase;
  }

  const parent = runGitOptional(['rev-parse', 'HEAD^']);
  return ensureDistinctBase(candidateSha, parent, 'fallback parent revision');
}

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function assertExactKeys(value, expected, label) {
  if (!isPlainObject(value)) {
    throw new Error(`${label} must be an object`);
  }
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
    throw new Error(
      `${label} has unsupported fields: expected ${wanted.join(', ')}, got ${actual.join(', ')}`,
    );
  }
}

function assertNonNegativeSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a non-negative safe integer`);
  }
}

function assertCanonicalPackagePath(file) {
  if (
    typeof file !== 'string' ||
    file.length === 0 ||
    file.startsWith('/') ||
    file.endsWith('/') ||
    file.includes('\\') ||
    /[\u0000-\u001f\u007f]/.test(file) ||
    path.posix.normalize(file) !== file ||
    file.split('/').some((segment) => segment === '' || segment === '.' || segment === '..')
  ) {
    throw new Error(
      `inventory path is not a canonical relative package path: ${JSON.stringify(file)}`,
    );
  }
}

function canonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const VSIX_PAYLOAD_PREFIX = 'extension/';

/**
 * Read the package inventory from the exact VSIX archive that was produced,
 * never from the mutable worktree projection that `vsce ls` reports.
 *
 * Packaging hooks, archive metadata, or a post-package worktree mutation can
 * make the two disagree; only the archive is the artifact that ships.
 *
 * @param {string} vsixPath
 * @returns {{
 *   inventory: { schema_version: number, total_files: number, total_bytes: number, files: Record<string, number> },
 *   archive_sha256: string,
 *   metadata_entries: string[],
 * }}
 */
function collectArchiveInventory(vsixPath) {
  const archiveBytes = fs.readFileSync(vsixPath);

  let zip;
  try {
    zip = new AdmZip(archiveBytes);
  } catch (error) {
    throw new Error(
      `unable to read VSIX archive ${vsixPath}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  /** @type {{ file: string, bytes: number }[]} */
  const entries = [];
  /** @type {string[]} */
  const metadataEntries = [];
  const seen = new Set();

  for (const entry of zip.getEntries()) {
    const rawName = String(entry.entryName);
    if (seen.has(rawName)) {
      throw new Error(`VSIX archive contains a duplicate entry name: ${JSON.stringify(rawName)}`);
    }
    seen.add(rawName);
    if (entry.isDirectory) {
      continue;
    }
    if (!rawName.startsWith(VSIX_PAYLOAD_PREFIX)) {
      // `[Content_Types].xml` and `extension.vsixmanifest` are vsce packaging
      // metadata; they are outside the inventory baseline's claim but are
      // still named in the receipt and covered by the whole-archive digest.
      metadataEntries.push(rawName);
      continue;
    }
    const file = rawName.slice(VSIX_PAYLOAD_PREFIX.length);
    assertCanonicalPackagePath(file);
    const bytes = entry.header.size;
    assertNonNegativeSafeInteger(bytes, `VSIX archive entry ${JSON.stringify(rawName)} size`);
    entries.push({ file, bytes });
  }

  if (entries.length === 0) {
    throw new Error(`VSIX archive ${vsixPath} contains no ${VSIX_PAYLOAD_PREFIX} payload entries`);
  }

  return {
    inventory: summarizeInventory(entries),
    archive_sha256: sha256Bytes(archiveBytes),
    metadata_entries: metadataEntries.sort(),
  };
}

function validateInventoryObject(value, label = 'inventory') {
  assertExactKeys(value, INVENTORY_KEYS, label);
  if (value.schema_version !== 1) {
    throw new Error(`${label}.schema_version must be 1`);
  }
  assertNonNegativeSafeInteger(value.total_files, `${label}.total_files`);
  assertNonNegativeSafeInteger(value.total_bytes, `${label}.total_bytes`);
  if (!isPlainObject(value.files)) {
    throw new Error(`${label}.files must be an object`);
  }

  const entries = Object.entries(value.files);
  let totalBytes = 0;
  for (const [file, bytes] of entries) {
    assertCanonicalPackagePath(file);
    assertNonNegativeSafeInteger(bytes, `${label}.files[${JSON.stringify(file)}]`);
    totalBytes += bytes;
    if (!Number.isSafeInteger(totalBytes)) {
      throw new Error(`${label}.total_bytes exceeds the safe integer range`);
    }
  }
  if (entries.length !== value.total_files) {
    throw new Error(
      `${label}.total_files is ${value.total_files}, but files contains ${entries.length} entries`,
    );
  }
  if (totalBytes !== value.total_bytes) {
    throw new Error(`${label}.total_bytes is ${value.total_bytes}, but files sum to ${totalBytes}`);
  }
  return value;
}

function normalizedInventory(value) {
  validateInventoryObject(value);
  const files = Object.fromEntries(
    Object.entries(value.files).sort(([left], [right]) => left.localeCompare(right)),
  );
  return {
    schema_version: 1,
    total_files: value.total_files,
    total_bytes: value.total_bytes,
    files,
  };
}

function semanticInventorySha256(value) {
  return sha256Bytes(Buffer.from(JSON.stringify(normalizedInventory(value)), 'utf8'));
}

function parseInventoryDocument(raw, source) {
  const text = Buffer.isBuffer(raw) ? raw.toString('utf8') : String(raw);
  let value;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `${source} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  validateInventoryObject(value, source);
  if (text !== canonicalJson(value)) {
    throw new Error(
      `${source} is not in canonical two-space JSON form; duplicate keys, hidden fields, or byte drift are not accepted`,
    );
  }
  return {
    value,
    file_sha256: sha256Bytes(Buffer.from(text, 'utf8')),
    inventory_sha256: semanticInventorySha256(value),
  };
}

function readBaselineAtRevision(revision) {
  const raw = runGitRaw(['show', `${revision}:${baselineRepoPath}`]);
  return parseInventoryDocument(raw, `${baselineRepoPath}@${revision}`);
}

function readCandidateBaseline() {
  return parseInventoryDocument(fs.readFileSync(baselinePath), baselineRepoPath);
}

function parseDeclarationDocument(raw, source = declarationPath) {
  const text = Buffer.isBuffer(raw) ? raw.toString('utf8') : String(raw);
  let value;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `${source} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  assertExactKeys(value, DECLARATION_KEYS, source);
  if (text !== canonicalJson(value)) {
    throw new Error(`${source} is not in canonical two-space JSON form`);
  }
  return value;
}

function readDeclaration() {
  return fs.existsSync(declarationPath)
    ? parseDeclarationDocument(fs.readFileSync(declarationPath))
    : null;
}

function validateDeclaration(declaration, baseDocument, candidateDocument) {
  if (!declaration) {
    return ['baseline changed without scripts/vsix-inventory-transition.json'];
  }

  const violations = [];
  if (declaration.schema_version !== 1) {
    violations.push('transition declaration schema_version must be 1');
  }
  if (!Number.isSafeInteger(declaration.owner_issue) || declaration.owner_issue <= 0) {
    violations.push('transition declaration owner_issue must be a positive safe integer');
  }
  if (typeof declaration.reason !== 'string' || declaration.reason.trim().length < 12) {
    violations.push('transition declaration reason must be a specific non-empty explanation');
  }
  for (const field of [
    'base_baseline_file_sha256',
    'candidate_baseline_file_sha256',
    'base_inventory_sha256',
    'candidate_inventory_sha256',
  ]) {
    if (!SHA256_PATTERN.test(declaration[field] || '')) {
      violations.push(`transition declaration ${field} must be a lowercase SHA-256 digest`);
    }
  }
  if (declaration.base_baseline_file_sha256 !== baseDocument.file_sha256) {
    violations.push(
      'transition declaration base_baseline_file_sha256 does not match the exact base bytes',
    );
  }
  if (declaration.candidate_baseline_file_sha256 !== candidateDocument.file_sha256) {
    violations.push(
      'transition declaration candidate_baseline_file_sha256 does not match the exact candidate bytes',
    );
  }
  if (declaration.base_inventory_sha256 !== baseDocument.inventory_sha256) {
    violations.push('transition declaration base_inventory_sha256 does not match base semantics');
  }
  if (declaration.candidate_inventory_sha256 !== candidateDocument.inventory_sha256) {
    violations.push(
      'transition declaration candidate_inventory_sha256 does not match candidate semantics',
    );
  }
  return violations;
}

function fileDigest(relativePath) {
  const absolute = path.join(repoRoot, relativePath);
  if (!fs.existsSync(absolute)) {
    return null;
  }
  const stat = fs.lstatSync(absolute);
  if (!stat.isFile() || stat.isSymbolicLink()) {
    throw new Error(`expected regular file for digest: ${relativePath}`);
  }
  return sha256Bytes(fs.readFileSync(absolute));
}

function projectInventory(inventory, platform, arch, ignoredFiles = []) {
  validateInventoryObject(inventory, 'projected inventory input');
  const target = `${platform}-${arch}`;
  const ignored = new Set(ignoredFiles);
  const entries = Object.entries(inventory.files)
    .filter(([file]) => {
      if (ignored.has(file)) {
        return false;
      }
      const fileTarget = bundleTargetForPackagedFile(file);
      return fileTarget === null || fileTarget === target;
    })
    .map(([file, bytes]) => ({ file, bytes }));
  return summarizeInventory(entries);
}

function inventoriesEqual(left, right) {
  return semanticInventorySha256(left) === semanticInventorySha256(right);
}

function inventoryDelta(before, after) {
  const oldFiles = normalizedInventory(before).files;
  const newFiles = normalizedInventory(after).files;
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

/**
 * @param {{
 *   actual: any,
 *   baseDocument: any,
 *   candidateDocument: any,
 *   declaration: any,
 *   platform: NodeJS.Platform,
 *   arch: NodeJS.Architecture,
 *   ignoredFiles?: string[],
 * }} input
 */
function evaluateTransition({
  actual,
  baseDocument,
  candidateDocument,
  declaration,
  platform,
  arch,
  ignoredFiles = [],
}) {
  validateInventoryObject(actual, 'actual package inventory');
  const baseBaseline = baseDocument.value;
  const candidateBaseline = candidateDocument.value;
  const projectedActual = projectInventory(actual, platform, arch, ignoredFiles);
  const projectedCandidate = projectInventory(candidateBaseline, platform, arch, ignoredFiles);
  const baselineChanged = baseDocument.file_sha256 !== candidateDocument.file_sha256;
  const actualMatchesCandidate = inventoriesEqual(projectedActual, projectedCandidate);
  const declarationViolations = baselineChanged
    ? validateDeclaration(declaration, baseDocument, candidateDocument)
    : [];
  const policyViolations = compareInventory(actual, candidateBaseline, platform, {
    allowedFiles: ignoredFiles,
    arch,
  });
  const packagePolicyClass = classifyInventoryViolations(policyViolations);

  let state;
  let passed = false;
  if (!baselineChanged && actualMatchesCandidate) {
    state = 'no_change';
    passed = policyViolations.length === 0;
  } else if (!baselineChanged) {
    state = 'transition_required';
  } else if (!actualMatchesCandidate) {
    state = 'invalid_baseline_update';
  } else if (declarationViolations.length > 0) {
    state = 'undeclared_transition';
  } else {
    state = 'transition_candidate';
    passed = policyViolations.length === 0;
  }

  // Fail closed: only explicitly behavior-safe package classes admit installed
  // behavior. Any future classification (including `not_proven`) stays unsafe
  // until it is deliberately listed here.
  const behaviorSafe =
    state !== 'invalid_baseline_update' &&
    (packagePolicyClass === 'pass' || packagePolicyClass === 'size_only');

  return {
    state,
    passed,
    behavior_safe: behaviorSafe,
    package_policy_class: packagePolicyClass,
    baseline_changed: baselineChanged,
    actual_matches_candidate_baseline: actualMatchesCandidate,
    base_baseline_file_sha256: baseDocument.file_sha256,
    candidate_baseline_file_sha256: candidateDocument.file_sha256,
    base_inventory_sha256: baseDocument.inventory_sha256,
    candidate_inventory_sha256: candidateDocument.inventory_sha256,
    actual_inventory_sha256: semanticInventorySha256(projectedActual),
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

function safeFilePart(value) {
  return String(value || 'unknown').replace(/[^A-Za-z0-9_.-]+/g, '-');
}

function defaultReceiptPath(candidateSha) {
  const configuredRoot = (process.env.PERL_LSP_SMOKE_RECEIPTS_DIR || '').trim();
  const root = configuredRoot
    ? path.resolve(configuredRoot)
    : path.join(repoRoot, 'target', 'receipts', 'vscode-smoke');
  return path.join(root, `vsix-inventory-transition-${safeFilePart(candidateSha)}.json`);
}

function boundedError(error) {
  const text = error instanceof Error ? error.message : String(error);
  return (
    text
      .replace(/[\u0000-\u001f\u007f]+/g, ' ')
      .trim()
      .slice(0, 1024) || 'unknown error'
  );
}

/**
 * @param {{
 *   candidateSha?: string | null,
 *   baseSha?: string | null,
 *   reason: unknown,
 * }} input
 */
function notProvenReceipt({ candidateSha = null, baseSha = null, reason }) {
  return {
    schema_version: 'vsix_inventory_transition.v1',
    receipt_kind: 'vsix_inventory_transition',
    candidate_sha: candidateSha,
    base_sha: baseSha,
    platform: process.platform,
    architecture: process.arch,
    state: 'not_proven',
    passed: false,
    behavior_safe: false,
    package_policy_class: 'not_proven',
    reason: boundedError(reason),
    claim_boundary:
      'The transition instrument did not establish package policy or installed behavior safety.',
  };
}

function main() {
  let args = { base: '', receipt: '' };
  /** @type {string | null} */
  let candidateSha = null;
  /** @type {string | null} */
  let baseSha = null;
  let receiptPath = defaultReceiptPath('unknown');

  try {
    args = parseArgs(process.argv.slice(2));
    candidateSha = resolveRevision('HEAD');
    receiptPath = args.receipt ? path.resolve(args.receipt) : defaultReceiptPath(candidateSha);
    baseSha = resolveBaseRevision(candidateSha, args.base);

    if (!args.vsix) {
      throw new Error(
        '--vsix must point to the exact package this candidate produced; the worktree projection cannot authorize a transition',
      );
    }
    const archive = collectArchiveInventory(path.resolve(args.vsix));
    const actual = archive.inventory;
    const baseDocument = readBaselineAtRevision(baseSha);
    const candidateDocument = readCandidateBaseline();
    const ignoredFiles =
      process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1'
        ? [currentSourceBundleFile(process.platform, process.arch)]
        : [];
    const declaration = readDeclaration();
    const evaluation = evaluateTransition({
      actual,
      baseDocument,
      candidateDocument,
      declaration,
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
      measured_package: {
        path: path.relative(repoRoot, path.resolve(args.vsix)).replaceAll('\\', '/'),
        archive_sha256: archive.archive_sha256,
        metadata_entries: archive.metadata_entries,
        source: 'vsix_archive',
      },
      inputs: {
        package_json_sha256: fileDigest('vscode-extension/package.json'),
        package_lock_sha256: fileDigest('vscode-extension/package-lock.json'),
        rolldown_config_sha256: fileDigest('vscode-extension/rolldown.config.mjs'),
        extension_bundle_sha256: fileDigest('vscode-extension/out/extension.js'),
      },
      declaration,
      claim_boundary:
        'Compares exact base/candidate baseline bytes and candidate package inventory; installed behavior remains an independent stage.',
      ...evaluation,
    };
    writeJsonAtomic(receiptPath, receipt);
    process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
    return receipt.passed ? 0 : 1;
  } catch (error) {
    const receipt = notProvenReceipt({ candidateSha, baseSha, reason: error });
    try {
      writeJsonAtomic(receiptPath, receipt);
      process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
    } catch (writeError) {
      process.stderr.write(
        `Unable to persist VSIX transition receipt: ${boundedError(writeError)}; original error: ${boundedError(error)}\n`,
      );
    }
    return 2;
  }
}

if (require.main === module) {
  process.exit(main());
}

module.exports = {
  canonicalJson,
  collectArchiveInventory,
  ensureDistinctBase,
  evaluateTransition,
  inventoriesEqual,
  inventoryDelta,
  notProvenReceipt,
  parseDeclarationDocument,
  parseInventoryDocument,
  projectInventory,
  semanticInventorySha256,
  validateDeclaration,
  validateInventoryObject,
  writeJsonAtomic,
};
