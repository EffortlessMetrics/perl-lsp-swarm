#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { createReporter } = require('./reporter');
const { TSCONFIG_FILES } = require('./check-typescript-authority');

const INVENTORY_PATH = path.join('scripts', 'typescript-config-inventory.json');

/**
 * @typedef {object} InventoryConfig
 * @property {string} path
 * @property {string} role
 * @property {boolean} blocking
 * @property {boolean} authority
 * @property {string} command
 * @property {boolean} emit
 * @property {string} outDir
 * @property {string} rootDir
 */

/**
 * @typedef {object} EffectiveConfig
 * @property {boolean} noEmit
 * @property {string | undefined} outDir
 * @property {string | undefined} rootDir
 * @property {string | undefined} [error]
 */

/**
 * @param {unknown} value
 * @returns {value is InventoryConfig}
 */
function isInventoryConfig(value) {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const candidate = /** @type {Record<string, unknown>} */ (value);
  return (
    typeof candidate.path === 'string' &&
    typeof candidate.role === 'string' &&
    typeof candidate.blocking === 'boolean' &&
    typeof candidate.authority === 'boolean' &&
    typeof candidate.command === 'string' &&
    typeof candidate.emit === 'boolean' &&
    typeof candidate.outDir === 'string' &&
    typeof candidate.rootDir === 'string'
  );
}

/**
 * @param {string} extensionRoot
 * @param {string | undefined} value
 * @returns {string | undefined}
 */
function normalizeOptionPath(extensionRoot, value) {
  if (value === undefined) {
    return undefined;
  }
  const relative = path.relative(extensionRoot, path.resolve(extensionRoot, value));
  return (relative || '.').replaceAll('\\', '/');
}

/**
 * @param {string} extensionRoot
 * @returns {string[]}
 */
function discoverTypeScriptConfigs(extensionRoot) {
  return fs
    .readdirSync(extensionRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /^tsconfig(?:\.[^.]+)*\.json$/.test(entry.name))
    .map((entry) => entry.name)
    .sort();
}

/**
 * @param {{
 *   inventory: {schema?: unknown, configs?: unknown},
 *   discoveredFiles: string[],
 *   authorityFiles: string[],
 *   typecheckAll: string,
 *   effectiveConfigs: Record<string, EffectiveConfig>
 * }} input
 * @returns {{ok: boolean, failures: string[], facts: string[]}}
 */
function evaluateTypeScriptConfigInventory(input) {
  /** @type {string[]} */
  const failures = [];
  /** @type {string[]} */
  const facts = [];

  if (input.inventory.schema !== 1) {
    failures.push(`inventory schema must be 1, received ${JSON.stringify(input.inventory.schema)}`);
  }
  if (!Array.isArray(input.inventory.configs)) {
    failures.push('inventory `configs` must be an array');
    return { ok: false, failures, facts };
  }

  /** @type {InventoryConfig[]} */
  const configs = [];
  for (const [index, value] of input.inventory.configs.entries()) {
    if (!isInventoryConfig(value)) {
      failures.push(`inventory config at index ${index} is malformed`);
      continue;
    }
    configs.push(value);
  }

  const discovered = new Set(input.discoveredFiles);
  const authority = new Set(input.authorityFiles);
  const seenPaths = new Set();
  const seenCommands = new Set();

  for (const config of configs) {
    if (seenPaths.has(config.path)) {
      failures.push(`inventory contains duplicate config path ${config.path}`);
    }
    seenPaths.add(config.path);

    if (seenCommands.has(config.command)) {
      failures.push(`inventory assigns command ${config.command} to more than one config`);
    }
    seenCommands.add(config.command);

    if (!discovered.has(config.path)) {
      failures.push(`inventory config ${config.path} does not exist in the extension root`);
    }

    const authorityOwns = authority.has(config.path);
    if (config.authority !== authorityOwns) {
      failures.push(
        `${config.path} authority=${String(config.authority)} but the compiler-authority set reports ${String(authorityOwns)}`,
      );
    }

    if (config.blocking && !input.typecheckAll.includes(`npm run ${config.command}`)) {
      failures.push(
        `${config.path} is blocking through ${config.command}, but typecheck:all does not invoke that command`,
      );
    }

    const effective = input.effectiveConfigs[config.path];
    if (effective === undefined) {
      failures.push(`${config.path} has no effective TypeScript configuration evidence`);
      continue;
    }
    if (effective.error !== undefined) {
      failures.push(`${config.path} effective configuration could not be read: ${effective.error}`);
      continue;
    }

    const expectedNoEmit = !config.emit;
    if (effective.noEmit !== expectedNoEmit) {
      failures.push(
        `${config.path} inventory emit=${String(config.emit)} but effective noEmit=${String(effective.noEmit)}`,
      );
    }
    if (effective.outDir !== config.outDir) {
      failures.push(
        `${config.path} inventory outDir=${config.outDir} but effective outDir=${String(effective.outDir)}`,
      );
    }
    if (effective.rootDir !== config.rootDir) {
      failures.push(
        `${config.path} inventory rootDir=${config.rootDir} but effective rootDir=${String(effective.rootDir)}`,
      );
    }
  }

  for (const file of input.discoveredFiles) {
    if (!seenPaths.has(file)) {
      failures.push(`discovered TypeScript config ${file} is unclassified`);
    }
  }
  for (const file of input.authorityFiles) {
    if (!seenPaths.has(file)) {
      failures.push(`compiler authority includes ${file}, but the inventory does not`);
    }
  }

  if (failures.length === 0) {
    facts.push(
      `${configs.length} TypeScript configs are classified and reconciled with aggregate typecheck, authority, and effective emit ownership`,
    );
  }
  return { ok: failures.length === 0, failures, facts };
}

/**
 * @param {string} extensionRoot
 * @returns {{ok: boolean, failures: string[], facts: string[]}}
 */
function checkTypeScriptConfigInventory(extensionRoot) {
  const inventory = JSON.parse(fs.readFileSync(path.join(extensionRoot, INVENTORY_PATH), 'utf8'));
  const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));
  const rawConfigs = Array.isArray(inventory.configs) ? inventory.configs : [];
  const configFiles = rawConfigs.filter(isInventoryConfig).map((config) => config.path);
  const tscEntry = path.join(extensionRoot, 'node_modules', 'typescript', 'bin', 'tsc');

  /** @type {Record<string, EffectiveConfig>} */
  const effectiveConfigs = {};
  for (const file of configFiles) {
    try {
      const shown = execFileSync(process.execPath, [tscEntry, '--showConfig', '-p', file], {
        cwd: extensionRoot,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      const parsed = JSON.parse(shown);
      effectiveConfigs[file] = {
        noEmit: parsed?.compilerOptions?.noEmit === true,
        outDir: normalizeOptionPath(extensionRoot, parsed?.compilerOptions?.outDir),
        rootDir: normalizeOptionPath(extensionRoot, parsed?.compilerOptions?.rootDir),
      };
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      effectiveConfigs[file] = {
        noEmit: false,
        outDir: undefined,
        rootDir: undefined,
        error: message.split(/\r?\n/, 1)[0] ?? message,
      };
    }
  }

  return evaluateTypeScriptConfigInventory({
    inventory,
    discoveredFiles: discoverTypeScriptConfigs(extensionRoot),
    authorityFiles: [...TSCONFIG_FILES].sort(),
    typecheckAll: packageJson.scripts?.['typecheck:all'] ?? '',
    effectiveConfigs,
  });
}

function main() {
  const reporter = createReporter('typescript-config-inventory');
  const result = checkTypeScriptConfigInventory(path.resolve(__dirname, '..'));
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
  reporter.info('PASS — TypeScript configuration authority inventory is reconciled.');
}

if (require.main === module) {
  main();
}

module.exports = {
  INVENTORY_PATH,
  discoverTypeScriptConfigs,
  evaluateTypeScriptConfigInventory,
  checkTypeScriptConfigInventory,
};
