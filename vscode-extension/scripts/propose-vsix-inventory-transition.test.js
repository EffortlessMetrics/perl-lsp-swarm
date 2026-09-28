const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const JSZip = require('jszip');
const {
  collectArchiveInventory,
  parseInventoryDocument,
  canonicalJson,
} = require('./check-vsix-inventory-transition');
const {
  assertCurrentSubject,
  candidateInventory,
  makeProposal,
  parseArgs,
  proposalTarget,
  writeProposal,
} = require('./propose-vsix-inventory-transition');
const { summarizeInventory } = require('./check-vsix-inventory');

function inventory(files) {
  return summarizeInventory(Object.entries(files).map(([file, bytes]) => ({ file, bytes })));
}

function document(files) {
  return parseInventoryDocument(canonicalJson(inventory(files)), 'fixture');
}

async function archive(files) {
  const zip = new JSZip();
  zip.file('extension.vsixmanifest', '<manifest/>');
  for (const [name, content] of Object.entries(files)) zip.file(`extension/${name}`, content);
  return collectArchiveInventory(
    await zip.generateAsync({ type: 'nodebuffer', compression: 'STORE' }),
  );
}

const reason = 'Reviewed candidate package bundle moved by the explicit source change.';
const source = { 'out/extension.js': 3, 'bin/win32-x64/perllsp.exe': 7 };

void test('requires exact subject and explicit human review fields', () => {
  const base = 'a'.repeat(40);
  const candidate = 'b'.repeat(40);
  const args = parseArgs([
    '--base',
    base,
    '--candidate',
    candidate,
    '--vsix',
    'exact.vsix',
    '--owner-issue',
    '6889',
    '--reason',
    reason,
  ]);
  assert.equal(args.write, undefined);
  assert.throws(
    () =>
      parseArgs([
        '--base',
        base,
        '--candidate',
        base,
        '--vsix',
        'exact.vsix',
        '--owner-issue',
        '6889',
        '--reason',
        reason,
      ]),
    /distinct full lowercase/,
  );
  assert.throws(
    () =>
      parseArgs([
        '--base',
        base,
        '--candidate',
        candidate,
        '--vsix',
        'exact.vsix',
        '--owner-issue',
        '6889',
        '--reason',
        'ok',
      ]),
    /specific explanation/,
  );
});

void test('rejects stale candidate HEAD and unrelated base before package admission', () => {
  const base = 'a'.repeat(40);
  const candidate = 'b'.repeat(40);
  const args = { base, candidate };
  assert.throws(() => assertCurrentSubject(args, () => 'c'.repeat(40)), /stale relative to HEAD/);
  assert.throws(
    () =>
      assertCurrentSubject(args, (gitArgs) =>
        gitArgs[0] === 'merge-base' ? 'd'.repeat(40) : gitArgs[1] === 'HEAD' ? candidate : base,
      ),
    /not an ancestor/,
  );
  assert.throws(
    () =>
      assertCurrentSubject(args, (gitArgs) => {
        if (gitArgs[0] === 'status') return ' M vscode-extension/package.json';
        if (gitArgs[1] === 'HEAD') return candidate;
        return base;
      }),
    /tracked worktree changes/,
  );
});

void test('proposes exact archive transition, retaining unobserved target baseline', async () => {
  const baseDocument = document(source);
  const measured = await archive({ 'out/extension.js': 'abcd' });
  const proposal = makeProposal({
    baseDocument,
    candidateDocument: baseDocument,
    archive: measured,
    ownerIssue: '6889',
    reason,
    platform: 'linux',
    arch: 'x64',
  });
  assert.equal(proposal.state, 'transition_candidate');
  assert.equal(proposal.baseline.files['bin/win32-x64/perllsp.exe'], 7);
  assert.equal(proposal.baseline.files['out/extension.js'], 4);
  assert.equal(proposal.declaration.base_baseline_file_sha256, baseDocument.file_sha256);
  assert.equal(
    proposal.declaration.candidate_baseline_file_sha256,
    parseInventoryDocument(canonicalJson(proposal.baseline), 'proposed').file_sha256,
  );
  assert.deepEqual(
    makeProposal({
      baseDocument,
      candidateDocument: baseDocument,
      archive: measured,
      ownerIssue: '6889',
      reason,
      platform: 'linux',
      arch: 'x64',
    }),
    proposal,
  );
});

void test('unchanged output proposes no declaration and cannot bless a baseline', async () => {
  const baseDocument = document(source);
  const measured = await archive({ 'out/extension.js': 'abc' });
  const proposal = makeProposal({
    baseDocument,
    candidateDocument: baseDocument,
    archive: measured,
    ownerIssue: '6889',
    reason,
    platform: 'linux',
    arch: 'x64',
  });
  assert.equal(proposal.state, 'no_change');
  assert.equal(proposal.baseline, undefined);
  assert.equal(proposal.declaration, undefined);
  assert.throws(() => writeProposal(proposal, baseDocument), /no transition to write/);
});

void test('rejects a candidate baseline edited independently of the selected base', async () => {
  const baseDocument = document(source);
  const changedDocument = document({ ...source, 'out/extension.js': 12 });
  const measured = await archive({ 'out/extension.js': 'abcd' });
  assert.throws(
    () =>
      makeProposal({
        baseDocument,
        candidateDocument: changedDocument,
        archive: measured,
        ownerIssue: '6889',
        reason,
        platform: 'linux',
        arch: 'x64',
      }),
    /already differs from exact base/,
  );
});

void test('refuses foreign target payload drift instead of overwriting its unobserved row', () => {
  assert.throws(
    () =>
      candidateInventory(
        inventory(source),
        inventory({
          'out/extension.js': 4,
          'bin/win32-x64/perllsp.exe': 9,
        }),
        'linux',
        'x64',
      ),
    /foreign target payload differs/,
  );
});

void test('an ignored payload path retains its accepted baseline row', () => {
  const candidate = candidateInventory(
    inventory(source),
    inventory({
      'out/extension.js': 4,
      'bin/win32-x64/perllsp.exe': 9,
    }),
    'win32',
    'x64',
    ['bin/win32-x64/perllsp.exe'],
  );
  assert.equal(candidate.files['bin/win32-x64/perllsp.exe'], 7);
  assert.equal(candidate.files['out/extension.js'], 4);
});

void test('proposalTarget honors the packaged target override like package-vsix', () => {
  assert.deepEqual(proposalTarget({ PERL_LSP_VSCODE_TARGET: 'alpine-x64' }), {
    platform: 'alpine',
    arch: 'x64',
  });
  assert.deepEqual(proposalTarget({}), { platform: process.platform, arch: process.arch });
  assert.throws(
    () => proposalTarget({ PERL_LSP_VSCODE_TARGET: 'freebsd-x64' }),
    /packaged <platform>-<arch> target/,
  );
});

void test('rejects invalid archive before a proposal exists', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vsix-proposal-'));
  try {
    const invalid = path.join(tmp, 'invalid.vsix');
    fs.writeFileSync(invalid, 'not a zip');
    await assert.rejects(collectArchiveInventory(invalid), /unable to read VSIX archive/);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

void test('guarded write produces both exact review documents and refuses stale inputs', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vsix-proposal-write-'));
  const baselineFile = path.join(tmp, 'baseline.json');
  const declarationFile = path.join(tmp, 'declaration.json');
  try {
    const baseDocument = document(source);
    fs.writeFileSync(baselineFile, canonicalJson(baseDocument.value));
    fs.writeFileSync(declarationFile, '{}\n');
    const measured = await archive({ 'out/extension.js': 'abcd' });
    const proposal = makeProposal({
      baseDocument,
      candidateDocument: baseDocument,
      archive: measured,
      ownerIssue: '6889',
      reason,
      platform: 'linux',
      arch: 'x64',
    });
    assert.equal(fs.readFileSync(baselineFile, 'utf8'), canonicalJson(baseDocument.value));
    assert.throws(
      () =>
        writeProposal(proposal, baseDocument, {
          baselineFile,
          declarationFile,
          runGit: () => ' M declaration.json',
        }),
      /already has worktree edits/,
    );
    assert.equal(fs.readFileSync(baselineFile, 'utf8'), canonicalJson(baseDocument.value));
    writeProposal(proposal, baseDocument, { baselineFile, declarationFile, runGit: () => '' });
    assert.equal(fs.readFileSync(baselineFile, 'utf8'), canonicalJson(proposal.baseline));
    assert.equal(fs.readFileSync(declarationFile, 'utf8'), canonicalJson(proposal.declaration));
    assert.throws(
      () =>
        writeProposal(proposal, baseDocument, {
          baselineFile,
          declarationFile,
          runGit: () => '',
        }),
      /baseline changed since proposal/,
    );
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

void test('a declaration committed after the base is never silently overwritten', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vsix-proposal-declaration-'));
  const baselineFile = path.join(tmp, 'baseline.json');
  const declarationFile = path.join(tmp, 'declaration.json');
  try {
    const baseDocument = document(source);
    fs.writeFileSync(baselineFile, canonicalJson(baseDocument.value));
    fs.writeFileSync(declarationFile, '{"committed":"candidate-review"}\n');
    const measured = await archive({ 'out/extension.js': 'abcd' });
    const proposal = makeProposal({
      baseDocument,
      candidateDocument: baseDocument,
      archive: measured,
      ownerIssue: '6889',
      reason,
      platform: 'linux',
      arch: 'x64',
    });
    // A clean worktree with a declaration that differs between the selected
    // base and HEAD is an independently committed review decision.
    assert.throws(
      () =>
        writeProposal(proposal, baseDocument, {
          baselineFile,
          declarationFile,
          baseSha: 'a'.repeat(40),
          runGit: (args) =>
            args[0] === 'show' ? (args[1].startsWith('HEAD:') ? 'committed' : 'base') : '',
        }),
      /declaration changed between base and candidate/,
    );
    assert.equal(fs.readFileSync(declarationFile, 'utf8'), '{"committed":"candidate-review"}\n');
    // Identical committed bytes pass the gate.
    writeProposal(proposal, baseDocument, {
      baselineFile,
      declarationFile,
      baseSha: 'a'.repeat(40),
      runGit: (args) => (args[0] === 'show' ? 'committed' : ''),
    });
    assert.equal(fs.readFileSync(declarationFile, 'utf8'), canonicalJson(proposal.declaration));
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

void test('failed second write restores the original baseline and declaration', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vsix-proposal-rollback-'));
  const baselineFile = path.join(tmp, 'baseline.json');
  const declarationFile = path.join(tmp, 'declaration.json');
  try {
    const baseDocument = document(source);
    const oldDeclaration = '{"prior":"review"}\n';
    fs.writeFileSync(baselineFile, canonicalJson(baseDocument.value));
    fs.writeFileSync(declarationFile, oldDeclaration);
    const measured = await archive({ 'out/extension.js': 'abcd' });
    const proposal = makeProposal({
      baseDocument,
      candidateDocument: baseDocument,
      archive: measured,
      ownerIssue: '6889',
      reason,
      platform: 'linux',
      arch: 'x64',
    });
    const fileSystem = {
      ...fs,
      renameSync(from, to) {
        if (to === declarationFile) throw new Error('simulated declaration failure');
        fs.renameSync(from, to);
      },
    };
    assert.throws(
      () =>
        writeProposal(proposal, baseDocument, {
          baselineFile,
          declarationFile,
          fileSystem,
          runGit: () => '',
        }),
      /simulated declaration failure/,
    );
    assert.equal(fs.readFileSync(baselineFile, 'utf8'), canonicalJson(baseDocument.value));
    assert.equal(fs.readFileSync(declarationFile, 'utf8'), oldDeclaration);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
