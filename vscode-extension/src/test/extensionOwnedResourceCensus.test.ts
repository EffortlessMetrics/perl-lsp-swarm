import {
  ACTIVATION_RESOURCE_CLASSES,
  ActivationTransaction,
  type ActivationResourceCensus,
} from '../activationTransaction';
import {
  type ClientResourceMeasurement,
  VscodeClientMeasurementRecorder,
  notProvenResource,
  observedResource,
  resourceReturnedToBaseline,
} from '../clientMeasurement';
import {
  DISPOSABLE_COUNT_UNAVAILABLE_REASON,
  EXTENSION_HOST_MEMORY_SHARED_REASON,
  NO_OWNED_ACTIVATION_REASON,
  RESOURCE_KIND_NOT_CLASSIFIED_REASON,
  extensionOwnedResourceMeasurements,
  recordExtensionOwnedResources,
} from '../extensionOwnedResourceCensus';

function noop(): void {
  /* intentionally empty */
}

function measurement(
  measurements: ClientResourceMeasurement[],
  id: string,
): ClientResourceMeasurement {
  const found = measurements.find((entry) => entry.id === id);
  if (found === undefined) {
    throw new Error(`missing measurement row: ${id}`);
  }
  return found;
}

describe('activation resource census', () => {
  test('counts only resources the extension registered, bucketed by class', () => {
    const transaction = new ActivationTransaction('attempt-census-1');
    transaction.registerResource({
      id: 'output-channel',
      phase: 'base',
      resource_class: 'support_surface_allowed_after_failure',
      cleanup: noop,
    });
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });
    transaction.registerResource({
      id: 'test-controller',
      phase: 'testing',
      resource_class: 'optional_degradable',
      cleanup: noop,
    });

    const census = transaction.resourceCensus();
    expect(census.live_total).toBe(3);
    expect(census.live_by_class).toEqual({
      mandatory_for_activation: 1,
      optional_degradable: 1,
      lazy_user_triggered: 0,
      support_surface_allowed_after_failure: 1,
    });
  });

  test('reports one deterministic bucket per declared resource class', () => {
    const census = new ActivationTransaction('attempt-census-empty').resourceCensus();
    expect(Object.keys(census.live_by_class).sort()).toEqual(
      [...ACTIVATION_RESOURCE_CLASSES].sort(),
    );
    expect(census.live_total).toBe(0);
  });

  test('deactivation drains the census so a clean shutdown returns to baseline', async () => {
    const transaction = new ActivationTransaction('attempt-census-2');
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });
    transaction.registerResource({
      id: 'listeners',
      phase: 'workspace_listeners',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });

    const baseline = transaction.resourceCensus();
    expect(baseline.live_total).toBe(2);

    const runtime = transaction.commit();
    expect(transaction.resourceCensus().live_total).toBe(2);

    await runtime.deactivate();
    expect(transaction.resourceCensus().live_total).toBe(0);
  });

  test('a rollback that retains support surfaces keeps them counted as owned', async () => {
    const transaction = new ActivationTransaction('attempt-census-3');
    transaction.registerResource({
      id: 'output-channel',
      phase: 'base',
      resource_class: 'support_surface_allowed_after_failure',
      cleanup: noop,
    });
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });

    await transaction.rollback({ retain_support_surfaces: true });

    const census = transaction.resourceCensus();
    expect(census.live_total).toBe(1);
    expect(census.live_by_class.support_surface_allowed_after_failure).toBe(1);
    expect(census.live_by_class.mandatory_for_activation).toBe(0);
  });

  test('a cleanup that throws leaves the resource counted as still owned', async () => {
    const transaction = new ActivationTransaction('attempt-census-4');
    transaction.registerResource({
      id: 'faulty',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        throw new Error('cleanup exploded');
      },
    });
    transaction.registerResource({
      id: 'healthy',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });

    const receipt = await transaction.rollback();
    expect(receipt.cleanup_failures).toHaveLength(1);
    // A throw means release was never confirmed. Counting it as released would
    // let the case most likely to BE a leak report a clean baseline, so the
    // faulty resource stays owned while the healthy one drops out.
    const census = transaction.resourceCensus();
    expect(census.live_total).toBe(1);
    expect(census.live_by_class.mandatory_for_activation).toBe(1);
  });

  test('a failed release keeps the count off zero after deactivation', async () => {
    const transaction = new ActivationTransaction('attempt-census-5');
    transaction.registerResource({
      id: 'faulty',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        throw new Error('dispose exploded');
      },
    });
    const before = measurement(
      extensionOwnedResourceMeasurements(transaction.resourceCensus()),
      'extension_owned_activation_resources',
    );

    const runtime = transaction.commit();
    await runtime.deactivate();

    const after = measurement(
      extensionOwnedResourceMeasurements(transaction.resourceCensus()),
      'extension_owned_activation_resources',
    );
    // A confirmed release would have taken the count to 0. It stays at 1, so
    // the unreleased resource is still visible after shutdown. The <= oracle
    // alone cannot express this (1 <= 1 reads as "returned"), which is why the
    // count itself, not just the oracle, is asserted.
    expect(before.value).toBe(1);
    expect(after.value).toBe(1);
    expect(resourceReturnedToBaseline(before, after)).toBe(true);
  });
});

describe('resource measurement builders', () => {
  test('reject an id outside the closed resource set', () => {
    expect(() => observedResource('host_wide_listeners', 3)).toThrow(
      /unsupported client resource id/,
    );
    expect(() => notProvenResource('host_wide_listeners', 'because')).toThrow(
      /unsupported client resource id/,
    );
  });

  test('reject a negative or non-finite observed value', () => {
    expect(() => observedResource('extension_owned_activation_resources', -1)).toThrow(
      /finite non-negative/,
    );
    expect(() => observedResource('extension_owned_activation_resources', Number.NaN)).toThrow(
      /finite non-negative/,
    );
  });

  test('reject an unavailable row with no reason', () => {
    expect(() => notProvenResource('extension_owned_timers', '   ')).toThrow(/requires a reason/);
  });
});

describe('extension-owned resource measurements', () => {
  test('reports the live owned count as the observed activation-resource counter', () => {
    const census: ActivationResourceCensus = {
      live_total: 7,
      live_by_class: {
        mandatory_for_activation: 5,
        optional_degradable: 1,
        lazy_user_triggered: 0,
        support_surface_allowed_after_failure: 1,
      },
    };

    const measurements = extensionOwnedResourceMeasurements(census);
    expect(measurement(measurements, 'extension_owned_activation_resources')).toEqual({
      id: 'extension_owned_activation_resources',
      availability: 'observed',
      value: 7,
      reason: null,
    });
  });

  test('does not report a disposable count the registry cannot produce', () => {
    const row = measurement(
      extensionOwnedResourceMeasurements({
        live_total: 4,
        live_by_class: {
          mandatory_for_activation: 4,
          optional_degradable: 0,
          lazy_user_triggered: 0,
          support_surface_allowed_after_failure: 0,
        },
      }),
      'extension_owned_disposables',
    );
    // The ledger mixes non-disposable cleanup callbacks in and omits
    // post-commit host-owned resources, so 4 is not a disposable count.
    expect(row.availability).toBe('not_proven');
    expect(row.value).toBeNull();
    expect(row.reason).toBe(DISPOSABLE_COUNT_UNAVAILABLE_REASON);
  });

  test('never serializes an unclassifiable counter as zero', () => {
    const measurements = extensionOwnedResourceMeasurements({
      live_total: 3,
      live_by_class: {
        mandatory_for_activation: 3,
        optional_degradable: 0,
        lazy_user_triggered: 0,
        support_surface_allowed_after_failure: 0,
      },
    });

    for (const id of ['extension_owned_timers', 'extension_owned_event_listeners']) {
      const row = measurement(measurements, id);
      expect(row.availability).toBe('not_proven');
      expect(row.value).toBeNull();
      expect(row.reason).toBe(RESOURCE_KIND_NOT_CLASSIFIED_REASON);
    }

    const rss = measurement(measurements, 'extension_host_rss_bytes');
    expect(rss.availability).toBe('not_proven');
    expect(rss.value).toBeNull();
    expect(rss.reason).toBe(EXTENSION_HOST_MEMORY_SHARED_REASON);
  });

  test('every not-proven row carries a non-empty reason', () => {
    const measurements = extensionOwnedResourceMeasurements(null);
    const notProven = measurements.filter((row) => row.availability === 'not_proven');
    expect(notProven.length).toBeGreaterThan(0);
    for (const row of notProven) {
      expect(row.reason).not.toBeNull();
      expect((row.reason ?? '').trim().length).toBeGreaterThan(0);
    }
  });

  test('an unobservable census is distinct from zero owned resources', () => {
    const absent = measurement(
      extensionOwnedResourceMeasurements(null),
      'extension_owned_activation_resources',
    );
    expect(absent.availability).toBe('not_proven');
    expect(absent.value).toBeNull();
    expect(absent.reason).toBe(NO_OWNED_ACTIVATION_REASON);

    const empty = measurement(
      extensionOwnedResourceMeasurements({
        live_total: 0,
        live_by_class: {
          mandatory_for_activation: 0,
          optional_degradable: 0,
          lazy_user_triggered: 0,
          support_surface_allowed_after_failure: 0,
        },
      }),
      'extension_owned_activation_resources',
    );
    expect(empty.availability).toBe('observed');
    expect(empty.value).toBe(0);
  });

  test('a reload that fails to release is detectable; a clean reload is not', async () => {
    // Residue is derived from the real cleanup path, never hand-injected: the
    // only difference between the two cycles is whether one dispose() throws.
    async function residueAfterCycle(failRelease: boolean): Promise<ClientResourceMeasurement> {
      const suffix = failRelease ? 'leak' : 'clean';
      const transaction = new ActivationTransaction(`attempt-cycle-${suffix}`);
      transaction.registerResource({
        id: 'commands',
        phase: 'commands',
        resource_class: 'mandatory_for_activation',
        cleanup: noop,
      });
      transaction.registerResource({
        id: 'listeners',
        phase: 'workspace_listeners',
        resource_class: 'mandatory_for_activation',
        cleanup: failRelease
          ? () => {
              throw new Error('dispose exploded');
            }
          : noop,
      });

      const runtime = transaction.commit();
      await runtime.deactivate();
      return measurement(
        extensionOwnedResourceMeasurements(transaction.resourceCensus()),
        'extension_owned_activation_resources',
      );
    }

    const emptyBaseline = measurement(
      extensionOwnedResourceMeasurements(
        new ActivationTransaction('attempt-baseline').resourceCensus(),
      ),
      'extension_owned_activation_resources',
    );
    expect(emptyBaseline.value).toBe(0);

    const clean = await residueAfterCycle(false);
    expect(clean.value).toBe(0);
    expect(resourceReturnedToBaseline(emptyBaseline, clean)).toBe(true);

    const leaked = await residueAfterCycle(true);
    expect(leaked.value).toBe(1);
    expect(resourceReturnedToBaseline(emptyBaseline, leaked)).toBe(false);
  });

  test('records through the recorder so the closed resource-id set stays authoritative', () => {
    const recorder = new VscodeClientMeasurementRecorder(
      {
        candidate: 'vsix-test',
        vscode_version: '1.125.1',
        platform: 'linux',
        architecture: 'x64',
        host_kind: 'local',
        scenario: 'census',
        cold_warm: 'cold',
        binary_role: 'managed',
        server_candidate: null,
      },
      0,
    );
    recordExtensionOwnedResources(recorder, {
      live_total: 2,
      live_by_class: {
        mandatory_for_activation: 2,
        optional_degradable: 0,
        lazy_user_triggered: 0,
        support_surface_allowed_after_failure: 0,
      },
    });

    const ids = recorder.snapshot().resources.map((row) => row.id);
    expect(ids).toEqual([
      'extension_host_rss_bytes',
      'extension_owned_activation_resources',
      'extension_owned_disposables',
      'extension_owned_event_listeners',
      'extension_owned_timers',
    ]);
  });
});
