# TS7 migration — PREP-2 receipts: ESLint → Oxlint (#3662)

PREP-2 of the TypeScript 6 → 7 migration train. Preparatory only: does **not**
upgrade the `typescript` compiler (stays `^6.0.3`). Replaces ESLint +
`@typescript-eslint` with [Oxlint](https://oxc.rs/docs/guide/usage/linter)
(syntactic rules) + [`oxlint-tsgolint`](https://github.com/oxc-project/tsgolint)
(type-aware rules), removing the second — and last — ecosystem coupling that
blocked the extension from ever running under TypeScript 7:
`@typescript-eslint/typescript-estree` peers `typescript >=4.8.4 <6.1.0`, with
no release, canary, or roadmap item that raises that cap
(typescript-eslint#12518, closed not-planned).

`oxlint-tsgolint` sidesteps this entirely: its type-aware backend is
[`typescript-go`](https://github.com/microsoft/typescript-go) (the engine
TypeScript 7 itself is built on), not the classic `typescript` npm compiler
API. It does not `require('typescript')`, has no peer dependency on the
`typescript` package at all, and needs no alias/shim. This PR does not touch
the TS7-compiler-swap axis (that remains a later PREP); it only removes the
lint-side blocker so that swap can land without a lint-tooling casualty.

## 1. ESLint baseline (before removal)

```
$ npx eslint src --ext .ts
(no output, exit 0)

$ npx eslint src --ext .ts -f json
files linted: 17
total messages: 0
```

The extension's source tree was already fully clean under
`@typescript-eslint/eslint-plugin@8.61.1` + `@typescript-eslint/parser@8.59.4`
with the six rules below — there was nothing latent for Oxlint to catch on
real source. The parity proof below therefore rests on (a) an identical
zero-finding run against the same 17 files, and (b) a **deliberate** injected
violation exercising the one type-aware rule, since the baseline had no real
example to compare against.

## 2. Package verification (registry facts, not assumed)

```
$ npm view oxlint versions            → ... 1.70.0, 1.71.0, 1.72.0, 1.73.0
$ npm view oxlint dist-tags           → { latest: '1.73.0' }
$ npm view oxlint bin                 → { oxlint: 'bin/oxlint' }
$ npm view oxlint-tsgolint versions   → ... 0.22.1, 0.23.0, 0.24.0
$ npm view oxlint-tsgolint dist-tags  → { latest: '0.24.0' }
$ npm view oxlint-tsgolint bin        → { tsgolint: 'bin/tsgolint.js' }
$ npm view oxlint peerDependencies    → { 'oxlint-tsgolint': '>=0.24.0', 'vite-plus': '*' }
```

Installed `oxlint@^1.73.0` + `oxlint-tsgolint@^0.24.0` with plain
`npm install` — **no `--force`, no `--legacy-peer-deps`**. Peers resolved
cleanly; the earlier `typescript` peer conflict does not exist for this
package pair (neither depends on `typescript`).

## 3. Rule translation (`.oxlintrc.json`)

The former `eslint.config.js` enabled exactly six rules, scoped to `src/**/*.ts`
excluding `src/test/**`. `parserOptions.project` made the config type-aware,
but only one of the six rules actually consumes type information
(`@typescript-eslint/no-floating-promises` — confirmed against
typescript-eslint's own rule docs; the other five are AST-only).

| ESLint rule | Oxlint rule | Type-aware? | Severity (before → after) |
|---|---|---|---|
| `@typescript-eslint/no-explicit-any` | `typescript/no-explicit-any` | No | warn → warn |
| `@typescript-eslint/consistent-type-imports` | `typescript/consistent-type-imports` | No (syntactic) | warn → warn (`prefer: type-imports`, unchanged) |
| `@typescript-eslint/no-floating-promises` | `typescript/no-floating-promises` | **Yes** | error → error |
| `@typescript-eslint/no-unused-vars` | `no-unused-vars` | No | warn → warn (`argsIgnorePattern: ^_`, unchanged) |
| `no-console` | `no-console` | No | warn → warn |
| `eqeqeq` | `eqeqeq` | No | error → error (`always`, unchanged) |

`.oxlintrc.json` sets `categories.correctness: "off"` and `plugins: ["typescript"]`
so only these six rules are active — verified via `oxlint src --print-config`,
which reports exactly 6 rules, matching the ESLint config 1:1 (no default
Oxlint rule sets leaking in).

## 4. Type-aware run — clean

```
$ npx oxlint src                 (syntactic only)  → exit 0, no output
$ npx oxlint src --type-aware    (+ no-floating-promises) → exit 0, no output
$ npm run lint                   (oxlint src --type-aware) → exit 0, no output
```

Same 17-file scope as the ESLint baseline (`.oxlintrc.json` `ignorePatterns`
mirrors the former flat-config `ignores`: `out/**`, `out-test/**`,
`node_modules/**`, `src/test/**`, `*.js`). Zero findings — matches the
ESLint baseline exactly.

## 5. Deliberate-violation parity proof (the load-bearing check)

Since the real source had no findings to diff, a scratch file
`src/__oxlint_probe.ts` (never committed) was added with a classic bare-call
floating promise:

```ts
async function doAsyncThing(): Promise<void> {
  return Promise.resolve();
}
export function callSite(): void {
  doAsyncThing();   // bare call — no await/void/catch/then
}
```

```
$ npx eslint src/__oxlint_probe.ts
  11:3  error  Promises must be awaited, end with a call to .catch, end with a
               call to .then with a rejection handler or be explicitly marked
               as ignored with the `void` operator  @typescript-eslint/no-floating-promises
✖ 1 problem (1 error, 0 warnings)
exit 1

$ npx oxlint src --type-aware
src/__oxlint_probe.ts:11:3: error typescript(no-floating-promises): Promises
  must be awaited, add void operator to ignore. help: The promise must end
  with a call to .catch, or end with a call to .then with a rejection
  handler, or be explicitly marked as ignored with the `void` operator.
exit 1
```

Both tools flag the exact same line:column (11:3) with the same underlying
message. The probe file was deleted immediately after (`git status` confirms
it was never staged); `npx oxlint src --type-aware` was re-run afterward and
confirmed clean again (exit 0).

**No coverage gap found.** No alternative type-aware linter was evaluated as
a fallback because none was needed — `oxlint-tsgolint` reproduces
`no-floating-promises` behavior exactly on the one case exercised. (Heuristic
alternatives like `eslint-plugin-promise` were considered during the earlier
investigation phase and rejected as unsound — they pattern-match `.then()`/
`.catch()` syntax rather than resolving the expression's static type, so they
would have missed the exact bare-call case above. That question is now moot:
`oxlint-tsgolint` is a real type-aware engine, not a heuristic.)

## 6. Dependency removal

```
$ rm -rf node_modules && npm install
added 683 packages (was 752 with eslint/@typescript-eslint present)
0 vulnerabilities related to the removed tree
no ERESOLVE, no --force, no --legacy-peer-deps
```

Verified zero remnants:
```
$ find node_modules -maxdepth 1 -iname "*eslint*"   → (none)
$ grep -c '"eslint'  package-lock.json               → 0
$ grep -c '@typescript-eslint' package-lock.json      → 0
$ grep -rn "eslint" src *.json *.js                   → only the new
    oxlint.test.ts contract test (asserting absence) and one stray, inert
    `// eslint-disable-next-line no-console` comment inside
    src/test/packagedSemanticTokensSmoke.test.ts (src/test/** was never
    linted before or after — the comment was already a no-op; left as-is,
    out of scope for this PR)
```

`package.json` devDependencies: removed `eslint`, `@typescript-eslint/eslint-plugin`,
`@typescript-eslint/parser`; added `oxlint`, `oxlint-tsgolint`. `typescript`
stays `^6.0.3` — unrelated to this change.

## 7. Contract test

`src/test/eslint.test.ts` (asserted the ESLint config's existence/shape) was
replaced by `src/test/oxlint.test.ts`, asserting: `.oxlintrc.json` exists and
is valid JSON, declares the `typescript` plugin and all six translated rules,
`npm run lint` invokes `oxlint --type-aware`, `oxlint`/`oxlint-tsgolint`
are present in devDependencies, and — the negative check — that
`eslint`/`@typescript-eslint/*` are **absent** from devDependencies and that
`eslint.config.js` no longer exists.

## 8. CI wiring

`.github/workflows/ux-regression-gate.yml` — the `extension-jest` job's
`Lint (eslint)` step (`npm run lint`, `ubuntu-24.04`, unchanged runner) is
renamed `Lint (oxlint --type-aware)`; the command was already `npm run lint`
so no invocation changes beyond the underlying script. No new job, no
Windows/non-Ubuntu runner added.

`.github/dependabot.yml` — the `typescript` npm dependency group's patterns
(`@typescript-eslint/*`, `eslint*`) were swapped for `oxlint`, `oxlint-tsgolint`.

`docs/how-to/DEPENDENCY_MANAGEMENT.md` — updated the one-line description of
that dependabot group from "TypeScript and ESLint tooling" to "TypeScript and
Oxlint tooling".

`docs/policy/NON_RUST_INVENTORY.md` was **not** regenerated in this PR: a
`cargo xtask non-rust inventory` run showed the file is already ~660 lines
stale from unrelated merges since it was last generated (files added by
PREP-1 and others were never synced in). Bundling that unrelated drift into
this PR would be scope creep; it's left for a dedicated inventory-sync pass.
The two entries this PR's renames make stale
(`vscode-extension/eslint.config.js`, `vscode-extension/src/test/eslint.test.ts`)
are part of that same pre-existing backlog.

## 9. Alpha caveats checked

Oxlint type-aware linting is alpha upstream; the extension was checked
against the known failure modes before relying on it:

- **Legacy tsconfig options** (e.g. `baseUrl`): `vscode-extension/tsconfig.json`
  has none — `moduleResolution: "node16"`, no `baseUrl`/`paths`.
- **Broad `include` / perf**: the tsconfig's `include` is `["src/**/*"]` on a
  17-file, single-package extension — no monorepo-scale tree. `oxlint --type-aware`
  ran in well under a second locally; no timeout or slowdown observed.
- **Memory blowup on large trees**: not applicable at this scale; not observed.

## 10. Final verification (all green)

```
$ npm run lint          → oxlint src --type-aware, exit 0
$ npm run compile       → tsc -p ./ (still TS 6.0.3), exit 0
$ npm run test:ci       → Test Suites: 1 skipped, 23 passed, 23 of 24 total
                           Tests:       1 skipped, 645 passed, 646 total
                           (PREP-1 baseline: 643 passed/1 skipped; +2 net from
                           oxlint.test.ts replacing eslint.test.ts, 7 tests vs 5)
$ npm run test:grammar  → 9/9 fixtures pass, exit 0
```

## Scope boundary

This PR does not: bump `typescript`, touch `tsconfig*.json`, change the Jest
pipeline (PREP-1, already merged), or add any CI runner. The TS7 compiler
swap (bump `typescript` 6 → 7, no alias/override) is the next PREP in the
train, unblocked on the lint axis by this PR and — per the earlier
investigation on this issue — already proven to compile the extension's
source cleanly under the real `typescript@7.0.2` CLI with byte-identical JS
emission (zero source changes required).
