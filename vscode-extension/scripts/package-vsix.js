#!/usr/bin/env node

const path = require('node:path');
const fs = require('node:fs');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

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
  const expectedMember = context.target === 'win32-x64' ? 'perllsp.exe' : 'perllsp';
  if (manifest.server.member !== expectedMember || manifest.server.target !== context.rustTarget) {
    throw new Error('prebuilt payload target member mismatch');
  }
  if (manifest.server.candidateId !== manifest.candidate.id) {
    throw new Error('prebuilt payload candidate mismatch');
  }
  if (manifest.server.sha256 !== sha256(context.serverBytes)) {
    throw new Error('prebuilt payload server payload SHA mismatch');
  }
  if (manifest.dap?.disposition === 'required_present' && !manifest.dap.payload) {
    throw new Error('prebuilt payload manifest is missing required DAP payload');
  }
  if (manifest.dap?.payload) {
    if (manifest.dap.payload.candidateId !== manifest.candidate.id) {
      throw new Error('prebuilt payload DAP candidate mismatch');
    }
    const expectedDap = context.target === 'win32-x64' ? 'perl-dap.exe' : 'perl-dap';
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

function preparePrebuiltPayload(fileSystem = fs, env = process.env) {
  const manifestPath = (env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST || '').trim();
  if (!manifestPath) return () => {};
  const serverPath = (env.PERL_LSP_PREBUILT_SERVER_PATH || '').trim();
  if (!serverPath) {
    throw new Error('prebuilt payload manifest requires PERL_LSP_PREBUILT_SERVER_PATH');
  }
  const manifest = JSON.parse(fileSystem.readFileSync(manifestPath, 'utf8'));
  const target = (env.PERL_LSP_VSCODE_TARGET || `${process.platform}-${process.arch}`).trim();
  if (!/^(?:win32|linux|alpine|darwin)-(?:x64|arm64)$/.test(target)) {
    throw new Error(`unsupported VS Code target for prebuilt payload: ${target}`);
  }
  const rustTarget = (env.PERL_LSP_RUST_TARGET || '').trim();
  const sourceSha = (env.PERL_LSP_CURRENT_SOURCE_SHA || '').trim();
  const serverBytes = fileSystem.readFileSync(serverPath);
  const dapPath = (env.PERL_LSP_PREBUILT_DAP_PATH || '').trim();
  const dapBytes = dapPath ? fileSystem.readFileSync(dapPath) : null;
  validatePrebuiltPayload(manifest, { sourceSha, target, rustTarget, serverBytes, dapBytes });
  const payloads = [{ member: manifest.server.member, bytes: serverBytes }];
  if (manifest.dap?.payload && dapBytes) {
    payloads.push({ member: manifest.dap.payload.member, bytes: dapBytes });
  }
  const previous = payloads.map(({ member }) => {
    const destination = path.join(extensionRoot, 'bin', target, member);
    return {
      destination,
      bytes: fileSystem.existsSync(destination) ? fileSystem.readFileSync(destination) : null,
    };
  });
  for (const [index, payload] of payloads.entries()) {
    const prior = previous[index];
    if (!prior) throw new Error('prebuilt payload staging state is inconsistent');
    const destination = prior.destination;
    fileSystem.mkdirSync(path.dirname(destination), { recursive: true });
    fileSystem.writeFileSync(destination, payload.bytes);
  }
  return () => {
    for (const item of previous) {
      if (item.bytes !== null) fileSystem.writeFileSync(item.destination, item.bytes);
      else fileSystem.rmSync(item.destination, { force: true });
    }
  };
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
  const restorePrebuiltPayload = preparePrebuiltPayload(fileSystem, env);
  try {
    if (fileSystem.existsSync(vsixPath)) {
      fileSystem.rmSync(vsixPath, { force: true });
    }
    if (!run(vsceEntry, ['package', '--out', vsixName])) {
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
    const manifestPath = (env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST || '').trim();
    if (manifestPath) {
      const manifest = JSON.parse(fileSystem.readFileSync(manifestPath, 'utf8'));
      const target = (env.PERL_LSP_VSCODE_TARGET || `${process.platform}-${process.arch}`).trim();
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
    }
    return run(path.join(__dirname, 'check-vsix-inventory.js'), ['--vsix', vsixName]);
  } finally {
    restorePrebuiltPayload();
  }
}

if (require.main === module) {
  try {
    if (!packageVsix()) {
      process.exitCode ||= 1;
    }
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  packageVsix,
  preparePrebuiltPayload,
  validatePrebuiltPayload,
  vsixName,
  vsceEntry,
};
