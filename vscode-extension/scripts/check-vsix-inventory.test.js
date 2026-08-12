const assert = require('node:assert/strict');
const { test } = require('node:test');
const {
  baselineForPlatform,
  classifyInventoryViolations,
  compareInventory,
  currentSourceBundleFile,
  platformForPackagedFile,
  summarizeInventory,
} = require('./check-vsix-inventory');

void test('summarizes packaged file sizes', () => {
  assert.deepEqual(
    summarizeInventory([
      { file: 'a.txt', bytes: 2 },
      { file: 'b.js', bytes: 3 },
    ]),
    {
      schema_version: 1,
      total_files: 2,
      total_bytes: 5,
      files: { 'a.txt': 2, 'b.js': 3 },
    },
  );
});

void test('rejects package growth and inventory drift', () => {
  const violations = compareInventory(
    { total_files: 2, total_bytes: 8, files: { 'a.txt': 5, 'new.js': 3 } },
    { total_files: 2, total_bytes: 5, files: { 'a.txt': 2, 'old.js': 3 } },
  );
  assert.deepEqual(violations, [
    'total bytes grew from 5 to 8',
    'file a.txt grew from 2 to 5 bytes',
    'new packaged file: new.js',
    'baseline packaged file is missing: old.js',
  ]);
});

void test('classifies byte-only growth separately from structural package drift', () => {
  assert.equal(
    classifyInventoryViolations([
      'total bytes grew from 10 to 12',
      'file out/extension.js grew from 8 to 10 bytes',
    ]),
    'size_only',
  );
  assert.equal(
    classifyInventoryViolations(['new packaged file: unexpected.exe']),
    'structural',
  );
});

void test('classifies an unchanged inventory as pass', () => {
  assert.equal(classifyInventoryViolations([]), 'pass');
});

void test('uses only the current platform baseline entries', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };
  const linuxBaseline = baselineForPlatform(baseline, 'linux', 'x64');

  assert.deepEqual(linuxBaseline, {
    schema_version: 1,
    total_files: 2,
    total_bytes: 10,
    files: { 'README.md': 2, 'bin/linux-x64/perllsp': 8 },
  });
  assert.deepEqual(
    compareInventory(
      {
        total_files: 3,
        total_bytes: 20,
        files: {
          'README.md': 2,
          'bin/linux-x64/perllsp': 8,
          'bin/win32-x64/perllsp.exe': 10,
        },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('still rejects growth for a platform-owned file', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 13,
        files: {
          'README.md': 2,
          'bin/win32-x64/perllsp.exe': 11,
        },
      },
      baseline,
      'win32',
      { arch: 'x64' },
    ),
    ['total bytes grew from 12 to 13', 'file bin/win32-x64/perllsp.exe grew from 10 to 11 bytes'],
  );
});

void test('accepts a known foreign platform bundle without requiring it on this host', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 20,
    files: {
      'README.md': 2,
      'bin/win32-x64/perllsp.exe': 10,
      'bin/linux-x64/perllsp': 8,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 3,
        total_bytes: 20,
        files: {
          'README.md': 2,
          'bin/linux-x64/perllsp': 8,
          'bin/win32-x64/perllsp.exe': 10,
        },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('rejects an unexpected foreign platform bundle', () => {
  const baseline = {
    total_files: 1,
    total_bytes: 2,
    files: { 'README.md': 2 },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, 'bin/darwin-arm64/perllsp': 10 },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    ['unexpected foreign-platform packaged file: bin/darwin-arm64/perllsp'],
  );
});

void test('allows the explicitly staged current-source target when no baseline exists', () => {
  const baseline = {
    total_files: 1,
    total_bytes: 2,
    files: { 'README.md': 2 },
  };
  const currentSourceFile = currentSourceBundleFile('darwin', 'arm64');

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, [currentSourceFile]: 10 },
      },
      baseline,
      'darwin',
      { allowedFiles: [currentSourceFile], arch: 'arm64' },
    ),
    [],
  );
});

void test('checks an explicitly staged current-source target already in the baseline', () => {
  const baseline = {
    total_files: 2,
    total_bytes: 12,
    files: { 'README.md': 2, 'bin/darwin-arm64/perllsp': 10 },
  };
  const currentSourceFile = currentSourceBundleFile('darwin', 'arm64');

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 13,
        files: { 'README.md': 2, [currentSourceFile]: 11 },
      },
      baseline,
      'darwin',
      { allowedFiles: [currentSourceFile], arch: 'arm64' },
    ),
    [],
  );
});

void test('selects the exact platform and architecture baseline', () => {
  const baseline = {
    total_files: 3,
    total_bytes: 30,
    files: {
      'README.md': 2,
      'bin/linux-x64/perllsp': 10,
      'bin/linux-arm64/perllsp': 18,
    },
  };

  assert.deepEqual(
    compareInventory(
      {
        total_files: 2,
        total_bytes: 12,
        files: { 'README.md': 2, 'bin/linux-x64/perllsp': 10 },
      },
      baseline,
      'linux',
      { arch: 'x64' },
    ),
    [],
  );
});

void test('does not classify ordinary files as platform-owned', () => {
  assert.equal(platformForPackagedFile('assets/demo-project/main.pl'), null);
  assert.equal(platformForPackagedFile('bin/win32-x64/perllsp.exe'), 'win32');
  assert.equal(platformForPackagedFile('bin/linux-x64/perllsp'), 'linux');
  assert.equal(platformForPackagedFile('bin/darwin-arm64/perllsp'), 'darwin');
});
