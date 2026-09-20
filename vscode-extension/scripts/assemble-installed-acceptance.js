const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function requireIdentity(value, field) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`${field} is required`);
  }
  return value.trim();
}

function requireArtifactHash(value, field) {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/i.test(value)) {
    throw new Error(`${field} must be a 64-character SHA-256 hex digest`);
  }
  return value.toLowerCase();
}

function relativeReceiptPath(parentPath, artifactPath) {
  const parentDirectory = path.dirname(parentPath);
  const relative = path.relative(parentDirectory, artifactPath).split(path.sep).join('/');
  if (!relative || relative === '..' || relative.startsWith('../') || path.isAbsolute(relative)) {
    throw new Error(`artifact must be below the parent receipt directory: ${artifactPath}`);
  }
  return relative;
}

function assembleInstalledAcceptance({
  parentReceiptPath,
  sourceReceiptPath,
  verifiedArtifactPath,
  outputPath,
  candidateId,
  frozenProductSha,
  artifactSetId,
}) {
  const identity = {
    candidateId: requireIdentity(candidateId, 'candidateId'),
    frozenProductSha: requireIdentity(frozenProductSha, 'frozenProductSha'),
    artifactSetId: requireIdentity(artifactSetId, 'artifactSetId'),
  };
  const parent = readJson(parentReceiptPath);
  const source = readJson(sourceReceiptPath);
  const verified = readJson(verifiedArtifactPath);

  if (!parent.candidate || parent.candidate.candidate_id !== identity.candidateId) {
    throw new Error('parent candidate identity does not match the requested candidate');
  }
  if (parent.candidate.frozen_product_sha !== identity.frozenProductSha) {
    throw new Error('parent frozen product SHA does not match the requested candidate');
  }
  if (parent.candidate.artifact_set_id !== identity.artifactSetId) {
    throw new Error('parent artifact-set identity does not match the requested candidate');
  }
  for (const [field, expected] of Object.entries({
    candidate_id: identity.candidateId,
    frozen_product_sha: identity.frozenProductSha,
    artifact_set_id: identity.artifactSetId,
  })) {
    if (verified[field] !== expected) {
      throw new Error(`verified artifact ${field} does not match candidate identity`);
    }
  }
  if (source.repository_sha !== identity.frozenProductSha) {
    throw new Error('source receipt repository SHA does not match candidate identity');
  }
  if (verified.receipt_schema_version !== 'installed_acceptance.v1') {
    throw new Error('verified artifact is not an installed_acceptance.v1 envelope');
  }
  const verifiedHashes = verified.artifact_hashes;
  if (!verifiedHashes || typeof verifiedHashes !== 'object') {
    throw new Error('verified artifact lacks artifact_hashes');
  }
  const artifactHashes = {
    vsix_sha256: requireArtifactHash(verifiedHashes.vsix_sha256, 'verified artifact vsix_sha256'),
    bundled_server_sha256: requireArtifactHash(
      verifiedHashes.bundled_server_sha256,
      'verified artifact bundled_server_sha256',
    ),
  };
  const sourceHashes = source.artifact_hashes;
  if (!sourceHashes || typeof sourceHashes !== 'object') {
    throw new Error('source receipt lacks artifact_hashes');
  }
  if (sourceHashes.vsix_sha256 !== artifactHashes.vsix_sha256) {
    throw new Error('source receipt VSIX SHA-256 differs from the verified artifact');
  }
  if (sourceHashes.bundled_server_sha256 !== artifactHashes.bundled_server_sha256) {
    throw new Error('source receipt bundled-server SHA-256 differs from the verified artifact');
  }

  const sourceBytes = fs.readFileSync(sourceReceiptPath);
  const verifiedBytes = fs.readFileSync(verifiedArtifactPath);
  const installed = parent.child_receipts?.installed_acceptance;
  if (!installed) {
    throw new Error('parent receipt lacks child_receipts.installed_acceptance');
  }
  installed.source_artifact_path = relativeReceiptPath(parentReceiptPath, sourceReceiptPath);
  installed.source_sha256 = sha256(sourceBytes);
  installed.artifact_path = relativeReceiptPath(parentReceiptPath, verifiedArtifactPath);
  installed.sha256 = sha256(verifiedBytes);
  installed.candidate_id = identity.candidateId;
  installed.status = verified.status;
  installed.claim_boundary = verified.claim_boundary;
  installed.limitation = verified.limitation;
  installed.artifact_hashes = artifactHashes;

  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(parent, null, 2)}\n`);
  return parent;
}

function prepareParentReceipt({
  templatePath,
  outputPath,
  candidateId,
  frozenProductSha,
  artifactSetId,
}) {
  const parent = readJson(templatePath);
  parent.candidate = {
    ...parent.candidate,
    candidate_id: requireIdentity(candidateId, 'candidateId'),
    frozen_product_sha: requireIdentity(frozenProductSha, 'frozenProductSha'),
    artifact_set_id: requireIdentity(artifactSetId, 'artifactSetId'),
  };
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(parent, null, 2)}\n`);
  return parent;
}

module.exports = { assembleInstalledAcceptance, prepareParentReceipt };

if (require.main === module) {
  const args = process.argv.slice(2);
  const value = (name) => {
    const index = args.indexOf(name);
    if (index < 0 || args[index + 1] === undefined) {
      throw new Error(`${name} is required`);
    }
    return args[index + 1];
  };
  const identity = {
    candidateId: value('--candidate-id'),
    frozenProductSha: value('--frozen-product-sha'),
    artifactSetId: value('--artifact-set-id'),
  };
  if (args.includes('--prepare-parent')) {
    prepareParentReceipt({
      templatePath: value('--template'),
      outputPath: value('--output'),
      ...identity,
    });
  } else {
    assembleInstalledAcceptance({
      parentReceiptPath: value('--parent'),
      sourceReceiptPath: value('--source'),
      verifiedArtifactPath: value('--verified'),
      outputPath: value('--output'),
      ...identity,
    });
  }
}
