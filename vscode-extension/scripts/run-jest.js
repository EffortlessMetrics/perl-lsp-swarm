#!/usr/bin/env node

/**
 * Thin wrapper around the Jest CLI that remaps `--runTestsByPath` operands
 * under `src/` to their compiled `out-test/` counterparts before handing off
 * to Jest.
 *
 * jest.config.js runs Jest directly against tsc's compile-ahead output
 * (`roots: ['<rootDir>/out-test/test']`, `transform: {}` — no ts-jest, no
 * Babel/SWC/esbuild). Jest itself never sees a `.ts` path. Without this
 * wrapper, a targeted run such as
 *   npm test -- --runTestsByPath src/test/commands.test.ts
 * silently matches nothing: `--runTestsByPath` compares literal file paths,
 * and the source path was never on Jest's radar.
 *
 * This script only rewrites arguments in `--runTestsByPath` operand
 * position — both the `--runTestsByPath <path>` and
 * `--runTestsByPath=<path>` forms, and any further bare paths immediately
 * following the flag (Jest allows multiple). It does not rewrite arbitrary
 * `src/`-prefixed tokens: an unrelated option value that happens to start
 * with `src/` (e.g. `--testNamePattern src/test`) passes through untouched,
 * because it never occupies `--runTestsByPath` operand position. So
 * `npm test -- --runTestsByPath src/test/commands.test.ts` keeps working
 * exactly as before the ts-jest removal — now against the emitted JS, with
 * coverage and failure locations still mapped back to the .ts via the inline
 * source maps `tsconfig.test.json` emits.
 */

const path = require('path');

const ROOT = path.join(__dirname, '..');
const SRC_DIR = path.join(ROOT, 'src');
const OUT_TEST_DIR = path.join(ROOT, 'out-test');
const RUN_TESTS_BY_PATH_FLAG = '--runTestsByPath';

function mapSourcePathToCompiled(arg) {
  if (!arg) {
    return arg;
  }

  const absolute = path.isAbsolute(arg) ? arg : path.resolve(ROOT, arg);
  const relativeToSrc = path.relative(SRC_DIR, absolute);

  // Not under src/ — e.g. already an out-test/ path. Leave it alone.
  if (relativeToSrc.startsWith('..') || path.isAbsolute(relativeToSrc)) {
    return arg;
  }

  return path.join(OUT_TEST_DIR, withCompiledExtension(relativeToSrc));
}

function withCompiledExtension(relativePath) {
  if (relativePath.endsWith('.ts')) {
    return `${relativePath.slice(0, -3)}.js`;
  }
  if (relativePath.endsWith('.js')) {
    return relativePath;
  }
  // No recognized extension (e.g. `src/test/commands.test`, matching how
  // some Jest CLI flags accept an extension-less path) — tsc always emits
  // `.js`, so that is the only compiled artifact that could exist.
  return `${relativePath}.js`;
}

// Rewrite `--runTestsByPath` operands only — everything else (flags, other
// option values, already-compiled paths) passes through untouched. This is
// deliberately narrower than "any src/-prefixed argument": an option value
// that happens to start with `src/` but isn't a --runTestsByPath operand
// (e.g. `--testNamePattern src/test`) must not be treated as a file path.
//
// `--runTestsByPath=<path>` is split into the two-token `--runTestsByPath
// <path>` form rather than reassembled as `--runTestsByPath=<mapped>`:
// Jest's own CLI (yargs, `runTestsByPath` is boolean-typed) does not treat
// the `=value` form as a path operand — verified directly against
// `node_modules/jest/bin/jest.js` on this HEAD, where
// `--runTestsByPath=out-test/test/commands.test.js` silently runs the
// entire suite ("Ran all test suites"), while
// `--runTestsByPath out-test/test/commands.test.js` correctly scopes to
// that one file ("Ran all test suites within paths ..."). Splitting to the
// two-token form is what makes the `=` syntax actually work end to end.
function remapRunTestsByPathArgs(argv) {
  const result = [];
  let collectingPaths = false;

  for (const arg of argv) {
    const equalsForm = arg.startsWith(`${RUN_TESTS_BY_PATH_FLAG}=`);
    if (equalsForm) {
      const value = arg.slice(RUN_TESTS_BY_PATH_FLAG.length + 1);
      result.push(RUN_TESTS_BY_PATH_FLAG, mapSourcePathToCompiled(value));
      collectingPaths = true;
      continue;
    }

    if (arg === RUN_TESTS_BY_PATH_FLAG) {
      result.push(arg);
      collectingPaths = true;
      continue;
    }

    if (collectingPaths && !arg.startsWith('-')) {
      result.push(mapSourcePathToCompiled(arg));
      continue;
    }

    // Any other flag ends path collection — its own value (if any) is not a
    // --runTestsByPath operand and must not be remapped.
    if (arg.startsWith('-')) {
      collectingPaths = false;
    }

    result.push(arg);
  }

  return result;
}

const jestArgs = remapRunTestsByPathArgs(process.argv.slice(2));

// Run Jest's own CLI entry point in-process (require it directly) rather
// than spawning it as a child process. Two reasons:
//   - `npx`/`npx.cmd` needs `shell: true` on Windows just to reach the
//     .cmd shim — a quoting hazard avoided entirely by not shelling out.
//   - `spawnSync` would add an extra process layer between this wrapper
//     and Jest's own worker pool (jest-worker forks its own child
//     processes for parallel test execution). That extra nesting caused
//     worker forks to fail intermittently in CI ("Jest worker encountered
//     N child process exceptions, exceeding retry limit") — a regression
//     introduced by wrapping Jest in a subprocess at all. Requiring
//     Jest's CLI entry directly makes this script's process the same
//     top-level process Jest's workers fork from, exactly as when `jest`
//     is invoked directly (no wrapper).
const jestBin = require.resolve('jest/bin/jest', { paths: [ROOT] });
process.chdir(ROOT);
process.argv = [process.argv[0], jestBin, ...jestArgs];
require(jestBin);
