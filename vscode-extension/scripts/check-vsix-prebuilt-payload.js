#!/usr/bin/env node

const crypto = require('node:crypto');
const yauzl = require('yauzl');

async function verifyPayloadMember(vsix, member, expected) {
  const archive = await yauzl.openPromise(vsix, {
    lazyEntries: true,
    decodeStrings: false,
    validateEntrySizes: true,
  });
  let matches = 0;
  try {
    for await (const entry of archive.eachEntry()) {
      const name = Buffer.from(entry.fileName).toString('utf8');
      if (name !== `extension/${member}`) continue;
      matches += 1;
      if (matches > 1) throw new Error(`archive has duplicate payload member: ${member}`);
      const unixMode = (entry.externalFileAttributes >>> 16) & 0xffff;
      const fileType = unixMode & 0xf000;
      const dosDirectory = entry.externalFileAttributes & 0x10;
      if ((fileType !== 0 && fileType !== 0x8000) || dosDirectory || name.endsWith('/')) {
        throw new Error(`archive payload member is not a regular file: ${member}`);
      }
      const stream = await archive.openReadStreamPromise(entry);
      const hash = crypto.createHash('sha256');
      for await (const chunk of stream) hash.update(chunk);
      const actual = hash.digest('hex');
      if (actual !== expected) {
        throw new Error(`archive payload SHA mismatch for ${member}: ${actual}`);
      }
    }
  } finally {
    archive.close();
  }
  if (matches === 0) throw new Error(`archive payload is missing extension/${member}`);
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : '';
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
    const {
      collectArchiveInventory,
      semanticInventorySha256,
    } = require('./check-vsix-inventory-transition');
    const actual = await collectArchiveInventory(vsix);
    const digest = semanticInventorySha256(actual.inventory);
    if (digest !== expectedInventory) {
      throw new Error(`archive inventory SHA mismatch: ${digest}`);
    }
    return;
  }
  await verifyPayloadMember(vsix, member, expected);
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

module.exports = { main, verifyPayloadMember };
