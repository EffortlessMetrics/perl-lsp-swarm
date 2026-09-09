#!/usr/bin/env node

const {
  buildVsixCandidatePayloadManifest,
  canonicalVsixPayloadJson,
  deriveVsixTargetProjection,
} = require('../src/vsixPackageProjection.ts');

let input = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => { input += chunk; });
process.stdin.on('end', () => {
  try {
    const value = JSON.parse(input);
    const projection = deriveVsixTargetProjection(value.projection).find(
      (row) => row.rustTarget === value.target,
    );
    if (!projection) throw new Error(`projection has no target ${value.target}`);
    const manifest = buildVsixCandidatePayloadManifest({
      extension: value.extension,
      candidate: value.candidate,
      releaseTopologySha256: value.releaseTopologySha256,
      projection,
      packageInventorySha256: value.packageInventorySha256,
      server: { ...value.server, member: projection.serverMember },
      dap: { ...value.dap, member: projection.dapMember },
    });
    process.stdout.write(canonicalVsixPayloadJson(manifest));
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
});
