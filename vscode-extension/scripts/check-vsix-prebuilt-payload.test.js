const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const JSZip = require('jszip');
const { test } = require('node:test');
const { verifyPayloadMember } = require('./check-vsix-prebuilt-payload');

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
