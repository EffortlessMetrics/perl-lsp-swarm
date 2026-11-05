'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');
const {
  TYPESCRIPT_AUTHORITY_MAJOR,
  TSCONFIG_FILES,
  evaluateTypeScriptAuthority,
  checkTypeScriptAuthority,
} = require('./check-typescript-authority');
const { checkTypeScriptConfigInventory } = require('./check-typescript-config-inventory');

const EXTENSION_ROOT = path.resolve(__dirname, '..');

/**
 * A tree that satisfies every invariant, so each negative case below differs
 * from a green run by exactly one drifted fact.
 *
 * @param {Record<string, unknown>} [overrides]
 */
function healthyInput(overrides = {}) {
  return {
    expectedMajor: 7,
    declaredRange: '^7.0.2',
    lockEntry: {
      version: '7.0.2',
      resolved: 'https://registry.npmjs.org/typescript/-/typescript-7.0.2.tgz',
    },
    lockIntegrity: 'sha512-deadbeef',
    installedVersion: '7.0.2',
    binaryVersionOutput: 'Version 7.0.2\n',
    binShim: {
      resolved: '/ext/node_modules/typescript/bin/tsc',
      expected: '/ext/node_modules/typescript/bin/tsc',
    },
    tsconfigs: [{ file: 'tsconfig.json', ignoreDeprecations: undefined }],
    ...overrides,
  };
}

/**
 * @param {ReturnType<typeof evaluateTypeScriptAuthority>} result
 * @param {RegExp} pattern
 */
function assertFailedWith(result, pattern) {
  assert.equal(result.ok, false, `expected a red result, got: ${JSON.stringify(result)}`);
  assert.ok(
    result.failures.some((failure) => pattern.test(failure)),
    `no failure matched ${String(pattern)}; failures were ${JSON.stringify(result.failures)}`,
  );
}

void test('a healthy TS7 tree passes and reports its evidence', () => {
  const result = evaluateTypeScriptAuthority(healthyInput());
  assert.equal(result.ok, true, JSON.stringify(result.failures));
  assert.deepEqual(result.failures, []);
  assert.ok(result.facts.length > 0);
});

void test('a TS6 declared range is red even though TS6 compiles this tree clean', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: '^6.0.3' })),
    /floors at major 6/,
  );
});

void test('an npm: alias specifier is rejected rather than parsed', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: 'npm:typescript@^6.0.3' })),
    /non-registry specifier/,
  );
});

void test('a file: or git specifier is rejected', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: 'file:../local-tsc' })),
    /non-registry specifier/,
  );
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: 'git+https://example.invalid/ts' })),
    /non-registry specifier/,
  );
});

void test('a legitimate bounded range is gated on its floor, not rejected for its shape', () => {
  // `>=7.0.2 <8.0.0` is a valid npm range whose floor is readable. Rejecting it
  // would be a false red on a valid config, which costs the gate the
  // credibility its real failures depend on.
  for (const range of ['>=7.0.2 <8.0.0', '>= 7.0.2', '=7.0.2', 'v7.0.2']) {
    const result = evaluateTypeScriptAuthority(healthyInput({ declaredRange: range }));
    assert.equal(result.ok, true, `${range} should pass: ${JSON.stringify(result.failures)}`);
  }
});

/** @param {string} range @param {string} version */
function versionInput(range, version) {
  return healthyInput({
    declaredRange: range,
    lockEntry: {
      version,
      resolved: `https://registry.npmjs.org/typescript/-/typescript-${version}.tgz`,
    },
    installedVersion: version,
    binaryVersionOutput: `Version ${version}\n`,
  });
}

void test('the locked compiler must satisfy the complete declared range, not only its major', () => {
  /** @type {[string, string][]} */
  const cases = [
    ['^7.1.0', '7.0.2'],
    ['~7.0.2', '7.1.0'],
    ['>=7.0.2 <7.1.0', '7.1.0'],
    ['7.1.0', '7.1.1'],
  ];
  for (const [range, version] of cases) {
    assertFailedWith(
      evaluateTypeScriptAuthority(versionInput(range, version)),
      /does not satisfy the declared typescript range/,
    );
  }
});

void test('explicit beta and nightly versions retain normal semver agreement', () => {
  /** @type {[string, string][]} */
  const cases = [
    ['7.1.0-beta.1', '7.1.0-beta.1'],
    ['^7.1.0-beta.1', '7.1.0-beta.2'],
    ['^7.1.0-beta.1', '7.1.0'],
    ['7.1.0-dev.20260930.4', '7.1.0-dev.20260930.4'],
  ];
  for (const [range, version] of cases) {
    const result = evaluateTypeScriptAuthority(versionInput(range, version));
    assert.equal(result.ok, true, `${range} / ${version}: ${JSON.stringify(result.failures)}`);
  }
});

void test('a different prerelease or an undeclared prerelease cannot supply compiler authority', () => {
  /** @type {[string, string][]} */
  const cases = [
    ['7.1.0-beta.1', '7.1.0-beta.2'],
    ['7.1.0-dev.20260930.4', '7.1.0-dev.20260930.5'],
    ['^7.0.2', '7.1.0-beta.1'],
    ['^7.1.0', '7.1.0-beta.1'],
  ];
  for (const [range, version] of cases) {
    assertFailedWith(
      evaluateTypeScriptAuthority(versionInput(range, version)),
      /does not satisfy the declared typescript range/,
    );
  }
});

void test('invalid trailing range syntax cannot be hidden behind a valid major prefix', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(versionInput('>=7.0.2 definitely-not-a-range', '7.0.2')),
    /semver range/,
  );
});

void test('a bounded range whose floor is a pre-authority major is still red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: '>=6.0.3 <8.0.0' })),
    /floors at major 6/,
  );
});

void test('a union range is rejected even when its first term floors correctly', () => {
  // `^7.0.2 || ^6.0.3` floors at 7 but can still install TS6 depending on what
  // else is in the tree — exactly the ambiguity this gate exists to remove.
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: '^7.0.2 || ^6.0.3' })),
    /union range/,
  );
});

void test('an unpinned range such as * or latest is rejected', () => {
  assertFailedWith(evaluateTypeScriptAuthority(healthyInput({ declaredRange: '*' })), /semver/);
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: 'latest' })),
    /semver/,
  );
});

void test('a missing typescript devDependency is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ declaredRange: undefined })),
    /declares no `typescript` devDependency/,
  );
});

void test('a lockfile that resolves a pre-authority major is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        lockEntry: {
          version: '6.0.3',
          resolved: 'https://registry.npmjs.org/typescript/-/typescript-6.0.3.tgz',
        },
      }),
    ),
    /package-lock\.json resolves typescript 6\.0\.3/,
  );
});

void test('a lockfile alias redirect is named as an alias, not just a version mismatch', () => {
  const result = evaluateTypeScriptAuthority(
    healthyInput({
      lockEntry: {
        name: 'typescript6',
        version: '7.0.2',
        resolved: 'https://registry.npmjs.org/typescript6/-/typescript6-7.0.2.tgz',
      },
    }),
  );
  assertFailedWith(result, /aliases node_modules\/typescript to "typescript6"/);
});

void test('a lockfile resolving a non-registry tarball is red', () => {
  // A `file:` specifier has no registry origin, so it is now rejected on the
  // origin rather than on the path shape — a stricter reason for the same
  // verdict.
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        lockEntry: { version: '7.0.2', resolved: 'file:../vendor/typescript-7.0.2.tgz' },
      }),
    ),
    /not the approved npm registry/,
  );
});

void test('a missing lockfile entry is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ lockEntry: undefined })),
    /no `node_modules\/typescript` entry/,
  );
});

void test('an uninstalled compiler is red rather than silently skipped', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({ installedVersion: undefined, binaryVersionOutput: undefined }),
    ),
    /is it installed/,
  );
});

void test('an installed major below the authority major is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({ installedVersion: '6.0.3', binaryVersionOutput: 'Version 6.0.3\n' }),
    ),
    /installed typescript is 6\.0\.3/,
  );
});

void test('a same-major installed package that is not the locked version is red', () => {
  // The hole this closes: lockfile 7.0.2, installed 7.1.0, binary 7.1.0. Every
  // major check passes and the binary agrees with its own package metadata, so
  // the gate would report lock + install + executing compiler as one authority
  // chain while the installed bytes are not the locked artifact — a stale or
  // tampered node_modules.
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({ installedVersion: '7.1.0', binaryVersionOutput: 'Version 7.1.0\n' }),
    ),
    /installed typescript is 7\.1\.0 but package-lock\.json pins 7\.0\.2/,
  );
});

void test('only the opposite-platform shim being present is red, not accepted', () => {
  // `npm run typecheck` resolves `tsc` through node_modules/.bin. On POSIX a
  // shell will not select `tsc.cmd`; it falls through to whatever `tsc` is on
  // PATH — a compiler this gate never inspected. Accepting the other
  // platform's wrapper because it happens to resolve to the pinned package
  // would be a false green.
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        binShim: {
          error:
            'only tsc.cmd is present, but this platform (linux) executes tsc — the compiler `npm run` would resolve is not the one verified here',
        },
      }),
    ),
    /is not the one verified here/,
  );
});

void test('a binary whose version disagrees with its package metadata is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ binaryVersionOutput: 'Version 6.0.3\n' })),
    /the compiler that runs is not the package that is pinned/,
  );
});

void test('an unreadable tsc --version is red, never assumed green', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ binaryVersionOutput: '' })),
    /no readable version/,
  );
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ binaryVersionOutput: undefined })),
    /could not run the installed/,
  );
});

void test('a tarball from a non-npm host is red even with the right path shape', () => {
  // The hole this closes: a substring test on the path accepts
  // https://attacker.example/typescript/-/typescript-7.0.2.tgz, and the gate
  // would then report that TypeScript resolved "from the npm registry".
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        lockEntry: {
          version: '7.0.2',
          resolved: 'https://attacker.example/typescript/-/typescript-7.0.2.tgz',
        },
      }),
    ),
    /from the origin "https:\/\/attacker\.example", not the approved npm registry/,
  );
});

void test('a tarball path naming a different version than the lockfile is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        lockEntry: {
          version: '7.0.2',
          resolved: 'https://registry.npmjs.org/typescript/-/typescript-6.0.3.tgz',
        },
      }),
    ),
    /tarball path "\/typescript\/-\/typescript-6\.0\.3\.tgz", which is not the registry path/,
  );
});

void test('a lockfile entry with no integrity hash is red', () => {
  // Origin plus path pin *where* the tarball came from; integrity pins *what*
  // is inside it. Without the hash the contents are unverified.
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ lockIntegrity: undefined })),
    /records no `integrity`/,
  );
});

void test('a missing .bin/tsc shim is red', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(healthyInput({ binShim: undefined })),
    /node_modules\/\.bin\/tsc is missing/,
  );
});

void test('a .bin/tsc shim pointing at a different tsc is red', () => {
  // The hole this closes: every other invariant inspects
  // node_modules/typescript, but `npm run typecheck` resolves `tsc` through
  // node_modules/.bin. A stale shim left by a removed package, or one
  // redirected at a hoisted or globally-linked install, executes a compiler
  // this gate never looked at. Presence alone cannot detect that.
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        binShim: {
          resolved: '/usr/lib/node_modules/typescript/bin/tsc',
          expected: '/ext/node_modules/typescript/bin/tsc',
        },
      }),
    ),
    /resolves to \/usr\/lib\/node_modules\/typescript\/bin\/tsc, not the pinned package/,
  );
});

void test('a .bin/tsc shim that cannot be resolved is red, not silently accepted', () => {
  // A dangling symlink or an unreadable generated wrapper leaves the executing
  // compiler unproven. "Could not determine" must never read as "fine".
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        binShim: {
          expected: '/ext/node_modules/typescript/bin/tsc',
          error: 'ENOENT: no such file or directory',
        },
      }),
    ),
    /could not be resolved to a real target .*ENOENT.*unproven/,
  );
});

void test('a correctly bound shim is reported as evidence on a green run', () => {
  const result = evaluateTypeScriptAuthority(healthyInput());
  assert.ok(result.ok, `expected green, got: ${result.failures.join('; ')}`);
  assert.ok(
    result.facts.some((fact) => /\.bin\/tsc resolves to the pinned package/.test(fact)),
    `expected the shim binding among the facts, got: ${result.facts.join('; ')}`,
  );
});

void test('a reintroduced ignoreDeprecations escape hatch is red and names the file', () => {
  assertFailedWith(
    evaluateTypeScriptAuthority(
      healthyInput({
        tsconfigs: [
          { file: 'tsconfig.json', ignoreDeprecations: undefined },
          { file: 'tsconfig.test.json', ignoreDeprecations: '6.0' },
        ],
      }),
    ),
    /tsconfig\.test\.json sets "ignoreDeprecations"/,
  );
});

void test('an unreadable tsconfig is red, and is not counted as clean', () => {
  // "did not run" must never look like "passed".
  const result = evaluateTypeScriptAuthority(
    healthyInput({
      tsconfigs: [
        { file: 'tsconfig.json', ignoreDeprecations: undefined },
        { file: 'tsconfig.test.json', ignoreDeprecations: undefined, error: 'ENOENT' },
      ],
    }),
  );
  assertFailedWith(result, /tsconfig\.test\.json could not be read/);
  assertFailedWith(result, /state is therefore unknown, not clean/);
  // Only the readable one may be counted in the green-run evidence.
  assert.ok(
    result.facts.some((fact) => /^1 tsconfig authority files/.test(fact)),
    `expected the fact line to count only readable files, got ${JSON.stringify(result.facts)}`,
  );
});

void test('every drifted fact is reported, not just the first', () => {
  const result = evaluateTypeScriptAuthority(
    healthyInput({
      declaredRange: '^6.0.3',
      lockEntry: {
        version: '6.0.3',
        resolved: 'https://registry.npmjs.org/typescript/-/typescript-6.0.3.tgz',
      },
      installedVersion: '6.0.3',
      binaryVersionOutput: 'Version 6.0.3\n',
    }),
  );
  assert.equal(result.ok, false);
  // A wholesale slide back to TS6 is internally consistent — the binary
  // agrees with its package, so check 4 stays quiet. The declared range,
  // the lockfile, and the installed package must each still name it.
  for (const pattern of [
    /declared typescript range/,
    /package-lock\.json resolves typescript/,
    /installed typescript is/,
  ]) {
    assertFailedWith(result, pattern);
  }
});

void test('the real extension tree satisfies the compiler-authority invariants', () => {
  const result = checkTypeScriptAuthority(EXTENSION_ROOT);
  assert.equal(result.ok, true, JSON.stringify(result.failures, null, 2));
});

void test('a drifted tree on disk goes red through the real file-reading path', () => {
  // The pure-evaluator cases above prove the invariants; this one proves the
  // gathering path actually reaches them — that a TS6 range in a real
  // package.json, an aliased lockfile entry, and an ignoreDeprecations in a
  // real (commented) tsconfig are read off disk rather than assumed clean.
  const drifted = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-ts-authority-'));
  try {
    fs.writeFileSync(
      path.join(drifted, 'package.json'),
      JSON.stringify({ devDependencies: { typescript: '^6.0.3' } }),
    );
    fs.writeFileSync(
      path.join(drifted, 'package-lock.json'),
      JSON.stringify({
        packages: {
          'node_modules/typescript': {
            name: 'typescript6',
            version: '6.0.3',
            resolved: 'https://registry.npmjs.org/typescript6/-/typescript6-6.0.3.tgz',
          },
        },
      }),
    );
    const result = checkTypeScriptAuthority(drifted);
    assertFailedWith(result, /floors at major 6/);
    assertFailedWith(result, /aliases node_modules\/typescript to "typescript6"/);
    // No node_modules under the scratch root: an absent compiler is red, and
    // is never mistaken for a clean one. Because effective tsconfig options are
    // read by running `tsc`, an absent compiler also means every config's
    // state is unknown — which must be reported, not assumed clean.
    assertFailedWith(result, /is it installed/);
    assertFailedWith(result, /node_modules\/\.bin\/tsc is missing/);
    assertFailedWith(result, /could not be read as a TypeScript configuration/);
    assert.ok(
      !result.facts.some((fact) => /tsconfig authority files carry no/.test(fact)),
      `no config was readable, so none may be reported clean: ${JSON.stringify(result.facts)}`,
    );
  } finally {
    fs.rmSync(drifted, { recursive: true, force: true });
  }
});

void test('an ignoreDeprecations inherited through extends is caught', () => {
  // The hole this closes: four of the five authority configs `extends` another,
  // so a base config carrying the escape hatch would be applied by `tsc` while
  // every individual file's own `compilerOptions` still looked clean. Reading
  // effective options via `tsc --showConfig` is what makes this red.
  //
  // Run against a copy of the real extension root so the installed compiler and
  // node_modules are present; only the config graph is drifted.
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-ts-extends-'));
  const root = path.join(scratch, 'ext');
  fs.mkdirSync(root);
  try {
    for (const entry of ['package.json', 'package-lock.json', ...TSCONFIG_FILES]) {
      fs.copyFileSync(path.join(EXTENSION_ROOT, entry), path.join(root, entry));
    }
    // A directory junction avoids requiring elevated symlink privileges on Windows.
    fs.symlinkSync(
      path.join(EXTENSION_ROOT, 'node_modules'),
      path.join(root, 'node_modules'),
      process.platform === 'win32' ? 'junction' : 'dir',
    );
    fs.mkdirSync(path.join(root, 'src'), { recursive: true });
    fs.writeFileSync(path.join(root, 'src', 'index.ts'), 'export const ok = 1;\n');

    // Sanity: the copied tree is clean before the drift, so the assertion below
    // cannot pass for an unrelated reason.
    assert.equal(
      checkTypeScriptAuthority(root).ok,
      true,
      'the copied tree should be clean before the extends drift is introduced',
    );

    // Push the escape hatch into a NEW base that tsconfig.json extends. No
    // authority file's own compilerOptions mentions it.
    fs.writeFileSync(
      path.join(root, 'tsconfig.base.json'),
      JSON.stringify({ compilerOptions: { ignoreDeprecations: '6.0' } }),
    );
    const rootConfig = JSON.parse(
      fs.readFileSync(path.join(root, 'tsconfig.json'), 'utf8').replace(/^\s*\/\/.*$/gm, ''),
    );
    rootConfig.extends = './tsconfig.base.json';
    fs.writeFileSync(path.join(root, 'tsconfig.json'), JSON.stringify(rootConfig));

    const result = checkTypeScriptAuthority(root);
    assertFailedWith(result, /sets "ignoreDeprecations"/);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

void test('the declared authority major is the one this gate enforces', () => {
  assert.equal(TYPESCRIPT_AUTHORITY_MAJOR, 7);
  assert.equal(TSCONFIG_FILES.length, 5);
});

/**
 * A package with a relocated declared compiler and a misleading legacy entrypoint.
 * The declared compiler reads the fixture configs; the old path always reports the
 * initial clean options. These fixtures test process routing, not TS semantics.
 *
 * @param {import('node:test').TestContext} context
 */
function relocatedCompilerFixture(context) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl lsp declared compiler '));
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const compilerRoot = path.join(root, 'node_modules', 'typescript');
  fs.mkdirSync(path.join(compilerRoot, 'cli'), { recursive: true });
  fs.mkdirSync(path.join(compilerRoot, 'bin'), { recursive: true });
  fs.mkdirSync(path.join(root, 'node_modules', '.bin'), { recursive: true });
  fs.mkdirSync(path.join(root, 'scripts'));

  const inventory = JSON.parse(
    fs.readFileSync(
      path.join(EXTENSION_ROOT, 'scripts', 'typescript-config-inventory.json'),
      'utf8',
    ),
  );
  fs.writeFileSync(
    path.join(root, 'scripts', 'typescript-config-inventory.json'),
    JSON.stringify(inventory),
  );
  fs.writeFileSync(
    path.join(root, 'package.json'),
    JSON.stringify({
      devDependencies: { typescript: '7.0.2' },
      scripts: {
        'typecheck:all': inventory.configs
          .map((config) => `npm run ${config.command}`)
          .join(' && '),
      },
    }),
  );
  fs.writeFileSync(
    path.join(root, 'package-lock.json'),
    JSON.stringify({
      packages: {
        'node_modules/typescript': {
          ...healthyInput().lockEntry,
          integrity: 'sha512-fixture',
        },
      },
    }),
  );
  const manifest = path.join(compilerRoot, 'package.json');
  fs.writeFileSync(
    manifest,
    JSON.stringify({ version: '7.0.2', bin: { tsc: 'cli/compiler.cjs' } }),
  );
  const cleanConfigs = Object.fromEntries(
    inventory.configs.map((config) => [
      config.path,
      {
        compilerOptions: {
          noEmit: !config.emit,
          outDir: config.outDir,
          rootDir: config.rootDir,
        },
      },
    ]),
  );
  for (const [file, config] of Object.entries(cleanConfigs)) {
    fs.writeFileSync(path.join(root, file), JSON.stringify(config));
  }
  const declaredEntry = path.join(compilerRoot, 'cli', 'compiler.cjs');
  fs.writeFileSync(
    declaredEntry,
    "const fs = require('node:fs');\n" +
      "if (process.argv.includes('--version')) process.stdout.write('Version 7.0.2\\n');\n" +
      "else process.stdout.write(fs.readFileSync(process.argv[process.argv.indexOf('-p') + 1], 'utf8'));\n",
  );
  const legacyEntry = path.join(compilerRoot, 'bin', 'tsc');
  fs.writeFileSync(
    legacyEntry,
    `const configs = ${JSON.stringify(cleanConfigs)};\n` +
      "process.stdout.write(JSON.stringify(configs[process.argv[process.argv.indexOf('-p') + 1]]));\n",
  );
  if (process.platform === 'win32') {
    // Relocated Windows text shims remain unsupported. Config-specific
    // assertions below must not count that independent rejection as proof.
    fs.writeFileSync(
      path.join(root, 'node_modules', '.bin', 'tsc.cmd'),
      '@ECHO off\r\nnode "%~dp0%\\..\\typescript\\cli\\compiler.cjs" %*\r\n',
    );
  } else {
    fs.symlinkSync(
      '../typescript/cli/compiler.cjs',
      path.join(root, 'node_modules', '.bin', 'tsc'),
    );
  }
  return { root, manifest, declaredEntry, legacyEntry };
}

void test('effective config probes use a relocated declared compiler without a legacy bin/tsc', (t) => {
  const fixture = relocatedCompilerFixture(t);
  fs.unlinkSync(fixture.legacyEntry);
  const authority = checkTypeScriptAuthority(fixture.root);
  const configFailures = authority.failures.filter((failure) =>
    TSCONFIG_FILES.some((file) => failure.startsWith(file)),
  );
  assert.deepEqual(configFailures, [], JSON.stringify(authority.failures));
  assert.ok(authority.facts.some((fact) => fact.includes('5 tsconfig authority files')));
  if (process.platform !== 'win32') assert.equal(authority.ok, true);
  const inventory = checkTypeScriptConfigInventory(fixture.root);
  assert.equal(inventory.ok, true, JSON.stringify(inventory.failures));
});

void test('a stale legacy compiler cannot hide effective deprecation or emit drift', (t) => {
  const fixture = relocatedCompilerFixture(t);
  const configPath = path.join(fixture.root, 'tsconfig.test.json');
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  config.compilerOptions.ignoreDeprecations = '6.0';
  config.compilerOptions.noEmit = true;
  fs.writeFileSync(configPath, JSON.stringify(config));
  assertFailedWith(
    checkTypeScriptAuthority(fixture.root),
    /tsconfig\.test\.json sets "ignoreDeprecations"/,
  );
  assertFailedWith(
    checkTypeScriptConfigInventory(fixture.root),
    /tsconfig\.test\.json inventory emit=true.*noEmit=true/,
  );
});

void test('missing and broken declared entrypoints never fall back to the legacy compiler', (t) => {
  for (const bin of [undefined, { tsc: 'cli/missing.cjs' }]) {
    const fixture = relocatedCompilerFixture(t);
    fs.writeFileSync(fixture.manifest, JSON.stringify({ version: '7.0.2', bin }));
    assertFailedWith(
      checkTypeScriptAuthority(fixture.root),
      /tsconfig\.json could not be read as a TypeScript configuration/,
    );
    assertFailedWith(
      checkTypeScriptConfigInventory(fixture.root),
      /tsconfig\.json effective configuration could not be read/,
    );
  }
});
