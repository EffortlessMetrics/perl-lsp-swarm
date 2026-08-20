import {
  buildManagedCandidateManifest,
  type ManagedCandidateSubject,
} from '../managedCacheProtocol';
import {
  classifyManagedCandidateRetention,
  createManagedHostReference,
  mayGarbageCollectManagedCandidate,
  publishManagedCurrentSelection,
  releaseManagedHostReference,
  resolveManagedCandidateForHost,
  validateManagedCurrentSelection,
  type ManagedCandidateCatalogEntry,
  type ManagedCurrentSelection,
  type ManagedHostCandidateReference,
  type ManagedHostReferenceState,
  type ManagedRetentionInput,
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

function retention(overrides: Partial<ManagedRetentionInput>): ManagedRetentionInput {
  return {
    current: null,
    catalog: [],
    host_references: [],
    host_references_complete: true,
    compatible_retained_ids: new Set(),
    ...overrides,
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

  test('refuses to publish over a prior selection with a corrupt generation', () => {
    const candidate = entry('a');
    const corruptPrior = {
      schema_version: 'managed_current_selection.v1' as const,
      selection_generation: 0,
      candidate_id: candidate.manifest.candidate_id,
    };

    expect(() => publishManagedCurrentSelection(candidate.manifest, corruptPrior)).toThrow(
      'cannot publish over an invalid prior selection',
    );
  });

  test('rejects a current selection whose catalog record does not claim immutability', () => {
    const candidate = entry('a');
    const selection = publishManagedCurrentSelection(candidate.manifest, null);
    const mutableEntry = {
      ...candidate,
      immutable: false,
    } as unknown as ManagedCandidateCatalogEntry;

    expect(validateManagedCurrentSelection(selection, [mutableEntry])).toContain(
      'current selection candidate must be immutable',
    );
  });

  test('rejects a current selection carrying an unsupported schema version', () => {
    const candidate = entry('a');
    const selection = publishManagedCurrentSelection(candidate.manifest, null);
    const foreign = {
      ...selection,
      schema_version: 'managed_current_selection.v2',
    } as unknown as ManagedCurrentSelection;

    expect(validateManagedCurrentSelection(foreign, [candidate])).toContain(
      'current selection carries an unsupported schema version',
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
        compatible_candidate_ids: [
          oldCandidate.manifest.candidate_id,
          newCandidate.manifest.candidate_id,
        ],
        running_candidate_id: oldCandidate.manifest.candidate_id,
      }),
    ).toEqual({ kind: 'bound_running', candidate_id: oldCandidate.manifest.candidate_id });
    expect(current.candidate_id).toBe(newCandidate.manifest.candidate_id);
  });

  test('reports restart-required instead of silently rebinding a host whose candidate is gone', () => {
    const collectedCandidate = entry('a');
    const newCandidate = entry('f');
    const current = publishManagedCurrentSelection(newCandidate.manifest, null);

    // The launched candidate is no longer in the catalog. Returning the
    // replacement id as an ordinary selection would misreport a live process
    // as already running bytes it never launched.
    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [newCandidate],
        compatible_candidate_ids: [
          collectedCandidate.manifest.candidate_id,
          newCandidate.manifest.candidate_id,
        ],
        running_candidate_id: collectedCandidate.manifest.candidate_id,
      }),
    ).toEqual({ kind: 'restart_required', candidate_id: newCandidate.manifest.candidate_id });
  });

  test('requires restart when a running catalog candidate is no longer compatible', () => {
    const runningCandidate = entry('a');
    const replacementCandidate = entry('f');
    const current = publishManagedCurrentSelection(replacementCandidate.manifest, null);

    // The candidate is still catalogued, but the host can no longer use it.
    // Returning a replacement as if it were already running would authorize
    // a silent process rebinding across an incompatible candidate boundary.
    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [runningCandidate, replacementCandidate],
        compatible_candidate_ids: [replacementCandidate.manifest.candidate_id],
        running_candidate_id: runningCandidate.manifest.candidate_id,
      }),
    ).toEqual({
      kind: 'restart_required',
      candidate_id: replacementCandidate.manifest.candidate_id,
    });
  });

  test('selects the current candidate for a fresh host when it is usable', () => {
    const candidate = entry('a');
    const current = publishManagedCurrentSelection(candidate.manifest, null);

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [candidate],
        compatible_candidate_ids: [candidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'selected_current', candidate_id: candidate.manifest.candidate_id });
  });

  test('does not treat a current selection with an invalid schema or generation as current', () => {
    const candidate = entry('a');
    const valid = publishManagedCurrentSelection(candidate.manifest, null);
    const malformed = {
      ...valid,
      schema_version: 'managed_current_selection.v2',
      selection_generation: 0,
    } as unknown as ManagedCurrentSelection;

    expect(
      resolveManagedCandidateForHost({
        current: malformed,
        candidates: [candidate],
        compatible_candidate_ids: [candidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'selected_compatible', candidate_id: candidate.manifest.candidate_id });
  });

  test('does not select a catalog entry with an invalid manifest schema', () => {
    const candidate = entry('a');
    const current = publishManagedCurrentSelection(candidate.manifest, null);
    const malformed = {
      ...candidate,
      manifest: { ...candidate.manifest, schema_version: 'managed_candidate_manifest.v2' },
    } as unknown as ManagedCandidateCatalogEntry;

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [malformed],
        compatible_candidate_ids: [candidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'no_compatible_candidate' });
  });

  test('does not treat forged provenance as a current managed candidate', () => {
    const candidate = entry('a');
    const current = publishManagedCurrentSelection(candidate.manifest, null);
    const forged = {
      ...candidate,
      manifest: {
        ...candidate.manifest,
        verification: {
          ...candidate.manifest.verification,
          provenance: 'forged',
        },
      },
    } as unknown as ManagedCandidateCatalogEntry;

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [forged],
        compatible_candidate_ids: [candidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'no_compatible_candidate' });
  });

  test('does not select a catalog entry whose identity no longer matches its subject', () => {
    const candidate = entry('a');
    const current = publishManagedCurrentSelection(candidate.manifest, null);
    const malformed = {
      ...candidate,
      manifest: { ...candidate.manifest, candidate_id: `candidate-${'0'.repeat(64)}` },
    } as unknown as ManagedCandidateCatalogEntry;

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [malformed],
        compatible_candidate_ids: [candidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'no_compatible_candidate' });
  });

  test('lets an older client select a compatible retained candidate without downgrading global current', () => {
    const oldCandidate = entry('a');
    const newCandidate = entry('f');
    const current = publishManagedCurrentSelection(newCandidate.manifest, null);

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [oldCandidate, newCandidate],
        compatible_candidate_ids: [oldCandidate.manifest.candidate_id],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'selected_compatible', candidate_id: oldCandidate.manifest.candidate_id });
    expect(current.candidate_id).toBe(newCandidate.manifest.candidate_id);
  });

  test('refuses GC for a host reference carrying an unrecognized state', () => {
    const currentCandidate = entry('a');
    const referencedCandidate = entry('f');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    // A parseable reference record written by a newer extension version may
    // carry a state this version does not know (mixed-VSIX install). Unknown
    // evidence must never read as safe-to-delete.
    const unrecognized = {
      ...createManagedHostReference('session-future', referencedCandidate.manifest.candidate_id),
      state: 'quarantined' as ManagedHostReferenceState,
    };
    const input = retention({
      current,
      catalog: [currentCandidate, referencedCandidate],
      host_references: [unrecognized],
    });

    expect(
      classifyManagedCandidateRetention(referencedCandidate.manifest.candidate_id, input),
    ).toBe('unknown_reference');
    expect(
      mayGarbageCollectManagedCandidate(referencedCandidate.manifest.candidate_id, input),
    ).toBe(false);
  });

  test('host references require the complete canonical candidate identity, not only the prefix', () => {
    const canonical = entry('a').manifest.candidate_id;

    expect(createManagedHostReference('session-ok', canonical).candidate_id).toBe(canonical);

    const malformed = [
      'candidate-', // prefix only, no digest
      `candidate-${'a'.repeat(63)}`, // short digest
      `candidate-${'a'.repeat(65)}`, // long digest
      `candidate-${'g'.repeat(64)}`, // non-hex digest
      'candidate-../../etc/passwd', // path-shaped suffix
      `candidate-${'a'.repeat(32)}/${'a'.repeat(31)}`, // slash payload
      `candidate-${'a'.repeat(32)}\\${'a'.repeat(31)}`, // backslash payload
      `candidate-${'A'.repeat(64)}`, // minted identity is lowercase-only
    ];
    for (const candidateId of malformed) {
      expect(() => createManagedHostReference('session-bad', candidateId)).toThrow(
        'host reference must name a canonical managed candidate',
      );
    }
  });

  test('reports no compatible candidate rather than an incompatible fallback', () => {
    const newCandidate = entry('f');
    const current = publishManagedCurrentSelection(newCandidate.manifest, null);

    expect(
      resolveManagedCandidateForHost({
        current,
        candidates: [newCandidate],
        compatible_candidate_ids: [],
        running_candidate_id: null,
      }),
    ).toEqual({ kind: 'no_compatible_candidate' });
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
    const input = retention({
      current,
      catalog: [
        currentCandidate,
        liveCandidate,
        unknownCandidate,
        retainedCandidate,
        staleCandidate,
      ],
      host_references: [live, unknown],
      compatible_retained_ids: new Set([retainedCandidate.manifest.candidate_id]),
    });

    expect(classifyManagedCandidateRetention(currentCandidate.manifest.candidate_id, input)).toBe(
      'current_default',
    );
    expect(classifyManagedCandidateRetention(liveCandidate.manifest.candidate_id, input)).toBe(
      'live_referenced',
    );
    expect(classifyManagedCandidateRetention(unknownCandidate.manifest.candidate_id, input)).toBe(
      'unknown_reference',
    );
    expect(classifyManagedCandidateRetention(retainedCandidate.manifest.candidate_id, input)).toBe(
      'compatible_retained',
    );
    expect(mayGarbageCollectManagedCandidate(staleCandidate.manifest.candidate_id, input)).toBe(
      true,
    );
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
        retention({
          current,
          catalog: [currentCandidate, oldCandidate],
          host_references: [released],
        }),
      ),
    ).toBe(true);
  });

  test('does not let an unsupported released reference authorize GC', () => {
    const currentCandidate = entry('a');
    const staleCandidate = entry('f');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const forgedReleased = {
      ...createManagedHostReference('session-forged', staleCandidate.manifest.candidate_id),
      schema_version: 'managed_host_candidate_reference.v2',
      state: 'released' as const,
    } as unknown as ManagedHostCandidateReference;

    expect(() => releaseManagedHostReference(forgedReleased)).toThrow(
      'cannot release invalid managed host reference',
    );
    const input = retention({
      current,
      catalog: [currentCandidate, staleCandidate],
      host_references: [forgedReleased],
    });

    expect(classifyManagedCandidateRetention(staleCandidate.manifest.candidate_id, input)).toBe(
      'unknown_not_safe_to_delete',
    );
    expect(mayGarbageCollectManagedCandidate(staleCandidate.manifest.candidate_id, input)).toBe(
      false,
    );
  });

  test('a forged record naming another candidate still stalls GC globally', () => {
    const currentCandidate = entry('a');
    const forgedTarget = entry('f');
    const uninvolved = entry('1');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    // The forged record names a different candidate than the one being
    // classified: a structurally invalid record poisons the enumeration
    // itself, so protection is not scoped to the forged record's subject.
    const forgedElsewhere = {
      ...createManagedHostReference('session-forged', forgedTarget.manifest.candidate_id),
      schema_version: 'managed_host_candidate_reference.v2',
      state: 'released' as const,
    } as unknown as ManagedHostCandidateReference;
    const input = retention({
      current,
      catalog: [currentCandidate, forgedTarget, uninvolved],
      host_references: [forgedElsewhere],
    });

    expect(classifyManagedCandidateRetention(uninvolved.manifest.candidate_id, input)).toBe(
      'unknown_not_safe_to_delete',
    );
    expect(mayGarbageCollectManagedCandidate(uninvolved.manifest.candidate_id, input)).toBe(false);
  });

  test('forged session ids and non-canonical candidate ids are invalid host references', () => {
    const currentCandidate = entry('a');
    const staleCandidate = entry('f');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const valid = createManagedHostReference('session-valid', staleCandidate.manifest.candidate_id);
    const badSession = { ...valid, session_id: '../escape' };
    const badCandidate = { ...valid, candidate_id: 'candidate-not-a-digest' };

    for (const forged of [badSession, badCandidate]) {
      expect(() => releaseManagedHostReference(forged)).toThrow(
        'cannot release invalid managed host reference',
      );
      const input = retention({
        current,
        catalog: [currentCandidate, staleCandidate],
        host_references: [forged],
      });
      expect(classifyManagedCandidateRetention(staleCandidate.manifest.candidate_id, input)).toBe(
        'unknown_not_safe_to_delete',
      );
    }
  });

  test('refuses GC when host-reference enumeration was not proven exhaustive', () => {
    const currentCandidate = entry('a');
    const staleLookingCandidate = entry('f');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const input = retention({
      current,
      catalog: [currentCandidate, staleLookingCandidate],
      host_references: [],
      host_references_complete: false,
    });

    // An empty reference list from a failed enumeration must not read as
    // "nothing references this candidate".
    expect(
      classifyManagedCandidateRetention(staleLookingCandidate.manifest.candidate_id, input),
    ).toBe('unknown_not_safe_to_delete');
    expect(
      mayGarbageCollectManagedCandidate(staleLookingCandidate.manifest.candidate_id, input),
    ).toBe(false);
  });

  test('refuses GC entirely when the current-selection record cannot be read', () => {
    const candidate = entry('a');
    const input = retention({ current: null, catalog: [candidate] });

    expect(classifyManagedCandidateRetention(candidate.manifest.candidate_id, input)).toBe(
      'unknown_not_safe_to_delete',
    );
    expect(mayGarbageCollectManagedCandidate(candidate.manifest.candidate_id, input)).toBe(false);
  });

  test('refuses GC when a non-null current selection is malformed or absent from the catalog', () => {
    const currentCandidate = entry('a');
    const staleCandidate = entry('f');
    const missingCandidate = entry('1');
    const validCurrent = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const malformedSelections = [
      { ...validCurrent, schema_version: 'managed_current_selection.v2' },
      { ...validCurrent, selection_generation: 0 },
      { ...validCurrent, candidate_id: 'candidate-not-a-digest' },
      { ...validCurrent, candidate_id: missingCandidate.manifest.candidate_id },
    ] as unknown as ManagedCurrentSelection[];

    for (const current of malformedSelections) {
      const input = retention({
        current,
        catalog: [currentCandidate, staleCandidate],
      });

      expect(classifyManagedCandidateRetention(staleCandidate.manifest.candidate_id, input)).toBe(
        'unknown_not_safe_to_delete',
      );
      expect(mayGarbageCollectManagedCandidate(staleCandidate.manifest.candidate_id, input)).toBe(
        false,
      );
    }
  });

  test('refuses GC for half-published or identity-mismatched candidate bytes', () => {
    const currentCandidate = entry('a');
    const current = publishManagedCurrentSelection(currentCandidate.manifest, null);
    const absent = entry('f');
    const corrupt = entry('1');
    corrupt.manifest.candidate_id = `candidate-${'0'.repeat(64)}`;

    const input = retention({
      current,
      catalog: [currentCandidate, corrupt],
    });

    // Present on disk but absent from the validated catalog.
    expect(classifyManagedCandidateRetention(absent.manifest.candidate_id, input)).toBe(
      'partial_or_invalid',
    );
    expect(mayGarbageCollectManagedCandidate(absent.manifest.candidate_id, input)).toBe(false);

    // Catalogued but the manifest no longer matches its own subject.
    expect(classifyManagedCandidateRetention(corrupt.manifest.candidate_id, input)).toBe(
      'partial_or_invalid',
    );
    expect(mayGarbageCollectManagedCandidate(corrupt.manifest.candidate_id, input)).toBe(false);
  });
});
