const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  packageVsix,
  preparePrebuiltPayload,
  validateProjectionManifest,
  validatePrebuiltPayload,
  vsixName,
  vsceEntry,
} = require('./package-vsix');

const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'));

function prebuiltManifest(overrides = {}) {
  return {
    schema: 'vsix_candidate_payload.v1',
    extension: {
      id: 'EffortlessMetrics.perl-lsp-rs',
      version: '0.17.0',
      sourceSha: 'a'.repeat(40),
    },
    candidate: { id: 'candidate-a', release: '0.18.0', sourceSha: 'a'.repeat(40) },
    releaseTopologySha256: 'b'.repeat(64),
    package: {
      vscodeTargetId: 'win32-x64',
      rustTarget: 'x86_64-pc-windows-msvc',
      mode: 'target_specific',
      inventorySha256: 'c'.repeat(64),
    },
    server: {
      candidateId: 'candidate-a',
      target: 'x86_64-pc-windows-msvc',
      member: 'perllsp.exe',
      sha256: 'd'.repeat(64),
      identityRef: 'server-a',
    },
    dap: {
      disposition: 'required_present',
      payload: {
        candidateId: 'candidate-a',
        target: 'x86_64-pc-windows-msvc',
        member: 'perl-dap.exe',
        sha256: 'e'.repeat(64),
        identityRef: 'dap-a',
      },
    },
    ...overrides,
  };
}

function projectionInput() {
  return {
    releaseTopologySha256: 'b'.repeat(64),
    includeUniversalManaged: false,
    targets: [
      {
        target: 'x86_64-pc-windows-msvc',
        os: 'windows',
        architecture: 'x86_64',
        libc: null,
        archiveName: 'perllsp-0.18.0-x86_64-pc-windows-msvc.zip',
        requiredMembers: ['perllsp.exe', 'perl-dap.exe'],
      },
    ],
  };
}

function posixProjectionInput() {
  return {
    releaseTopologySha256: 'b'.repeat(64),
    includeUniversalManaged: false,
    targets: [
      {
        target: 'x86_64-unknown-linux-gnu',
        os: 'linux',
        architecture: 'x86_64',
        libc: 'gnu',
        archiveName: 'perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz',
        requiredMembers: ['perllsp', 'perl-dap'],
      },
    ],
  };
}

void test('prebuilt manifest binds exact Windows server source, target, and bytes', () => {
  const serverBytes = Buffer.from('candidate server bytes');
  const dapBytes = Buffer.from('candidate dap bytes');
  const valid = prebuiltManifest({
    server: {
      ...prebuiltManifest().server,
      sha256: require('node:crypto').createHash('sha256').update(serverBytes).digest('hex'),
    },
    dap: {
      ...prebuiltManifest().dap,
      payload: {
        ...prebuiltManifest().dap.payload,
        sha256: require('node:crypto').createHash('sha256').update(dapBytes).digest('hex'),
      },
    },
  });
  assert.doesNotThrow(() =>
    validatePrebuiltPayload(valid, {
      sourceSha: 'a'.repeat(40),
      target: 'win32-x64',
      rustTarget: 'x86_64-pc-windows-msvc',
      serverBytes,
      dapBytes,
    }),
  );
  assert.doesNotThrow(() =>
    validatePrebuiltPayload(
      {
        ...valid,
        package: {
          ...valid.package,
          vscodeTargetId: 'win32-arm64',
          rustTarget: 'aarch64-pc-windows-msvc',
        },
        server: { ...valid.server, target: 'aarch64-pc-windows-msvc' },
        dap: {
          ...valid.dap,
          payload: { ...valid.dap.payload, target: 'aarch64-pc-windows-msvc' },
        },
      },
      {
        sourceSha: 'a'.repeat(40),
        target: 'win32-arm64',
        rustTarget: 'aarch64-pc-windows-msvc',
        serverBytes,
        dapBytes,
      },
    ),
  );
  assert.throws(
    () =>
      validatePrebuiltPayload(
        { ...valid, package: { ...valid.package, vscodeTargetId: 'linux-x64' } },
        { sourceSha: 'a'.repeat(40), target: 'win32-x64', serverBytes },
      ),
    /target mismatch/,
  );
  assert.throws(
    () =>
      validatePrebuiltPayload(
        { ...valid, package: { ...valid.package, rustTarget: 'x86_64-unknown-linux-gnu' } },
        {
          sourceSha: 'a'.repeat(40),
          target: 'win32-x64',
          rustTarget: 'x86_64-pc-windows-msvc',
          serverBytes,
        },
      ),
    /target mismatch/,
  );
  assert.throws(
    () =>
      validatePrebuiltPayload(valid, {
        sourceSha: 'e'.repeat(40),
        target: 'win32-x64',
        rustTarget: 'x86_64-pc-windows-msvc',
        serverBytes,
      }),
    /source SHA mismatch/,
  );
  assert.throws(
    () =>
      validatePrebuiltPayload(valid, {
        sourceSha: 'a'.repeat(40),
        target: 'win32-x64',
        rustTarget: 'x86_64-pc-windows-msvc',
        serverBytes: Buffer.from('host local bytes'),
      }),
    /server payload SHA mismatch/,
  );
  assert.throws(
    () =>
      validatePrebuiltPayload(
        { ...valid, dap: { disposition: 'required_present', payload: null } },
        {
          sourceSha: 'a'.repeat(40),
          target: 'win32-x64',
          rustTarget: 'x86_64-pc-windows-msvc',
          serverBytes,
        },
      ),
    /missing required DAP payload/,
  );
  assert.throws(
    () =>
      validateProjectionManifest(
        { ...valid, dap: { disposition: 'preview_unavailable', payload: null } },
        projectionInput(),
        'win32-x64',
      ),
    /required DAP payload/,
  );
  assert.throws(
    () =>
      validateProjectionManifest(
        {
          ...valid,
          package: {
            ...valid.package,
            vscodeTargetId: 'win32-arm64',
            rustTarget: 'aarch64-pc-windows-msvc',
          },
          server: { ...valid.server, target: 'aarch64-pc-windows-msvc' },
          dap: {
            ...valid.dap,
            payload: { ...valid.dap.payload, target: 'aarch64-pc-windows-msvc' },
          },
        },
        projectionInput(),
        'win32-x64',
      ),
    /another target/,
  );
  assert.throws(
    () =>
      validateProjectionManifest({ ...valid, package: undefined }, projectionInput(), 'win32-x64'),
    /missing package or DAP identity/,
  );
  assert.throws(
    () => validateProjectionManifest({ ...valid, dap: undefined }, projectionInput(), 'win32-x64'),
    /missing package or DAP identity/,
  );
});

void test('manifest staging rolls back partial writes and rejects symlink destinations', () => {
  const serverBytes = Buffer.from('candidate server bytes');
  const dapBytes = Buffer.from('candidate dap bytes');
  const manifest = prebuiltManifest({
    server: {
      ...prebuiltManifest().server,
      sha256: require('node:crypto').createHash('sha256').update(serverBytes).digest('hex'),
    },
    dap: {
      ...prebuiltManifest().dap,
      payload: {
        ...prebuiltManifest().dap.payload,
        sha256: require('node:crypto').createHash('sha256').update(dapBytes).digest('hex'),
      },
    },
  });
  const writes = [];
  const removals = [];
  const chmods = [];
  const priorServerPath = path.join('bin', 'win32-x64', 'perllsp.exe');
  const projectionPath = 'projection.json';
  /** @type {any} */
  const fileSystem = {
    existsSync: (file) => file.endsWith(priorServerPath),
    readFileSync: (file, encoding) => {
      if (file === 'manifest.json')
        return encoding ? JSON.stringify(manifest) : Buffer.from(JSON.stringify(manifest));
      if (file === projectionPath)
        return encoding
          ? JSON.stringify(projectionInput())
          : Buffer.from(JSON.stringify(projectionInput()));
      if (file === 'server.bin') return serverBytes;
      if (file === 'dap.bin') return dapBytes;
      if (file.endsWith(priorServerPath)) return Buffer.from('prior-server');
      throw new Error(`unexpected read: ${file}`);
    },
    mkdirSync: () => {},
    writeFileSync: (file, bytes) => {
      writes.push({ file, bytes });
      if (writes.length === 2) throw new Error('simulated second payload failure');
    },
    rmSync: (file) => removals.push(file),
    lstatSync: () => ({ mode: 0o640, isSymbolicLink: () => false }),
    chmodSync: (file, mode) => chmods.push({ file, mode }),
  };
  assert.throws(
    () =>
      preparePrebuiltPayload(fileSystem, {
        PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: 'manifest.json',
        PERL_LSP_VSIX_PROJECTION_INPUT: projectionPath,
        PERL_LSP_PREBUILT_SERVER_PATH: 'server.bin',
        PERL_LSP_PREBUILT_DAP_PATH: 'dap.bin',
        PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
        PERL_LSP_RUST_TARGET: 'x86_64-pc-windows-msvc',
        PERL_LSP_VSCODE_TARGET: 'win32-x64',
      }),
    /simulated second payload failure/,
  );
  assert.equal(removals.length, 1);
  assert.equal(chmods.at(-1).mode & 0o777, 0o640);

  assert.throws(
    () =>
      preparePrebuiltPayload(
        /** @type {any} */
        {
          ...fileSystem,
          existsSync: () => true,
          lstatSync: () => ({ isSymbolicLink: () => true }),
        },
        {
          PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: 'manifest.json',
          PERL_LSP_VSIX_PROJECTION_INPUT: projectionPath,
          PERL_LSP_PREBUILT_SERVER_PATH: 'server.bin',
          PERL_LSP_PREBUILT_DAP_PATH: 'dap.bin',
          PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
          PERL_LSP_RUST_TARGET: 'x86_64-pc-windows-msvc',
          PERL_LSP_VSCODE_TARGET: 'win32-x64',
        },
      ),
    /symbolic link/,
  );
});

void test('manifest-enabled packaging stages the supplied payload and verifies the archive member', () => {
  const serverBytes = Buffer.from('candidate server bytes');
  const dapBytes = Buffer.from('candidate dap bytes');
  const manifest = prebuiltManifest({
    server: {
      ...prebuiltManifest().server,
      sha256: require('node:crypto').createHash('sha256').update(serverBytes).digest('hex'),
    },
    dap: {
      ...prebuiltManifest().dap,
      payload: {
        ...prebuiltManifest().dap.payload,
        sha256: require('node:crypto').createHash('sha256').update(dapBytes).digest('hex'),
      },
    },
  });
  const manifestPath = 'manifest.json';
  const projectionPath = 'projection.json';
  const serverPath = 'prebuilt/perllsp.exe';
  const dapPath = 'prebuilt/perl-dap.exe';
  const writes = [];
  const calls = [];
  /** @type {any} */
  const fileSystem = {
    existsSync: () => false,
    rmSync: () => {},
    readFileSync: (file, encoding) => {
      if (file === manifestPath)
        return encoding ? JSON.stringify(manifest) : Buffer.from(JSON.stringify(manifest));
      if (file === projectionPath)
        return encoding
          ? JSON.stringify(projectionInput())
          : Buffer.from(JSON.stringify(projectionInput()));
      if (file === serverPath) return serverBytes;
      if (file === dapPath) return dapBytes;
      throw new Error(`unexpected read: ${file}`);
    },
    mkdirSync: () => {},
    writeFileSync: (file, bytes) => writes.push({ file, bytes }),
    statSync: () => ({ isFile: () => true, size: 1 }),
  };
  const env = {
    PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: manifestPath,
    PERL_LSP_VSIX_PROJECTION_INPUT: projectionPath,
    PERL_LSP_PREBUILT_SERVER_PATH: serverPath,
    PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
    PERL_LSP_RUST_TARGET: 'x86_64-pc-windows-msvc',
    PERL_LSP_VSCODE_TARGET: 'win32-x64',
    PERL_LSP_PREBUILT_DAP_PATH: dapPath,
  };
  const run = (script, args) => {
    calls.push({ script, args });
    return true;
  };
  const envWithoutTarget = Object.fromEntries(
    Object.entries(env).filter(([key]) => key !== 'PERL_LSP_VSCODE_TARGET'),
  );
  assert.equal(packageVsix(run, fileSystem, envWithoutTarget), true);
  assert.equal(writes[0].file.endsWith(path.join('bin', 'win32-x64', 'perllsp.exe')), true);
  assert.equal(writes[0].bytes, serverBytes);
  assert.deepEqual(calls[0].args, ['package', '--target', 'win32-x64', '--out', vsixName]);
  assert.deepEqual(
    calls.slice(1).map(({ script }) => path.basename(script)),
    [
      'check-vsix-prebuilt-payload.js',
      'check-vsix-prebuilt-payload.js',
      'check-vsix-prebuilt-payload.js',
      'check-vsix-inventory.js',
    ],
  );
});

void test('ordinary packaging checks the exact VSIX it just produced', () => {
  const command = packageJson.scripts.package;
  assert.equal(command, 'node scripts/package-vsix.js');
  assert.equal(vsixName, `perl-lsp-rs-${packageJson.version}.vsix`);
  const calls = [];
  /** @type {any} */
  const fileSystem = {
    existsSync: (file) => file === path.join(path.resolve(__dirname, '..'), vsixName),
    rmSync: (file, options) => calls.push({ file, options }),
    statSync: () => ({ isFile: () => true, size: 1 }),
  };
  const run = (script, args) => {
    calls.push({ script, args });
    return true;
  };
  assert.equal(packageVsix(run, fileSystem), true);
  assert.deepEqual(calls, [
    { file: path.join(path.resolve(__dirname, '..'), vsixName), options: { force: true } },
    { script: vsceEntry, args: ['package', '--out', vsixName] },
    {
      script: path.join(__dirname, 'check-vsix-inventory.js'),
      args: ['--vsix', vsixName],
    },
  ]);
});

void test('ambient native payloads require the candidate manifest path', () => {
  /** @type {any} */
  const ambientFileSystem = {
    existsSync: () => true,
    lstatSync: () => ({ isSymbolicLink: () => false }),
  };
  assert.throws(
    () => preparePrebuiltPayload(ambientFileSystem, { PERL_LSP_VSCODE_TARGET: 'win32-x64' }),
    /requires a candidate payload manifest/,
  );
  /** @type {any} */
  const danglingLinkFileSystem = {
    existsSync: () => false,
    lstatSync: () => ({ isSymbolicLink: () => true }),
  };
  assert.throws(
    () => preparePrebuiltPayload(danglingLinkFileSystem, { PERL_LSP_VSCODE_TARGET: 'win32-x64' }),
    /ambient native payload is a symbolic link/,
  );
});

void test('owned manifest staging rejects a real dangling native link', () => {
  const stagingRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-9933-'));
  const serverPath = path.join(stagingRoot, 'server.bin');
  const dapPath = path.join(stagingRoot, 'dap.bin');
  const manifestPath = path.join(stagingRoot, 'manifest.json');
  const projectionPath = path.join(stagingRoot, 'projection.json');
  const serverBytes = Buffer.from('server');
  const dapBytes = Buffer.from('dap');
  const base = prebuiltManifest();
  const manifest = {
    ...base,
    server: {
      ...base.server,
      sha256: require('node:crypto').createHash('sha256').update(serverBytes).digest('hex'),
    },
    dap: {
      ...base.dap,
      payload: {
        ...base.dap.payload,
        sha256: require('node:crypto').createHash('sha256').update(dapBytes).digest('hex'),
      },
    },
  };
  fs.writeFileSync(serverPath, serverBytes);
  fs.writeFileSync(dapPath, dapBytes);
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  fs.writeFileSync(projectionPath, JSON.stringify(projectionInput()));
  const destination = path.join(stagingRoot, 'bin', 'win32-x64', 'perl-dap.exe');
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  try {
    fs.symlinkSync(path.join(stagingRoot, 'missing-dap.exe'), destination);
    assert.throws(
      () =>
        preparePrebuiltPayload(
          fs,
          {
            PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: manifestPath,
            PERL_LSP_VSIX_PROJECTION_INPUT: projectionPath,
            PERL_LSP_PREBUILT_SERVER_PATH: serverPath,
            PERL_LSP_PREBUILT_DAP_PATH: dapPath,
            PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
            PERL_LSP_RUST_TARGET: 'x86_64-pc-windows-msvc',
            PERL_LSP_VSCODE_TARGET: 'win32-x64',
          },
          stagingRoot,
        ),
      /prebuilt payload destination is a symbolic link/,
    );
    assert.equal(fs.existsSync(path.join(stagingRoot, 'missing-dap.exe')), false);
  } finally {
    fs.rmSync(stagingRoot, { recursive: true, force: true });
  }
});

void test(
  'real POSIX staging marks new native payloads executable',
  { skip: process.platform === 'win32' },
  () => {
    const stagingRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-9933-mode-'));
    const serverPath = path.join(stagingRoot, 'server.bin');
    const dapPath = path.join(stagingRoot, 'dap.bin');
    const manifestPath = path.join(stagingRoot, 'manifest.json');
    const projectionPath = path.join(stagingRoot, 'projection.json');
    const serverBytes = Buffer.from('server');
    const dapBytes = Buffer.from('dap');
    const base = prebuiltManifest({
      package: {
        ...prebuiltManifest().package,
        vscodeTargetId: 'linux-x64',
        rustTarget: 'x86_64-unknown-linux-gnu',
      },
      server: {
        ...prebuiltManifest().server,
        target: 'x86_64-unknown-linux-gnu',
        member: 'perllsp',
      },
      dap: {
        ...prebuiltManifest().dap,
        payload: {
          ...prebuiltManifest().dap.payload,
          target: 'x86_64-unknown-linux-gnu',
          member: 'perl-dap',
        },
      },
    });
    const manifest = {
      ...base,
      dap: {
        disposition: 'required_present',
        payload: {
          ...base.dap.payload,
          sha256: require('node:crypto').createHash('sha256').update(dapBytes).digest('hex'),
        },
      },
      server: {
        ...base.server,
        sha256: require('node:crypto').createHash('sha256').update(serverBytes).digest('hex'),
      },
    };
    fs.writeFileSync(serverPath, serverBytes);
    fs.writeFileSync(dapPath, dapBytes);
    fs.writeFileSync(manifestPath, JSON.stringify(manifest));
    fs.writeFileSync(projectionPath, JSON.stringify(posixProjectionInput()));
    const destination = path.join(stagingRoot, 'bin', 'linux-x64', 'perllsp');
    const dapDestination = path.join(stagingRoot, 'bin', 'linux-x64', 'perl-dap');
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, Buffer.from('prior-server'));
    fs.chmodSync(destination, 0o640);
    try {
      const staged = preparePrebuiltPayload(
        fs,
        {
          PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: manifestPath,
          PERL_LSP_VSIX_PROJECTION_INPUT: projectionPath,
          PERL_LSP_PREBUILT_SERVER_PATH: serverPath,
          PERL_LSP_PREBUILT_DAP_PATH: dapPath,
          PERL_LSP_CURRENT_SOURCE_SHA: 'a'.repeat(40),
          PERL_LSP_RUST_TARGET: 'x86_64-unknown-linux-gnu',
          PERL_LSP_VSCODE_TARGET: 'linux-x64',
        },
        stagingRoot,
      );
      assert.equal(fs.statSync(destination).mode & 0o111, 0o111);
      assert.equal(fs.statSync(dapDestination).mode & 0o111, 0o111);
      staged.cleanup();
      assert.deepEqual(fs.readFileSync(destination), Buffer.from('prior-server'));
      assert.equal(fs.statSync(destination).mode & 0o777, 0o640);
      assert.equal(fs.existsSync(dapDestination), false);
    } finally {
      fs.rmSync(stagingRoot, { recursive: true, force: true });
    }
  },
);

void test('packaging failure prevents archive validation', () => {
  const calls = [];
  /** @type {any} */
  const fileSystem = {
    existsSync: (file) => file === path.join(path.resolve(__dirname, '..'), vsixName),
    rmSync: (file, options) => calls.push({ file, options }),
  };
  const run = (script, args) => {
    calls.push({ script, args });
    return false;
  };

  assert.equal(packageVsix(run, fileSystem), false);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].script, vsceEntry);
});

void test('a successful packager without a fresh archive cannot validate a stale file', () => {
  const calls = [];
  let staleFile = true;
  /** @type {any} */
  const fileSystem = {
    existsSync: (file) => staleFile && file === path.join(path.resolve(__dirname, '..'), vsixName),
    rmSync: (file, options) => {
      staleFile = false;
      calls.push({ file, options });
    },
    statSync: () => {
      if (staleFile) {
        return { isFile: () => true, size: 123 };
      }
      throw new Error('ENOENT');
    },
  };

  assert.throws(
    () => packageVsix(() => true, fileSystem),
    new RegExp(`without producing ${vsixName.replace('.', '\\.')}`),
  );
  assert.equal(calls.length, 1);
});

void test('archive validation failure propagates after packaging', () => {
  const calls = [];
  /** @type {any} */
  const fileSystem = {
    existsSync: () => false,
    rmSync: () => {},
    statSync: () => ({ isFile: () => true, size: 1 }),
  };
  const run = (script, args) => {
    calls.push({ script, args });
    return calls.length === 1;
  };

  assert.equal(packageVsix(run, fileSystem), false);
  assert.equal(calls.length, 2);
});

// Benign synthetic composition: no VSCE, downloads or publisher is executed.
async function mappedPackageFixture(extraFile = '') {
  const crypto = require('node:crypto');
  const JSZip = require('jszip');
  const {
    buildVsixCandidatePayloadManifest,
    canonicalVsixPayloadJson,
    deriveVsixTargetProjection,
  } = require('../src/vsixPackageProjection.ts');
  const { semanticInventorySha256 } = require('./check-vsix-inventory-transition');
  const { summarizeInventory } = require('./check-vsix-inventory');
  const repo = path.resolve(__dirname, '../..');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-mapped-package-'));
  const root = path.join(directory, 'vscode-extension');
  fs.mkdirSync(root);
  const digest = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
  const topology = JSON.parse(
    fs.readFileSync(path.join(repo, 'fixtures/rc_vsix_binding/valid.topology.v4.json'), 'utf8'),
  );
  const matrixPath = 'docs/reference/downstream-dap-integrations.json';
  const downloaderPath = 'vscode-extension/src/downloader.ts';
  for (const relative of [matrixPath, downloaderPath]) {
    const destination = path.join(directory, relative);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    const bytes = fs.readFileSync(path.join(repo, relative));
    fs.writeFileSync(destination, bytes);
    topology.sources[relative] = { path: relative, sha256: digest(bytes) };
  }
  const schemaPath = 'schemas/release_topology.v4.schema.json';
  topology.sources[schemaPath].sha256 = digest(fs.readFileSync(path.join(repo, schemaPath)));
  const triples = JSON.parse(fs.readFileSync(path.join(repo, matrixPath), 'utf8')).targets.map(
    (row) => row.triple,
  );
  topology.binary_targets = triples.map((target) => ({
    target,
    runner: target.includes('windows')
      ? 'windows-2022'
      : target.includes('darwin')
        ? 'macos-14'
        : 'ubuntu-22.04',
    os: target.includes('windows') ? 'windows' : target.includes('darwin') ? 'macos' : 'linux',
    architecture: target.startsWith('aarch64') ? 'aarch64' : 'x86_64',
    libc: target.includes('musl') ? 'musl' : target.includes('linux') ? 'gnu' : null,
    archive_name: `perllsp-${topology.release}-${target}${target.includes('windows') ? '.zip' : '.tar.gz'}`,
    required_members: target.includes('windows')
      ? ['perllsp.exe', 'perl-dap.exe']
      : ['perllsp', 'perl-dap'],
  }));
  topology.archive_count = triples.length;
  topology.vsix.managed_targets = [...triples].sort();
  const packageBytes = Buffer.from(
    JSON.stringify({
      publisher: topology.vsix.publisher,
      name: topology.vsix.name,
      version: topology.vsix.version,
    }),
  );
  fs.writeFileSync(path.join(root, 'package.json'), packageBytes);
  const topologyBytes = Buffer.from(JSON.stringify(topology));
  fs.writeFileSync(path.join(directory, 'topology.json'), topologyBytes);
  const projection = deriveVsixTargetProjection({
    releaseTopologySha256: digest(topologyBytes),
    includeUniversalManaged: true,
    targets: topology.binary_targets.map((row) => ({
      target: row.target,
      os: row.os,
      architecture: row.architecture,
      libc: row.libc,
      archiveName: row.archive_name,
      requiredMembers: row.required_members,
    })),
  }).find((row) => row.packageMode === 'universal_managed');
  if (!projection) throw new Error('synthetic universal projection missing');
  const payload = JSON.parse(
    JSON.stringify(
      buildVsixCandidatePayloadManifest({
        schema: 'vsix_candidate_payload.v2',
        preRelease: true,
        extension: {
          id: `${topology.vsix.publisher}.${topology.vsix.name}`,
          version: topology.vsix.version,
          sourceSha: topology.prepared_swarm_sha,
        },
        candidate: {
          id: topology.vsix.candidate_id,
          release: topology.release,
          sourceSha: topology.prepared_swarm_sha,
        },
        releaseTopologySha256: digest(topologyBytes),
        projection,
        packageInventorySha256: 'a'.repeat(64),
      }),
    ),
  );
  const baseline = JSON.parse(
    fs.readFileSync(path.join(__dirname, 'vsix-inventory-baseline.json'), 'utf8'),
  );
  const files = Object.fromEntries(
    Object.keys(baseline.files).map((file) => [file, Buffer.from('x')]),
  );
  files['package.json'] = packageBytes;
  if (extraFile) files[extraFile] = Buffer.from('extra');
  files['vsix-candidate-payload.json'] = Buffer.from(canonicalVsixPayloadJson(payload));
  payload.package.inventorySha256 = semanticInventorySha256(
    summarizeInventory(
      Object.entries(files).map(([file, bytes]) => ({ file, bytes: bytes.length })),
    ),
  );
  fs.writeFileSync(path.join(directory, 'payload.json'), canonicalVsixPayloadJson(payload));
  const env = {
    PERL_LSP_RELEASE_TOPOLOGY: path.join(directory, 'topology.json'),
    PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST: path.join(directory, 'payload.json'),
    PERL_LSP_CURRENT_SOURCE_SHA: topology.prepared_swarm_sha,
  };
  const xml = `<PackageManifest><Metadata><Identity Id="${topology.vsix.name}" Publisher="${topology.vsix.publisher}" Version="${topology.vsix.version}"/><Properties><Property Id="Microsoft.VisualStudio.Code.PreRelease" Value="true"/></Properties></Metadata></PackageManifest>`;
  const output = path.join(root, topology.vsix.asset_name);
  const calls = [];
  /** @param {string} script @param {string[]} args @param {(zip: import("jszip")) => void} change */
  async function run(script, args, change = () => {}) {
    calls.push({ script, args });
    const zip = new JSZip();
    for (const [file, bytes] of Object.entries(files)) zip.file(`extension/${file}`, bytes);
    zip.file(
      'extension/vsix-candidate-payload.json',
      fs.readFileSync(path.join(root, 'vsix-candidate-payload.json')),
    );
    zip.file('extension.vsixmanifest', xml);
    change(zip);
    fs.writeFileSync(output, await zip.generateAsync({ type: 'nodebuffer' }));
    return true;
  }
  return {
    directory,
    root,
    topology,
    payload,
    env,
    output,
    calls,
    run,
    triples,
    cleanup: () => fs.rmSync(directory, { recursive: true, force: true }),
  };
}

async function mappedPackageTests() {
  const { packageMappedVsix } = require('./package-vsix');
  await test('mapped production package preserves the full canonical managed matrix and exact universal identity', async () => {
    const f = await mappedPackageFixture();
    try {
      const before = JSON.stringify(f.env);
      const prior = path.join(f.root, 'vsix-candidate-payload.json');
      fs.writeFileSync(prior, 'prior contents');
      const priorMode = fs.statSync(prior).mode;
      assert.equal(await packageMappedVsix(f.run, fs, f.env, f.root), true);
      assert.deepEqual(
        f.calls.map((row) => row.args),
        [['package', '--pre-release', '--out', f.topology.vsix.asset_name]],
      );
      assert.deepEqual([...f.topology.vsix.managed_targets].sort(), [...f.triples].sort());
      assert.equal(fs.readFileSync(prior, 'utf8'), 'prior contents');
      assert.equal(fs.statSync(prior).mode, priorMode);
      assert.equal(JSON.stringify(f.env), before);
      assert.ok(fs.statSync(f.output).size > 0);
    } finally {
      f.cleanup();
    }
  });
  await test('mapped production admission rejects competing selection and independent identity changes before staging', async () => {
    const cases = [
      (f) => {
        f.payload.package.mode = 'target_specific';
      },
      (f) => {
        f.payload = [f.payload, f.payload];
      },
      (f) => {
        f.topology.vsix.bundled_targets = [f.triples[0]];
      },
      (f) => {
        f.topology.vsix.managed_targets.pop();
      },
      (f) => {
        f.topology.vsix.managed_targets[0] = 'another-target';
      },
      (f) => {
        f.payload.preRelease = false;
      },
      (f) => {
        delete f.payload.preRelease;
      },
      (f) => {
        f.payload.extension.version = '0.19.8';
      },
      (f) => {
        f.payload.candidate.release = '0.18.0-rc.8';
      },
      (f) => {
        f.payload.candidate.id = 'other-candidate';
      },
      (f) => {
        f.env.PERL_LSP_CURRENT_SOURCE_SHA = 'c'.repeat(40);
      },
      (f) => {
        f.payload.releaseTopologySha256 = 'c'.repeat(64);
      },
      (f) => {
        f.env.PERL_LSP_VSCODE_TARGET = 'linux-x64';
      },
      (f) => {
        f.topology.sources['vscode-extension/src/downloader.ts'].sha256 = 'c'.repeat(64);
      },
    ];
    for (const change of cases) {
      const f = await mappedPackageFixture();
      try {
        change(f);
        fs.writeFileSync(f.env.PERL_LSP_RELEASE_TOPOLOGY, JSON.stringify(f.topology));
        fs.writeFileSync(f.env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST, JSON.stringify(f.payload));
        await assert.rejects(packageMappedVsix(f.run, fs, f.env, f.root));
        assert.equal(f.calls.length, 0);
        assert.equal(fs.existsSync(path.join(f.root, 'vsix-candidate-payload.json')), false);
        assert.equal(fs.existsSync(f.output), false);
      } finally {
        f.cleanup();
      }
    }
  });
  await test('mapped production actual archives reject missing or changed metadata and unexpected files with rollback', async () => {
    const cases = [
      (zip) => zip.remove('extension/vsix-candidate-payload.json'),
      (zip) => zip.file('extension/vsix-candidate-payload.json', '{}'),
      (zip) => zip.file('extension.vsixmanifest', '<PackageManifest/>'),
      (zip) => zip.file('extension/unexpected.txt', 'unexpected'),
      (zip) => zip.file('extension/package.json', '{}'),
    ];
    for (const change of cases) {
      const f = await mappedPackageFixture();
      try {
        await assert.rejects(
          packageMappedVsix((script, args) => f.run(script, args, change), fs, f.env, f.root),
        );
        assert.equal(fs.existsSync(f.output), false);
        assert.equal(fs.existsSync(path.join(f.root, 'vsix-candidate-payload.json')), false);
      } finally {
        f.cleanup();
      }
    }
    for (const duplicate of [false, true]) {
      const f = await mappedPackageFixture();
      try {
        await assert.rejects(
          packageMappedVsix(
            async (script, args) => {
              await f.run(script, args, (zip) => {
                const bytes = fs.readFileSync(path.join(f.root, 'vsix-candidate-payload.json'));
                if (duplicate) zip.file('extension/vsix-candidate-payloae.json', bytes);
                else
                  zip.file(
                    'extension/vsix-candidate-payload.json',
                    bytes.toString().replace('synthetic-rc-seven', 'synthetic-rc-seveX'),
                  );
              });
              if (duplicate) {
                const bytes = fs.readFileSync(f.output);
                const from = Buffer.from('vsix-candidate-payloae.json');
                const to = Buffer.from('vsix-candidate-payload.json');
                for (
                  let at = bytes.indexOf(from);
                  at >= 0;
                  at = bytes.indexOf(from, at + to.length)
                )
                  to.copy(bytes, at);
                fs.writeFileSync(f.output, bytes);
              }
              return true;
            },
            fs,
            f.env,
            f.root,
          ),
        );
        assert.equal(fs.existsSync(f.output), false);
      } finally {
        f.cleanup();
      }
    }
  });
  await test('mapped production exact inventory proof cannot waive additional files or ambient native payloads', async () => {
    for (const extra of ['unexpected.txt', 'bin/linux-x64/perllsp']) {
      const f = await mappedPackageFixture(extra);
      try {
        await assert.rejects(
          packageMappedVsix(f.run, fs, f.env, f.root),
          extra.startsWith('bin/') ? /contains native payload/ : /inventory policy refused/,
        );
        assert.equal(fs.existsSync(f.output), false);
      } finally {
        f.cleanup();
      }
    }
    const f = await mappedPackageFixture();
    try {
      const native = path.join(f.root, 'bin/linux-x64/perllsp');
      fs.mkdirSync(path.dirname(native), { recursive: true });
      fs.writeFileSync(native, 'unrelated native bytes');
      await assert.rejects(packageMappedVsix(f.run, fs, f.env, f.root), /ambient native payload/);
      assert.equal(fs.readFileSync(native, 'utf8'), 'unrelated native bytes');
      assert.equal(f.calls.length, 0);
    } finally {
      f.cleanup();
    }
  });
  await test('mapped production packager and cleanup failures never retain a success artifact', async () => {
    for (const outcome of ['false', 'throw', 'missing', 'empty', 'cleanup', 'stage']) {
      const f = await mappedPackageFixture();
      try {
        const facade = Object.create(fs);
        if (outcome === 'stage') {
          const destination = path.join(f.root, 'vsix-candidate-payload.json');
          fs.writeFileSync(destination, 'prior bytes');
          let first = true;
          facade.writeFileSync = (file, bytes) => {
            fs.writeFileSync(file, bytes);
            if (file === destination && first) {
              first = false;
              throw new Error('fixture partial stage failure');
            }
          };
        }

        if (outcome === 'cleanup')
          facade.rmSync = (file, options) => {
            if (file === path.join(f.root, 'vsix-candidate-payload.json'))
              throw new Error('fixture cleanup failure');
            return fs.rmSync(file, options);
          };
        const run = async (script, args) => {
          if (outcome === 'false') return false;
          if (outcome === 'throw') throw new Error('fixture packager failure');
          if (outcome === 'missing') return true;
          if (outcome === 'empty') {
            fs.writeFileSync(f.output, '');
            return true;
          }
          return f.run(script, args);
        };
        await assert.rejects(packageMappedVsix(run, facade, f.env, f.root));
        assert.equal(fs.existsSync(f.output), false);
        if (outcome === 'stage')
          assert.equal(
            fs.readFileSync(path.join(f.root, 'vsix-candidate-payload.json'), 'utf8'),
            'prior bytes',
          );
      } finally {
        f.cleanup();
      }
    }
    const f = await mappedPackageFixture();
    try {
      fs.writeFileSync(f.output, 'existing artifact');
      await assert.rejects(packageMappedVsix(f.run, fs, f.env, f.root), /already exists/);
      assert.equal(fs.readFileSync(f.output, 'utf8'), 'existing artifact');
      assert.equal(f.calls.length, 0);
    } finally {
      f.cleanup();
    }
  });
}
mappedPackageTests().catch((error) => {
  process.stderr.write(`${String(error)}\n`);
  process.exitCode = 1;
});
