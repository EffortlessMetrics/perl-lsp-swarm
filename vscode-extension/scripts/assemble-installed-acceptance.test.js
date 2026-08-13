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

void test('rejects candidate-bound assembly when frozen SHA is absent', () => {
  assert.throws(
    () =>
      assembleInstalledAcceptance({
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
