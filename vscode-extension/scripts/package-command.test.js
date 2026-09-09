const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const { packageVsix, vsixName, vsceEntry } = require('./package-vsix');

const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'));

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
