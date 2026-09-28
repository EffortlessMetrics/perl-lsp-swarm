#!/usr/bin/env node

// An explicit proposal from a produced VSIX. Nothing in this command accepts
// package policy on behalf of a reviewer; --write only prepares reviewable files.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const {
  baselinePath,
  canonicalJson,
  collectArchiveInventory,
  declarationPath,
  inventoriesEqual,
  inventoryDelta,
  parseInventoryDocument,
  projectInventory,
  readBaselineAtRevision,
  readCandidateBaseline,
  semanticInventorySha256,
  validateDeclaration,
} = require('./check-vsix-inventory-transition');
const {
  bundleTargetForPackagedFile,
  classifyInventoryViolations,
  compareInventory,
  currentSourceBundleFiles,
  summarizeInventory,
} = require('./check-vsix-inventory');

const repoRoot = path.resolve(__dirname, '../..');
const SHA = /^[0-9a-f]{40}$/;

function parseArgs(argv) {
  const args = {};
  const names = new Map([
    ['--base', 'base'],
    ['--candidate', 'candidate'],
    ['--vsix', 'vsix'],
    ['--owner-issue', 'ownerIssue'],
    ['--reason', 'reason'],
  ]);
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--write') {
      if (args.write) throw new Error('duplicate --write');
      args.write = true;
    } else if (names.has(arg)) {
      const name = names.get(arg);
      if (args[name] !== undefined) throw new Error(`duplicate ${arg}`);
      const value = argv[++i];
      if (!value || value.startsWith('--')) throw new Error(`${arg} requires a value`);
      args[name] = value;
    } else throw new Error(`unknown argument: ${arg}`);
  }
  for (const name of ['base', 'candidate', 'vsix', 'ownerIssue', 'reason']) {
    if (!args[name])
      throw new Error(`--${name === 'ownerIssue' ? 'owner-issue' : name} is required`);
  }
  if (!SHA.test(args.base) || !SHA.test(args.candidate) || args.base === args.candidate) {
    throw new Error('distinct full lowercase base and candidate commit SHAs are required');
  }
  if (!/^[1-9][0-9]*$/.test(args.ownerIssue) || !Number.isSafeInteger(Number(args.ownerIssue))) {
    throw new Error('--owner-issue must be a positive safe integer');
  }
  if (args.reason.trim().length < 12) throw new Error('--reason requires a specific explanation');
  return args;
}

function git(args) {
  const result = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8', windowsHide: true });
  if (result.error || result.status !== 0) {
    throw new Error(`git ${args.join(' ')} failed: ${result.error?.message || result.stderr}`);
  }
  return result.stdout.trim();
}

// The packaged target can differ from the host that runs this proposal: musl
// Linux mints `alpine`, and cross-packaged VSIX builds name their target with
// PERL_LSP_VSCODE_TARGET exactly as package-vsix does. Deriving from the host
// alone would classify every other target's payload as foreign drift.
function proposalTarget(env = process.env) {
  const target = (env.PERL_LSP_VSCODE_TARGET || '').trim() || `${process.platform}-${process.arch}`;
  const match = /^(win32|linux|alpine|darwin)-(x64|arm64)$/.exec(target);
  if (!match) {
    throw new Error(
      `PERL_LSP_VSCODE_TARGET must name a packaged <platform>-<arch> target, got: ${target}`,
    );
  }
  return { platform: match[1], arch: match[2] };
}

function assertCurrentSubject(args, runGit = git) {
  const head = runGit(['rev-parse', 'HEAD']);
  if (head !== args.candidate) throw new Error('candidate SHA is stale relative to HEAD');
  if (runGit(['rev-parse', `${args.base}^{commit}`]) !== args.base) {
    throw new Error('base must resolve to the exact supplied commit');
  }
  if (runGit(['merge-base', args.base, args.candidate]) !== args.base) {
    throw new Error('base is not an ancestor of candidate');
  }
  // Generated bundle/package output may be ignored, but tracked inputs must
  // be committed before an archive can be attributed to an exact source SHA.
  if (runGit(['status', '--porcelain', '--untracked-files=no'])) {
    throw new Error('tracked worktree changes prevent an exact candidate source claim');
  }
  return head;
}

function candidateInventory(baseInventory, actual, platform, arch, ignoredFiles = []) {
  const target = `${platform}-${arch}`;
  const ignored = new Set(ignoredFiles);
  const files = { ...baseInventory.files };
  for (const name of Object.keys(files)) {
    // An ignored staged payload never proposes its own bytes, so an accepted
    // baseline row for it must survive unchanged rather than read as removed.
    if (ignored.has(name)) continue;
    const fileTarget = bundleTargetForPackagedFile(name);
    if (fileTarget === null || fileTarget === target) delete files[name];
  }
  for (const [name, bytes] of Object.entries(actual.files)) {
    if (ignored.has(name)) continue;
    const fileTarget = bundleTargetForPackagedFile(name);
    if (fileTarget === null || fileTarget === target) files[name] = bytes;
    else if (!Object.hasOwn(baseInventory.files, name) || baseInventory.files[name] !== bytes) {
      throw new Error(`foreign target payload differs from the accepted baseline: ${name}`);
    }
  }
  return summarizeInventory(Object.entries(files).map(([file, bytes]) => ({ file, bytes })));
}

function makeProposal({
  baseDocument,
  candidateDocument,
  archive,
  ownerIssue,
  reason,
  platform,
  arch,
  ignoredFiles = [],
}) {
  if (candidateDocument.file_sha256 !== baseDocument.file_sha256) {
    throw new Error(
      'candidate baseline already differs from exact base; reconcile it before proposing',
    );
  }
  const candidate = candidateInventory(
    baseDocument.value,
    archive.inventory,
    platform,
    arch,
    ignoredFiles,
  );
  const candidateBytes = canonicalJson(candidate);
  const policyViolations = compareInventory(archive.inventory, baseDocument.value, platform, {
    arch,
    allowedFiles: ignoredFiles,
  });
  const packagePolicyClass = classifyInventoryViolations(policyViolations);
  const proposedDocument = parseInventoryDocument(candidateBytes, 'proposed baseline');
  if (
    !inventoriesEqual(
      projectInventory(candidate, platform, arch, ignoredFiles),
      projectInventory(archive.inventory, platform, arch, ignoredFiles),
    )
  ) {
    throw new Error('proposed baseline does not match exact archive projection');
  }
  if (inventoriesEqual(candidate, baseDocument.value)) {
    return {
      state: 'no_change',
      archive_sha256: archive.archive_sha256,
      package_policy_class: packagePolicyClass,
      pretransition_policy_violations: policyViolations,
      base_baseline_file_sha256: baseDocument.file_sha256,
      delta: inventoryDelta(baseDocument.value, candidate),
    };
  }
  const declaration = {
    schema_version: 1,
    owner_issue: Number(ownerIssue),
    reason: reason.trim(),
    base_baseline_file_sha256: baseDocument.file_sha256,
    candidate_baseline_file_sha256: proposedDocument.file_sha256,
    base_inventory_sha256: baseDocument.inventory_sha256,
    candidate_inventory_sha256: semanticInventorySha256(candidate),
  };
  const violations = validateDeclaration(declaration, baseDocument, proposedDocument);
  if (violations.length) throw new Error(violations.join('; '));
  return {
    state: 'transition_candidate',
    archive_sha256: archive.archive_sha256,
    package_policy_class: packagePolicyClass,
    pretransition_policy_violations: policyViolations,
    base_baseline_file_sha256: baseDocument.file_sha256,
    candidate_baseline_file_sha256: proposedDocument.file_sha256,
    delta: inventoryDelta(baseDocument.value, candidate),
    baseline: candidate,
    declaration,
  };
}

function writeProposal(proposal, baseDocument, options = {}) {
  if (proposal.state !== 'transition_candidate') throw new Error('no transition to write');
  const {
    fileSystem = fs,
    baselineFile = baselinePath,
    declarationFile = declarationPath,
    runGit = git,
    baseSha = 'HEAD',
  } = options;
  const existing = parseInventoryDocument(fileSystem.readFileSync(baselineFile), baselineFile);
  if (existing.file_sha256 !== baseDocument.file_sha256) {
    throw new Error('baseline changed since proposal was measured');
  }
  const existingDeclaration = fileSystem.readFileSync(declarationFile);
  const desiredBaseline = canonicalJson(proposal.baseline);
  const desiredDeclaration = canonicalJson(proposal.declaration);
  const changed = runGit([
    'status',
    '--porcelain',
    '--',
    path.relative(repoRoot, baselineFile),
    path.relative(repoRoot, declarationFile),
  ]);
  if (changed) throw new Error('baseline or declaration already has worktree edits');
  // The worktree check above cannot see an edit that the candidate commit
  // already made to the declaration; compare the committed bytes instead of
  // silently overwriting an independently reviewed declaration.
  const declarationRepoPath = path.relative(repoRoot, declarationFile).split(path.sep).join('/');
  if (
    runGit(['show', `HEAD:${declarationRepoPath}`]) !==
    runGit(['show', `${baseSha}:${declarationRepoPath}`])
  ) {
    throw new Error(
      'declaration changed between base and candidate; reconcile it before proposing',
    );
  }
  // Stage both canonical documents first. Restore the original bytes if either
  // rename fails; a future invocation rejects a partly changed baseline.
  const tempBaseline = `${baselineFile}.${process.pid}.proposal`;
  const tempDeclaration = `${declarationFile}.${process.pid}.proposal`;
  try {
    fileSystem.writeFileSync(tempBaseline, desiredBaseline, { flag: 'wx' });
    fileSystem.writeFileSync(tempDeclaration, desiredDeclaration, { flag: 'wx' });
    fileSystem.renameSync(tempBaseline, baselineFile);
    fileSystem.renameSync(tempDeclaration, declarationFile);
  } catch (error) {
    if (fileSystem.readFileSync(baselineFile, 'utf8') === desiredBaseline) {
      fileSystem.writeFileSync(baselineFile, canonicalJson(baseDocument.value));
    }
    fileSystem.writeFileSync(declarationFile, existingDeclaration);
    throw error;
  } finally {
    fileSystem.rmSync(tempBaseline, { force: true });
    fileSystem.rmSync(tempDeclaration, { force: true });
  }
}

async function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  assertCurrentSubject(args);
  const archive = await collectArchiveInventory(path.resolve(args.vsix));
  const baseDocument = readBaselineAtRevision(args.base);
  const candidateDocument = readCandidateBaseline();
  const { platform, arch } = proposalTarget();
  const ignoredFiles =
    process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1'
      ? currentSourceBundleFiles(
          platform,
          arch,
          process.env.PERL_LSP_CURRENT_SOURCE_DAP_STAGED === '1',
        )
      : [];
  const proposal = makeProposal({
    baseDocument,
    candidateDocument,
    archive,
    ownerIssue: args.ownerIssue,
    reason: args.reason,
    platform,
    arch,
    ignoredFiles,
  });
  if (args.write && proposal.state === 'no_change') {
    throw new Error('no inventory transition exists to write');
  }
  // Recheck before any write: HEAD and measured archive may have moved.
  if (args.write && proposal.state === 'transition_candidate') {
    assertCurrentSubject(args);
    const after = await collectArchiveInventory(path.resolve(args.vsix));
    if (after.archive_sha256 !== archive.archive_sha256)
      throw new Error('VSIX changed during proposal');
    writeProposal(proposal, baseDocument, { baseSha: args.base });
  }
  process.stdout.write(
    `${JSON.stringify(
      {
        ...proposal,
        base_sha: args.base,
        candidate_sha: args.candidate,
        written: Boolean(args.write && proposal.state === 'transition_candidate'),
        acceptance: 'proposal_only',
      },
      null,
      2,
    )}\n`,
  );
}

if (require.main === module)
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });

module.exports = {
  parseArgs,
  assertCurrentSubject,
  candidateInventory,
  makeProposal,
  proposalTarget,
  writeProposal,
  main,
};
