const assert = require('node:assert/strict');
const { test } = require('node:test');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const JSZip = require('jszip');
const {
  baselineForPlatform,
  classifyInventoryViolations,
  compareInventory,
  currentSourceBundleFile,
  parseArgs,
  platformForPackagedFile,
  summarizeInventory,
} = require('./check-vsix-inventory');

void test('rejects unknown, missing, and duplicate archive arguments', () => {
  assert.throws(() => parseArgs(['--wrong']), /Unknown argument/);
  assert.throws(() => parseArgs(['--vsix']), /requires a value/);
  assert.throws(() => parseArgs(['--vsix', 'one.vsix', '--vsix', 'two.vsix']), /duplicate --vsix/);
  assert.throws(
    () => parseArgs(['--update-baseline', '--update-baseline']),
    /duplicate --update-baseline/,
  );
});

void test('summarizes packaged file sizes', () => {
  assert.deepEqual(
    summarizeInventory([
      { file: 'a.txt', bytes: 2 },
      { file: 'b.js', bytes: 3 },
    ]),
    {
      schema_version: 1,
      total_files: 2,
      total_bytes: 5,
      files: { 'a.txt': 2, 'b.js': 3 },
    },
  );
});

void test('rejects package growth and inventory drift', () => {
  const violations = compareInventory(
    { total_files: 2, total_bytes: 8, files: { 'a.txt': 5, 'new.js': 3 } },
    { total_files: 2, total_bytes: 5, files: { 'a.txt': 2, 'old.js': 3 } },
  );
  assert.deepEqual(violations, [
    'total bytes grew from 5 to 8',
    'file a.txt grew from 2 to 5 bytes',
    'new packaged file: new.js',
    'baseline packaged file is missing: old.js',
  ]);
});

void test('classifies byte-only growth separately from structural package drift', () => {
  assert.equal(
    classifyInventoryViolations([
      'total bytes grew from 10 to 12',
      'file out/extension.js grew from 8 to 10 bytes',
    ]),
    'size_only',
  );
  assert.equal(classifyInventoryViolations(['new packaged file: unexpected.exe']), 'structural');
});

void test('near-miss size messages are never classified as size-only', () => {
  // size_only admits installed behavior through behavior_safe, so messages
  // that merely resemble the size pattern must stay structural.
  assert.equal(
    classifyInventoryViolations(['file out/extension.js grew from 8 to 10']),
    'structural',
  );
  assert.equal(
    classifyInventoryViolations(['file out/extension.js grew from 8 to 10 bytes; see receipt']),
    'structural',
  );
  assert.equal(classifyInventoryViolations(['total bytes grew from 10 to 12 ']), 'structural');
});

void test('classifies an unchanged inventory as pass', () => {
  assert.equal(classifyInventoryViolations([]), 'pass');
});

void test('uses only the current platform baseline entries', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };
  const linuxBaseline = baselineForPlatform(baseline, 'linux', 'x64');

  assert.deepEqual(linuxBaseline, {
    schema_version: 1,
    total_files: 2,
    total_bytes: 10,
    files: { 'README.md': 2, 'bin/linux-x64/perllsp': 8 },
  });
  assert.deepEqual(
    compareInventory(
      {
        total_files: 3,
        total_bytes: 20,
        files: {
          'README.md': 2,
          'bin/linux-x64/perllsp': 8,
          'bin/win32-x64/perllsp.exe': 10,
        },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('still rejects growth for a platform-owned file', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 13,
        files: {
          'README.md': 2,
          'bin/win32-x64/perllsp.exe': 11,
        },
      },
      baseline,
      'win32',
      { arch: 'x64' },
    ),
    ['total bytes grew from 12 to 13', 'file bin/win32-x64/perllsp.exe grew from 10 to 11 bytes'],
  );
});

void test('accepts a known foreign platform bundle without requiring it on this host', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 3,
        total_bytes: 20,
        files: {
          'README.md': 2,
          'bin/linux-x64/perllsp': 8,
          'bin/win32-x64/perllsp.exe': 10,
        },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('rejects an unexpected foreign platform bundle', () => {
  const baseline = {
    total_files: 1,
    total_bytes: 2,
    files: { 'README.md': 2 },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, 'bin/darwin-arm64/perllsp': 10 },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    ['unexpected foreign-platform packaged file: bin/darwin-arm64/perllsp'],
  );
});

void test('allows the explicitly staged current-source target when no baseline exists', () => {
  const baseline = {
    total_files: 1,
    total_bytes: 2,
    files: { 'README.md': 2 },
  };
  const currentSourceFile = currentSourceBundleFile('darwin', 'arm64');

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, [currentSourceFile]: 10 },
      },
      baseline,
      'darwin',
      { allowedFiles: [currentSourceFile], arch: 'arm64' },
    ),
    [],
  );
});

void test('checks an explicitly staged current-source target already in the baseline', () => {
  const baseline = {
    total_files: 2,
    total_bytes: 12,
    files: { 'README.md': 2, 'bin/darwin-arm64/perllsp': 10 },
  };
  const currentSourceFile = currentSourceBundleFile('darwin', 'arm64');

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 13,
        files: { 'README.md': 2, [currentSourceFile]: 11 },
      },
      baseline,
      'darwin',
      { allowedFiles: [currentSourceFile], arch: 'arm64' },
    ),
    [],
  );
});

void test('selects the exact platform and architecture baseline', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 30,
    files: {
      'README.md': 2,
      'bin/linux-x64/perllsp': 10,
      'bin/linux-arm64/perllsp': 18,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, 'bin/linux-x64/perllsp': 10 },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('does not classify ordinary files as platform-owned', () => {
  assert.equal(platformForPackagedFile('assets/demo-project/main.pl'), null);
  assert.equal(platformForPackagedFile('bin/win32-x64/perllsp.exe'), 'win32');
  assert.equal(platformForPackagedFile('bin/linux-x64/perllsp'), 'linux');
  assert.equal(platformForPackagedFile('bin/darwin-arm64/perllsp'), 'darwin');
});

const extensionRoot = path.resolve(__dirname, '..');
const checker = path.join(__dirname, 'check-vsix-inventory.js');
const baseline = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'vsix-inventory-baseline.json'), 'utf8'),
);
const packageVersion = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
).version;
const sourceSha = 'a'.repeat(40);
const topologySha = 'b'.repeat(64);

function sha256(value) {
  return crypto.createHash('sha256').update(value).digest('hex');
}

function semanticInventory(files) {
  const inventory = {
    schema_version: 1,
    total_files: Object.keys(files).length,
    total_bytes: Object.values(files).reduce((sum, bytes) => sum + bytes, 0),
    files: Object.fromEntries(Object.entries(files).sort()),
  };
  return { inventory, sha: sha256(Buffer.from(JSON.stringify(inventory))) };
}

async function fixture(directory, extraFiles = {}, missingMembers = []) {
  const files = {
    ...baseline.files,
    'bin/win32-x64/perllsp.exe': 6,
    'bin/win32-x64/perl-dap.exe': 3,
    ...extraFiles,
  };
  for (const member of missingMembers) delete files[`bin/win32-x64/${member}`];
  const archive = new JSZip();
  for (const [name, bytes] of Object.entries(files)) {
    archive.file(`extension/${name}`, Buffer.alloc(bytes));
  }
  archive.file('[Content_Types].xml', '<Types/>');
  archive.file('extension.vsixmanifest', '<PackageManifest/>');
  const vsixPath = path.join(directory, 'fixture.vsix');
  fs.writeFileSync(vsixPath, await archive.generateAsync({ type: 'nodebuffer' }));
  const projection = {
    releaseTopologySha256: topologySha,
    targets: [
      {
        target: 'x86_64-pc-windows-msvc',
        os: 'windows',
        architecture: 'x86_64',
        libc: null,
        archiveName: 'fixture.zip',
        requiredMembers: ['perllsp.exe', 'perl-dap.exe'],
      },
    ],
    includeUniversalManaged: false,
  };
  const projectionPath = path.join(directory, 'projection.json');
  fs.writeFileSync(projectionPath, JSON.stringify(projection));
  const inventory = semanticInventory(files);
  const manifest = {
    schema: 'vsix_candidate_payload.v1',
    extension: { id: 'EffortlessMetrics.perl-lsp-rs', version: packageVersion, sourceSha },
    candidate: { id: 'fixture', release: packageVersion, sourceSha },
    releaseTopologySha256: topologySha,
    package: {
      vscodeTargetId: 'win32-x64',
      rustTarget: 'x86_64-pc-windows-msvc',
      mode: 'target_specific',
      inventorySha256: inventory.sha,
    },
    server: {
      candidateId: 'fixture',
      target: 'x86_64-pc-windows-msvc',
      member: 'perllsp.exe',
      sha256: sha256(Buffer.alloc(6)),
      identityRef: 'fixture:server',
    },
    dap: {
      disposition: 'required_present',
      payload: {
        candidateId: 'fixture',
        target: 'x86_64-pc-windows-msvc',
        member: 'perl-dap.exe',
        sha256: sha256(Buffer.alloc(3)),
        identityRef: 'fixture:dap',
      },
    },
  };
  const manifestPath = path.join(directory, 'manifest.json');
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  return { manifest, manifestPath, projectionPath, vsixPath, files };
}

function runChecker(paths, { currentSourceSmoke = false } = {}) {
  /** @type {Record<string, string>} */
  const environment = {
    ...process.env,
    PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: paths.manifestPath,
    PERL_LSP_VSIX_PROJECTION_INPUT: paths.projectionPath,
    PERL_LSP_VSCODE_TARGET: 'win32-x64',
  };
  if (currentSourceSmoke) environment.PERL_LSP_CURRENT_SOURCE_SMOKE = '1';
  return spawnSync(process.execPath, [checker, '--vsix', paths.vsixPath], {
    cwd: extensionRoot,
    env: environment,
    encoding: 'utf8',
    windowsHide: true,
  });
}

void test('CLI admits validated manifest server and DAP members', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-manifest-'));
  try {
    const paths = await fixture(directory);
    const result = runChecker(paths);
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /"violations": \[\]/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('CLI rejects forged manifest identity and inventory claims', async () => {
  /** @type {Record<string, RegExp>} */
  const expectedErrors = {
    schema: /canonical projection output/,
    member: /member disagrees with the topology projection/,
    target: /canonical projection output/,
    inventory: /inventory SHA does not match/,
  };
  for (const [mutation, expectedError] of Object.entries(expectedErrors)) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-manifest-'));
    try {
      const paths = await fixture(directory);
      const manifest = JSON.parse(fs.readFileSync(paths.manifestPath, 'utf8'));
      if (mutation === 'schema') manifest.schema = 'forged.v1';
      if (mutation === 'member') manifest.server.member = 'forged.exe';
      if (mutation === 'target') manifest.package.vscodeTargetId = 'linux-x64';
      if (mutation === 'inventory') manifest.package.inventorySha256 = 'c'.repeat(64);
      fs.writeFileSync(paths.manifestPath, JSON.stringify(manifest));
      const result = runChecker(paths);
      assert.equal(result.error, undefined, `${mutation} failed to launch: ${result.error}`);
      assert.notEqual(result.status, 0, `${mutation} unexpectedly passed`);
      assert.match(`${result.stdout}\n${result.stderr}`, expectedError);
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
  }
});

void test('CLI rejects foreign native and unrelated additions despite matching manifest digest', async () => {
  for (const extra of ['bin/linux-x64/perllsp', 'unexpected/extra.txt']) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-manifest-'));
    try {
      const paths = await fixture(directory, { [extra]: 4 });
      const manifest = JSON.parse(fs.readFileSync(paths.manifestPath, 'utf8'));
      manifest.package.inventorySha256 = semanticInventory(paths.files).sha;
      fs.writeFileSync(paths.manifestPath, JSON.stringify(manifest));
      const result = runChecker(paths, { currentSourceSmoke: true });
      assert.notEqual(result.status, 0, `${extra} unexpectedly passed`);
      assert.match(result.stdout, /unexpected|foreign|new packaged file/);
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
  }
});

void test('CLI rejects a manifest whose required native member is absent', async () => {
  for (const member of ['perllsp.exe', 'perl-dap.exe']) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-manifest-'));
    try {
      const paths = await fixture(directory, {}, [member]);
      const result = runChecker(paths);
      assert.equal(result.error, undefined, `checker failed to launch: ${result.error}`);
      assert.notEqual(result.status, 0, `${member} unexpectedly passed`);
      assert.match(
        `${result.stdout}\n${result.stderr}`,
        /member is missing from the produced VSIX/,
      );
    } finally {
      fs.rmSync(directory, { recursive: true, force: true });
    }
  }
});
