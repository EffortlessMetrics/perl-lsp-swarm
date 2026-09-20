#!/usr/bin/env node

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const yauzl = require('yauzl');

async function mappedMetadata(vsix) {
  const wanted = new Set(['extension/package.json', 'extension/vsix-candidate-payload.json', 'extension.vsixmanifest']);
  const values = new Map();
  const archive = await yauzl.openPromise(vsix, { lazyEntries: true, decodeStrings: false, validateEntrySizes: true });
  try {
    for await (const entry of archive.eachEntry()) {
      const name = Buffer.from(entry.fileName).toString('utf8');
      if (!wanted.has(name)) continue;
      const type = ((entry.externalFileAttributes >>> 16) & 0xffff) & 0xf000;
      if (values.has(name) || entry.uncompressedSize > 1024 * 1024 ||
          (type !== 0 && type !== 0x8000) || (entry.externalFileAttributes & 0x10)) {
        throw new Error(`Invalid or duplicate VSIX metadata member: ${name}`);
      }
      const chunks = []; let size = 0;
      for await (const chunk of await archive.openReadStreamPromise(entry)) {
        size += chunk.length;
        if (size > 1024 * 1024) throw new Error('VSIX metadata exceeds limit');
        chunks.push(chunk);
      }
      values.set(name, new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)));
    }
  } finally { archive.close(); }
  if (values.size !== wanted.size) throw new Error('Mapped VSIX metadata is missing');
  return values;
}

async function fileDigest(file) {
  const hash = crypto.createHash('sha256');
  for await (const chunk of fs.createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

async function verifyMappedRc(vsix, topologyBytes, expectedDigest) {
  if (!/^[0-9a-f]{64}$/.test(expectedDigest) || await fileDigest(vsix) !== expectedDigest) {
    throw new Error('Mapped VSIX bytes differ from expected digest');
  }
  const topology = JSON.parse(topologyBytes);
  const Ajv2020 = require('ajv/dist/2020');
  const validator = new Ajv2020({ strict: true }).compile(
    JSON.parse(fs.readFileSync(path.join(__dirname, '../../schemas/release_topology.v4.schema.json'), 'utf8')),
  );
  if (!validator(topology)) throw new Error(`Invalid mapped topology: ${JSON.stringify(validator.errors)}`);
  if (topology.schema !== 4 || !/^[0-9a-f]{40}$/.test(topology.prepared_swarm_sha ?? '') ||
      topology.prepared_swarm_sha === topology.frozen_product_sha) {
    throw new Error('Mapped RC requires prepared topology v4');
  }
  const selected = topology.vsix;
  if (!selected || selected.pre_release !== true ||
      !/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(selected.publisher ?? '') ||
      !/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(selected.name ?? '') ||
      selected.asset_name !== `${selected.name}-${selected.version}-${topology.release}.vsix` ||
      path.basename(vsix) !== selected.asset_name) {
    throw new Error('VSIX asset does not match selected RC identity');
  }
  const values = await mappedMetadata(vsix);
  const pkg = JSON.parse(values.get('extension/package.json'));
  const payload = JSON.parse(values.get('extension/vsix-candidate-payload.json'));
  if (pkg.publisher !== selected.publisher || pkg.name !== selected.name || pkg.version !== selected.version) {
    throw new Error('Packaged extension identity differs from topology');
  }
  // Reuse the XML parser already pinned by the artifact packager dependency.
  const { createRequire } = require('node:module');
  const packagerRequire = createRequire(require.resolve('@vscode/vsce/package.json'));
  const { parseStringPromise } = packagerRequire('xml2js');
  const xml = await parseStringPromise(values.get('extension.vsixmanifest'), { explicitArray: true, strict: true });
  const metadata = xml.PackageManifest?.Metadata;
  const identities = metadata?.length === 1 ? metadata[0].Identity : undefined;
  const properties = metadata?.length === 1 && metadata[0].Properties?.length === 1 ? metadata[0].Properties[0].Property : undefined;
  const markers = (properties ?? []).filter((item) => item.$?.Id === 'Microsoft.VisualStudio.Code.PreRelease');
  if (identities?.length !== 1 || identities[0].$.Id !== selected.name ||
      identities[0].$.Publisher !== selected.publisher || identities[0].$.Version !== selected.version ||
      markers.length !== 1 || markers[0].$.Value !== 'true') {
    throw new Error('VSIX manifest identity or prerelease marker differs from mapping');
  }
  const topologyDigest = crypto.createHash('sha256').update(topologyBytes).digest('hex');
  const { buildVsixCandidatePayloadManifest, canonicalVsixPayloadJson, deriveVsixTargetProjection } = require('../src/vsixPackageProjection.ts');
  const projections = deriveVsixTargetProjection({
    releaseTopologySha256: topologyDigest,
    includeUniversalManaged: true,
    targets: topology.binary_targets.map((row) => ({ target: row.target, os: row.os, architecture: row.architecture, libc: row.libc, archiveName: row.archive_name, requiredMembers: row.required_members })),
  });
  const projection = projections.find((row) => row.vscodeTargetId === payload.package?.vscodeTargetId);
  if (!projection) throw new Error('VSIX target is absent from selected topology');
  const manifestTarget = identities[0].$.TargetPlatform;
  if ((projection.packageMode === 'target_specific' && manifestTarget !== projection.vscodeTargetId) ||
      (projection.packageMode === 'universal_managed' && manifestTarget !== undefined)) {
    throw new Error('VSIX manifest target platform differs from payload projection');
  }
  const expected = buildVsixCandidatePayloadManifest({
    schema: 'vsix_candidate_payload.v2', preRelease: true,
    extension: { id: `${selected.publisher}.${selected.name}`, version: selected.version, sourceSha: topology.prepared_swarm_sha },
    candidate: { id: selected.candidate_id, release: topology.release, sourceSha: topology.prepared_swarm_sha },
    releaseTopologySha256: topologyDigest, projection,
    packageInventorySha256: payload.package?.inventorySha256,
    server: payload.server ?? undefined, dap: payload.dap?.payload ?? undefined,
  });
  if (canonicalVsixPayloadJson(payload) !== canonicalVsixPayloadJson(expected)) {
    throw new Error('Packaged payload differs from selected RC/topology/source identity');
  }
  for (const native of [payload.server, payload.dap.payload]) {
    if (native) await verifyPayloadMember(vsix, `bin/${projection.vscodeTargetId}/${native.member}`, native.sha256);
  }
  const { collectArchiveInventory, semanticInventorySha256 } = require('./check-vsix-inventory-transition');
  if (semanticInventorySha256((await collectArchiveInventory(vsix)).inventory) !== payload.package.inventorySha256) {
    throw new Error('Mapped VSIX package inventory differs from payload');
  }
  if (await fileDigest(vsix) !== expectedDigest) throw new Error('Mapped VSIX bytes changed during verification');
}

async function verifyPayloadMember(vsix, member, expected) {
  const archive = await yauzl.openPromise(vsix, {
    lazyEntries: true,
    decodeStrings: false,
    validateEntrySizes: true,
  });
  let matches = 0;
  try {
    for await (const entry of archive.eachEntry()) {
      const name = Buffer.from(entry.fileName).toString('utf8');
      if (name !== `extension/${member}`) continue;
      matches += 1;
      if (matches > 1) throw new Error(`archive has duplicate payload member: ${member}`);
      const unixMode = (entry.externalFileAttributes >>> 16) & 0xffff;
      const fileType = unixMode & 0xf000;
      const dosDirectory = entry.externalFileAttributes & 0x10;
      if ((fileType !== 0 && fileType !== 0x8000) || dosDirectory || name.endsWith('/')) {
        throw new Error(`archive payload member is not a regular file: ${member}`);
      }
      const stream = await archive.openReadStreamPromise(entry);
      const hash = crypto.createHash('sha256');
      for await (const chunk of stream) hash.update(chunk);
      const actual = hash.digest('hex');
      if (actual !== expected) {
        throw new Error(`archive payload SHA mismatch for ${member}: ${actual}`);
      }
    }
  } finally {
    archive.close();
  }
  if (matches === 0) throw new Error(`archive payload is missing extension/${member}`);
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : '';
}

async function main() {
  const vsix = argument('--vsix');
  const topology = argument('--mapped-topology');
  if (topology) {
    if (!vsix) throw new Error('--mapped-topology requires --vsix');
    await verifyMappedRc(vsix, fs.readFileSync(topology), argument('--vsix-sha256'));
    return;
  }
  const member = argument('--member');
  const expectedInventory = argument('--inventory-sha256');
  const expected = argument('--sha256') ?? '';
  if (!vsix || (!member && !expectedInventory) || (member && !/^[0-9a-f]{64}$/.test(expected))) {
    throw new Error(
      'usage: --vsix <path> (--member <archive member> --sha256 <digest> | --inventory-sha256 <digest>)',
    );
  }
  if (expectedInventory) {
    if (!/^[0-9a-f]{64}$/.test(expectedInventory)) {
      throw new Error('--inventory-sha256 must be a lowercase SHA-256 digest');
    }
    const {
      collectArchiveInventory,
      semanticInventorySha256,
    } = require('./check-vsix-inventory-transition');
    const actual = await collectArchiveInventory(vsix);
    const digest = semanticInventorySha256(actual.inventory);
    if (digest !== expectedInventory) {
      throw new Error(`archive inventory SHA mismatch: ${digest}`);
    }
    return;
  }
  await verifyPayloadMember(vsix, member, expected);
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

module.exports = { main, verifyPayloadMember, verifyMappedRc };
