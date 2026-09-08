/**
 * Contract tests for the Rolldown production bundle configuration.
 *
 * The final step of the TS7 migration (#3662): Rolldown replaces TypeScript
 * EMISSION as the production artifact builder (src/extension.ts ->
 * out/extension.js) — it does NOT type-check. TypeScript 7 (`tsc --noEmit`,
 * the `typecheck` npm script) remains the sole type-check authority.
 *
 * These tests verify that:
 *   - rolldown.config.mjs exists and is loadable
 *   - the config produces a single CJS file at the exact path package.json's
 *     "main" and the debugger's "program" both expect (out/extension.js)
 *   - `vscode` is externalized (never bundled — it's supplied by the
 *     extension host, not resolvable as a real package)
 *   - code splitting is disabled (strict single-file output; this exact
 *     regression was caught during development: a type-only `import type`
 *     produced a stray out/commandResults.js facade chunk until
 *     `codeSplitting: false` was added)
 *   - minification is off (deliberately out of scope for this first PR)
 *   - source maps are enabled
 *   - package.json's compile/typecheck scripts are correctly wired
 *   - rolldown is exactly pinned (no ^/~)
 *   - the packaged VSIX ships no node_modules/** at all (every runtime
 *     dependency is now bundled) and no dev-tooling native binaries
 */

import { execFileSync } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';

const EXT_ROOT = path.resolve(__dirname, '..', '..');

describe('Rolldown bundle configuration', () => {
  test('rolldown.config.mjs exists at extension root', () => {
    const configPath = path.join(EXT_ROOT, 'rolldown.config.mjs');
    expect(fs.existsSync(configPath)).toBe(true);
  });

  test('patches only the pinned language-client source and rejects drift', () => {
    const sourcePath = path.join(
      EXT_ROOT,
      'node_modules',
      'vscode-languageclient',
      'lib',
      'common',
      'client.js',
    );
    const script = `
      import fs from 'node:fs';
      import { patchPinnedLanguageClientSource } from './rolldown.config.mjs';
      const sourcePath = ${JSON.stringify(sourcePath)};
      const source = fs.readFileSync(sourcePath, 'utf8');
      const patched = patchPinnedLanguageClientSource(source, sourcePath);
      if (!patched || !patched.includes('return promise;')) process.exit(11);
      const start = patched.indexOf('    async start() {');
      const end = patched.indexOf('    createOnStartPromise()', start);
      const body = patched.slice(start, end);
      if ((body.match(/return this\\._onStart;/g) || []).length !== 1) process.exit(14);
      if ((body.match(/return promise;/g) || []).length !== 1) process.exit(15);
      if (patchPinnedLanguageClientSource(source, 'other-module/client.js') !== null) process.exit(12);
      let rejected = false;
      try { patchPinnedLanguageClientSource(source + '\\n', sourcePath); } catch { rejected = true; }
      if (!rejected) process.exit(13);
    `;
    execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      cwd: EXT_ROOT,
      stdio: 'pipe',
    });
  });

  test('patches the pinned jsonrpc write-failure rejection without an orphaned throw', () => {
    const sourcePath = require.resolve('vscode-jsonrpc');
    const script = `
      import fs from 'node:fs';
      import { patchPinnedJsonRpcConnectionSource } from './rolldown.config.mjs';
      const sourcePath = ${JSON.stringify(sourcePath.replace(/\\/g, '/').replace(/api\\.js$/, 'common/connection.js'))};
      const source = fs.readFileSync(sourcePath, 'utf8');
      const patched = patchPinnedJsonRpcConnectionSource(source, sourcePath);
      if (!patched) process.exit(21);
      const catchStart = patched.indexOf('catch (error) {');
      const catchEnd = patched.indexOf('        });', catchStart);
      const body = patched.slice(catchStart, catchEnd);
      if (!body.includes('responsePromise.reject(new messages_1.ResponseError')) process.exit(22);
      if (!body.includes('logger.error(\`Sending request failed.\`);')) process.exit(23);
      if (!body.includes('                    return;')) process.exit(24);
      if (body.includes('                    throw error;')) process.exit(25);
      if (patchPinnedJsonRpcConnectionSource(source, 'other-module/connection.js') !== null) process.exit(26);
      let rejected = false;
      try { patchPinnedJsonRpcConnectionSource(source + '\\n', sourcePath); } catch { rejected = true; }
      if (!rejected) process.exit(27);
    `;
    execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      cwd: EXT_ROOT,
      stdio: 'pipe',
    });
  });

  test('the transformed jsonrpc connection rejects failed writes without an unhandled rejection', () => {
    const script = `
      import Module, { createRequire } from 'node:module';
      import fs from 'node:fs';
      import { patchPinnedJsonRpcConnectionSource } from './rolldown.config.mjs';
      const require = createRequire(import.meta.url);
      const packageEntry = require.resolve('vscode-jsonrpc');
      const sourcePath = packageEntry.replace(/api\\.js$/, 'connection.js');
      const source = fs.readFileSync(sourcePath, 'utf8');
      const patched = patchPinnedJsonRpcConnectionSource(source, sourcePath);
      if (!patched) process.exit(31);
      const originalLoader = Module._extensions['.js'];
      Module._extensions['.js'] = (module, filename) => {
        if (filename === sourcePath) module._compile(patched, filename);
        else originalLoader(module, filename);
      };
      const { createMessageConnection } = await import('vscode-jsonrpc/node');
      const disposable = () => ({ dispose() {} });
      const reader = {
        onClose: disposable,
        onError: disposable,
        listen() { return disposable(); },
      };
      let unhandled = 0;
      const onUnhandled = () => { unhandled += 1; };
      process.on('unhandledRejection', onUnhandled);
      const failingWriter = {
        onClose: disposable,
        onError: disposable,
        write() { return Promise.reject(new Error('write boom')); },
        end() {},
        dispose() {},
      };
      const failing = createMessageConnection(reader, failingWriter);
      failing.listen();
      let failure;
      try { await failing.sendRequest('test/failure', {}); } catch (error) { failure = error; }
      if (!(failure instanceof Error) || !failure.message.includes('write boom')) process.exit(32);
      await new Promise((resolve) => setTimeout(resolve, 25));
      if (unhandled !== 0) process.exit(33);
      failing.dispose();

      let response;
      const successReader = {
        onClose: disposable,
        onError: disposable,
        listen(callback) {
          setTimeout(() => callback({ jsonrpc: '2.0', id: 0, result: 'ok' }), 0);
          return disposable();
        },
      };
      const successWriter = {
        onClose: disposable,
        onError: disposable,
        write() { return Promise.resolve(); },
        end() {},
        dispose() {},
      };
      const successful = createMessageConnection(successReader, successWriter);
      successful.listen();
      response = await successful.sendRequest('test/success', {});
      if (response !== 'ok') process.exit(34);
      successful.dispose();
      process.off('unhandledRejection', onUnhandled);
      Module._extensions['.js'] = originalLoader;
    `;
    execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      cwd: EXT_ROOT,
      stdio: 'pipe',
    });
  });

  test('rolldown.config.mjs targets the exact main/debugger entry path (out/extension.js)', () => {
    const configPath = path.join(EXT_ROOT, 'rolldown.config.mjs');
    const source = fs.readFileSync(configPath, 'utf8');
    expect(source).toContain("input: 'src/extension.ts'");
    expect(source).toContain("file: 'out/extension.js'");

    const pkg = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
    expect(pkg.main).toBe('./out/extension.js');
    const debugger_ = pkg.contributes?.debuggers?.find((d: { type?: string }) => d.type === 'perl');
    expect(debugger_?.program).toBe('./out/extension.js');
  });

  test('rolldown.config.mjs externalizes vscode and disables code splitting/minification', () => {
    const configPath = path.join(EXT_ROOT, 'rolldown.config.mjs');
    const source = fs.readFileSync(configPath, 'utf8');
    expect(source).toContain("id === 'vscode'");
    expect(source).toContain('codeSplitting: false');
    expect(source).toContain('minify: false');
    expect(source).toContain('sourcemap: true');
    expect(source).toContain("format: 'cjs'");
    expect(source).toContain("platform: 'node'");
  });

  test('development maps are retained while the VSIX excludes them', () => {
    const config = fs.readFileSync(path.join(EXT_ROOT, 'rolldown.config.mjs'), 'utf8');
    const vscodeIgnore = fs.readFileSync(path.join(EXT_ROOT, '.vscodeignore'), 'utf8');
    expect(config).toContain('sourcemap: true');
    expect(vscodeIgnore).toContain('**/*.map');
  });

  test('package.json prepublish verifies the toolchain, typechecks all surfaces, then compiles', () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
    // "clean:out" first: out/ is a shared build directory (the integration/
    // published-smoke test harnesses also emit into out/test/** via a
    // separate tsc step) — without cleaning stray top-level files first, a
    // leftover from an earlier build (verified: tsc -p tsconfig.integration.json
    // emitting out/commandResults.js as a type-only-import byproduct) can
    // survive a subsequent `npm run bundle` and leak into a packaged VSIX.
    expect(pkg.scripts.bundle).toBe('npm run clean:out && rolldown -c rolldown.config.mjs');
    expect(pkg.scripts['clean:out']).toContain("f!=='test'");
    // Every public TypeScript execution routes through the governed seam, so
    // the compiler-authority gate runs before any `tsc` the scripts invoke.
    expect(pkg.scripts.typecheck).toBe('node scripts/governed-tsc.js --noEmit -p ./tsconfig.json');
    // The real release/packaging path must run the checked build — authority,
    // config inventory, all-config type-check, then bundle. A bundler alone
    // cannot catch a type error.
    expect(pkg.scripts['vscode:prepublish']).toBe('npm run doctor && npm run build');
    // ...and the checked build must first establish *which* compiler is doing
    // the checking. TS6 and TS7 compile and emit identically for this tree,
    // so a slide back to the old compiler passes every `tsc` invocation green.
    expect(pkg.scripts.build).toMatch(/^npm run typecheck:authority &&/);
    expect(pkg.scripts.build.endsWith('npm run bundle')).toBe(true);
    expect(pkg.scripts['typecheck:authority']).toBe('node scripts/check-typescript-authority.js');
  });

  test('package.json has rolldown devDependency, exactly pinned (no ^/~)', () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
    expect(pkg.devDependencies).toHaveProperty('rolldown');
    expect(pkg.devDependencies.rolldown).toMatch(/^\d+\.\d+\.\d+$/);
  });

  test('package and lockfile declare the Node 26 toolchain authority', () => {
    const pkg = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
    const lock = JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package-lock.json'), 'utf8'));

    expect(pkg.engines.node).toBe('>=26.0.0 <27');
    expect(pkg.engines.npm).toBe('11.18.0');
    expect(pkg.packageManager).toBe('npm@11.18.0');
    expect(pkg.devDependencies['@types/node']).toMatch(/^\^26(?:\.|$)/);
    expect(lock.packages[''].engines).toEqual(pkg.engines);
  });
});

describe('VSIX packaging ships a single bundled artifact and no raw node_modules', () => {
  // `vsce ls` is the packaging tool's own dry-run manifest of every file that
  // would be included in the published .vsix. Oxfmt/Oxlint/Rolldown all ship
  // native platform bindings via optionalDependencies under devDependencies
  // — vsce is expected to exclude devDependencies-only packages from the
  // package, but that exclusion is vsce's dependency-analysis behavior, not
  // this repo's own code. A future change that moves one of them to
  // `dependencies` by mistake, or an .vscodeignore edit that accidentally
  // widens node_modules inclusion, would silently bundle a multi-megabyte
  // native binary into the published extension with no other signal. This
  // test asserts against the real packaging manifest, not just config
  // intent, so that regression is caught here instead of in a published
  // release.
  //
  // Since the Rolldown production bundle inlines every runtime dependency
  // (adm-zip, tar, vscode-languageclient — verified pure JS, no
  // __dirname-relative asset loading, no native .node bindings anywhere in
  // their transitive trees) into the single out/extension.js artifact,
  // node_modules/** is excluded from the VSIX entirely (see
  // .vscodeignore). These assertions are stricter than the pre-Rolldown
  // version (which only checked for dev-tooling names): no node_modules/**
  // entries of ANY kind are expected now.
  //
  // `vsce ls` packages whatever is currently on disk under out/ — unlike
  // `vsce package`, it does NOT run the "vscode:prepublish" script itself.
  // Nothing else in the jest pipeline (compile:test only rebuilds
  // out-test/, a separate directory) guarantees out/extension.js exists.
  // Build it explicitly here so this suite is self-sufficient regardless
  // of what ran before it (a bare `npm test`, a CI job ordering change,
  // etc.) and reflects the current source, not a stale leftover build.
  beforeAll(() => {
    // Replicate the "bundle" npm script (clean:out + rolldown) via direct
    // `node` invocations rather than `npm.cmd`/`npx` — npm's own Windows
    // .cmd shim EINVALs under spawnSync without shell:true, and shell:true
    // string-concatenates args instead of escaping them (the same class of
    // issue already fixed in scripts/lint-canary.js during PREP-2).
    const outDir = path.join(EXT_ROOT, 'out');
    if (fs.existsSync(outDir)) {
      for (const entry of fs.readdirSync(outDir)) {
        if (entry !== 'test') {
          fs.rmSync(path.join(outDir, entry), { recursive: true, force: true });
        }
      }
    }
    const rolldownEntry = path.join(EXT_ROOT, 'node_modules', 'rolldown', 'bin', 'cli.mjs');
    execFileSync(process.execPath, [rolldownEntry, '-c', 'rolldown.config.mjs'], {
      cwd: EXT_ROOT,
    });
  }, 60000);

  function listVsixFiles(): string[] {
    // Invoke vsce's own JS entry point directly via `node`, not `npx` (which
    // needs a shell on Windows to resolve — shell:true string-concatenates
    // args instead of escaping them, a real risk once paths contain spaces).
    const vsceEntry = path.join(EXT_ROOT, 'node_modules', '@vscode', 'vsce', 'vsce');
    const output = execFileSync(process.execPath, [vsceEntry, 'ls'], {
      cwd: EXT_ROOT,
      encoding: 'utf8',
    });
    return output.split(/\r?\n/).filter((line) => line.trim().length > 0);
  }

  test('vsce ls manifest contains no node_modules entries at all', () => {
    const files = listVsixFiles();
    expect(files.length).toBeGreaterThan(0);

    const nodeModulesEntries = files.filter((f) => /(^|[\\/])node_modules[\\/]/.test(f));
    expect(nodeModulesEntries).toEqual([]);

    // The two tiny rc config files are expected and harmless (not code, not
    // a binary) — assert their presence so this test also documents that
    // deliberate, known inclusion rather than silently tolerating it.
    expect(files).toEqual(expect.arrayContaining(['.oxfmtrc.json', '.oxlintrc.json']));
  }, 30000);

  test('vsce ls manifest ships exactly one bundled artifact at out/extension.js', () => {
    const files = listVsixFiles();
    const outFiles = files.filter((f) => f.replace(/\\/g, '/').startsWith('out/'));

    // Strict single-file bundle: no facade/split chunks (e.g. a stray
    // out/commandResults.js from an under-elided type-only import — this
    // exact regression was caught and fixed via output.codeSplitting: false
    // in rolldown.config.mjs), no leftover .map (excluded via
    // .vscodeignore's **/*.map), no rolldown.config.mjs itself (also
    // excluded via .vscodeignore).
    expect(outFiles).toEqual(['out/extension.js']);
  }, 30000);

  test('vsce ls manifest contains no rolldown config or dev-tooling rc leaks beyond the known two', () => {
    const files = listVsixFiles();
    expect(files).not.toContain('rolldown.config.mjs');
    const rcFiles = files.filter((f) => f.endsWith('rc.json'));
    expect(rcFiles.sort()).toEqual(['.oxfmtrc.json', '.oxlintrc.json']);
  }, 30000);
});
