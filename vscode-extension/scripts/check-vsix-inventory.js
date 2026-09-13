#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const { validateProjectionManifest, vsixName } = require('./package-vsix');

const EXTENSION_ROOT = path.resolve(__dirname, '..');
const BASELINE_PATH = path.join(__dirname, 'vsix-inventory-baseline.json');

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

function classifyInventoryViolations(violations) {
  if (violations.length === 0) {
    return 'pass';
  }
  const sizeOnly = violations.every(
    (violation) =>
      /^total bytes grew from \d+ to \d+$/.test(violation) ||
      /^file .+ grew from \d+ to \d+ bytes$/.test(violation),
  );
  return sizeOnly ? 'size_only' : 'structural';
}

function currentSourceBundleFile(platform = process.platform, arch = process.arch) {
  const binaryName = platform === 'win32' ? 'perllsp.exe' : 'perllsp';
  return `bin/${platform}-${arch}/${binaryName}`;
}

/** @returns {string[]} */
function currentSourceBundleFiles(
  platform = process.platform,
  arch = process.arch,
  includeDap = false,
) {
  const dapName = platform === 'win32' ? 'perl-dap.exe' : 'perl-dap';
  return [
    currentSourceBundleFile(platform, arch),
    ...(includeDap ? [`bin/${platform}-${arch}/${dapName}`] : []),
  ];
}

function parseArgs(argv) {
  let updateBaseline = false;
  /** @type {string | null} */
  let vsixPath = null;
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--update-baseline') {
      if (updateBaseline) {
        throw new Error('duplicate --update-baseline option');
      }
      updateBaseline = true;
      continue;
    }
    if (argument === '--vsix') {
      if (vsixPath !== null) {
        throw new Error('duplicate --vsix option');
      }
      const value = argv[++index];
      if (!value || value.startsWith('--')) {
        throw new Error('--vsix requires a value');
      }
      vsixPath = path.resolve(EXTENSION_ROOT, value);
      continue;
    }
    throw new Error(`Unknown argument: ${argument}`);
  }
  return { updateBaseline, vsixPath };
}

async function main() {
  const { updateBaseline, vsixPath: requestedVsixPath } = parseArgs(process.argv.slice(2));
  const baseline =
    updateBaseline && !fs.existsSync(BASELINE_PATH)
      ? null
      : JSON.parse(fs.readFileSync(BASELINE_PATH, 'utf8'));
  const vsixPath = requestedVsixPath || path.join(EXTENSION_ROOT, vsixName);
  const transition = require('./check-vsix-inventory-transition');
  const actual = (await transition.collectArchiveInventory(vsixPath)).inventory;
  if (updateBaseline) {
    fs.writeFileSync(BASELINE_PATH, `${JSON.stringify(actual, null, 2)}\n`);
    process.stdout.write(`Updated ${BASELINE_PATH}\n`);
    return;
  }
  const manifestPath = (process.env.PERL_LSP_CANDIDATE_PAYLOAD_MANIFEST || '').trim();
  const allowedFiles = [];
  if (!manifestPath && process.env.PERL_LSP_CURRENT_SOURCE_SMOKE === '1') {
    allowedFiles.push(
      ...currentSourceBundleFiles(
        process.platform,
        process.arch,
        process.env.PERL_LSP_CURRENT_SOURCE_DAP_STAGED === '1',
      ),
    );
  }
  if (manifestPath) {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
    const projectionPath = (process.env.PERL_LSP_VSIX_PROJECTION_INPUT || '').trim();
    if (!projectionPath) {
      throw new Error('candidate payload manifest requires a projection input');
    }
    const projection = JSON.parse(fs.readFileSync(projectionPath, 'utf8'));
    const target = (
      process.env.PERL_LSP_VSCODE_TARGET ||
      manifest?.package?.vscodeTargetId ||
      ''
    ).trim();
    const validatedManifest = validateProjectionManifest(manifest, projection, target);
    const inventorySha = validatedManifest.package.inventorySha256;
    if (transition.semanticInventorySha256(actual) !== inventorySha) {
      throw new Error('candidate payload manifest inventory SHA does not match the produced VSIX');
    }
    const members = [
      validatedManifest.server?.member,
      validatedManifest.dap?.payload?.member,
    ].filter((member) => typeof member === 'string');
    for (const member of members) {
      const packagedFile = `bin/${target}/${member}`;
      if (!Object.hasOwn(actual.files, packagedFile)) {
        throw new Error(
          `candidate payload member is missing from the produced VSIX: ${packagedFile}`,
        );
      }
      allowedFiles.push(packagedFile);
    }
  }
  const violations = compareInventory(actual, baseline, process.platform, {
    allowedFiles,
    arch: process.arch,
  });
  const classification = classifyInventoryViolations(violations);
  process.stdout.write(
    `${JSON.stringify(
      {
        ...actual,
        baseline: BASELINE_PATH,
        platform: process.platform,
        classification,
        violations,
      },
      null,
      2,
    )}\n`,
  );
  if (violations.length > 0) {
    process.exitCode = 1;
  }
}

module.exports = {
  baselineForPlatform,
  classifyInventoryViolations,
  compareInventory,
  currentSourceBundleFile,
  currentSourceBundleFiles,
  bundleTargetForPackagedFile,
  platformForPackagedFile,
  parseArgs,
  summarizeInventory,
};

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
