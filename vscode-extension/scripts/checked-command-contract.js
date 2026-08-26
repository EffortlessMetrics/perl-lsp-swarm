#!/usr/bin/env node
'use strict';

/**
 * Structural contract for the checked command surface (#9842).
 *
 * `governed-tsc.js` governs one invocation at a time. This evaluator governs
 * the surface itself: given the npm script map from `package.json`, it proves
 * the three properties the issue's acceptance rows name —
 *
 *   1. every public TypeScript execution path routes through
 *      `scripts/governed-tsc.js` (directly, or by composing scripts that do);
 *   2. the npm-script graph is non-recursive, and every `npm run` reference
 *      resolves (a dangling or cyclic reference is red, naming the script);
 *   3. `build` is authority -> config inventory -> all-config type-check ->
 *      bundle, joined fail-closed, and `bundle`/`watch:bundle` are Rolldown
 *      only — so a type error cannot be bundled past and a bundle failure
 *      cannot read as build success.
 *
 * Like `check-typescript-config-inventory.js`, evaluation is pure so the
 * negative controls run against synthetic script maps in
 * `checked-command-contract.test.js` rather than against a mutated tree.
 */

const fs = require('node:fs');
const path = require('node:path');
const { createReporter } = require('./reporter');

/** The single governed TypeScript execution seam. */
const GOVERNED_TSC = 'scripts/governed-tsc.js';

/** A stage that invokes the seam directly. */
const GOVERNED_STAGE = /^node\s+\.?\/?\s*scripts\/governed-tsc\.js(?:\s|$)/;

/** An exact `npm run <name>` stage, which composes one whole script. */
const NPM_RUN_STAGE = /^npm\s+run\s+([\w:-]+)$/;

/**
 * A stage that starts an npm-run but carries extra shell text after it —
 * including text glued on without whitespace (`npm run x;tsc …`), where the
 * trailing command would otherwise escape classification entirely.
 */
const NPM_RUN_WITH_TRAILING = /^npm\s+run\s+[\w:-]+\s*\S/;

/**
 * A bare `tsc` command token: `tsc …`, `npx tsc …`, `npm exec … tsc …`, a
 * direct `…/.bin/tsc …` (POSIX or Windows separators and `.cmd`/`.ps1`
 * wrappers), or one glued behind a shell separator (`…;tsc …`). The token
 * must not be part of a longer name: the wrapper filename `governed-tsc.js`
 * (`-` before `tsc`) and words like `tsconfig.json` (`o` after) do not match,
 * while path separators and shell metacharacters do.
 */
const BARE_TSC_TOKEN = /(^|[^\w.-])tsc(?:\.(?:cmd|ps1))?(?=$|[\s;|&)"'])/;

/**
 * @typedef {object} ScriptResolution
 * @property {boolean} executesTypeScript Whether the script (transitively) runs the compiler.
 * @property {string[]} violations Stage-level ungoverned TypeScript findings for this script only.
 * @property {string[]} composes Scripts this one references that execute TypeScript.
 */

/**
 * @typedef {object} ContractResult
 * @property {boolean} ok
 * @property {string[]} failures
 * @property {string[]} facts
 */

/**
 * Splits a command into `&&`-joined stages.
 *
 * @param {string} command
 * @returns {string[]}
 */
function splitStages(command) {
  return command
    .split('&&')
    .map((stage) => stage.trim())
    .filter((stage) => stage.length > 0);
}

/**
 * Detects composition operators whose ordering cannot be proven fail-closed
 * once `&&` has been split away: `;`, a single `|`, or a single `&` inside
 * one stage could sequence or background a compiler run invisibly.
 *
 * @param {string} stage
 * @returns {string[]}
 */
function unsupportedOperators(stage) {
  /** @type {string[]} */
  const ops = [];
  if (stage.includes(';')) {
    ops.push(';');
  }
  if (/[|]/.test(stage)) {
    ops.push('|');
  }
  if (/[&]/.test(stage)) {
    ops.push('&');
  }
  return ops;
}

/**
 * Evaluates the checked command contract against a script map.
 *
 * @param {{scripts: Record<string, string>}} input
 * @returns {ContractResult}
 */
function evaluateCheckedCommandContract(input) {
  /** @type {string[]} */
  const failures = [];
  /** @type {string[]} */
  const facts = [];
  const scripts = input.scripts;

  const stagesByName = new Map(
    Object.entries(scripts).map(([name, command]) => [name, splitStages(command)]),
  );

  /**
   * Resolves one script's TypeScript behavior. Depth-first with an explicit
   * stack so a `npm run` cycle is reported as recursion rather than looping.
   *
   * @param {string} name
   * @param {string[]} stack
   * @param {Map<string, ScriptResolution>} memo
   * @returns {ScriptResolution}
   */
  function resolve(name, stack, memo) {
    // The stack is consulted before the memo: a node still being resolved is
    // on both, and reading it as complete would hide exactly the recursion
    // this check exists to name.
    if (stack.includes(name)) {
      const cycle = [...stack.slice(stack.indexOf(name)), name];
      failures.push(
        `scripts form a recursive npm-run cycle: ${cycle.join(' -> ')} — recursive script graphs are not allowed`,
      );
      return { executesTypeScript: false, violations: [], composes: [] };
    }
    const cached = memo.get(name);
    if (cached) {
      return cached;
    }
    const stages = stagesByName.get(name);
    if (stages === undefined) {
      return { executesTypeScript: false, violations: [], composes: [] };
    }

    /** @type {ScriptResolution} */
    const result = { executesTypeScript: false, violations: [], composes: [] };
    memo.set(name, result);
    stack.push(name);
    for (const stage of stages) {
      if (GOVERNED_STAGE.test(stage)) {
        result.executesTypeScript = true;
        const ops = unsupportedOperators(stage);
        if (ops.length > 0) {
          result.violations.push(
            `script "${name}" stage \`${stage}\` composes with ${ops.join(' ')} — only fail-closed && ordering is provable`,
          );
        }
        continue;
      }
      const npmRun = NPM_RUN_STAGE.exec(stage);
      const referenced = npmRun?.[1];
      if (typeof referenced === 'string') {
        if (!(referenced in scripts)) {
          failures.push(
            `script "${name}" references unknown npm script "${referenced}" — the command graph must resolve`,
          );
          continue;
        }
        const nested = resolve(referenced, stack, memo);
        if (nested.executesTypeScript) {
          result.executesTypeScript = true;
          result.composes.push(referenced);
        }
        result.violations.push(...nested.violations);
        continue;
      }
      if (NPM_RUN_WITH_TRAILING.test(stage)) {
        result.executesTypeScript = true;
        result.violations.push(
          `script "${name}" stage \`${stage}\` composes \`npm run\` with additional shell text — only exact \`npm run <name>\` stages have provable ordering`,
        );
        continue;
      }
      if (BARE_TSC_TOKEN.test(stage)) {
        result.executesTypeScript = true;
        result.violations.push(
          `script "${name}" stage \`${stage}\` executes TypeScript outside ${GOVERNED_TSC} — every public tsc path must route through the governed seam`,
        );
      }
    }
    stack.pop();
    return result;
  }

  /** @type {Map<string, ScriptResolution>} */
  const resolved = new Map();
  for (const name of Object.keys(scripts)) {
    const resolution = resolve(name, [], resolved);
    for (const violation of resolution.violations) {
      if (!failures.includes(violation)) {
        failures.push(violation);
      }
    }
  }

  // Enumeration fact: which public commands reach the compiler, and how.
  /** @type {string[]} */
  const direct = [];
  /** @type {string[]} */
  const aggregates = [];
  for (const [name, stages] of stagesByName) {
    if (stages.some((stage) => GOVERNED_STAGE.test(stage))) {
      direct.push(name);
    } else if (resolved.get(name)?.executesTypeScript) {
      aggregates.push(name);
    }
  }
  if (failures.length === 0) {
    facts.push(
      `${direct.length} scripts invoke ${GOVERNED_TSC} directly: ${direct.sort().join(', ')}`,
    );
    facts.push(
      `${aggregates.length} aggregates compose governed subcommands only: ${aggregates.sort().join(', ')}`,
    );
  }

  // Build contract: the exact fail-closed sequence authority -> config
  // inventory -> all-config type-check -> bundle. The exact-shape comparison
  // (not presence checks) is load-bearing: a permutation that type-checks
  // before the inventory gate, or an extra middle stage, must be red.
  const buildStages = stagesByName.get('build');
  if (buildStages === undefined) {
    failures.push('script "build" is missing — the checked build contract requires it');
  } else {
    const expectedBuild = [
      'npm run typecheck:authority',
      'npm run check:tsconfig-inventory',
      'npm run typecheck:all',
      'npm run bundle',
    ];
    const firstDifference = expectedBuild.findIndex((stage, index) => buildStages[index] !== stage);
    if (buildStages.length !== expectedBuild.length || firstDifference !== -1) {
      failures.push(
        `script "build" must be exactly \`${expectedBuild.join(' && ')}\` (fail-closed && ordering; ` +
          'compiler authority and config inventory gate before any type-check, bundle last so type ' +
          'failures block bundling and bundle failures fail the build) — ' +
          `received \`${buildStages.join(' && ')}\``,
      );
    }
  }

  // Bundle purity: the exact Rolldown-only shape, never a type-check surface.
  /** @type {[string, string][]} */
  const bundleScripts = [
    ['bundle', 'npm run clean:out && rolldown -c rolldown.config.mjs'],
    ['watch:bundle', 'npm run clean:out && rolldown -c rolldown.config.mjs --watch'],
  ];
  for (const [name, exactCommand] of bundleScripts) {
    const stages = stagesByName.get(name);
    if (stages === undefined) {
      failures.push(`script "${name}" is missing — the command contract requires it`);
      continue;
    }
    if (stages.join(' && ') !== exactCommand) {
      failures.push(
        `script "${name}" must be exactly \`${exactCommand}\` — Rolldown only, no additional stages ` +
          `(use \`build\` for the checked build); received \`${stages.join(' && ')}\``,
      );
    }
    if (resolved.get(name)?.executesTypeScript) {
      failures.push(
        `script "${name}" executes TypeScript — bundling must not type-check (use \`build\` for the checked build)`,
      );
    }
  }

  // watch:types: authority-gated source watch, and nothing else.
  const watchTypes = stagesByName.get('watch:types');
  if (watchTypes === undefined) {
    failures.push('script "watch:types" is missing — the command contract requires it');
  } else {
    const governedStages = watchTypes.filter((stage) => GOVERNED_STAGE.test(stage));
    if (governedStages.length === 0) {
      failures.push(
        `script "watch:types" must invoke ${GOVERNED_TSC} so the authority preflight runs before watch mode`,
      );
    } else if (!governedStages.some((stage) => stage.includes('./tsconfig.json'))) {
      failures.push('script "watch:types" must watch the source config ./tsconfig.json');
    }
    if (watchTypes.some((stage) => BARE_TSC_TOKEN.test(stage) && !GOVERNED_STAGE.test(stage))) {
      failures.push('script "watch:types" reaches tsc outside the governed seam');
    }
  }

  if (failures.length === 0) {
    facts.push('script graph is non-recursive and every npm-run reference resolves');
    facts.push(
      'build = authority -> config inventory -> all-config type-check -> bundle, fail-closed',
    );
  }
  return { ok: failures.length === 0, failures, facts };
}

/**
 * Evaluates the contract against the real extension tree.
 *
 * @param {string} extensionRoot
 * @returns {ContractResult}
 */
function checkCheckedCommandContract(extensionRoot) {
  const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));
  return evaluateCheckedCommandContract({ scripts: packageJson.scripts ?? {} });
}

function main() {
  const reporter = createReporter('checked-command-contract');
  const result = checkCheckedCommandContract(path.resolve(__dirname, '..'));
  if (!result.ok) {
    for (const failure of result.failures) {
      reporter.error(`FAIL: ${failure}`);
    }
    process.exitCode = 1;
    return;
  }
  for (const fact of result.facts) {
    reporter.info(`OK  ${fact}`);
  }
  reporter.info('PASS — checked command contract holds.');
}

if (require.main === module) {
  main();
}

module.exports = {
  GOVERNED_TSC,
  evaluateCheckedCommandContract,
  checkCheckedCommandContract,
};
