import {
  type ClientMeasurementSubject,
  VscodeClientMeasurementRecorder,
  resourceReturnedToBaseline,
} from '../clientMeasurement';

function subject(): ClientMeasurementSubject {
  return {
    candidate: 'vsix-0.18.0-test',
    vscode_version: '1.125.1',
    platform: 'linux',
    architecture: 'x64',
    host_kind: 'local',
    scenario: 'cold-perl-document',
    cold_warm: 'cold',
    binary_role: 'managed',
    server_candidate: 'candidate-test',
  };
}

describe('VS Code client measurement recorder', () => {
  test('records phase offsets relative to one explicit origin', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 100);
    recorder.markPhase('activation_requested', 100);
    recorder.markPhase('base_surface_ready', 112.4);
    recorder.markPhase('activation_committed', 149.7);

    const snapshot = recorder.snapshot();
    expect(snapshot.phases.find((phase) => phase.phase === 'activation_requested')).toEqual({
      phase: 'activation_requested',
      availability: 'observed',
      offset_ms: 0,
    });
    expect(snapshot.phases.find((phase) => phase.phase === 'base_surface_ready')?.offset_ms).toBe(
      12,
    );
    expect(snapshot.phases.find((phase) => phase.phase === 'activation_committed')?.offset_ms).toBe(
      50,
    );
    expect(snapshot.instrument_complete).toBe(false);
  });

  test('never serializes unavailable timing as zero', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 100);
    recorder.markPhase('activation_requested', 100);
    recorder.markPhaseNotProven('initialize_accepted');

    const initialize = recorder
      .snapshot()
      .phases.find((phase) => phase.phase === 'initialize_accepted');
    expect(initialize).toEqual({
      phase: 'initialize_accepted',
      availability: 'not_proven',
      offset_ms: null,
    });
  });

  test('keeps first observation when a phase is marked twice', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 10);
    recorder.markPhase('registrations_complete', 20);
    recorder.markPhase('registrations_complete', 40);

    expect(
      recorder.snapshot().phases.find((phase) => phase.phase === 'registrations_complete')
        ?.offset_ms,
    ).toBe(10);
  });

  test('records observed and not-proven resources separately', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 0);
    recorder.observeResource('extension_owned_timers', 3);
    recorder.markResourceNotProven(
      'extension_host_rss_bytes',
      'shared host attribution unavailable',
    );

    expect(recorder.snapshot().resources).toEqual([
      {
        id: 'extension_host_rss_bytes',
        availability: 'not_proven',
        value: null,
        reason: 'shared host attribution unavailable',
      },
      {
        id: 'extension_owned_timers',
        availability: 'observed',
        value: 3,
        reason: null,
      },
    ]);
  });

  test('rejects negative or non-finite resource counts', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 0);
    expect(() => recorder.observeResource('extension_owned_timers', -1)).toThrow();
    expect(() => recorder.observeResource('extension_owned_timers', Number.NaN)).toThrow();
  });

  test('rejects unsupported resource ids', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 0);
    expect(() => recorder.observeResource('timers', 1)).toThrow(/unsupported client resource id/);
    expect(() => recorder.markResourceNotProven('event_listeners', 'x')).toThrow(
      /unsupported client resource id/,
    );
  });

  test('snapshot clones resource records so callers cannot mutate recorder state', () => {
    const recorder = new VscodeClientMeasurementRecorder(subject(), 0);
    recorder.observeResource('extension_owned_timers', 2);
    const first = recorder.snapshot();
    first.resources[0].value = 99;
    expect(recorder.snapshot().resources[0].value).toBe(2);
  });

  test('compares restart/reload resource baselines only when both observations exist', () => {
    expect(
      resourceReturnedToBaseline(
        { id: 'extension_owned_timers', availability: 'observed', value: 4, reason: null },
        { id: 'extension_owned_timers', availability: 'observed', value: 3, reason: null },
      ),
    ).toBe(true);
    expect(
      resourceReturnedToBaseline(
        { id: 'extension_owned_timers', availability: 'observed', value: 4, reason: null },
        { id: 'extension_owned_timers', availability: 'observed', value: 5, reason: null },
      ),
    ).toBe(false);
    expect(
      resourceReturnedToBaseline(
        {
          id: 'extension_owned_timers',
          availability: 'not_proven',
          value: null,
          reason: 'missing',
        },
        { id: 'extension_owned_timers', availability: 'observed', value: 3, reason: null },
      ),
    ).toBeNull();
    expect(
      resourceReturnedToBaseline(
        { id: 'extension_owned_timers', availability: 'observed', value: 4, reason: null },
        {
          id: 'extension_owned_event_listeners',
          availability: 'observed',
          value: 1,
          reason: null,
        },
      ),
    ).toBeNull();
  });
});
