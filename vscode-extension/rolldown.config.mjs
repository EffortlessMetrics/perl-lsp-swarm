// Rolldown production bundle config for the VS Code extension (#3662, final
// step of the TS7 migration train). Rolldown replaces TypeScript EMISSION as
// the production artifact builder — it does NOT type-check. TypeScript 7
// (`tsc --noEmit`, the `typecheck` npm script) remains the sole type-check
// authority; this config only turns already-valid TypeScript into the single
// runtime artifact VS Code loads.
//
// Single CJS entry: src/extension.ts -> out/extension.js. This exact path is
// load-bearing: package.json's "main" and the debugger's "program" both
// point at ./out/extension.js, and this config preserves it byte-for-byte.
//
// Rolldown is ESM-only (no CJS export), so this config file is itself ESM
// (.mjs) even though the rest of the extension's tooling is CommonJS.
import { createHash } from 'node:crypto';
import { builtinModules, createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { defineConfig, RolldownMagicString } from 'rolldown';

// Node built-ins must never be bundled — Node resolves `require('fs')` etc.
// natively at runtime. Cover both the bare form (`fs`) and the explicit
// `node:` prefix form (`node:fs`), since source may use either.
const nodeBuiltins = new Set([...builtinModules, ...builtinModules.map((m) => `node:${m}`)]);

// Runtime dependency classification (package.json "dependencies"):
//   - adm-zip: pure JS, no native bindings, no dynamic environment-based
//     require. Safe to bundle.
//   - tar: pure JS (no native/optional bindings of its own), heavy internal
//     module graph but no dynamic `require(computedPath)` patterns. Safe to
//     bundle — verified via the parity-proof integration tests, which
//     exercise real archive extraction end-to-end against the bundled
//     artifact, not just a grep.
//   - vscode-languageclient: pure JS/TS, itself already depends on `vscode`
//     as an external. Safe to bundle.
// `vscode` itself is supplied by the VS Code extension host at runtime, not
// resolvable as a real package — it MUST stay external regardless of the
// above.
const external = (id) => id === 'vscode' || nodeBuiltins.has(id);

const PINNED_LANGUAGE_CLIENT_SOURCE_SHA256 =
    'FB34F029620E1990B00F351D9A79CEEDA40432AA5D177FD4245BD62053DF05A8';
const PINNED_JSONRPC_CONNECTION_SOURCE_SHA256 =
    '0A3A46FAD254B78FC2EE371D8438037C860B9BC73124FDDB038BF3BB5A3C94F4';
const resolvedLanguageClientEntry = createRequire(import.meta.url).resolve('vscode-languageclient');
const resolvedJsonRpcEntry = createRequire(import.meta.url).resolve('vscode-jsonrpc');
const LANGUAGE_CLIENT_SOURCE_ID = join(
    dirname(dirname(dirname(resolvedLanguageClientEntry))),
    'lib',
    'common',
    'client.js',
);
const JSONRPC_CONNECTION_SOURCE_ID = join(
    dirname(dirname(dirname(resolvedJsonRpcEntry))),
    'lib',
    'common',
    'connection.js',
);

function normalizeModuleId(id) {
    return id.replaceAll('\\', '/').toLowerCase();
}

/**
 * vscode-languageclient 10.1.1 returns the mutable `_onStart` field after
 * `handleConnectionClosed` can clear it. That can orphan the rejection from
 * the start operation. Keep this narrowly pinned to the exact resolved source
 * and hash until the dependency ships the equivalent upstream correction.
 */
function patchPinnedLanguageClientDetails(source, id) {
    if (normalizeModuleId(id) !== normalizeModuleId(LANGUAGE_CLIENT_SOURCE_ID)) {
        return null;
    }
    const sourceSha256 = createHash('sha256').update(source).digest('hex').toUpperCase();
    if (sourceSha256 !== PINNED_LANGUAGE_CLIENT_SOURCE_SHA256) {
        throw new Error(
            `Refusing to patch unexpected vscode-languageclient source ${id}: expected ${PINNED_LANGUAGE_CLIENT_SOURCE_SHA256}, got ${sourceSha256}.`,
        );
    }
    const start = source.indexOf('    async start() {');
    const end = source.indexOf('    createOnStartPromise()', start);
    if (start < 0 || end <= start) {
        throw new Error(
            `Refusing to patch vscode-languageclient source with an unexpected start() shape: ${id}.`,
        );
    }
    const body = source.slice(start, end);
    const returnText = '        return this._onStart;';
    const returnOffsets = [];
    let offset = body.indexOf(returnText);
    while (offset >= 0) {
        returnOffsets.push(offset);
        offset = body.indexOf(returnText, offset + returnText.length);
    }
    if (returnOffsets.length !== 2) {
        throw new Error(
            `Refusing to patch vscode-languageclient start() with ${returnOffsets.length} mutable-return sites; expected 2.`,
        );
    }
    const finalReturnOffset = start + returnOffsets[returnOffsets.length - 1];
    return { start: finalReturnOffset, end: finalReturnOffset + returnText.length };
}

export function patchPinnedLanguageClientSource(source, id) {
    const details = patchPinnedLanguageClientDetails(source, id);
    if (details === null) return null;
    return `${source.slice(0, details.start)}        return promise;${source.slice(details.end)}`;
}

function patchPinnedJsonRpcConnectionDetails(source, id) {
    // vscode-jsonrpc 9.0.2 rejects the public response promise and then throws
    // from an async Promise executor. The throw creates a second, orphaned
    // rejection when a stream write fails; return after the explicit rejection.
    if (normalizeModuleId(id) !== normalizeModuleId(JSONRPC_CONNECTION_SOURCE_ID)) {
        return null;
    }
    const sourceSha256 = createHash('sha256').update(source).digest('hex').toUpperCase();
    if (sourceSha256 !== PINNED_JSONRPC_CONNECTION_SOURCE_SHA256) {
        throw new Error(
            `Refusing to patch unexpected vscode-jsonrpc source ${id}: expected ${PINNED_JSONRPC_CONNECTION_SOURCE_SHA256}, got ${sourceSha256}.`,
        );
    }
    const rejectText =
        "                    responsePromise.reject(new messages_1.ResponseError(messages_1.ErrorCodes.MessageWriteError, error.message ? error.message : 'Unknown reason'));";
    const loggerText = '                    logger.error(`Sending request failed.`);';
    const throwText = '                    throw error;';
    const rejectOffset = source.indexOf(rejectText);
    const loggerOffset = source.indexOf(loggerText, rejectOffset + rejectText.length);
    const throwOffset = source.indexOf(throwText, loggerOffset + loggerText.length);
    if (rejectOffset < 0 || loggerOffset < 0 || throwOffset < 0) {
        throw new Error(`Refusing to patch unexpected vscode-jsonrpc sendRequest shape: ${id}.`);
    }
    return { start: throwOffset, end: throwOffset + throwText.length };
}

export function patchPinnedJsonRpcConnectionSource(source, id) {
    const details = patchPinnedJsonRpcConnectionDetails(source, id);
    if (details === null) return null;
    return `${source.slice(0, details.start)}                    return;${source.slice(details.end)}`;
}

let pinnedLanguageClientPatchApplied = false;
let pinnedJsonRpcPatchApplied = false;
const pinnedLanguageClientPatch = {
    name: 'patch-pinned-languageclient-and-jsonrpc-promises',
    transform(source, id) {
        const details = patchPinnedLanguageClientDetails(source, id);
        const jsonRpcDetails = patchPinnedJsonRpcConnectionDetails(source, id);
        if (details === null && jsonRpcDetails === null) return null;
        const magic = new RolldownMagicString(source);
        if (details !== null) {
            pinnedLanguageClientPatchApplied = true;
            magic.overwrite(details.start, details.end, '        return promise;');
        }
        if (jsonRpcDetails !== null) {
            pinnedJsonRpcPatchApplied = true;
            magic.overwrite(
                jsonRpcDetails.start,
                jsonRpcDetails.end,
                '                    return;',
            );
        }
        return { code: magic };
    },
    buildEnd() {
        if (!pinnedLanguageClientPatchApplied) {
            throw new Error(`Refusing to build without patching ${LANGUAGE_CLIENT_SOURCE_ID}.`);
        }
        if (!pinnedJsonRpcPatchApplied) {
            throw new Error(`Refusing to build without patching ${JSONRPC_CONNECTION_SOURCE_ID}.`);
        }
    },
};
export default defineConfig({
    input: 'src/extension.ts',
    tsconfig: './tsconfig.json',
    platform: 'node',
    external,
    plugins: [pinnedLanguageClientPatch],
    output: {
        file: 'out/extension.js',
        format: 'cjs',
        sourcemap: true,
        // No minification in this first PR — the migration charter is explicit
        // that this pass proves parity of the bundling step alone. Minification
        // is a separate, later decision.
        minify: false,
        // Strict single-file output. Without this, Rolldown split a facade
        // chunk (out/commandResults.js) for a type-only `import type {...}
        // from './commandResults'` in extension.ts even though there is no
        // runtime dynamic import() anywhere in the source (verified). CJS has
        // no native async chunk-loading anyway, so a split there would just be
        // a synchronous `require()` of a sibling file — codeSplitting: false
        // inlines everything into the one artifact the "single CJS entry" spec
        // requires, and matches out/extension.js being the sole path referenced
        // by package.json's "main" and the debugger's "program".
        codeSplitting: false,
    },
});

// NOTE on out/ hygiene: `output.cleanDir` was tried here but does not apply
// in single-file (`output.file`) mode — verified empirically: a stray
// out/commandResults.js + .map left behind by an unrelated
// `tsc -p tsconfig.integration.json` run (out/ is a shared build directory;
// that command also emits the integration test harness into out/test/**)
// survived cleanDir:true across a subsequent `npm run bundle`. The actual
// fix is the "clean:out" npm script (removes everything under out/ except
// out/test/**, which the test-harness tsc builds manage separately) that
// runs before this config is invoked — see package.json's "bundle" script.
