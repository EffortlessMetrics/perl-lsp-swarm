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
    expect(runtime.resourceCensus().live_total).toBe(2);

    await runtime.deactivate();
    expect(runtime.resourceCensus().live_total).toBe(0);
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

  test('a cleanup that throws is not inflated into a retained resource', async () => {
    const transaction = new ActivationTransaction('attempt-census-4');
    transaction.registerResource({
      id: 'faulty',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        throw new Error('cleanup exploded');
      },
    });

    const receipt = await transaction.rollback();
    expect(receipt.cleanup_failures).toHaveLength(1);
    // The failure is reported through the receipt, not by leaving the resource
    // counted as still owned.
    expect(transaction.resourceCensus().live_total).toBe(0);
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
    expect(() => observedResource('extension_owned_disposables', -1)).toThrow(
      /finite non-negative/,
    );
    expect(() => observedResource('extension_owned_disposables', Number.NaN)).toThrow(
      /finite non-negative/,
    );
  });

  test('reject an unavailable row with no reason', () => {
    expect(() => notProvenResource('extension_owned_timers', '   ')).toThrow(/requires a reason/);
  });
});

describe('extension-owned resource measurements', () => {
  test('reports the live owned count as the observed disposables counter', () => {
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
    expect(measurement(measurements, 'extension_owned_disposables')).toEqual({
      id: 'extension_owned_disposables',
      availability: 'observed',
      value: 7,
      reason: null,
    });
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
      'extension_owned_disposables',
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
      'extension_owned_disposables',
    );
    expect(empty.availability).toBe('observed');
    expect(empty.value).toBe(0);
  });

  test('a leaked resource across reload is detectable, a clean reload is not a leak', async () => {
    async function ownedCountAfterCycle(leak: boolean): Promise<ClientResourceMeasurement> {
      const transaction = new ActivationTransaction(`attempt-cycle-${leak ? 'leak' : 'clean'}`);
      transaction.registerResource({
        id: 'commands',
        phase: 'commands',
        resource_class: 'mandatory_for_activation',
        cleanup: noop,
      });
      const runtime = transaction.commit();
      await runtime.deactivate();

      const next = new ActivationTransaction(`attempt-cycle-${leak ? 'leak' : 'clean'}-2`);
      next.registerResource({
        id: 'commands',
        phase: 'commands',
        resource_class: 'mandatory_for_activation',
        cleanup: noop,
      });
      if (leak) {
        // A reload that re-registers a listener the previous attempt never
        // released shows up as a strictly larger owned set.
        next.registerResource({
          id: 'stale-listener',
          phase: 'workspace_listeners',
          resource_class: 'mandatory_for_activation',
          cleanup: noop,
        });
      }
      return measurement(
        extensionOwnedResourceMeasurements(next.resourceCensus()),
        'extension_owned_disposables',
      );
    }

    const firstActivation = new ActivationTransaction('attempt-baseline');
    firstActivation.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: noop,
    });
    const before = measurement(
      extensionOwnedResourceMeasurements(firstActivation.resourceCensus()),
      'extension_owned_disposables',
    );

    expect(resourceReturnedToBaseline(before, await ownedCountAfterCycle(false))).toBe(true);
    expect(resourceReturnedToBaseline(before, await ownedCountAfterCycle(true))).toBe(false);
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
      'extension_owned_disposables',
      'extension_owned_event_listeners',
      'extension_owned_timers',
    ]);
  });
});
