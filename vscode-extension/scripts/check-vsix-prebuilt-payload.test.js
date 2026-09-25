const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const JSZip = require('jszip');
const { test } = require('node:test');
const { verifyPayloadMember } = require('./check-vsix-prebuilt-payload');
const { verifyMappedRc } = require('./check-vsix-prebuilt-payload');

async function runTests() {
  /** @param {(data: any) => void} change @param {boolean} native */
  async function mappedFixture(change = () => {}, native = false) {
    const {
      buildVsixCandidatePayloadManifest,
      deriveVsixTargetProjection,
    } = require('../src/vsixPackageProjection.ts');
    const {
      collectArchiveInventory,
      semanticInventorySha256,
    } = require('./check-vsix-inventory-transition');
    const source = 'b'.repeat(40);
    const topology = JSON.parse(
      fs.readFileSync(
        path.join(__dirname, '../../fixtures/rc_vsix_binding/valid.topology.v4.json'),
        'utf8',
      ),
    );
    const schemaPath = 'schemas/release_topology.v4.schema.json';
    topology.sources[schemaPath].sha256 = sha256(
      fs.readFileSync(path.join(__dirname, '../..', schemaPath)),
    );
    const bytes = Buffer.from(JSON.stringify(topology));
    const projections = deriveVsixTargetProjection({
      releaseTopologySha256: sha256(bytes),
      includeUniversalManaged: true,
      targets: topology.binary_targets.map((row) => ({
        target: row.target,
        os: row.os,
        architecture: row.architecture,
        libc: row.libc,
        archiveName: row.archive_name,
        requiredMembers: row.required_members,
      })),
    });
    const projection = projections.find(
      (row) => row.vscodeTargetId === (native ? 'linux-x64' : 'universal'),
    );
    if (!projection) throw new Error('fixture projection missing');
    const nativeTarget = projection.rustTarget;
    if (native && !nativeTarget) throw new Error('native fixture target missing');
    const payload = buildVsixCandidatePayloadManifest({
      schema: 'vsix_candidate_payload.v2',
      preRelease: true,
      extension: {
        id: 'fixture-publisher.fixture-extension',
        version: '0.19.7',
        sourceSha: source,
      },
      candidate: { id: 'synthetic-rc-seven', release: topology.release, sourceSha: source },
      releaseTopologySha256: sha256(bytes),
      projection,
      packageInventorySha256: 'a'.repeat(64),
      ...(native
        ? {
            server: {
              candidateId: 'synthetic-rc-seven',
              target: nativeTarget ?? '',
              member: 'perllsp',
              sha256: sha256('server'),
              identityRef: 'synthetic:server',
            },
            dap: {
              candidateId: 'synthetic-rc-seven',
              target: nativeTarget ?? '',
              member: 'perl-dap',
              sha256: sha256('dap'),
              identityRef: 'synthetic:dap',
            },
          }
        : {}),
    });
    /** @type {any} */
    const data = {
      payload,
      package: { publisher: 'fixture-publisher', name: 'fixture-extension', version: '0.19.7' },
      serverBytes: 'server',
      dapBytes: 'dap',
      xml: '<PackageManifest><Metadata><Identity Id="fixture-extension" Publisher="fixture-publisher" Version="0.19.7"/><Properties><Property Id="Microsoft.VisualStudio.Code.PreRelease" Value="true"/></Properties></Metadata></PackageManifest>',
    };
    if (native) data.xml = data.xml.replace('<Identity ', '<Identity TargetPlatform="linux-x64" ');
    change(data);
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-mapped-rc-'));
    const archive = path.join(root, topology.vsix.asset_name);
    async function write() {
      const zip = new JSZip();
      zip.file('extension/package.json', data.packageRaw ?? JSON.stringify(data.package));
      zip.file(
        'extension/vsix-candidate-payload.json',
        data.payloadRaw ?? JSON.stringify(data.payload),
      );
      zip.file('extension.vsixmanifest', data.xml);
      if (native) {
        zip.file('extension/bin/linux-x64/perllsp', data.serverBytes);
        zip.file('extension/bin/linux-x64/perl-dap', data.dapBytes);
      }
      fs.writeFileSync(archive, await zip.generateAsync({ type: 'nodebuffer' }));
    }
    await write();
    data.payload.package.inventorySha256 = semanticInventorySha256(
      (await collectArchiveInventory(archive)).inventory,
    );
    await write();
    return { root, archive, topologyBytes: bytes, digest: sha256(fs.readFileSync(archive)) };
  }

  await test('mapped RC validates actual metadata and preserves every exact subject', async () => {
    const bundled = await mappedFixture(() => {}, true);
    try {
      await verifyMappedRc(bundled.archive, bundled.topologyBytes, bundled.digest);
    } finally {
      fs.rmSync(bundled.root, { recursive: true, force: true });
    }
    const substituted = await mappedFixture((data) => {
      data.dapBytes = 'another dap';
    }, true);
    try {
      await assert.rejects(
        verifyMappedRc(substituted.archive, substituted.topologyBytes, substituted.digest),
        /payload SHA mismatch/,
      );
    } finally {
      fs.rmSync(substituted.root, { recursive: true, force: true });
    }
    const valid = await mappedFixture();
    try {
      await verifyMappedRc(valid.archive, valid.topologyBytes, valid.digest);
      await assert.rejects(
        verifyMappedRc(valid.archive, valid.topologyBytes, 'c'.repeat(64)),
        /bytes differ/,
      );
      const wrongSource = JSON.parse(valid.topologyBytes.toString());
      wrongSource.prepared_swarm_sha = 'c'.repeat(40);
      await assert.rejects(
        verifyMappedRc(valid.archive, Buffer.from(JSON.stringify(wrongSource)), valid.digest),
        /payload differs/,
      );
      const wrongRc = JSON.parse(valid.topologyBytes.toString());
      wrongRc.release = '0.18.0-rc.8';
      await assert.rejects(
        verifyMappedRc(valid.archive, Buffer.from(JSON.stringify(wrongRc)), valid.digest),
        /asset/,
      );
      fs.appendFileSync(valid.archive, 'changed');
      await assert.rejects(
        verifyMappedRc(valid.archive, valid.topologyBytes, valid.digest),
        /bytes differ/,
      );
    } finally {
      fs.rmSync(valid.root, { recursive: true, force: true });
    }
    for (const [label, mutate] of /** @type {[string, (data: any) => void][]} */ ([
      [
        'package version',
        (x) => {
          x.package.version = '0.19.8';
        },
      ],
      [
        'VSIX identity version',
        (x) => {
          x.xml = x.xml.replace('Version="0.19.7"', 'Version="0.19.8"');
        },
      ],
      [
        'different RC iteration',
        (x) => {
          x.payload.candidate.release = '0.18.0-rc.8';
        },
      ],
      [
        'payload source',
        (x) => {
          x.payload.extension.sourceSha = 'c'.repeat(40);
        },
      ],
      [
        'payload topology digest',
        (x) => {
          x.payload.releaseTopologySha256 = 'c'.repeat(64);
        },
      ],
      [
        'legacy payload',
        (x) => {
          x.payload.schema = 'vsix_candidate_payload.v1';
          delete x.payload.preRelease;
        },
      ],
      [
        'absent marker',
        (x) => {
          x.xml = x.xml.replace(
            '<Property Id="Microsoft.VisualStudio.Code.PreRelease" Value="true"/>',
            '',
          );
        },
      ],
      [
        'false marker',
        (x) => {
          x.xml = x.xml.replace('Value="true"', 'Value="false"');
        },
      ],
      [
        'duplicate marker',
        (x) => {
          x.xml = x.xml.replace(
            '</Properties>',
            '<Property Id="Microsoft.VisualStudio.Code.PreRelease" Value="true"/></Properties>',
          );
        },
      ],
    ])) {
      const wrong = await mappedFixture(mutate);
      try {
        await assert.rejects(
          verifyMappedRc(wrong.archive, wrong.topologyBytes, wrong.digest),
          /./,
          label,
        );
      } finally {
        fs.rmSync(wrong.root, { recursive: true, force: true });
      }
    }
  });

  await test('mapped RC binds manifest platform to native or universal projection', async () => {
    for (const native of [true, false]) {
      const valid = await mappedFixture(() => {}, native);
      try {
        await verifyMappedRc(valid.archive, valid.topologyBytes, valid.digest);
      } finally {
        fs.rmSync(valid.root, { recursive: true, force: true });
      }
      const mutations = native
        ? [
            (xml) => xml.replace('TargetPlatform="linux-x64"', 'TargetPlatform="win32-x64"'),
            (xml) => xml.replace('TargetPlatform="linux-x64"', ''),
          ]
        : [
            (xml) => xml.replace('<Identity ', '<Identity TargetPlatform="linux-x64" '),
            (xml) => xml.replace('<Identity ', '<Identity TargetPlatform="" '),
          ];
      for (const mutate of mutations) {
        const wrong = await mappedFixture((data) => {
          data.xml = mutate(data.xml);
        }, native);
        try {
          await assert.rejects(
            verifyMappedRc(wrong.archive, wrong.topologyBytes, wrong.digest),
            /target platform differs/,
          );
        } finally {
          fs.rmSync(wrong.root, { recursive: true, force: true });
        }
      }
    }
  });

  function sha256(value) {
    return crypto.createHash('sha256').update(value).digest('hex');
  }

  async function writeZip(files) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-member-'));
    const archive = path.join(root, 'fixture.vsix');
    const zip = new JSZip();
    for (const file of files) zip.file(file.name, file.value, file.options);
    fs.writeFileSync(archive, await zip.generateAsync({ type: 'nodebuffer' }));
    return { root, archive };
  }

  async function writeDuplicateZip() {
    const fixture = await writeZip([
      { name: 'extension/perllsp.exe', value: 'one' },
      { name: 'extension/perllsp.exf', value: 'one' },
    ]);
    const bytes = fs.readFileSync(fixture.archive);
    const from = Buffer.from('extension/perllsp.exf');
    const to = Buffer.from('extension/perllsp.exe');
    for (let offset = 0; ;) {
      const index = bytes.indexOf(from, offset);
      if (index < 0) break;
      to.copy(bytes, index);
      offset = index + to.length;
    }
    fs.writeFileSync(fixture.archive, bytes);
    return fixture;
  }

  function setCentralDirectoryAttributes(bytes, attributes) {
    const signature = Buffer.from([0x50, 0x4b, 0x01, 0x02]);
    const name = Buffer.from('extension/perllsp.exe');
    for (let offset = 0; ;) {
      const index = bytes.indexOf(signature, offset);
      if (index < 0) break;
      const nameLength = bytes.readUInt16LE(index + 28);
      if (bytes.subarray(index + 46, index + 46 + nameLength).equals(name)) {
        bytes.writeUInt32LE(attributes >>> 0, index + 38);
      }
      offset = index + 4;
    }
  }

  function runChecker(script, archive, expected) {
    const result = spawnSync(
      process.execPath,
      [script, '--vsix', archive, '--member', 'perllsp.exe', '--sha256', expected],
      {
        encoding: 'utf8',
        env: { ...process.env, NODE_PATH: path.join(__dirname, '..', 'node_modules') },
        windowsHide: true,
      },
    );
    if (result.error) {
      throw new Error(`failed to launch payload checker ${script}: ${result.error.message}`);
    }
    return result;
  }

  void test('streams and verifies one regular payload member', async () => {
    const value = Buffer.from('server-bytes');
    const fixture = await writeZip([{ name: 'extension/perllsp.exe', value }]);
    try {
      await verifyPayloadMember(fixture.archive, 'perllsp.exe', sha256(value));
      await assert.rejects(
        verifyPayloadMember(fixture.archive, 'perllsp.exe', sha256(Buffer.from('wrong'))),
        /archive payload SHA mismatch/,
      );
    } finally {
      fs.rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  void test('rejects duplicate payload members', async () => {
    const fixture = await writeDuplicateZip();
    try {
      const result = runChecker(
        path.join(__dirname, 'check-vsix-prebuilt-payload.js'),
        fixture.archive,
        sha256(Buffer.from('one')),
      );
      assert.notEqual(result.status, 0);
      assert.match(result.stderr ?? '', /duplicate payload member/);
    } finally {
      fs.rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  void test('rejects non-regular payload types', async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-link-'));
    try {
      for (const [label, fileTypeValue] of [
        ['symlink', 'S_IFLNK'],
        ['directory', 'S_IFDIR'],
        ['fifo', 'S_IFIFO'],
        ['device', 'S_IFCHR'],
        ['dos-directory', 'DOS'],
      ]) {
        const fileType = fileTypeValue ?? '';
        const archive = path.join(root, `${label}.vsix`);
        const zip = new JSZip();
        zip.file('extension/perllsp.exe', 'target');
        const bytes = await zip.generateAsync({ type: 'nodebuffer', platform: 'UNIX' });
        const attributes =
          fileType === 'DOS'
            ? 0x10
            : { S_IFLNK: 0o120777, S_IFDIR: 0o40777, S_IFIFO: 0o010777, S_IFCHR: 0o020666 }[
                fileType
              ] << 16;
        setCentralDirectoryAttributes(bytes, attributes);
        fs.writeFileSync(archive, bytes);
        const expected = sha256(Buffer.from('target'));
        const result = runChecker(
          path.join(__dirname, 'check-vsix-prebuilt-payload.js'),
          archive,
          expected,
        );
        assert.notEqual(result.status, 0, `${label}: checker accepted unsafe member`);
        assert.match(result.stderr ?? '', /not a regular file/);
      }
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  await test('mapped identities reject duplicate keys and bind one archive snapshot', async () => {
    const { parseUniqueJson } = require('./check-vsix-prebuilt-payload');
    assert.deepEqual(parseUniqueJson('{"outer":{"value":1}}'), { outer: { value: 1 } });
    for (const raw of [
      '{"value":1,"value":1}',
      '{"outer":{"value":1,"value":2}}',
      '{"a":1,"\\u0061":2}',
    ]) {
      assert.throws(() => parseUniqueJson(raw), /Duplicate/);
    }
    for (const field of ['packageRaw', 'payloadRaw']) {
      const f = await mappedFixture((data) => {
        data[field] = '{"version":"1.2.3","version":"1.2.3"}';
      });
      try {
        await assert.rejects(verifyMappedRc(f.archive, f.topologyBytes, f.digest), /Duplicate/);
      } finally {
        fs.rmSync(f.root, { recursive: true, force: true });
      }
    }
    const f = await mappedFixture(() => {}, true);
    try {
      const duplicateTopology = Buffer.from(
        f.topologyBytes.toString().replace('"schema":4', '"schema":4,"schema":4'),
      );
      await assert.rejects(verifyMappedRc(f.archive, duplicateTopology, f.digest), /Duplicate/);
      const wrongSchema = JSON.parse(f.topologyBytes.toString());
      wrongSchema.sources['schemas/release_topology.v4.schema.json'].sha256 = 'c'.repeat(64);
      await assert.rejects(
        verifyMappedRc(f.archive, Buffer.from(JSON.stringify(wrongSchema)), f.digest),
        /schema source hash/,
      );
      const original = fs.readFileSync;
      let archiveReads = 0;
      fs.readFileSync = function (file, ...args) {
        if (file === f.archive) archiveReads += 1;
        return original.call(this, file, ...args);
      };
      try {
        await verifyMappedRc(f.archive, f.topologyBytes, f.digest);
      } finally {
        fs.readFileSync = original;
      }
      assert.equal(
        archiveReads,
        1,
        'metadata, native members and inventory consume one captured artifact',
      );
    } finally {
      fs.rmSync(f.root, { recursive: true, force: true });
    }
  });
}

runTests().catch((error) => {
  process.stderr.write(`${String(error)}\n`);
  process.exitCode = 1;
});
