const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');
const JSZip = require('jszip');
const {
  collectArchiveInventory,
  ensureDistinctBase,
  evaluateTransition,
  notProvenReceipt,
  parseArgs,
  parseDeclarationDocument,
  parseInventoryDocument,
  projectInventory,
  resolveBaseRevision,
  resolvePullRequestMergeBase,
  semanticInventorySha256,
} = require('./check-vsix-inventory-transition');
const { currentSourceBundleFiles } = require('./check-vsix-inventory');

function inventory(files, extra = {}) {
  return {
    schema_version: 1,
    total_files: Object.keys(files).length,
    total_bytes: Object.values(files).reduce((total, bytes) => total + bytes, 0),
    files,
    ...extra,
  };
}

async function writeJsZip(destination, files) {
  const zip = new JSZip();
  for (const [name, contents] of Object.entries(files)) {
    zip.file(name, contents);
  }
  fs.writeFileSync(
    destination,
    await zip.generateAsync({ type: 'nodebuffer', compression: 'STORE' }),
  );
}

function document(files) {
  const value = inventory(files);
  return parseInventoryDocument(`${JSON.stringify(value, null, 2)}\n`, 'fixture baseline');
}

function git(cwd, args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 0, `git ${args.join(' ')} failed: ${result.stderr || result.stdout}`);
  return result.stdout.trim();
}

function declaration(baseDocument, candidateDocument) {
  return {
    schema_version: 1,
    owner_issue: 7041,
    reason: 'Accept the exact candidate package inventory transition.',
    base_baseline_file_sha256: baseDocument.file_sha256,
    candidate_baseline_file_sha256: candidateDocument.file_sha256,
    base_inventory_sha256: baseDocument.inventory_sha256,
    candidate_inventory_sha256: candidateDocument.inventory_sha256,
  };
}

void test('accepts a canonical closed baseline document', () => {
  const parsed = document({ 'README.md': 2, 'out/extension.js': 8 });
  assert.equal(parsed.value.total_bytes, 10);
  assert.match(parsed.file_sha256, /^[0-9a-f]{64}$/);
  assert.match(parsed.inventory_sha256, /^[0-9a-f]{64}$/);
});

void test('rejects unsupported baseline fields and totals drift', () => {
  const withExtra = inventory({ 'README.md': 2 }, { policy: 'hidden' });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(withExtra, null, 2)}\n`, 'extra fixture'),
    /unsupported fields/,
  );

  const wrongTotal = inventory({ 'README.md': 2 });
  wrongTotal.total_bytes = 99;
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(wrongTotal, null, 2)}\n`, 'total fixture'),
    /files sum to 2/,
  );
});

void test('rejects unsafe integers and non-canonical package paths', () => {
  const unsafe = inventory({ 'README.md': Number.MAX_SAFE_INTEGER + 1 });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(unsafe, null, 2)}\n`, 'unsafe fixture'),
    /non-negative safe integer/,
  );

  const escaping = inventory({ '../README.md': 2 });
  assert.throws(
    () => parseInventoryDocument(`${JSON.stringify(escaping, null, 2)}\n`, 'path fixture'),
    /canonical relative package path/,
  );
});

void test('rejects non-canonical bytes and duplicate JSON keys', () => {
  const compact = JSON.stringify(inventory({ 'README.md': 2 }));
  assert.throws(() => parseInventoryDocument(compact, 'compact fixture'), /canonical two-space/);

  const duplicate = `{
  "schema_version": 1,
  "total_files": 1,
  "total_bytes": 2,
  "files": {
    "README.md": 1,
    "README.md": 2
  }
}\n`;
  assert.throws(
    () => parseInventoryDocument(duplicate, 'duplicate fixture'),
    /canonical two-space/,
  );
});

void test('requires a closed canonical transition declaration', () => {
  const baseDocument = document({ 'README.md': 2 });
  const candidateDocument = document({ 'README.md': 3 });
  const value = declaration(baseDocument, candidateDocument);
  const parsed = parseDeclarationDocument(`${JSON.stringify(value, null, 2)}\n`, 'declaration');
  assert.equal(parsed.owner_issue, 7041);

  const extra = { ...value, hidden: true };
  assert.throws(
    () => parseDeclarationDocument(`${JSON.stringify(extra, null, 2)}\n`, 'declaration'),
    /unsupported fields/,
  );
});

void test('accepts an unchanged exact package', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const result = evaluateTransition({
    actual: baseline.value,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'no_change');
  assert.equal(result.passed, true);
  assert.equal(result.behavior_safe, true);
  assert.equal(result.package_policy_class, 'pass');
});

void test('keeps a size-only transition red while allowing behavioral smoke', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.passed, false);
  assert.equal(result.package_policy_class, 'size_only');
  assert.equal(result.behavior_safe, true);
});

void test('does not execute a structural package transition', () => {
  const baseline = document({ 'README.md': 2, 'out/extension.js': 8 });
  const actual = inventory({ 'README.md': 2, 'new.js': 8 });
  const result = evaluateTransition({
    actual,
    baseDocument: baseline,
    candidateDocument: baseline,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.package_policy_class, 'structural');
  assert.equal(result.behavior_safe, false);
});

void test('preserves behavior evidence for an undeclared but structurally safe baseline move', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateDocument.value,
    baseDocument,
    candidateDocument,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'undeclared_transition');
  assert.equal(result.passed, false);
  assert.equal(result.package_policy_class, 'pass');
  assert.equal(result.behavior_safe, true);
});

void test('rejects a candidate baseline that does not match the produced package', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const actual = inventory({ 'README.md': 2, 'out/extension.js': 9 });
  const result = evaluateTransition({
    actual,
    baseDocument,
    candidateDocument,
    declaration: declaration(baseDocument, candidateDocument),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'invalid_baseline_update');
  assert.equal(result.behavior_safe, false);
});

void test('accepts a declared exact candidate transition', () => {
  const baseDocument = document({ 'README.md': 2, 'out/extension.js': 8 });
  const candidateDocument = document({ 'README.md': 2, 'out/extension.js': 10 });
  const result = evaluateTransition({
    actual: candidateDocument.value,
    baseDocument,
    candidateDocument,
    declaration: declaration(baseDocument, candidateDocument),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_candidate');
  assert.equal(result.passed, true);
  assert.equal(result.behavior_safe, true);
  assert.deepEqual(result.delta.changed, [
    { file: 'out/extension.js', before: 8, after: 10, delta: 2 },
  ]);
});

void test('declared vsce archive-name transition turns the rename delta green', () => {
  // vsce normalizes the three metadata names inside the produced archive; the
  // baseline adopts archive naming with identical bytes, and the declaration
  // binds both baseline documents. This is the live #7041 transition.
  const onDisk = { 'README.md': 2, LICENSE: 1, 'CHANGELOG.md': 3, 'out/extension.js': 8 };
  const archiveNames = {
    'readme.md': 2,
    'LICENSE.txt': 1,
    'changelog.md': 3,
    'out/extension.js': 8,
  };
  const baseDocument = document(onDisk);
  const candidateDocument = document(archiveNames);
  const result = evaluateTransition({
    actual: inventory(archiveNames),
    baseDocument,
    candidateDocument,
    declaration: declaration(baseDocument, candidateDocument),
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_candidate');
  assert.equal(result.passed, true);
  assert.deepEqual(result.delta.removals.map((entry) => entry.file).sort(), [
    'CHANGELOG.md',
    'LICENSE',
    'README.md',
  ]);
  assert.deepEqual(result.delta.additions.map((entry) => entry.file).sort(), [
    'LICENSE.txt',
    'changelog.md',
    'readme.md',
  ]);
});

void test('an archive-name rename without a declaration fails loudly', () => {
  const onDisk = { 'README.md': 2, LICENSE: 1, 'CHANGELOG.md': 3, 'out/extension.js': 8 };
  const archiveNames = {
    'readme.md': 2,
    'LICENSE.txt': 1,
    'changelog.md': 3,
    'out/extension.js': 8,
  };
  const baseDocument = document(onDisk);
  const candidateDocument = document(archiveNames);
  const result = evaluateTransition({
    actual: inventory(archiveNames),
    baseDocument,
    candidateDocument,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'undeclared_transition');
  // The gate stays red, but behavior evidence survives: the archive still
  // matches the candidate baseline byte-for-byte, so the class is safe.
  assert.equal(result.passed, false);
  assert.equal(result.behavior_safe, true);
});

void test('a stale on-disk-name baseline against archive measurement stays red', () => {
  // If a future vsce naming change moves the archive names again, the
  // unchanged baseline must keep the transition red until re-declared.
  const onDisk = { 'README.md': 2, LICENSE: 1, 'CHANGELOG.md': 3, 'out/extension.js': 8 };
  const shiftedArchiveNames = {
    'readme.markdown': 2,
    'LICENSE.txt': 1,
    'changelog.md': 3,
    'out/extension.js': 8,
  };
  const baseDocument = document(onDisk);
  const result = evaluateTransition({
    actual: inventory(shiftedArchiveNames),
    baseDocument,
    candidateDocument: baseDocument,
    declaration: null,
    platform: 'linux',
    arch: 'x64',
  });

  assert.equal(result.state, 'transition_required');
  assert.equal(result.passed, false);
});

void test('ignores the staged current-source server while retaining ordinary package files', () => {
  const projected = projectInventory(
    inventory({
      'README.md': 2,
      'out/extension.js': 8,
      'bin/linux-x64/perllsp': 100,
      'bin/win32-x64/perllsp.exe': 200,
    }),
    'linux',
    'x64',
    ['bin/linux-x64/perllsp'],
  );

  assert.deepEqual(projected.files, { 'README.md': 2, 'out/extension.js': 8 });
});

void test('transition projection ignores the exact current-source DAP target too', () => {
  /** @type {[string, string]} */
  const currentSourceFiles = currentSourceBundleFiles('linux', 'x64');
  const projected = projectInventory(
    inventory({
      'README.md': 2,
      [currentSourceFiles[0]]: 100,
      [currentSourceFiles[1]]: 200,
      'bin/linux-arm64/perl-dap': 300,
    }),
    'linux',
    'x64',
    currentSourceFiles,
  );

  assert.deepEqual(projected.files, { 'README.md': 2 });
});

void test('semantic digests ignore source object insertion order', () => {
  assert.equal(
    semanticInventorySha256(inventory({ b: 2, a: 1 })),
    semanticInventorySha256(inventory({ a: 1, b: 2 })),
  );
});

void test('rejects a requested base that resolves to the candidate', () => {
  assert.throws(
    () => ensureDistinctBase('a'.repeat(40), 'a'.repeat(40), 'manual base'),
    /candidate itself/,
  );
});

void test('resolves pull request bases through the exact event base merge-base', () => {
  const candidate = 'c'.repeat(40);
  const eventBase = 'e'.repeat(40);
  const mergeBase = 'm'.repeat(40);
  const calls = [];
  const resolved = resolvePullRequestMergeBase(candidate, eventBase, {
    resolveRevision: (revision) => {
      calls.push(['resolve', revision]);
      return eventBase;
    },
    runGitOptional: (args) => {
      calls.push(['git', args]);
      return mergeBase;
    },
  });

  assert.equal(resolved, mergeBase);
  assert.deepEqual(calls, [
    ['resolve', eventBase],
    ['git', ['merge-base', candidate, eventBase]],
  ]);
});

void test('uses a temporary git fork and rejects an unrelated event base', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-merge-base-'));
  try {
    git(directory, ['init', '-q', '-b', 'main']);
    git(directory, ['config', 'user.email', 'test@example.invalid']);
    git(directory, ['config', 'user.name', 'Inventory test']);
    fs.writeFileSync(path.join(directory, 'package.txt'), 'base\n');
    git(directory, ['add', 'package.txt']);
    git(directory, ['commit', '-q', '-m', 'base']);
    const mergeBase = git(directory, ['rev-parse', 'HEAD']);

    git(directory, ['checkout', '-q', '-b', 'candidate']);
    fs.writeFileSync(path.join(directory, 'test.rs'), 'candidate test\n');
    git(directory, ['add', 'test.rs']);
    git(directory, ['commit', '-q', '-m', 'candidate test']);
    const candidate = git(directory, ['rev-parse', 'HEAD']);

    git(directory, ['checkout', '-q', 'main']);
    fs.writeFileSync(path.join(directory, 'package.txt'), 'base package update\n');
    git(directory, ['add', 'package.txt']);
    git(directory, ['commit', '-q', '-m', 'event package']);
    const eventBase = git(directory, ['rev-parse', 'HEAD']);
    const resolve = (revision) => git(directory, ['rev-parse', `${revision}^{commit}`]);
    const runGitOptional = (args) => {
      const result = spawnSync('git', args, {
        cwd: directory,
        encoding: 'utf8',
        windowsHide: true,
      });
      return result.status === 0 ? result.stdout.trim() : null;
    };

    assert.equal(
      resolvePullRequestMergeBase(candidate, eventBase, {
        resolveRevision: resolve,
        runGitOptional,
      }),
      mergeBase,
    );
    assert.equal(git(directory, ['diff', '--name-only', mergeBase, candidate]), 'test.rs');
    assert.equal(git(directory, ['diff', '--name-only', mergeBase, eventBase]), 'package.txt');
    const unchanged = document({ 'package.txt': 2 });
    const result = evaluateTransition({
      actual: unchanged.value,
      baseDocument: unchanged,
      candidateDocument: unchanged,
      declaration: null,
      platform: 'linux',
      arch: 'x64',
    });
    assert.equal(result.state, 'no_change');
    assert.equal(result.passed, true);

    git(directory, ['checkout', '-q', '--orphan', 'unrelated']);
    fs.writeFileSync(path.join(directory, 'unrelated.txt'), 'unrelated\n');
    git(directory, ['add', 'unrelated.txt']);
    git(directory, ['commit', '-q', '-m', 'unrelated']);
    const unrelated = git(directory, ['rev-parse', 'HEAD']);
    assert.throws(
      () =>
        resolvePullRequestMergeBase(candidate, unrelated, {
          resolveRevision: resolve,
          runGitOptional,
        }),
      /pull request merge base/,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('rejects missing or malformed pull request base bindings', () => {
  assert.throws(() => parseArgs(['--merge-base-with']), /--merge-base-with requires a value/);
  assert.throws(
    () =>
      resolvePullRequestMergeBase('c'.repeat(40), 'not-a-sha', {
        resolveRevision: () => 'e'.repeat(40),
        runGitOptional: () => 'm'.repeat(40),
      }),
    /pull request base revision must be a full lowercase commit SHA/,
  );
});

void test('manual accepted bases remain distinct from pull request mode', () => {
  assert.throws(
    () => resolveBaseRevision('c'.repeat(40), 'a'.repeat(40), 'b'.repeat(40)),
    /both an accepted base revision and a pull request base revision/,
  );
});

void test('instrument failures produce a bounded not-proven receipt', () => {
  const receipt = notProvenReceipt({
    candidateSha: 'a'.repeat(40),
    baseSha: null,
    reason: new Error('git show failed\nwith details'),
  });
  assert.equal(receipt.state, 'not_proven');
  assert.equal(receipt.passed, false);
  assert.equal(receipt.behavior_safe, false);
  assert.equal(receipt.package_policy_class, 'not_proven');
  assert.equal(receipt.reason, 'git show failed with details');
});

void test('package inventory is measured from the archive, not the worktree projection', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    // The worktree file and the archived entry deliberately disagree: only the
    // archive is the artifact that ships, so only the archive may be measured.
    const worktreeFile = path.join(directory, 'extension.js');
    fs.writeFileSync(worktreeFile, 'x'.repeat(4096));

    const vsixPath = path.join(directory, 'candidate.vsix');
    await writeJsZip(vsixPath, {
      'extension/out/extension.js': 'a'.repeat(11),
      'extension/package.json': 'b'.repeat(7),
      '[Content_Types].xml': '<Types/>',
      'extension.vsixmanifest': '<PackageManifest/>',
    });

    const result = await collectArchiveInventory(vsixPath);

    assert.deepEqual(result.inventory.files, {
      'out/extension.js': 11,
      'package.json': 7,
    });
    assert.equal(result.inventory.total_files, 2);
    assert.equal(result.inventory.total_bytes, 18);
    assert.deepEqual(result.metadata_entries, ['[Content_Types].xml', 'extension.vsixmanifest']);
    assert.equal(
      result.archive_sha256,
      crypto.createHash('sha256').update(fs.readFileSync(vsixPath)).digest('hex'),
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('an archive with no extension payload cannot authorize a transition', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    const vsixPath = path.join(directory, 'empty.vsix');
    await writeJsZip(vsixPath, { '[Content_Types].xml': '<Types/>' });

    await assert.rejects(collectArchiveInventory(vsixPath), /no extension\/ payload entries/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('rejects a payload corrupted without damaging the central directory', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    const entryName = 'extension/package.json';
    const archive = storedZip([
      [entryName, '{"name":"perl-lsp-rs"}'],
      ['[Content_Types].xml', '<Types/>'],
    ]);
    const payloadOffset = 30 + Buffer.byteLength(entryName);
    archive[payloadOffset] = (archive[payloadOffset] ?? 0) ^ 0xff;
    const vsixPath = path.join(directory, 'payload-corrupt.vsix');
    fs.writeFileSync(vsixPath, archive);

    await assert.rejects(
      collectArchiveInventory(vsixPath),
      /CRC mismatch|unable to read VSIX archive entry/,
    );
    const checker = spawnSync(
      process.execPath,
      [path.join(__dirname, 'check-vsix-inventory.js'), '--vsix', vsixPath],
      { cwd: path.resolve(__dirname, '..'), encoding: 'utf8', windowsHide: true },
    );
    assert.notEqual(checker.status, 0, `corrupt archive unexpectedly passed: ${checker.stdout}`);
    assert.match(
      `${checker.stdout}\n${checker.stderr}`,
      /CRC mismatch|unable to read VSIX archive entry/,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

// JSZip normalizes traversal names when it writes, so a hostile archive has to
// be assembled byte-wise to reach the canonical-path guard at all.
function storedZip(entries) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, contents] of entries) {
    const nameBytes = Buffer.from(name, 'utf8');
    const data = Buffer.from(contents, 'utf8');
    const crc = zlib.crc32(data);

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 8); // stored
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, data);

    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(0, 10); // stored
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(data.length, 20);
    header.writeUInt32LE(data.length, 24);
    header.writeUInt16LE(nameBytes.length, 28);
    header.writeUInt32LE(offset, 42);
    central.push(header, nameBytes);

    offset += local.length + nameBytes.length + data.length;
  }

  const centralBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);

  return Buffer.concat([...locals, centralBytes, end]);
}

void test('an archive entry escaping the package root is rejected', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    const vsixPath = path.join(directory, 'escape.vsix');
    fs.writeFileSync(
      vsixPath,
      storedZip([
        ['extension/out/extension.js', 'ok'],
        ['extension/../../escape.js', 'escaped'],
      ]),
    );

    await assert.rejects(collectArchiveInventory(vsixPath), /canonical relative package path/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('a duplicate archive entry name is rejected', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    const vsixPath = path.join(directory, 'duplicate.vsix');
    fs.writeFileSync(
      vsixPath,
      storedZip([
        ['extension/out/extension.js', 'first'],
        ['extension/out/extension.js', 'second'],
      ]),
    );

    await assert.rejects(collectArchiveInventory(vsixPath), /duplicate entry name/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('an unreadable archive is an instrument failure, not a package verdict', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-archive-'));
  try {
    const vsixPath = path.join(directory, 'corrupt.vsix');
    fs.writeFileSync(vsixPath, 'this is not a zip archive');

    await assert.rejects(collectArchiveInventory(vsixPath), /unable to read VSIX archive/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
