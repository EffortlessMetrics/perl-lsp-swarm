import {
  buildManagedCandidateManifest,
  type ManagedCandidateSubject,
} from '../managedCacheProtocol';
import {
  candidateBytesMayChangeAfterPublication,
  classifyManagedCandidateRetention,
  createManagedHostReference,
  mayGarbageCollectManagedCandidate,
  publishManagedCurrentSelection,
  releaseManagedHostReference,
  resolveManagedCandidateForHost,
  validateManagedCurrentSelection,
  type ManagedCandidateCatalogEntry,
} from '../managedCandidateSelection';

function subject(seed: string): ManagedCandidateSubject {
  return {
    release: '0.18.0',
    version: `0.18.${seed}`,
    target: 'x86_64-unknown-linux-gnu',
    topology_digest: seed.repeat(64).slice(0, 64),
    perllsp_digest: (seed === 'a' ? 'b' : 'c').repeat(64),
    perl_dap_digest: (seed === 'a' ? 'd' : 'e').repeat(64),
  };
}

function entry(seed: string): ManagedCandidateCatalogEntry {
  return {
    manifest: buildManagedCandidateManifest(subject(seed)),
    immutable: true,
  };
}

describe('managed candidate publication and selection', () => {
  test('publishes current selection by immutable candidate identity with monotonic generation', () => {
    const first = entry('a');
    const second = entry('f');

    const selection1 = publishManagedCurrentSelection(first.manifest, null);
    const selection2 = publishManagedCurrentSelection(second.manifest, selection1);

    expect(selection1).toEqual({
      schema_version: 'managed_current_selection.v1',
      selection_generation: 1,
      candidate_id: first.manifest.candidate_id,
    });
    expect(selection2.selection_generation).toBe(2);
    expect(selection2.candidate_id).toBe(second.manifest.candidate_id);
    expect(validateManagedCurrentSelection(selection2, [first, second])).toEqual([]);
  });

  test('refuses to publish a manifest whose candidate identity no longer matches its bytes/subject', () => {
    const candidate = entry('a');
    candidate.manifest.candidate_id = `candidate-${'0'.repeat(64)}`;

    expect(() => publishManagedCurrentSelection(candidate.manifest, null)).toThrow(
      'cannot publish invalid managed candidate',
    );
  });

  test('keeps a running host bound to its launched candidate when the shared default moves', () => {
    const oldCandidate = entry('a');
    const newCandidate = entry('f');
    const current = publishManagedCurrentSelection(newCandidate.manifest, null);

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [oldCandidate, newCandidate],
        compatible_candidate_ids: [oldCandidate.manifest.candidate_id, newCandidate.manifest.candidate_id],
        running_candidate_id: oldCandidate.manifest.candidate_id,
      }),
    ).toBe(oldCandidate.manifest.candidate_id);
    expect(current.candidate_id).toBe(newCandidate.manifest.candidate_id);
  });

  test('lets an older client select a compatible retained candidate without downgrading global current', () => {
    const oldCandidate = entry('a');
    const newCandidate = entry('f');
    const current = publishManagedCurrentSelection(newCandidate.manifest, null);

    const selectedForOldClient = resolveManagedCandidateForHost({
      current,
      candidates: [oldCandidate, newCandidate],
      compatible_candidate_ids: [oldCandidate.manifest.candidate_id],
      running_candidate_id: null,
    });

    expect(selectedForOldClient).toBe(oldCandidate.manifest.candidate_id);
    expect(current.candidate_id).toBe(newCandidate.manifest.candidate_id);
  });

  test('protects current, live, unknown, and compatibility-retained candidates from GC', () => {
    const currentCandidate = entry('a');
    const liveCandidate = entry('f');
    const unknownCandidate = entry('1');
    const retainedCandidate = entry('2');
    const staleCandidate = entry('3');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);

    const live = createManagedHostReference('session-live', liveCandidate.manifest.candidate_id);
    const unknown = {
      ...createManagedHostReference('session-unknown', unknownCandidate.manifest.candidate_id),
      state: 'unknown' as const,
    };
    const retained = new Set([retainedCandidate.manifest.candidate_id]);

    expect(
      classifyManagedCandidateRetention(
        currentCandidate.manifest.candidate_id,
        current,
        [live, unknown],
        retained,
      ),
    ).toBe('current_default');
    expect(
      classifyManagedCandidateRetention(
        liveCandidate.manifest.candidate_id,
        current,
        [live, unknown],
        retained,
      ),
    ).toBe('live_referenced');
    expect(
      classifyManagedCandidateRetention(
        unknownCandidate.manifest.candidate_id,
        current,
        [live, unknown],
        retained,
      ),
    ).toBe('unknown_reference');
    expect(
      classifyManagedCandidateRetention(
        retainedCandidate.manifest.candidate_id,
        current,
        [live, unknown],
        retained,
      ),
    ).toBe('compatible_retained');
    expect(
      mayGarbageCollectManagedCandidate(
        staleCandidate.manifest.candidate_id,
        current,
        [live, unknown],
        retained,
      ),
    ).toBe(true);
  });

  test('released host references stop protecting a stale candidate', () => {
    const currentCandidate = entry('a');
    const oldCandidate = entry('f');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const released = releaseManagedHostReference(
      createManagedHostReference('session-released', oldCandidate.manifest.candidate_id),
    );

    expect(
      mayGarbageCollectManagedCandidate(
        oldCandidate.manifest.candidate_id,
        current,
        [released],
        new Set(),
      ),
    ).toBe(true);
  });

  test('published candidate entries are immutable by contract', () => {
    expect(candidateBytesMayChangeAfterPublication(entry('a'))).toBe(false);
  });
});
