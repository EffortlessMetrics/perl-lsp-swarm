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

function runChecker(script, archive, expected) {
  return spawnSync(
    process.execPath,
    [script, '--vsix', archive, '--member', 'perllsp.exe', '--sha256', expected],
    {
      encoding: 'utf8',
      env: { ...process.env, NODE_PATH: path.join(__dirname, '..', 'node_modules') },
      windowsHide: true,
    },
  );
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-vsix-duplicate-'));
  const archive = path.join(root, 'fixture.vsix');
  try {
    const python = spawnSync('python', [
      '-c',
      "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1],'w'); z.writestr('extension/perllsp.exe',b'one'); z.writestr('extension/perllsp.exe',b'one'); z.close()",
      archive,
    ]);
    assert.equal(python.status, 0);
    const result = runChecker(
      path.join(__dirname, 'check-vsix-prebuilt-payload.js'),
      archive,
      sha256(Buffer.from('one')),
    );
    assert.notEqual(result.status, 0);
    assert.match(result.stderr ?? '', /duplicate payload member/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
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
      const python = spawnSync('python', [
        '-c',
        "import stat,sys,zipfile; z=zipfile.ZipFile(sys.argv[1],'w'); i=zipfile.ZipInfo('extension/perllsp.exe'); mode=0x10 if sys.argv[2]=='DOS' else (getattr(stat,sys.argv[2])|0o777)<<16; i.external_attr=mode; z.writestr(i,b'target'); z.close()",
        archive,
        fileType,
      ]);
      assert.equal(python.status, 0);
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
