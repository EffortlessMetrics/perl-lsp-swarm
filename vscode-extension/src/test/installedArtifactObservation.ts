import * as crypto from 'node:crypto';
import * as fs from 'node:fs';
import * as path from 'node:path';

export interface ArtifactObservation {
  readonly path: string;
  readonly sha256: string;
  readonly size: number;
}

export class ArtifactObservationError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ArtifactObservationError';
  }
}

/** Snapshot actual bytes; this is neither source authentication nor process-image attestation. */
export function observeArtifact(file: string, containmentRoot?: string): ArtifactObservation {
  try {
    if (!path.isAbsolute(file)) {
      throw new ArtifactObservationError('invalid_path', 'artifact path must be absolute');
    }
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink() || !stat.isFile()) {
      throw new ArtifactObservationError('nonregular', 'artifact must be a regular non-link file');
    }
    const canonical = fs.realpathSync(file);
    if (containmentRoot !== undefined) {
      const relative = path.relative(fs.realpathSync(containmentRoot), canonical);
      if (
        !relative ||
        path.isAbsolute(relative) ||
        relative === '..' ||
        relative.startsWith(`..${path.sep}`)
      ) {
        throw new ArtifactObservationError(
          'outside_root',
          'artifact escapes its canonical installed root',
        );
      }
    }
    const bytes = fs.readFileSync(canonical);
    return Object.freeze({
      path: canonical,
      sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
      size: bytes.length,
    });
  } catch (error: unknown) {
    if (error instanceof ArtifactObservationError) throw error;
    throw new ArtifactObservationError(
      'unavailable',
      error instanceof Error ? error.message : String(error),
    );
  }
}

export function requireSameArtifact(before: ArtifactObservation, after: ArtifactObservation): void {
  if (before.path !== after.path || before.sha256 !== after.sha256 || before.size !== after.size) {
    throw new ArtifactObservationError(
      'changed_artifact',
      'artifact path or observed bytes changed',
    );
  }
}

export function requireSameBytes(input: ArtifactObservation, installed: ArtifactObservation): void {
  if (input.sha256 !== installed.sha256 || input.size !== installed.size) {
    throw new ArtifactObservationError(
      'byte_mismatch',
      'installed bytes differ from observed input',
    );
  }
}
