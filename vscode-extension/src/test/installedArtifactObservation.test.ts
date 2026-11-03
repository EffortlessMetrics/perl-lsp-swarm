import * as crypto from 'node:crypto';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import {
  ArtifactObservationError,
  observeArtifact,
  requireSameArtifact,
  requireSameBytes,
} from './installedArtifactObservation';

describe('installed artifact byte observations', () => {
  let directory: string;
  beforeEach(() => {
    directory = fs.mkdtempSync(path.join(os.tmpdir(), 'installed-byte-observation-'));
  });
  afterEach(() => {
    fs.rmSync(directory, { recursive: true, force: true });
  });
  function file(name: string, bytes = 'known server bytes'): string {
    const target = path.join(directory, name);
    fs.writeFileSync(target, bytes);
    return target;
  }
  test('reads known bytes and preserves unchanged snapshot identity', () => {
    const target = file('server');
    const observed = observeArtifact(target, directory);
    expect(observed).toEqual({
      path: fs.realpathSync(target),
      size: 18,
      sha256: crypto.createHash('sha256').update('known server bytes').digest('hex'),
    });
    requireSameArtifact(observed, observeArtifact(target, directory));
  });
  test('same-length replacement cannot hide behind a declared expected hash', () => {
    const target = file('server', 'AAAA');
    const before = observeArtifact(target);
    fs.writeFileSync(target, 'BBBB');
    const after = observeArtifact(target);
    expect(() => requireSameArtifact(before, after)).toThrow(ArtifactObservationError);
    expect(() => requireSameBytes(before, after)).toThrow(ArtifactObservationError);
  });
  test('restart selecting another same-byte file is a different artifact', () => {
    const before = observeArtifact(file('first'));
    const after = observeArtifact(file('second'));
    requireSameBytes(before, after);
    expect(() => requireSameArtifact(before, after)).toThrow(ArtifactObservationError);
  });
  test('missing, directory and relative paths refuse observation', () => {
    expect(() => observeArtifact(path.join(directory, 'missing'))).toThrow(
      ArtifactObservationError,
    );
    expect(() => observeArtifact(directory)).toThrow(ArtifactObservationError);
    expect(() => observeArtifact('relative')).toThrow(ArtifactObservationError);
  });
  test('prefix sibling refuses canonical containment', () => {
    const root = path.join(directory, 'installed');
    fs.mkdirSync(root);
    const sibling = path.join(directory, 'installed-other');
    fs.mkdirSync(sibling);
    const target = path.join(sibling, 'server');
    fs.writeFileSync(target, 'server');
    expect(() => observeArtifact(target, root)).toThrow(ArtifactObservationError);
  });
  test('directory junction or symlink escape refuses containment', () => {
    const root = path.join(directory, 'installed');
    const external = path.join(directory, 'external');
    fs.mkdirSync(root);
    fs.mkdirSync(external);
    fs.writeFileSync(path.join(external, 'server'), 'server');
    const link = path.join(root, 'linked');
    fs.symlinkSync(external, link, process.platform === 'win32' ? 'junction' : 'dir');
    try {
      expect(() => observeArtifact(link, root)).toThrow(ArtifactObservationError);
      expect(() => observeArtifact(path.join(link, 'server'), root)).toThrow(
        ArtifactObservationError,
      );
    } finally {
      fs.unlinkSync(link);
    }
  });
});
