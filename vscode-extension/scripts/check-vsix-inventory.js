#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const EXTENSION_ROOT = path.resolve(__dirname, '..');
const BASELINE_PATH = path.join(__dirname, 'vsix-inventory-baseline.json');
const VSCE_ENTRY = path.join(EXTENSION_ROOT, 'node_modules', '@vscode', 'vsce', 'vsce');

function collectPackagedFiles() {
  const result = spawnSync(process.execPath, [VSCE_ENTRY, 'ls'], {
    cwd: EXTENSION_ROOT,
    encoding: 'utf8',
    windowsHide: true,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`vsce ls failed: ${(result.stderr || result.stdout || '').trim()}`);
  }
  return result.stdout
    .split(/\r?\n/)
    .map((line) => line.trim().replaceAll('\\', '/'))
    .filter((file) => file.length > 0)
    .map((file) => ({ file, bytes: fs.statSync(path.join(EXTENSION_ROOT, file)).size }));
}

function summarizeInventory(entries) {
  const files = Object.fromEntries(entries.map(({ file, bytes }) => [file, bytes]));
  return {
    schema_version: 1,
    total_files: entries.length,
    total_bytes: entries.reduce((total, { bytes }) => total + bytes, 0),
    files,
  };
}

function platformForPackagedFile(file) {
  const match = /^bin\/(linux|darwin|win32)(?:-[^/]+)?\//.exec(file);
  return match ? match[1] : null;
}

function bundleTargetForPackagedFile(file) {
  const match = /^bin\/(linux|darwin|win32)-([^/]+)\//.exec(file);
  return match ? `${match[1]}-${match[2]}` : null;
}

function baselineForPlatform(baseline, platform, arch = 'x64') {
  const target = `${platform}-${arch}`;
  const files = Object.fromEntries(
    Object.entries(baseline.files).filter(([file]) => {
      const fileTarget = bundleTargetForPackagedFile(file);
      return fileTarget === null || fileTarget === target;
    }),
  );
  return summarizeInventory(Object.entries(files).map(([file, bytes]) => ({ file, bytes })));
}

function compareInventory(actual, baseline, platform = process.platform, options = {}) {
  const allowedFiles = new Set(options.allowedFiles ?? []);
  const arch = options.arch ?? process.arch;
  const target = `${platform}-${arch}`;
  const effectiveBaseline = summarizeInventory(
    Object.entries(baselineForPlatform(baseline, platform, arch).files)
      .filter(([file]) => !allowedFiles.has(file))
      .map(([file, bytes]) => ({ file, bytes })),
  );
  const projectedActual = Object.entries(actual.files).filter(([file]) => {
    const fileTarget = bundleTargetForPackagedFile(file);
    if (allowedFiles.has(file)) {
      return false;
    }
    return fileTarget === null || fileTarget === target;
  });
  const effectiveActual = summarizeInventory(
    projectedActual.map(([file, bytes]) => ({ file, bytes })),
  );
  const violations = [];
  if (effectiveActual.total_files > effectiveBaseline.total_files) {
    violations.push(
      `file count grew from ${effectiveBaseline.total_files} to ${effectiveActual.total_files}`,
    );
  }
  if (effectiveActual.total_bytes > effectiveBaseline.total_bytes) {
    violations.push(
      `total bytes grew from ${effectiveBaseline.total_bytes} to ${effectiveActual.total_bytes}`,
    );
  }
  for (const [file, bytes] of Object.entries(effectiveActual.files)) {
    if (!Object.hasOwn(effectiveBaseline.files, file)) {
      if (!allowedFiles.has(file)) {
        violations.push(`new packaged file: ${file}`);
      }
    } else if (bytes > effectiveBaseline.files[file]) {
      violations.push(`file ${file} grew from ${effectiveBaseline.files[file]} to ${bytes} bytes`);
    }
  }
  for (const [file, bytes] of Object.entries(actual.files)) {
    const fileTarget = bundleTargetForPackagedFile(file);
    if (allowedFiles.has(file)) {
      continue;
    }
    if (
      fileTarget !== null &&
      fileTarget !== target &&
      Object.hasOwn(baseline.files, file) &&
      bytes > baseline.files[file]
    ) {
      violations.push(`file ${file} grew from ${baseline.files[file]} to ${bytes} bytes`);
    }
    if (fileTarget !== null && fileTarget !== target && !Object.hasOwn(baseline.files, file)) {
      violations.push(`unexpected foreign-platform packaged file: ${file}`);
    }
  }
  for (const file of Object.keys(effectiveBaseline.files)) {
    if (!Object.hasOwn(effectiveActual.files, file)) {
      violations.push(`baseline packaged file is missing: ${file}`);
    }
  }
  return violations;
}

function currentSourceBundleFile(platform = process.platform, arch = process.arch) {
  const binaryName = platform === 'win32' ? 'perllsp.exe' : 'perllsp';
  return `bin/${platform}-${arch}/${binaryName}`;
}

function main() {
  const updateBaseline = process.argv.includes('--update-baseline');
  const baseline =
    updateBaseline && !fs.existsSync(BASELINE_PATH)
      ? null
      : JSON.parse(fs.readFileSync(BASELINE_PATH, 'utf8'));
  const actual = summarizeInventory(collectPackagedFiles());
  if (updateBaseline) {
    fs.writeFileSync(BASELINE_PATH, `${JSON.stringify(actual, null, 2)}\n`);
    process.stdout.write(`Updated ${BASELINE_PATH}\n`);
    return;
  }
  const allowedFiles =
    process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1' ? [currentSourceBundleFile()] : [];
  const violations = compareInventory(actual, baseline, process.platform, {
    allowedFiles,
    arch: process.arch,
  });
  process.stdout.write(
    `${JSON.stringify(
      { ...actual, baseline: BASELINE_PATH, platform: process.platform, violations },
      null,
      2,
    )}\n`,
  );
  if (violations.length > 0) {
    process.exitCode = 1;
  }
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  baselineForPlatform,
  bundleTargetForPackagedFile,
  collectPackagedFiles,
  compareInventory,
  currentSourceBundleFile,
  platformForPackagedFile,
  summarizeInventory,
};
