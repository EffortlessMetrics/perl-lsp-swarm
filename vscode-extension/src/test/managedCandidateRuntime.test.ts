/**
 * Unit tests for the managed candidate runtime wiring (#10083): the
 * persistence, enumeration, and garbage-collection layer that feeds the
 * landed pure policy in managedCandidateSelection.ts (#11780).
 *
 * The load-bearing falsifiers:
 * 1. GC must never remove a candidate a live host reference points at.
 * 2. Retention outcomes must match the landed policy table exactly — the
 *    wiring never re-adjudicates classification.
 * 3. Unknown, partial, or incomplete evidence blocks destructive GC.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, test } from '@jest/globals';
import {
  buildManagedCandidateManifest,
  type ManagedCandidateManifest,
  type ManagedCandidateSubject,
} from '../managedCacheProtocol';
import {
  MANAGED_CANDIDATE_MANIFEST_FILE,
  MANAGED_CURRENT_SELECTION_FILE,
  acquireLaunchManagedCandidateReference,
  acquireSessionManagedHostReference,
  collectStaleManagedCandidates,
  commitManagedCandidateSelection,
  enumerateManagedCandidateCatalog,
  enumerateManagedHostReferences,
  mayReleaseManagedCandidateReferences,
  readInstalledManagedCandidateManifest,
  readManagedCurrentSelection,
  readSessionManagedHostReference,
  releaseManagedCandidateSessionReferences,
  writeInstalledManagedCandidateManifest,
} from '../managedCandidateRuntime';

/**
 * Each seed derives its own hex digit per digest field so every fixture
 * subject differs in every field: a wrong identity implementation that
 * narrows the candidate hash to any subset of the subject is falsified,
 * not silently re-scenarioed.
 */
const SEED_INDEX: Record<string, number> = { a: 1, b: 2, c: 3, d: 4, e: 5 };

function seedHexDigit(seed: string, offset: number): string {
  const index = SEED_INDEX[seed] ?? 6;
  return ((index + offset) % 16).toString(16);
}

function subject(seed: string): ManagedCandidateSubject {
  return {
    release: '0.18.0',
    version: `0.18.${seed}`,
    target: 'x86_64-unknown-linux-gnu',
    topology_digest: seedHexDigit(seed, 0).repeat(64),
    perllsp_digest: seedHexDigit(seed, 5).repeat(64),
    perl_dap_digest: seed === 'a' ? null : seedHexDigit(seed, 10).repeat(64),
  };
}

function hexSeed(seed: string): string {
  return seed.repeat(64).slice(0, 64);
}

const quietLog = (): void => {
  /* proof runs stay quiet; assertions read the returned result */
};

let baseDir: string;

beforeEach(() => {
  baseDir = fs.mkdtempSync(path.join(os.tmpdir(), 'managed-runtime-test-'));
});

afterEach(() => {
  fs.rmSync(baseDir, { recursive: true, force: true });
});

/**
 * Populates one install dir with a valid manifest. `ageSeconds` orders the
 * namespace newest-first the way real installs do; it is retention metadata
 * only — no assertion below lets age authorize a deletion.
 */
function installCandidate(
  namespaceDir: string,
  dirName: string,
  candidateSubject: ManagedCandidateSubject,
  ageSeconds: number,
): { dir: string; manifest: ManagedCandidateManifest } {
  const dir = path.join(namespaceDir, dirName);
  fs.mkdirSync(dir, { recursive: true });
  const manifest = buildManagedCandidateManifest(candidateSubject);
  fs.writeFileSync(path.join(dir, MANAGED_CANDIDATE_MANIFEST_FILE), JSON.stringify(manifest));
  const stamp = Date.now() / 1000 - ageSeconds;
  fs.utimesSync(dir, stamp, stamp);
  return { dir, manifest };
}

describe('managed candidate persistence', () => {
  test('manifest write/read roundtrip mints a canonical candidate identity', () => {
    const installDir = path.join(baseDir, 'v0.18.0-a');
    fs.mkdirSync(installDir);

    const written = writeInstalledManagedCandidateManifest(installDir, subject('a'), quietLog);

    expect(written).not.toBeNull();
    expect(written?.candidate_id).toMatch(/^candidate-[0-9a-f]{64}$/);
    expect(readInstalledManagedCandidateManifest(installDir)?.candidate_id).toBe(
      written?.candidate_id,
    );
  });

  test('selection commit mints a generation and increments across commits', () => {
    const first = commitManagedCandidateSelection(
      baseDir,
      buildManagedCandidateManifest(subject('a')),
      quietLog,
    );
    const second = commitManagedCandidateSelection(
      baseDir,
      buildManagedCandidateManifest(subject('b')),
      quietLog,
    );

    expect(first?.selection_generation).toBe(1);
    expect(second?.selection_generation).toBe(2);
    expect(readManagedCurrentSelection(baseDir)?.candidate_id).toBe(
      buildManagedCandidateManifest(subject('b')).candidate_id,
    );
  });

  test('selection commit refuses to overwrite an invalid prior generation', () => {
    const invalidPrior = {
      schema_version: 'managed_current_selection.v1',
      selection_generation: 0,
      candidate_id: `candidate-${hexSeed('a')}`,
    };
    fs.writeFileSync(
      path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE),
      JSON.stringify(invalidPrior),
    );

    const committed = commitManagedCandidateSelection(
      baseDir,
      buildManagedCandidateManifest(subject('b')),
      quietLog,
    );

    // The refusal leaves the prior bytes untouched; the landed classifier,
    // not the reader, is what rejects an invalid generation fail-safe.
    expect(committed).toBeNull();
    expect(
      JSON.parse(fs.readFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE), 'utf8')),
    ).toEqual(invalidPrior);
  });

  test('selection commit refuses to replace an unreadable prior record', () => {
    // Evidence that exists but cannot be interpreted must not be replaced
    // by a generation-1 reset that destroys it.
    const garbage = '{garbage';
    fs.writeFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE), garbage);

    const committed = commitManagedCandidateSelection(
      baseDir,
      buildManagedCandidateManifest(subject('b')),
      quietLog,
    );

    expect(committed).toBeNull();
    expect(fs.readFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE), 'utf8')).toBe(
      garbage,
    );
  });
});

describe('managed host reference lifecycle', () => {
  test('acquire persists a live reference this session can read back', () => {
    const reference = acquireSessionManagedHostReference(
      baseDir,
      'window-a',
      buildManagedCandidateManifest(subject('a')).candidate_id,
      quietLog,
    );

    expect(reference?.state).toBe('live');
    expect(readSessionManagedHostReference(baseDir, 'window-a')?.candidate_id).toBe(
      reference?.candidate_id,
    );
  });

  test('acquire rejects session ids that are not path-safe', () => {
    const reference = acquireSessionManagedHostReference(
      baseDir,
      '../escape',
      buildManagedCandidateManifest(subject('a')).candidate_id,
      quietLog,
    );

    expect(reference).toBeNull();
    expect(fs.existsSync(path.join(baseDir, 'host-refs'))).toBe(false);
  });

  test('acquire rejects session ids containing a Windows-drive/ADS colon', () => {
    // On NTFS a ':' in a filename addresses an alternate data stream, which
    // readdir never lists — a reference could hide outside a "complete"
    // enumeration, the unsafe direction. The runtime layer is deliberately
    // narrower than the landed policy's in-memory session grammar.
    const reference = acquireSessionManagedHostReference(
      baseDir,
      'window:x',
      buildManagedCandidateManifest(subject('a')).candidate_id,
      quietLog,
    );

    expect(reference).toBeNull();
    expect(fs.existsSync(path.join(baseDir, 'host-refs'))).toBe(false);
  });

  test('reselection overwrites the session own earlier reference', () => {
    // The documented reselection transition: one file per session, naming
    // the candidate the session will launch next. An implementation that
    // appended a second file, or refused the second acquire, fails here.
    const candidateA = buildManagedCandidateManifest(subject('a')).candidate_id;
    const candidateB = buildManagedCandidateManifest(subject('b')).candidate_id;
    acquireSessionManagedHostReference(baseDir, 'window-a', candidateA, quietLog);

    const rebound = acquireSessionManagedHostReference(baseDir, 'window-a', candidateB, quietLog);

    expect(rebound?.candidate_id).toBe(candidateB);
    expect(readSessionManagedHostReference(baseDir, 'window-a')?.candidate_id).toBe(candidateB);
    expect(fs.readdirSync(path.join(baseDir, 'host-refs'))).toEqual(['window-a.json']);
  });

  test("release marks only this session's references across namespaces", () => {
    const candidateA = buildManagedCandidateManifest(subject('a')).candidate_id;
    const candidateB = buildManagedCandidateManifest(subject('b')).candidate_id;
    const nsA = path.join(baseDir, 'managed', 'x86_64-unknown-linux-gnu');
    const nsB = path.join(baseDir, 'managed', 'aarch64-pc-windows-msvc');
    acquireSessionManagedHostReference(nsA, 'window-a', candidateA, quietLog);
    acquireSessionManagedHostReference(nsB, 'window-a', candidateB, quietLog);
    acquireSessionManagedHostReference(nsA, 'window-b', candidateA, quietLog);

    releaseManagedCandidateSessionReferences(baseDir, 'window-a', quietLog);

    expect(readSessionManagedHostReference(nsA, 'window-a')?.state).toBe('released');
    expect(readSessionManagedHostReference(nsB, 'window-a')?.state).toBe('released');
    // Another session's reference is never released by this session.
    expect(readSessionManagedHostReference(nsA, 'window-b')?.state).toBe('live');
  });

  test('release leaves unrecognizable reference records untouched', () => {
    const refsDir = path.join(baseDir, 'managed', 'x86_64-unknown-linux-gnu', 'host-refs');
    fs.mkdirSync(refsDir, { recursive: true });
    fs.writeFileSync(path.join(refsDir, 'window-a.json'), JSON.stringify({ junk: true }));

    releaseManagedCandidateSessionReferences(baseDir, 'window-a', quietLog);

    expect(fs.readFileSync(path.join(refsDir, 'window-a.json'), 'utf8')).toBe(
      JSON.stringify({ junk: true }),
    );
  });

  test('launch binding derives the namespace and candidate from the server path', () => {
    const installDir = path.join(baseDir, 'managed', 'x86_64-unknown-linux-gnu', 'v0.18.0-a');
    const manifest = installCandidate(
      path.join(baseDir, 'managed', 'x86_64-unknown-linux-gnu'),
      'v0.18.0-a',
      subject('a'),
      0,
    );

    const bound = acquireLaunchManagedCandidateReference(
      path.join(installDir, 'perllsp'),
      'window-a',
      quietLog,
    );

    expect(bound).toBe(manifest.manifest.candidate_id);
    expect(
      readSessionManagedHostReference(
        path.join(baseDir, 'managed', 'x86_64-unknown-linux-gnu'),
        'window-a',
      )?.state,
    ).toBe('live');
  });

  test('launch binding is a no-op for install dirs without a manifest', () => {
    const installDir = path.join(baseDir, 'legacy-flat');
    fs.mkdirSync(installDir);

    expect(
      acquireLaunchManagedCandidateReference(
        path.join(installDir, 'perllsp'),
        'window-a',
        quietLog,
      ),
    ).toBeNull();
    expect(fs.existsSync(path.join(baseDir, 'host-refs'))).toBe(false);
  });

  test('only a proven-terminal shutdown may release session references', () => {
    // The lifecycle's stop() resolves without rejecting even when stop or
    // dispose timed out (state `failed`), so only the clean `stopped` state
    // counts as termination evidence (#10083).
    expect(mayReleaseManagedCandidateReferences('stopped')).toBe(true);
    for (const unproven of ['failed', 'stopping', 'running', 'starting', 'resolving']) {
      expect(mayReleaseManagedCandidateReferences(unproven)).toBe(false);
    }
  });
});

describe('managed host reference enumeration', () => {
  test('a namespace that never wrote references enumerates exhaustively empty', () => {
    expect(enumerateManagedHostReferences(baseDir)).toEqual({ references: [], complete: true });
  });

  test('an unparsable reference file makes the enumeration incomplete', () => {
    const refsDir = path.join(baseDir, 'host-refs');
    fs.mkdirSync(refsDir, { recursive: true });
    fs.writeFileSync(path.join(refsDir, 'window-a.json'), '{not json');

    expect(enumerateManagedHostReferences(baseDir).complete).toBe(false);
  });
});

describe('managed candidate catalog enumeration', () => {
  test('manifest dirs map to candidate ids newest-first; manifest-less dirs stay out', () => {
    const newer = installCandidate(baseDir, 'newer', subject('a'), 0);
    const older = installCandidate(baseDir, 'older', subject('b'), 60);
    fs.mkdirSync(path.join(baseDir, 'legacy-no-manifest'));

    const catalog = enumerateManagedCandidateCatalog(baseDir);

    expect(catalog.complete).toBe(true);
    expect([...catalog.candidateDirs.keys()]).toEqual([
      newer.manifest.candidate_id,
      older.manifest.candidate_id,
    ]);
    expect(catalog.candidateDirs.get(newer.manifest.candidate_id)).toEqual([newer.dir]);
  });

  test('a manifest that exists but cannot be parsed makes enumeration incomplete', () => {
    fs.mkdirSync(path.join(baseDir, 'broken'));
    fs.writeFileSync(path.join(baseDir, 'broken', MANAGED_CANDIDATE_MANIFEST_FILE), '{not json');

    expect(enumerateManagedCandidateCatalog(baseDir).complete).toBe(false);
  });
});

describe('collectStaleManagedCandidates — landed policy table', () => {
  interface NamespaceFixture {
    current: string;
    prior: string;
    live: string;
    releasedStale: string;
    unreferencedStale: string;
    uncatalogued: string;
  }

  function populatedNamespace(): NamespaceFixture {
    // Newest first: current C, prior-known-good P, live-referenced L,
    // released-referenced S1, unreferenced S2, manifest-less U.
    const c = installCandidate(baseDir, 'c-current', subject('a'), 0);
    const p = installCandidate(baseDir, 'p-prior', subject('b'), 60);
    const l = installCandidate(baseDir, 'l-live', subject('c'), 120);
    const s1 = installCandidate(baseDir, 's1-released', subject('d'), 180);
    const s2 = installCandidate(baseDir, 's2-unreferenced', subject('e'), 240);
    const u = path.join(baseDir, 'u-no-manifest');
    fs.mkdirSync(u);
    fs.writeFileSync(path.join(u, 'perllsp'), 'legacy bytes');

    commitManagedCandidateSelection(baseDir, c.manifest, quietLog);
    acquireSessionManagedHostReference(baseDir, 'window-live', l.manifest.candidate_id, quietLog);
    const released = acquireSessionManagedHostReference(
      baseDir,
      'window-gone',
      s1.manifest.candidate_id,
      quietLog,
    );
    fs.writeFileSync(
      path.join(baseDir, 'host-refs', 'window-gone.json'),
      JSON.stringify({ ...released, state: 'released' }),
    );

    return {
      current: c.dir,
      prior: p.dir,
      live: l.dir,
      releasedStale: s1.dir,
      unreferencedStale: s2.dir,
      uncatalogued: u,
    };
  }

  test('deletes only stale_unreferenced generations and preserves every other class', () => {
    const dirs = populatedNamespace();

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.blockedReason).toBeNull();
    expect(result.removed.sort()).toEqual([dirs.releasedStale, dirs.unreferencedStale].sort());
    expect(fs.existsSync(dirs.current)).toBe(true); // current_default
    expect(fs.existsSync(dirs.prior)).toBe(true); // previous-known-good retention
    expect(fs.existsSync(dirs.live)).toBe(true); // live_referenced
    expect(fs.existsSync(dirs.uncatalogued)).toBe(true); // uncatalogued legacy bytes
    expect(result.retained.current_default).toBeDefined();
    expect(result.retained.compatible_retained).toBeDefined();
    expect(result.retained.live_referenced).toBeDefined();
  });

  test('GC never removes the candidate a live host reference points at', () => {
    // Current moves to a fresh generation while window-a still runs the old
    // one: the live reference must survive as deletion authority.
    const old = installCandidate(baseDir, 'old-running', subject('a'), 300);
    const fresh = installCandidate(baseDir, 'fresh-current', subject('b'), 0);
    commitManagedCandidateSelection(baseDir, fresh.manifest, quietLog);
    acquireSessionManagedHostReference(baseDir, 'window-a', old.manifest.candidate_id, quietLog);

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.removed).toEqual([]);
    expect(fs.existsSync(old.dir)).toBe(true);
    expect(result.retained.live_referenced).toContain(old.manifest.candidate_id);
    expect(result.retained.current_default).toContain(fresh.manifest.candidate_id);
  });

  test('rerunning GC with unchanged complete inputs is idempotent', () => {
    populatedNamespace();
    collectStaleManagedCandidates(baseDir, quietLog);

    const second = collectStaleManagedCandidates(baseDir, quietLog);

    expect(second.blockedReason).toBeNull();
    expect(second.removed).toEqual([]);
  });

  test('an absent or unreadable current-selection record blocks destructive GC', () => {
    const stale = installCandidate(baseDir, 'stale', subject('a'), 0);
    // No selection record was ever committed.

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.blockedReason).toContain('current selection');
    expect(result.removed).toEqual([]);
    expect(fs.existsSync(stale.dir)).toBe(true);

    fs.writeFileSync(path.join(baseDir, MANAGED_CURRENT_SELECTION_FILE), '{garbage');
    const garbage = collectStaleManagedCandidates(baseDir, quietLog);
    expect(garbage.blockedReason).toContain('current selection');
    expect(garbage.removed).toEqual([]);
  });

  test('incomplete host-reference enumeration blocks destructive GC', () => {
    const current = installCandidate(baseDir, 'current', subject('a'), 0);
    const stale = installCandidate(baseDir, 'stale', subject('b'), 60);
    commitManagedCandidateSelection(baseDir, current.manifest, quietLog);
    const refsDir = path.join(baseDir, 'host-refs');
    fs.mkdirSync(refsDir, { recursive: true });
    fs.writeFileSync(path.join(refsDir, 'unreadable.json'), '{not json');

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.blockedReason).toContain('host reference enumeration');
    expect(result.removed).toEqual([]);
    expect(fs.existsSync(stale.dir)).toBe(true);
  });

  test('a structurally invalid reference record is unknown evidence, not absence', () => {
    const current = installCandidate(baseDir, 'current', subject('a'), 0);
    const stale = installCandidate(baseDir, 'stale', subject('b'), 60);
    commitManagedCandidateSelection(baseDir, current.manifest, quietLog);
    const refsDir = path.join(baseDir, 'host-refs');
    fs.mkdirSync(refsDir, { recursive: true });
    // Parses, but carries a schema this policy cannot interpret.
    fs.writeFileSync(
      path.join(refsDir, 'future.json'),
      JSON.stringify({ schema_version: 'managed_host_candidate_reference.v9' }),
    );

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    // Enumeration completed, so GC was not blocked — but every candidate
    // classifies unknown_not_safe_to_delete and nothing is removed.
    expect(result.blockedReason).toBeNull();
    expect(result.removed).toEqual([]);
    expect(fs.existsSync(stale.dir)).toBe(true);
    expect(result.retained.unknown_not_safe_to_delete).toBeDefined();
  });

  test('a malformed catalog manifest anywhere blocks deletion of every candidate', () => {
    const current = installCandidate(baseDir, 'current', subject('a'), 0);
    const stale = installCandidate(baseDir, 'stale', subject('b'), 60);
    commitManagedCandidateSelection(baseDir, current.manifest, quietLog);
    const broken = path.join(baseDir, 'broken');
    fs.mkdirSync(broken);
    fs.writeFileSync(
      path.join(broken, MANAGED_CANDIDATE_MANIFEST_FILE),
      JSON.stringify({ schema_version: 'managed_candidate_manifest.v1', candidate_id: 'nope' }),
    );

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.blockedReason).toBeNull();
    expect(result.removed).toEqual([]);
    expect(fs.existsSync(stale.dir)).toBe(true);
    expect(result.retained.partial_or_invalid).toBeDefined();
  });

  test('a catalog manifest that exists but cannot be parsed blocks GC outright', () => {
    // Distinct from the structurally invalid entry above: an unparseable
    // manifest makes enumeration itself incomplete, and the collector must
    // refuse before classifying anything.
    const current = installCandidate(baseDir, 'current', subject('a'), 0);
    const stale = installCandidate(baseDir, 'stale', subject('b'), 60);
    commitManagedCandidateSelection(baseDir, current.manifest, quietLog);
    const broken = path.join(baseDir, 'broken');
    fs.mkdirSync(broken);
    fs.writeFileSync(path.join(broken, MANAGED_CANDIDATE_MANIFEST_FILE), '{not json');

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.blockedReason).toContain('candidate catalog');
    expect(result.removed).toEqual([]);
    expect(fs.existsSync(stale.dir)).toBe(true);
  });

  test('duplicate candidate ids across dirs share one classification', () => {
    // A forced reinstall of the same release mints the same subject twice.
    // Older duplicates beyond the prior-known-good generation are collected
    // together; the current selection's own duplicate stays put.
    const staleTwinOld = installCandidate(baseDir, 'stale-twin-old', subject('a'), 300);
    const staleTwinNew = installCandidate(baseDir, 'stale-twin-new', subject('a'), 240);
    const prior = installCandidate(baseDir, 'prior', subject('b'), 120);
    const current = installCandidate(baseDir, 'current', subject('c'), 0);
    commitManagedCandidateSelection(baseDir, current.manifest, quietLog);

    const result = collectStaleManagedCandidates(baseDir, quietLog);

    expect(result.removed.sort()).toEqual([staleTwinOld.dir, staleTwinNew.dir].sort());
    expect(fs.existsSync(prior.dir)).toBe(true);
    expect(fs.existsSync(current.dir)).toBe(true);
  });
});
