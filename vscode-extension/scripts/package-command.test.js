const assert = require('node:assert/strict');
const fs = require('node:fs');
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
      version: '0.18.0',
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
  const projectionPath = 'projection.json';
  /** @type {any} */
  const fileSystem = {
    existsSync: () => false,
    readFileSync: (file, encoding) => {
      if (file === 'manifest.json')
        return encoding ? JSON.stringify(manifest) : Buffer.from(JSON.stringify(manifest));
      if (file === projectionPath)
        return encoding
          ? JSON.stringify(projectionInput())
          : Buffer.from(JSON.stringify(projectionInput()));
      if (file === 'server.bin') return serverBytes;
      if (file === 'dap.bin') return dapBytes;
      throw new Error(`unexpected read: ${file}`);
    },
    mkdirSync: () => {},
    writeFileSync: (file, bytes) => {
      writes.push({ file, bytes });
      if (writes.length === 2) throw new Error('simulated second payload failure');
    },
    rmSync: (file) => removals.push(file),
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
  assert.equal(removals.length, 2);

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
  assert.equal(packageVsix(run, fileSystem, env), true);
  assert.equal(writes[0].file.endsWith('bin\\win32-x64\\perllsp.exe'), true);
  assert.equal(writes[0].bytes, serverBytes);
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
    existsSync: () => true,
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

void test('packaging failure prevents archive validation', () => {
  const calls = [];
  /** @type {any} */
  const fileSystem = {
    existsSync: () => true,
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
    existsSync: () => staleFile,
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
