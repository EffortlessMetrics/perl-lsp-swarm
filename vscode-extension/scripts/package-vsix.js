#!/usr/bin/env node

const path = require('node:path');
const fs = require('node:fs');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const {
  buildVsixCandidatePayloadManifest,
  canonicalVsixPayloadJson,
  deriveVsixTargetProjection,
} = require('../src/vsixPackageProjection.ts');

const extensionRoot = path.resolve(__dirname, '..');
const packageManifest = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
);
const vsixName = `perl-lsp-rs-${packageManifest.version}.vsix`;
const vsixPath = path.join(extensionRoot, vsixName);
const vsceEntry = path.join(extensionRoot, 'node_modules', '@vscode', 'vsce', 'vsce');

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function validatePrebuiltPayload(manifest, context) {
  if (!manifest || manifest.schema !== 'vsix_candidate_payload.v1') {
    throw new Error('prebuilt payload manifest schema is unsupported');
  }
  if (!manifest.extension || !manifest.candidate || !manifest.package || !manifest.server) {
    throw new Error('prebuilt payload manifest is missing required identity');
  }
  const server = manifest.server;
  if (
    manifest.extension.sourceSha !== context.sourceSha ||
    manifest.candidate.sourceSha !== context.sourceSha
  ) {
    throw new Error('prebuilt payload source SHA mismatch');
  }
  if (!/^[0-9a-f]{40}$/.test(context.sourceSha)) {
    throw new Error('prebuilt payload source SHA is not a full lowercase commit SHA');
  }
  if (manifest.package.mode !== 'target_specific') {
    throw new Error('prebuilt payload package mode is not target_specific');
  }
  if (
    manifest.package.vscodeTargetId !== context.target ||
    manifest.package.rustTarget !== context.rustTarget
  ) {
    throw new Error(`prebuilt payload target mismatch: expected ${context.target}`);
  }
  const expectedMember = context.target.startsWith('win32-') ? 'perllsp.exe' : 'perllsp';
  if (server.member !== expectedMember || server.target !== context.rustTarget) {
    throw new Error('prebuilt payload target member mismatch');
  }
  if (server.candidateId !== manifest.candidate.id) {
    throw new Error('prebuilt payload candidate mismatch');
  }
  if (server.sha256 !== sha256(context.serverBytes)) {
    throw new Error('prebuilt payload server payload SHA mismatch');
  }
  if (manifest.dap?.disposition === 'required_present' && !manifest.dap.payload) {
    throw new Error('prebuilt payload manifest is missing required DAP payload');
  }
  if (manifest.dap?.payload) {
    if (manifest.dap.payload.candidateId !== manifest.candidate.id) {
      throw new Error('prebuilt payload DAP candidate mismatch');
    }
    const expectedDap = context.target.startsWith('win32-') ? 'perl-dap.exe' : 'perl-dap';
    if (
      manifest.dap.payload.member !== expectedDap ||
      manifest.dap.payload.target !== context.rustTarget
    ) {
      throw new Error('prebuilt payload DAP target member mismatch');
    }
    if (!context.dapBytes || manifest.dap.payload.sha256 !== sha256(context.dapBytes)) {
      throw new Error('prebuilt payload DAP payload SHA mismatch');
    }
  }
  return manifest;
}

function validateProjectionManifest(manifest, projectionInput, target) {
  if (
    !manifest ||
    typeof manifest !== 'object' ||
    !manifest.package ||
    typeof manifest.package !== 'object' ||
    !manifest.dap ||
    typeof manifest.dap !== 'object'
  ) {
    throw new Error('prebuilt payload manifest is missing package or DAP identity');
  }
  const expectedExtensionId = `${packageManifest.publisher}.${packageManifest.name}`;
  if (
    manifest.extension?.id !== expectedExtensionId ||
    manifest.extension?.version !== packageManifest.version
  ) {
    throw new Error('prebuilt payload extension identity disagrees with package.json');
  }
  if (manifest.releaseTopologySha256 !== projectionInput.releaseTopologySha256) {
    throw new Error('prebuilt payload release topology SHA mismatch');
  }
  const projection = deriveVsixTargetProjection(projectionInput).find(
    (row) => row.vscodeTargetId === target,
  );
  if (!projection) {
    throw new Error(`prebuilt payload target is absent from the release projection: ${target}`);
  }
  const packageIdentity = manifest.package;
  const server = manifest.server;
  const dap = manifest.dap;
  const validated = buildVsixCandidatePayloadManifest({
    extension: manifest.extension,
    candidate: manifest.candidate,
    releaseTopologySha256: manifest.releaseTopologySha256,
    projection,
    packageInventorySha256: packageIdentity.inventorySha256,
    server: server ?? undefined,
    dap: dap.payload ?? undefined,
  });
  if (canonicalVsixPayloadJson(validated) !== canonicalVsixPayloadJson(manifest)) {
    throw new Error('prebuilt payload manifest is not the canonical projection output');
  }
  return validated;
}

function preparePrebuiltPayload(fileSystem = fs, env = process.env, stagingRoot = extensionRoot) {
  const manifestPath = (env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST || '').trim();
  const rawManifest = manifestPath
    ? JSON.parse(fileSystem.readFileSync(manifestPath, 'utf8'))
    : null;
  const target = (
    env.PERL_LSP_VSCODE_TARGET ||
    rawManifest?.package?.vscodeTargetId ||
    `${process.platform}-${process.arch}`
  ).trim();
  if (!manifestPath) {
    for (const ambientTarget of [
      'win32-x64',
      'win32-arm64',
      'linux-x64',
      'linux-arm64',
      'alpine-x64',
      'alpine-arm64',
      'darwin-x64',
      'darwin-arm64',
    ]) {
      for (const basename of ['perllsp', 'perl-dap']) {
        const member = ambientTarget.startsWith('win32-') ? `${basename}.exe` : basename;
        const destination = path.join(stagingRoot, 'bin', ambientTarget, member);
        let present = false;
        if (typeof fileSystem.lstatSync === 'function') {
          try {
            const stats = fileSystem.lstatSync(destination);
            present = true;
            if (stats.isSymbolicLink()) {
              throw new Error(`ambient native payload is a symbolic link: ${destination}`);
            }
          } catch (error) {
            if (error?.code !== 'ENOENT') throw error;
          }
        } else {
          present = fileSystem.existsSync(destination);
        }
        if (present || fileSystem.existsSync(destination)) {
          throw new Error('ambient native payload requires a candidate payload manifest');
        }
      }
    }
    return { manifest: null, cleanup: () => {} };
  }
  const serverPath = (env.PERL_LSP_PREBUILT_SERVER_PATH || '').trim();
  if (!serverPath) {
    throw new Error('prebuilt payload manifest requires PERL_LSP_PREBUILT_SERVER_PATH');
  }
  if (!/^(?:win32|linux|alpine|darwin)-(?:x64|arm64)$/.test(target)) {
    throw new Error(`unsupported VS Code target for prebuilt payload: ${target}`);
  }
  const projectionPath = (env.PERL_LSP_VSIX_PROJECTION_INPUT || '').trim();
  if (!projectionPath) {
    throw new Error('prebuilt payload manifest requires PERL_LSP_VSIX_PROJECTION_INPUT');
  }
  const projectionInput = JSON.parse(fileSystem.readFileSync(projectionPath, 'utf8'));
  const manifest = validateProjectionManifest(rawManifest, projectionInput, target);
  if (!manifest.server) throw new Error('prebuilt payload manifest has no server payload');
  const server = manifest.server;
  const rustTarget = (env.PERL_LSP_RUST_TARGET || '').trim();
  const sourceSha = (env.PERL_LSP_CURRENT_SOURCE_SHA || '').trim();
  const serverBytes = fileSystem.readFileSync(serverPath);
  const dapPath = (env.PERL_LSP_PREBUILT_DAP_PATH || '').trim();
  const dapBytes = dapPath ? fileSystem.readFileSync(dapPath) : null;
  validatePrebuiltPayload(manifest, { sourceSha, target, rustTarget, serverBytes, dapBytes });
  const payloads = [{ member: server.member, bytes: serverBytes }];
  if (manifest.dap?.payload && dapBytes) {
    payloads.push({ member: manifest.dap.payload.member, bytes: dapBytes });
  }
  const previous = payloads.map(({ member }) => {
    const destination = path.join(stagingRoot, 'bin', target, member);
    const binRoot = path.resolve(stagingRoot, 'bin');
    const resolved = path.resolve(destination);
    if (resolved !== binRoot && !resolved.startsWith(`${binRoot}${path.sep}`)) {
      throw new Error('prebuilt payload destination escapes extension bin directory');
    }
    if (typeof fileSystem.lstatSync === 'function') {
      for (const parent of [binRoot, path.dirname(destination), destination]) {
        try {
          if (fileSystem.lstatSync(parent).isSymbolicLink()) {
            throw new Error(`prebuilt payload destination is a symbolic link: ${parent}`);
          }
        } catch (error) {
          if (error?.code !== 'ENOENT') throw error;
        }
      }
    }
    return {
      destination,
      bytes: fileSystem.existsSync(destination) ? fileSystem.readFileSync(destination) : null,
      mode:
        typeof fileSystem.lstatSync === 'function' && fileSystem.existsSync(destination)
          ? fileSystem.lstatSync(destination).mode
          : null,
    };
  });
  const cleanup = () => {
    for (const item of previous) {
      if (item.bytes !== null) fileSystem.writeFileSync(item.destination, item.bytes);
      else fileSystem.rmSync(item.destination, { force: true });
      if (item.bytes !== null && item.mode !== null && typeof fileSystem.chmodSync === 'function') {
        fileSystem.chmodSync(item.destination, item.mode);
      }
    }
  };
  try {
    for (const [index, payload] of payloads.entries()) {
      const prior = previous[index];
      if (!prior) throw new Error('prebuilt payload staging state is inconsistent');
      const destination = prior.destination;
      fileSystem.mkdirSync(path.dirname(destination), { recursive: true });
      fileSystem.writeFileSync(destination, payload.bytes);
      if (typeof fileSystem.chmodSync === 'function') {
        fileSystem.chmodSync(destination, 0o755);
      }
    }
  } catch (error) {
    try {
      cleanup();
    } catch (rollbackError) {
      throw new Error(
        `prebuilt payload staging failed and rollback failed: ${rollbackError instanceof Error ? rollbackError.message : String(rollbackError)}`,
        { cause: error },
      );
    }
    throw error;
  }
  return { manifest, cleanup };
}

function runNode(script, args) {
  const result = spawnSync(process.execPath, [script, ...args], {
    cwd: extensionRoot,
    stdio: 'inherit',
    windowsHide: true,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exitCode = result.status ?? 1;
    return false;
  }
  return true;
}

function packageVsix(run = runNode, fileSystem = fs, env = process.env) {
  const staged = preparePrebuiltPayload(fileSystem, env, extensionRoot);
  const restorePrebuiltPayload = staged.cleanup;
  const manifest = staged.manifest;
  try {
    if (fileSystem.existsSync(vsixPath)) {
      fileSystem.rmSync(vsixPath, { force: true });
    }
    const packageArgs = ['package'];
    if (manifest) {
      const target = manifest.package.vscodeTargetId;
      packageArgs.push('--target', target, '--out', vsixName);
    } else {
      packageArgs.push('--out', vsixName);
    }
    if (!run(vsceEntry, packageArgs)) {
      return false;
    }
    let artifact;
    try {
      artifact = fileSystem.statSync(vsixPath);
    } catch {
      throw new Error(`packager exited successfully without producing ${vsixName}`);
    }
    if (!artifact.isFile() || artifact.size <= 0) {
      throw new Error(`packager exited successfully without producing ${vsixName}`);
    }
    if (manifest) {
      if (!manifest.server) throw new Error('prebuilt payload manifest has no server payload');
      const target = manifest.package.vscodeTargetId;
      const verifyScript = path.join(__dirname, 'check-vsix-prebuilt-payload.js');
      const payloads = [
        { member: `bin/${target}/${manifest.server.member}`, sha256: manifest.server.sha256 },
      ];
      if (manifest.dap?.payload) {
        payloads.push({
          member: `bin/${target}/${manifest.dap.payload.member}`,
          sha256: manifest.dap.payload.sha256,
        });
      }
      for (const payload of payloads) {
        if (
          !run(verifyScript, [
            '--vsix',
            vsixName,
            '--member',
            payload.member,
            '--sha256',
            payload.sha256,
          ])
        ) {
          return false;
        }
      }
      if (
        !run(verifyScript, [
          '--vsix',
          vsixName,
          '--inventory-sha256',
          manifest.package.inventorySha256,
        ])
      ) {
        return false;
      }
    }
    return run(path.join(__dirname, 'check-vsix-inventory.js'), ['--vsix', vsixName]);
  } finally {
    restorePrebuiltPayload();
  }
}

/** Build only the explicitly selected first-RC universal package.
 * @param {(script: string, args: string[]) => boolean|Promise<boolean>} run
 */
async function packageMappedVsix(
  run = runNode,
  fileSystem = fs,
  env = process.env,
  root = extensionRoot,
) {
  const {
    parseUniqueJson,
    validateMappedTopology,
    verifyMappedSnapshot,
  } = require('./check-vsix-prebuilt-payload');
  const { compareInventory, bundleTargetForPackagedFile } = require('./check-vsix-inventory');
  const topologyPath = (env.PERL_LSP_RELEASE_TOPOLOGY || '').trim();
  const manifestPath = (env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST || '').trim();
  if (!topologyPath || !manifestPath)
    throw new Error('Mapped packaging requires topology and payload inputs');
  const topologyBytes = Buffer.from(fileSystem.readFileSync(topologyPath));
  const topology = validateMappedTopology(topologyBytes);
  const supplied = parseUniqueJson(fileSystem.readFileSync(manifestPath));
  if (
    Array.isArray(supplied) ||
    supplied?.schema !== 'vsix_candidate_payload.v2' ||
    supplied?.package?.mode !== 'universal_managed' ||
    supplied?.package?.vscodeTargetId !== 'universal'
  ) {
    throw new Error('Mapped packaging selects exactly one universal_managed v2 package');
  }
  if (
    topology.vsix.bundled_targets.length !== 0 ||
    [
      'PERL_LSP_PREBUILT_SERVER_PATH',
      'PERL_LSP_PREBUILT_DAP_PATH',
      'PERL_LSP_RUST_TARGET',
      'PERL_LSP_VSIX_PROJECTION_INPUT',
    ].some((key) => (env[key] || '').trim()) ||
    (env.PERL_LSP_VSCODE_TARGET && env.PERL_LSP_VSCODE_TARGET !== 'universal')
  ) {
    throw new Error('Mapped universal packaging refuses native or competing package selection');
  }
  const repoRoot = path.resolve(root, '..');
  const matrixPath = 'docs/reference/downstream-dap-integrations.json';
  const downloaderPath = 'vscode-extension/src/downloader.ts';
  const matrixBytes = fileSystem.readFileSync(path.join(repoRoot, matrixPath));
  const downloaderBytes = fileSystem.readFileSync(path.join(repoRoot, downloaderPath));
  /** @type {Array<[string, Buffer]>} */
  const sourceBindings = [
    [matrixPath, matrixBytes],
    [downloaderPath, downloaderBytes],
  ];
  for (const [sourcePath, bytes] of sourceBindings) {
    if (
      topology.sources?.[sourcePath]?.path !== sourcePath ||
      topology.sources[sourcePath].sha256 !== sha256(bytes)
    ) {
      throw new Error(`Mapped packaging source binding differs: ${sourcePath}`);
    }
  }
  const canonicalTargets = parseUniqueJson(matrixBytes).targets.map((row) => row.triple);
  const sameTargets = (actual) =>
    Array.isArray(actual) &&
    new Set(actual).size === actual.length &&
    actual.length === canonicalTargets.length &&
    actual.every((target) => canonicalTargets.includes(target));
  if (
    new Set(canonicalTargets).size !== canonicalTargets.length ||
    !sameTargets(topology.vsix.managed_targets) ||
    !sameTargets(topology.binary_targets.map((row) => row.target))
  ) {
    throw new Error('Mapped packaging must preserve the complete canonical managed-target matrix');
  }
  const pkg = parseUniqueJson(fileSystem.readFileSync(path.join(root, 'package.json')));
  const selected = topology.vsix;
  if (
    pkg.publisher !== selected.publisher ||
    pkg.name !== selected.name ||
    pkg.version !== selected.version ||
    env.PERL_LSP_CURRENT_SOURCE_SHA !== topology.prepared_swarm_sha
  ) {
    throw new Error('Mapped packaging extension or prepared source differs');
  }
  const projection = deriveVsixTargetProjection({
    releaseTopologySha256: sha256(topologyBytes),
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
  if (!projection) throw new Error('Mapped topology lacks universal projection');
  const rebuilt = buildVsixCandidatePayloadManifest({
    schema: 'vsix_candidate_payload.v2',
    preRelease: true,
    extension: {
      id: `${selected.publisher}.${selected.name}`,
      version: selected.version,
      sourceSha: topology.prepared_swarm_sha,
    },
    candidate: {
      id: selected.candidate_id,
      release: topology.release,
      sourceSha: topology.prepared_swarm_sha,
    },
    releaseTopologySha256: sha256(topologyBytes),
    projection,
    packageInventorySha256: supplied.package.inventorySha256,
  });
  const payloadBytes = Buffer.from(canonicalVsixPayloadJson(rebuilt));
  if (canonicalVsixPayloadJson(supplied) !== payloadBytes.toString('utf8'))
    throw new Error('Mapped payload is not the admitted canonical universal projection');
  // Reuse the existing refusal of ambient native inputs without mutating caller env.
  preparePrebuiltPayload(fileSystem, {}, root);
  const destination = path.join(root, 'vsix-candidate-payload.json');
  const output = path.join(root, selected.asset_name);
  function existingRegular(file) {
    try {
      const info = fileSystem.lstatSync(file);
      if (!info.isFile() || info.isSymbolicLink())
        throw new Error(`Mapped staging path is not a regular file: ${file}`);
      return info;
    } catch (error) {
      if (error?.code === 'ENOENT') return null;
      throw error;
    }
  }
  if (existingRegular(output))
    throw new Error('Mapped output already exists; refusing to overwrite');
  const previous = existingRegular(destination);
  const priorBytes = previous ? fileSystem.readFileSync(destination) : null;
  let failure;
  let staged = false;
  let outputOwned = false;
  try {
    // Exclusive reservation closes the admission-to-creation collision window.
    const descriptor = fileSystem.openSync(output, 'wx');
    outputOwned = true;
    fileSystem.closeSync(descriptor);
    staged = true;
    fileSystem.writeFileSync(destination, payloadBytes);
    if (!(await run(vsceEntry, ['package', '--pre-release', '--out', selected.asset_name])))
      throw new Error('Mapped VSCE packaging failed');
    const info = existingRegular(output);
    if (!info || info.size <= 0)
      throw new Error('Mapped packager produced no nonempty regular output');
    const snapshot = Buffer.from(fileSystem.readFileSync(output));
    const inventory = await verifyMappedSnapshot(
      output,
      snapshot,
      topologyBytes,
      sha256(snapshot),
      payloadBytes,
    );
    if (Object.keys(inventory.files).some((file) => bundleTargetForPackagedFile(file) !== null))
      throw new Error('Universal package contains native payload');
    const baseline = parseUniqueJson(
      fileSystem.readFileSync(path.join(__dirname, 'vsix-inventory-baseline.json')),
    );
    const violations = compareInventory(inventory, baseline, process.platform, {
      allowedFiles: ['vsix-candidate-payload.json'],
      arch: process.arch,
    });
    if (violations.length)
      throw new Error(`Mapped package inventory policy refused: ${violations.join('; ')}`);
    // This is a local output digest only, not terminal or publication authority.
  } catch (error) {
    failure = error;
  }
  const cleanupErrors = [];
  if (staged) {
    try {
      if (priorBytes !== null) {
        fileSystem.writeFileSync(destination, priorBytes);
        if (previous) fileSystem.chmodSync(destination, previous.mode);
      } else fileSystem.rmSync(destination, { force: true });
    } catch (error) {
      cleanupErrors.push(String(error));
    }
  }
  if ((failure || cleanupErrors.length) && outputOwned) {
    try {
      fileSystem.rmSync(output, { force: true });
    } catch (error) {
      cleanupErrors.push(String(error));
    }
  }
  if (cleanupErrors.length)
    throw new Error(`Mapped packaging cleanup failed: ${cleanupErrors.join('; ')}`, {
      cause: failure,
    });
  if (failure) throw failure;
  return true;
}

if (require.main === module) {
  Promise.resolve()
    .then(() => (process.env.PERL_LSP_RELEASE_TOPOLOGY ? packageMappedVsix() : packageVsix()))
    .then((ok) => {
      if (!ok) process.exitCode ||= 1;
    })
    .catch((error) => {
      process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    });
}

module.exports = {
  packageMappedVsix,
  packageVsix,
  preparePrebuiltPayload,
  validateProjectionManifest,
  validatePrebuiltPayload,
  vsixName,
  vsceEntry,
};
