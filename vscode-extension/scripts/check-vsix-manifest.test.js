const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const JSZip = require('jszip');
const { test } = require('node:test');

const extensionRoot = path.resolve(__dirname, '..');
const checker = path.join(__dirname, 'check-vsix-inventory.js');
const baseline = JSON.parse(
  fs.readFileSync(path.join(__dirname, 'vsix-inventory-baseline.json'), 'utf8'),
);
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

async function fixture(directory, extraFiles = {}) {
  const files = {
    ...baseline.files,
    'bin/win32-x64/perllsp.exe': 6,
    'bin/win32-x64/perl-dap.exe': 3,
    ...extraFiles,
  };
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
    extension: { id: 'EffortlessMetrics.perl-lsp-rs', version: '0.17.0', sourceSha },
    candidate: { id: 'fixture', release: '0.17.0', sourceSha },
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
  for (const mutation of ['schema', 'member', 'target', 'inventory']) {
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
      assert.notEqual(result.status, 0, `${mutation} unexpectedly passed`);
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
