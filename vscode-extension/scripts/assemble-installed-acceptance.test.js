const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const childProcess = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const {
  assembleInstalledAcceptance,
  prepareParentReceipt,
} = require('./assemble-installed-acceptance');

void test('assembles candidate-bound installed source and verified references', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-installed-acceptance-'));
  try {
    const parentPath = path.join(directory, 'parent.json');
    const sourcePath = path.join(directory, 'sources', 'journey.json');
    const verifiedPath = path.join(directory, 'verified', 'installed.json');
    const outputPath = path.join(directory, 'assembled.json');
    const candidate = {
      candidate_id: 'v0.18.0-rc1',
      frozen_product_sha: '0123456789abcdef0123456789abcdef01234567',
      artifact_set_id: 'v0.18.0-rc1-primary',
    };
    fs.writeFileSync(
      parentPath,
      JSON.stringify({ candidate, child_receipts: { installed_acceptance: {} } }),
    );
    const preparedPath = path.join(directory, 'prepared-parent.json');
    prepareParentReceipt({
      templatePath: parentPath,
      outputPath: preparedPath,
      candidateId: candidate.candidate_id,
      frozenProductSha: candidate.frozen_product_sha,
      artifactSetId: candidate.artifact_set_id,
    });
    fs.mkdirSync(path.dirname(sourcePath), { recursive: true });
    fs.mkdirSync(path.dirname(verifiedPath), { recursive: true });
    fs.writeFileSync(
      sourcePath,
      JSON.stringify({
        repository_sha: candidate.frozen_product_sha,
        schema_version: 1,
        outcome: 'not_proven',
        known_limitations: ['not a release claim'],
        claim_boundary: 'bounded packaged journey',
        server_identity: { source: 'packaged_vsix_bundle', path: 'bin/linux-x64/perl-lsp' },
        artifact_hashes: {
          vsix_sha256: 'a'.repeat(64),
          bundled_server_sha256: 'b'.repeat(64),
        },
        vsix_identity: {
          extension_id: 'EffortlessMetrics.perl-lsp-rs',
          version: '0.18.0',
          path: 'extension',
        },
      }),
    );
    fs.writeFileSync(
      verifiedPath,
      JSON.stringify({
        ...candidate,
        receipt_schema_version: 'installed_acceptance.v1',
        status: 'not_proven',
        claim_boundary: 'bounded packaged journey',
        limitation: 'not a release claim',
        source_receipt_sha256: crypto
          .createHash('sha256')
          .update(fs.readFileSync(sourcePath))
          .digest('hex'),
        artifact_hashes: {
          vsix_sha256: 'a'.repeat(64),
          bundled_server_sha256: 'b'.repeat(64),
        },
      }),
    );
    const assembled = assembleInstalledAcceptance({
      parentReceiptPath: preparedPath,
      sourceReceiptPath: sourcePath,
      verifiedArtifactPath: verifiedPath,
      outputPath,
      candidateId: candidate.candidate_id,
      frozenProductSha: candidate.frozen_product_sha,
      artifactSetId: candidate.artifact_set_id,
    });
    const installed = assembled.child_receipts.installed_acceptance;
    assert.equal(installed.source_artifact_path, 'sources/journey.json');
    assert.equal(installed.artifact_path, 'verified/installed.json');
    assert.deepEqual(installed.artifact_hashes, {
      vsix_sha256: 'a'.repeat(64),
      bundled_server_sha256: 'b'.repeat(64),
    });
    assert.equal(
      installed.source_sha256,
      crypto.createHash('sha256').update(fs.readFileSync(sourcePath)).digest('hex'),
    );
    assert.equal(fs.existsSync(outputPath), true);

    const cliOutputPath = path.join(directory, 'cli-assembled.json');
    const cli = childProcess.spawnSync(
      process.execPath,
      [
        path.join(__dirname, 'assemble-installed-acceptance.js'),
        '--parent',
        parentPath,
        '--source',
        sourcePath,
        '--verified',
        verifiedPath,
        '--output',
        cliOutputPath,
        '--candidate-id',
        candidate.candidate_id,
        '--frozen-product-sha',
        candidate.frozen_product_sha,
        '--artifact-set-id',
        candidate.artifact_set_id,
      ],
      { encoding: 'utf8' },
    );
    assert.equal(cli.status, 0, cli.stderr);
    assert.equal(fs.existsSync(cliOutputPath), true);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function withBoundReceipts(run) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-installed-acceptance-'));
  try {
    const candidate = {
      candidate_id: 'candidate-a',
      frozen_product_sha: 'a'.repeat(40),
      artifact_set_id: 'set-a',
    };
    const parentReceiptPath = path.join(directory, 'parent.json');
    const sourceReceiptPath = path.join(directory, 'source.json');
    const verifiedArtifactPath = path.join(directory, 'verified.json');
    const outputPath = path.join(directory, 'output.json');
    const artifactHashes = {
      vsix_sha256: 'b'.repeat(64),
      bundled_server_sha256: 'c'.repeat(64),
    };
    const sourceBytes = Buffer.from(
      JSON.stringify({
        repository_sha: candidate.frozen_product_sha,
        artifact_hashes: artifactHashes,
      }),
    );
    const sourceDigest = crypto.createHash('sha256').update(sourceBytes).digest('hex');
    const verified = {
      ...candidate,
      receipt_schema_version: 'installed_acceptance.v1',
      status: 'not_proven',
      claim_boundary: 'bounded packaged journey',
      limitation: 'not a release claim',
      source_receipt_sha256: sourceDigest,
      artifact_hashes: artifactHashes,
    };
    const verifiedBytes = Buffer.from(JSON.stringify(verified));
    fs.writeFileSync(
      parentReceiptPath,
      JSON.stringify({
        candidate,
        child_receipts: { installed_acceptance: {} },
      }),
    );
    fs.writeFileSync(sourceReceiptPath, sourceBytes);
    fs.writeFileSync(verifiedArtifactPath, verifiedBytes);
    run({
      options: {
        parentReceiptPath,
        sourceReceiptPath,
        verifiedArtifactPath,
        outputPath,
        candidateId: candidate.candidate_id,
        frozenProductSha: candidate.frozen_product_sha,
        artifactSetId: candidate.artifact_set_id,
      },
      sourceBytes,
      sourceDigest,
      verified,
      verifiedBytes,
    });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

void test('rejects missing, malformed, or stale verified source binding without replacing output', () => {
  withBoundReceipts(({ options, verified, sourceDigest }) => {
    const previousOutput = 'previous valid handoff\n';
    fs.writeFileSync(options.outputPath, previousOutput);
    for (const digest of [undefined, 'not-a-digest', 'd'.repeat(64), sourceDigest.toUpperCase()]) {
      fs.writeFileSync(
        options.verifiedArtifactPath,
        JSON.stringify({
          ...verified,
          source_receipt_sha256: digest,
        }),
      );
      assert.throws(() => assembleInstalledAcceptance(options), /source_receipt_sha256/);
      assert.equal(fs.readFileSync(options.outputPath, 'utf8'), previousOutput);
    }
  });
});

void test('CLI rejects a stale source envelope and leaves the prior output intact', () => {
  withBoundReceipts(({ options, verified }) => {
    const previousOutput = 'previous valid handoff\n';
    fs.writeFileSync(options.outputPath, previousOutput);
    fs.writeFileSync(
      options.verifiedArtifactPath,
      JSON.stringify({
        ...verified,
        source_receipt_sha256: 'd'.repeat(64),
      }),
    );
    const cli = childProcess.spawnSync(
      process.execPath,
      [
        path.join(__dirname, 'assemble-installed-acceptance.js'),
        '--parent',
        options.parentReceiptPath,
        '--source',
        options.sourceReceiptPath,
        '--verified',
        options.verifiedArtifactPath,
        '--output',
        options.outputPath,
        '--candidate-id',
        options.candidateId,
        '--frozen-product-sha',
        options.frozenProductSha,
        '--artifact-set-id',
        options.artifactSetId,
      ],
      { encoding: 'utf8' },
    );
    assert.equal(cli.status, 1, cli.stderr);
    assert.match(cli.stderr, /source_receipt_sha256 differs/);
    assert.equal(fs.readFileSync(options.outputPath, 'utf8'), previousOutput);
  });
});

void test('rejects changed source bytes even when candidate and artifact hashes remain equal', () => {
  withBoundReceipts(({ options, sourceBytes }) => {
    fs.writeFileSync(options.sourceReceiptPath, Buffer.concat([sourceBytes, Buffer.from('\n')]));
    assert.throws(() => assembleInstalledAcceptance(options), /source_receipt_sha256/);
    assert.equal(fs.existsSync(options.outputPath), false);
  });
});

void test('hashes the same single source and verified snapshots used for assembly', (t) => {
  withBoundReceipts(({ options, sourceBytes, sourceDigest, verifiedBytes }) => {
    const readFile = fs.readFileSync;
    const reads = new Map();
    t.mock.method(fs, 'readFileSync', (file, ...args) => {
      if (file === options.sourceReceiptPath || file === options.verifiedArtifactPath) {
        const count = (reads.get(file) ?? 0) + 1;
        reads.set(file, count);
        const snapshot = file === options.sourceReceiptPath ? sourceBytes : verifiedBytes;
        // A second open sees a changed receipt, as can happen during a producer rewrite.
        const bytes = count === 1 ? snapshot : Buffer.concat([snapshot, Buffer.from('\n')]);
        return args[0] === 'utf8' ? bytes.toString('utf8') : bytes;
      }
      return readFile(file, ...args);
    });
    const installed = assembleInstalledAcceptance(options).child_receipts.installed_acceptance;
    assert.equal(installed.source_sha256, sourceDigest);
    assert.equal(installed.sha256, crypto.createHash('sha256').update(verifiedBytes).digest('hex'));
    assert.equal(reads.get(options.sourceReceiptPath), 1);
    assert.equal(reads.get(options.verifiedArtifactPath), 1);
    assert.equal(installed.status, 'not_proven');
    t.mock.restoreAll();
  });
});

void test('rejects candidate-bound assembly when frozen SHA is absent', () => {
  assert.throws(
    () =>
      assembleInstalledAcceptance({
        parentReceiptPath: 'parent.json',
        sourceReceiptPath: 'source.json',
        verifiedArtifactPath: 'verified.json',
        outputPath: 'output.json',
        candidateId: 'v0.18.0-rc1',
        frozenProductSha: '',
        artifactSetId: 'set',
      }),
    /frozenProductSha is required/,
  );
});

void test('rejects assembly when the verified artifact is cross-candidate', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-installed-acceptance-'));
  try {
    const parentPath = path.join(directory, 'parent.json');
    const sourcePath = path.join(directory, 'source.json');
    const verifiedPath = path.join(directory, 'verified.json');
    const candidate = {
      candidate_id: 'candidate-a',
      frozen_product_sha: 'a'.repeat(40),
      artifact_set_id: 'set-a',
    };
    fs.writeFileSync(
      parentPath,
      JSON.stringify({ candidate, child_receipts: { installed_acceptance: {} } }),
    );
    fs.writeFileSync(sourcePath, JSON.stringify({ repository_sha: candidate.frozen_product_sha }));
    fs.writeFileSync(
      verifiedPath,
      JSON.stringify({
        candidate_id: 'candidate-b',
        frozen_product_sha: candidate.frozen_product_sha,
        artifact_set_id: candidate.artifact_set_id,
        receipt_schema_version: 'installed_acceptance.v1',
        artifact_hashes: {
          vsix_sha256: 'a'.repeat(64),
          bundled_server_sha256: 'b'.repeat(64),
        },
      }),
    );
    assert.throws(
      () =>
        assembleInstalledAcceptance({
          parentReceiptPath: parentPath,
          sourceReceiptPath: sourcePath,
          verifiedArtifactPath: verifiedPath,
          outputPath: path.join(directory, 'output.json'),
          candidateId: candidate.candidate_id,
          frozenProductSha: candidate.frozen_product_sha,
          artifactSetId: candidate.artifact_set_id,
        }),
      /candidate_id/,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

void test('rejects missing or mismatched installed artifact hashes', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-installed-acceptance-'));
  try {
    const parentPath = path.join(directory, 'parent.json');
    const sourcePath = path.join(directory, 'source.json');
    const verifiedPath = path.join(directory, 'verified.json');
    const candidate = {
      candidate_id: 'candidate-a',
      frozen_product_sha: 'a'.repeat(40),
      artifact_set_id: 'set-a',
    };
    fs.writeFileSync(
      parentPath,
      JSON.stringify({ candidate, child_receipts: { installed_acceptance: {} } }),
    );
    const sourceHashes = {
      vsix_sha256: 'a'.repeat(64),
      bundled_server_sha256: 'b'.repeat(64),
    };
    fs.writeFileSync(
      sourcePath,
      JSON.stringify({
        repository_sha: candidate.frozen_product_sha,
        artifact_hashes: sourceHashes,
      }),
    );
    const verified = {
      ...candidate,
      receipt_schema_version: 'installed_acceptance.v1',
      status: 'not_proven',
      claim_boundary: 'bounded packaged journey',
      limitation: 'not a release claim',
      artifact_hashes: sourceHashes,
    };
    const { artifact_hashes: omittedArtifactHashes, ...missingHashes } = verified;
    void omittedArtifactHashes;
    fs.writeFileSync(verifiedPath, JSON.stringify(missingHashes));
    assert.throws(
      () =>
        assembleInstalledAcceptance({
          parentReceiptPath: parentPath,
          sourceReceiptPath: sourcePath,
          verifiedArtifactPath: verifiedPath,
          outputPath: path.join(directory, 'missing-output.json'),
          candidateId: candidate.candidate_id,
          frozenProductSha: candidate.frozen_product_sha,
          artifactSetId: candidate.artifact_set_id,
        }),
      /artifact_hashes/,
    );

    const mismatched = {
      ...verified,
      artifact_hashes: { ...verified.artifact_hashes, bundled_server_sha256: 'c'.repeat(64) },
    };
    fs.writeFileSync(verifiedPath, JSON.stringify(mismatched));
    assert.throws(
      () =>
        assembleInstalledAcceptance({
          parentReceiptPath: parentPath,
          sourceReceiptPath: sourcePath,
          verifiedArtifactPath: verifiedPath,
          outputPath: path.join(directory, 'mismatch-output.json'),
          candidateId: candidate.candidate_id,
          frozenProductSha: candidate.frozen_product_sha,
          artifactSetId: candidate.artifact_set_id,
        }),
      /bundled-server SHA-256 differs/,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
