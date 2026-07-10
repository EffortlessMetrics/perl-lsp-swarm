#!/usr/bin/env node

/**
 * Thin wrapper around the Jest CLI that remaps developer-facing `src/**\/*.ts`
 * test paths to their compiled `out-test/**\/*.js` counterparts before handing
 * off to Jest.
 *
 * jest.config.js runs Jest directly against tsc's compile-ahead output
 * (`roots: ['<rootDir>/out-test/test']`, `transform: {}` — no ts-jest, no
 * Babel/SWC/esbuild). Jest itself never sees a `.ts` path. Without this
 * wrapper, a targeted run such as
 *   npm test -- --runTestsByPath src/test/commands.test.ts
 * silently matches nothing: `--runTestsByPath` compares literal file paths,
 * and the source path was never on Jest's radar.
 *
 * This script only rewrites arguments that look like a filesystem path under
 * `src/` (relative or absolute, with or without a `.ts` extension). Flags
 * (`--ci`, `--coverage`, `--verbose`, ...) and any other positional argument
 * (patterns, already-compiled `out-test/` paths) pass through untouched, so
 * `npm test -- --runTestsByPath src/test/commands.test.ts` keeps working
 * exactly as before the ts-jest removal — now against the emitted JS, with
 * coverage and failure locations still mapped back to the .ts via the inline
 * source maps `tsconfig.test.json` emits.
 */

const path = require('path');
const { spawnSync } = require('child_process');

const ROOT = path.join(__dirname, '..');
const SRC_DIR = path.join(ROOT, 'src');
const OUT_TEST_DIR = path.join(ROOT, 'out-test');

function mapSourcePathToCompiled(arg) {
  if (!arg || arg.startsWith('-')) {
    return arg;
  }

  const absolute = path.isAbsolute(arg) ? arg : path.resolve(ROOT, arg);
  const relativeToSrc = path.relative(SRC_DIR, absolute);

  // Not under src/ — e.g. already an out-test/ path, or an unrelated
  // positional argument such as a testNamePattern. Leave it alone.
  if (relativeToSrc.startsWith('..') || path.isAbsolute(relativeToSrc)) {
    return arg;
  }

  const compiledRelative = relativeToSrc.endsWith('.ts')
    ? `${relativeToSrc.slice(0, -3)}.js`
    : relativeToSrc;

  return path.join(OUT_TEST_DIR, compiledRelative);
}

const jestArgs = process.argv.slice(2).map(mapSourcePathToCompiled);

// Resolve Jest's own CLI entry point and invoke it with `node` directly,
// rather than shelling out through `npx`/`npx.cmd` — the latter requires
// `shell: true` on Windows (a quoting hazard) just to launch a .cmd shim.
const jestBin = require.resolve('jest/bin/jest', { paths: [ROOT] });

const result = spawnSync(process.execPath, [jestBin, ...jestArgs], {
  stdio: 'inherit',
  cwd: ROOT,
});

if (result.error) {
  console.error('Failed to launch Jest:', result.error.message);
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
