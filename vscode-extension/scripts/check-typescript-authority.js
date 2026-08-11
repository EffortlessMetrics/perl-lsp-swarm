#!/usr/bin/env node
'use strict';

/**
 * TypeScript 7 compiler-authority gate (#3662 follow-through).
 *
 * `DEVELOPMENT.md` and the migration index both claim "TypeScript 7 is the
 * sole type-check authority". Nothing enforced that claim: the compiler-swap
 * receipt proved it once, by hand, at one commit.
 *
 * The failure mode this closes is precise, and the receipt itself documents
 * why it is invisible. TS6 and TS7 emitted byte-identical `.js` for this
 * tree, and all five tsconfigs compile clean under both. So if the resolved
 * `typescript` ever slid back to a 6.x — a hand-edited range, a `npm:` alias
 * or shim reintroduced during a dependency repair, a regenerated lockfile
 * resolving differently — every check in CI would still pass green. There is
 * no observable signal today, which makes it exactly the shape of invariant
 * `lint-canary.js` already makes blocking for type-aware Oxlint.
 *
 * This script makes the compiler's identity observable and blocking. It
 * asserts, against the real tree rather than a remembered measurement:
 *
 *   1. `package.json`'s declared `typescript` devDependency is a plain
 *      registry semver range whose floor major is the authority major — not
 *      an alias, tarball, git, or `file:` specifier;
 *   2. the lockfile resolves `node_modules/typescript` to a real registry
 *      `typescript` tarball at that major, with no alias `name` redirect;
 *   3. the installed package reports that major;
 *   4. the installed compiler binary itself reports the same version the
 *      package claims (a package.json version field is metadata; running it
 *      is evidence), and the `.bin/tsc` shim the npm scripts invoke exists;
 *   5. no tsconfig's EFFECTIVE options carry `ignoreDeprecations`, the TS6-era
 *      deprecation escape hatch the swap deliberately removed. TS7 tolerates
 *      it rather than rejecting it, so its return would otherwise be silent.
 *      Effective, not declared: four of the five authority configs `extends`
 *      another, so reading each file's own `compilerOptions` would miss one
 *      introduced in a shared base while `tsc` still applied it.
 *
 * Adopting a new compiler major is a deliberate act: bump
 * `TYPESCRIPT_AUTHORITY_MAJOR` in the same change that bumps the dependency.
 * A gate that derived its expectation from the dependency it is checking
 * would accept any downgrade it was handed.
 */

const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { createReporter } = require('./reporter');

/**
 * The compiler major this repository declares as its type-check authority.
 * Changing this is how a new TypeScript major is adopted; it is not derived
 * from `package.json`, because the point of the gate is to disagree with a
 * `package.json` that drifted.
 */
const TYPESCRIPT_AUTHORITY_MAJOR = 7;

/** Tsconfigs that are TypeScript authority surfaces for this extension. */
const TSCONFIG_FILES = [
  'tsconfig.json',
  'tsconfig.test.json',
  'tsconfig.integration.json',
  'tsconfig.published-smoke.json',
  'tsconfig.scripts.json',
];

/**
 * @typedef {object} AuthorityInput
 * @property {number} expectedMajor
 * @property {string | undefined} declaredRange devDependencies.typescript
 * @property {{version?: string, resolved?: string, name?: string} | undefined} lockEntry
 *   The `node_modules/typescript` entry from package-lock.json.
 * @property {string | undefined} installedVersion node_modules/typescript's own version.
 * @property {string | undefined} binaryVersionOutput Raw `tsc --version` stdout.
 * @property {boolean} binShimPresent Whether node_modules/.bin/tsc exists.
 * @property {Array<{file: string, ignoreDeprecations: unknown, error?: string}>} tsconfigs
 *   One entry per authority tsconfig. `error` is set when the file could not be
 *   read or parsed, in which case its `ignoreDeprecations` state is unknown.
 */

/**
 * @typedef {object} AuthorityResult
 * @property {boolean} ok
 * @property {string[]} facts Evidence worth printing on a green run.
 * @property {string[]} failures
 */

/**
 * Parses the leading major of a semver-ish string.
 *
 * @param {string} value
 * @returns {number | null}
 */
function leadingMajor(value) {
  const match = /(\d+)\.\d+\.\d+/.exec(value);
  return match ? Number(match[1]) : null;
}

/**
 * Decides whether a declared dependency range is a plain registry semver
 * range, and what major it floors at.
 *
 * Anything carrying a protocol (`npm:`, `file:`, `git+…`, `http…`) is rejected
 * outright rather than parsed: an alias is precisely the shim shape the
 * migration ruled out, and a range whose floor cannot be read cannot be gated.
 *
 * A union (`||`) is rejected even when its first term floors correctly, because
 * the later terms can admit a different major — `^6.0.3 || ^7.0.2` would
 * install either compiler depending on what else is in the tree, which is
 * exactly the ambiguity this gate exists to remove.
 *
 * Otherwise the leading comparator and version are read and trailing range
 * terms are allowed, so a legitimate bounded range like `>=7.0.2 <8.0.0` is
 * gated on its floor rather than rejected for its shape. Being stricter than
 * npm here would produce a confusing red on a valid config, which costs the
 * gate the credibility its real failures depend on.
 *
 * @param {string} range
 * @returns {{major: number} | {reason: string}}
 */
function readRangeFloor(range) {
  const trimmed = range.trim();
  if (/^[a-z+]+:/i.test(trimmed) || /^(git|https?)/i.test(trimmed)) {
    return {
      reason: `must be a plain registry semver range, received the non-registry specifier "${trimmed}"`,
    };
  }
  if (trimmed.includes('||')) {
    return {
      reason: `must resolve to a single compiler major, received the union range "${trimmed}"`,
    };
  }
  const match = /^(?:\^|~|>=|=|v)?\s*(\d+)\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?(?=$|\s)/.exec(trimmed);
  if (!match) {
    return {
      reason: `must begin with an exact or ^/~/>= pinned semver version (e.g. "^${TYPESCRIPT_AUTHORITY_MAJOR}.0.0"), received "${trimmed}"`,
    };
  }
  return { major: Number(match[1]) };
}

/**
 * Evaluates the compiler-authority invariants against already-gathered facts.
 *
 * Kept free of I/O so the invariants can be proven against synthetic drifted
 * trees in `check-typescript-authority.test.js` rather than only against
 * whatever happens to be installed.
 *
 * @param {AuthorityInput} input
 * @returns {AuthorityResult}
 */
function evaluateTypeScriptAuthority(input) {
  const expected = input.expectedMajor;
  /** @type {string[]} */
  const failures = [];
  /** @type {string[]} */
  const facts = [];

  // 1. Declared range.
  if (typeof input.declaredRange !== 'string' || input.declaredRange.length === 0) {
    failures.push('package.json declares no `typescript` devDependency');
  } else {
    const floor = readRangeFloor(input.declaredRange);
    if ('reason' in floor) {
      failures.push(`declared typescript range ${floor.reason}`);
    } else if (floor.major !== expected) {
      failures.push(
        `declared typescript range "${input.declaredRange}" floors at major ${floor.major}, not the authority major ${expected}`,
      );
    } else {
      facts.push(`declared range "${input.declaredRange}" (floor major ${floor.major})`);
    }
  }

  // 2. Lockfile resolution.
  if (!input.lockEntry) {
    failures.push('package-lock.json has no `node_modules/typescript` entry');
  } else {
    const { version, resolved, name } = input.lockEntry;
    if (typeof name === 'string' && name !== 'typescript') {
      failures.push(
        `package-lock.json aliases node_modules/typescript to "${name}" — the migration rules out aliases and shims`,
      );
    }
    if (typeof resolved !== 'string' || !/\/typescript\/-\/typescript-/.test(resolved)) {
      failures.push(
        `package-lock.json resolves node_modules/typescript to "${String(resolved)}", which is not a registry \`typescript\` tarball`,
      );
    }
    const lockMajor = typeof version === 'string' ? leadingMajor(version) : null;
    if (lockMajor === null) {
      failures.push(
        `package-lock.json records an unreadable typescript version "${String(version)}"`,
      );
    } else if (lockMajor !== expected) {
      failures.push(
        `package-lock.json resolves typescript ${String(version)} (major ${lockMajor}), not the authority major ${expected}`,
      );
    } else {
      facts.push(`lockfile resolves typescript ${String(version)} from the npm registry`);
    }
  }

  // 3. Installed package.
  const installedMajor =
    typeof input.installedVersion === 'string' ? leadingMajor(input.installedVersion) : null;
  if (installedMajor === null) {
    failures.push(
      `node_modules/typescript reports an unreadable version "${String(input.installedVersion)}" — is it installed?`,
    );
  } else if (installedMajor !== expected) {
    failures.push(
      `installed typescript is ${String(input.installedVersion)} (major ${installedMajor}), not the authority major ${expected}`,
    );
  }

  // 4. The binary that actually type-checks.
  if (typeof input.binaryVersionOutput !== 'string') {
    failures.push('could not run the installed `tsc --version`');
  } else {
    const binaryVersion = /Version\s+(\d+\.\d+\.\d+[^\s]*)/.exec(input.binaryVersionOutput);
    if (!binaryVersion) {
      failures.push(
        `\`tsc --version\` produced no readable version: ${JSON.stringify(input.binaryVersionOutput.trim())}`,
      );
    } else if (binaryVersion[1] !== input.installedVersion) {
      failures.push(
        `\`tsc --version\` reports ${String(binaryVersion[1])} but node_modules/typescript declares ${String(input.installedVersion)} — the compiler that runs is not the package that is pinned`,
      );
    } else {
      facts.push(`\`tsc --version\` reports ${String(binaryVersion[1])} from the pinned package`);
    }
  }
  if (!input.binShimPresent) {
    failures.push(
      'node_modules/.bin/tsc is missing — the npm typecheck scripts would resolve some other tsc',
    );
  }

  // 5. No TS6-era deprecation escape hatch.
  //
  // A tsconfig that cannot be read or parsed is a failure in its own right, not
  // a reason to crash: an unreadable authority file means this check did not
  // run against it, and "did not run" must never look like "passed".
  let readableTsconfigs = 0;
  for (const tsconfig of input.tsconfigs) {
    if (tsconfig.error) {
      failures.push(
        `${tsconfig.file} could not be read as a TypeScript configuration (${tsconfig.error}) — its \`ignoreDeprecations\` state is therefore unknown, not clean`,
      );
      continue;
    }
    readableTsconfigs += 1;
    if (tsconfig.ignoreDeprecations !== undefined) {
      failures.push(
        `${tsconfig.file} sets "ignoreDeprecations": ${JSON.stringify(tsconfig.ignoreDeprecations)} — a TS6-era escape hatch removed by the TS7 swap. TypeScript ${expected} tolerates it rather than rejecting it, so its return is otherwise silent.`,
      );
    }
  }
  if (readableTsconfigs > 0) {
    facts.push(`${readableTsconfigs} tsconfig authority files carry no \`ignoreDeprecations\``);
  }

  return { ok: failures.length === 0, facts, failures };
}

/**
 * Gathers the real facts from the extension tree and evaluates them.
 *
 * @param {string} extensionRoot
 * @returns {AuthorityResult}
 */
function checkTypeScriptAuthority(extensionRoot) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));
  const lockJson = JSON.parse(
    fs.readFileSync(path.join(extensionRoot, 'package-lock.json'), 'utf8'),
  );
  const typescriptDir = path.join(extensionRoot, 'node_modules', 'typescript');

  /** @type {string | undefined} */
  let installedVersion;
  try {
    installedVersion = JSON.parse(
      fs.readFileSync(path.join(typescriptDir, 'package.json'), 'utf8'),
    ).version;
  } catch {
    installedVersion = undefined;
  }

  // Invoke the package's own JS entry point through `process.execPath` rather
  // than the generated .bin shim: on Windows a bare `.cmd` EINVALs without
  // `shell: true`, and `shell: true` concatenates arguments instead of
  // escaping them. Same reasoning as lint-canary.js. The shim's presence is
  // asserted separately so a broken shim is still a red result.
  /** @type {string | undefined} */
  let binaryVersionOutput;
  try {
    binaryVersionOutput = execFileSync(
      process.execPath,
      [path.join(typescriptDir, 'bin', 'tsc'), '--version'],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
    );
  } catch {
    binaryVersionOutput = undefined;
  }

  // Read each config's EFFECTIVE options via `tsc --showConfig`, not its own
  // `compilerOptions` block. Four of the five authority configs `extends`
  // another, so an `ignoreDeprecations` introduced in a shared base would be
  // applied by `tsc` while every individual file still looked clean — the gate
  // would report green on exactly the regression it exists to block.
  //
  // The compiler's own resolver is the oracle rather than a hand-rolled
  // `extends` walk: it already handles relative paths, package references, and
  // arrays of bases, and it is by definition what `tsc` will actually apply.
  //
  // A config that is missing, unparseable, or otherwise rejected is reported as
  // its own named failure rather than thrown: an uncaught stack trace would
  // still exit nonzero, but it would bury the actionable fact under a crash and
  // make a routine editing mistake look like a broken gate.
  const tsconfigs = TSCONFIG_FILES.map((file) => {
    try {
      const shown = execFileSync(
        process.execPath,
        [path.join(typescriptDir, 'bin', 'tsc'), '--showConfig', '-p', file],
        { cwd: extensionRoot, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
      );
      const parsed = JSON.parse(shown);
      return { file, ignoreDeprecations: parsed?.compilerOptions?.ignoreDeprecations };
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return {
        file,
        ignoreDeprecations: undefined,
        error: message.split(/\r?\n/, 1)[0] ?? message,
      };
    }
  });

  return evaluateTypeScriptAuthority({
    expectedMajor: TYPESCRIPT_AUTHORITY_MAJOR,
    declaredRange: packageJson.devDependencies?.typescript,
    lockEntry: lockJson.packages?.['node_modules/typescript'],
    installedVersion,
    binaryVersionOutput,
    binShimPresent:
      fs.existsSync(path.join(extensionRoot, 'node_modules', '.bin', 'tsc')) ||
      fs.existsSync(path.join(extensionRoot, 'node_modules', '.bin', 'tsc.cmd')),
    tsconfigs,
  });
}

function main() {
  const reporter = createReporter('typescript-authority');
  const result = checkTypeScriptAuthority(path.resolve(__dirname, '..'));

  if (!result.ok) {
    for (const failure of result.failures) {
      reporter.error(`FAIL: ${failure}`);
    }
    reporter.error(
      `TypeScript ${TYPESCRIPT_AUTHORITY_MAJOR} is this extension's sole type-check authority. ` +
        'A red result here means the compiler that actually runs is not the one the repository ' +
        'claims — which every other check would pass green through, because TS6 and TS7 compile ' +
        'and emit identically for this tree. See scripts/check-typescript-authority.js.',
    );
    process.exitCode = 1;
    return;
  }

  for (const fact of result.facts) {
    reporter.info(`OK  ${fact}`);
  }
  reporter.info(
    `PASS — TypeScript ${TYPESCRIPT_AUTHORITY_MAJOR} is the resolved, installed, and executing compiler.`,
  );
}

if (require.main === module) {
  main();
}

module.exports = {
  TYPESCRIPT_AUTHORITY_MAJOR,
  TSCONFIG_FILES,
  evaluateTypeScriptAuthority,
  checkTypeScriptAuthority,
};
