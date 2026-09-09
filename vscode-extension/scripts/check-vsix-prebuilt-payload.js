#!/usr/bin/env node

const crypto = require('node:crypto');
const fs = require('node:fs');
const yauzl = require('yauzl');

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : '';
}

function inventoryDigest(inventory) {
  return crypto.createHash('sha256').update(JSON.stringify(inventory)).digest('hex');
}

async function main() {
  const vsix = argument('--vsix');
  const member = argument('--member');
  const expectedInventory = argument('--inventory-sha256');
  const expected = argument('--sha256') ?? '';
  if (!vsix || (!member && !expectedInventory) || (member && !/^[0-9a-f]{64}$/.test(expected))) {
    throw new Error(
      'usage: --vsix <path> (--member <archive member> --sha256 <digest> | --inventory-sha256 <digest>)',
    );
  }
  if (expectedInventory) {
    if (!/^[0-9a-f]{64}$/.test(expectedInventory)) {
      throw new Error('--inventory-sha256 must be a lowercase SHA-256 digest');
    }
    const { collectArchiveInventory } = require('./check-vsix-inventory-transition');
    const actual = await collectArchiveInventory(vsix);
    const digest = inventoryDigest(actual.inventory);
    if (digest !== expectedInventory) {
      throw new Error(`archive inventory SHA mismatch: ${digest}`);
    }
    return;
  }
  const archive = await yauzl.fromBufferPromise(fs.readFileSync(vsix), {
    lazyEntries: true,
    decodeStrings: false,
    validateEntrySizes: true,
  });
  let found = false;
  try {
    for await (const entry of archive.eachEntry()) {
      if (Buffer.from(entry.fileName).toString('utf8') !== `extension/${member}`) continue;
      const stream = await archive.openReadStreamPromise(entry);
      const hash = crypto.createHash('sha256');
      for await (const chunk of stream) hash.update(chunk);
      const actual = hash.digest('hex');
      if (actual !== expected) {
        throw new Error(`archive payload SHA mismatch for ${member}: ${actual}`);
      }
      found = true;
      break;
    }
  } finally {
    archive.close();
  }
  if (!found) throw new Error(`archive payload is missing extension/${member}`);
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

module.exports = { main };
