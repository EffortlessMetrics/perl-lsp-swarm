export type ClientMeasurementPhase =
  | 'activation_requested'
  | 'base_surface_ready'
  | 'registrations_complete'
  | 'binary_selection_started'
  | 'binary_selection_complete'
  | 'language_client_constructed'
  | 'server_spawn_handoff'
  | 'initialize_accepted'
  | 'document_replay_complete'
  | 'active_document_useful'
  | 'activation_committed'
  | 'deactivate_complete';

export type ClientMeasurementAvailability = 'observed' | 'not_proven';

export interface ClientMeasurementSubject {
  candidate: string;
  vscode_version: string;
  platform: string;
  architecture: string;
  host_kind: 'local' | 'remote' | 'unknown';
  scenario: string;
  cold_warm: 'cold' | 'warm';
  binary_role: 'managed' | 'user_supplied' | 'bundled' | 'unknown';
  server_candidate: string | null;
}

export interface ClientPhaseMeasurement {
  phase: ClientMeasurementPhase;
  availability: ClientMeasurementAvailability;
  offset_ms: number | null;
}

export interface ClientResourceMeasurement {
  id: string;
  availability: ClientMeasurementAvailability;
  value: number | null;
  reason: string | null;
}

export interface VscodeClientMeasurementSnapshot {
  schema_version: 'vscode_client_measurement.v1';
  subject: ClientMeasurementSubject;
  phases: ClientPhaseMeasurement[];
  resources: ClientResourceMeasurement[];
  instrument_complete: boolean;
}

const PHASE_ORDER: ClientMeasurementPhase[] = [
  'activation_requested',
  'base_surface_ready',
  'registrations_complete',
  'binary_selection_started',
  'binary_selection_complete',
  'language_client_constructed',
  'server_spawn_handoff',
  'initialize_accepted',
  'document_replay_complete',
  'active_document_useful',
  'activation_committed',
  'deactivate_complete',
];

export class VscodeClientMeasurementRecorder {
  private readonly originMs: number;
  private readonly phaseOffsets = new Map<ClientMeasurementPhase, number>();
  private readonly unavailablePhases = new Set<ClientMeasurementPhase>();
  private readonly resources = new Map<string, ClientResourceMeasurement>();

  public constructor(
    private readonly subject: ClientMeasurementSubject,
    originMs: number = performance.now(),
  ) {
    this.originMs = originMs;
  }

  public markPhase(
    phase: ClientMeasurementPhase,
    observedAtMs: number = performance.now(),
  ): void {
    if (this.phaseOffsets.has(phase) || this.unavailablePhases.has(phase)) {
      return;
    }
    this.phaseOffsets.set(phase, Math.max(0, Math.round(observedAtMs - this.originMs)));
  }

  public markPhaseNotProven(phase: ClientMeasurementPhase): void {
    if (this.phaseOffsets.has(phase)) {
      return;
    }
    this.unavailablePhases.add(phase);
  }

  public observeResource(id: string, value: number): void {
    if (!Number.isFinite(value) || value < 0) {
      throw new Error(`resource measurement must be a finite non-negative number: ${id}`);
    }
    this.resources.set(id, {
      id,
      availability: 'observed',
      value,
      reason: null,
    });
  }

  public markResourceNotProven(id: string, reason: string): void {
    const normalizedReason = reason.trim();
    if (normalizedReason.length === 0) {
      throw new Error(`not-proven resource measurement requires a reason: ${id}`);
    }
    this.resources.set(id, {
      id,
      availability: 'not_proven',
      value: null,
      reason: normalizedReason,
    });
  }

  public snapshot(): VscodeClientMeasurementSnapshot {
    const phases = PHASE_ORDER.map<ClientPhaseMeasurement>((phase) => {
      const observed = this.phaseOffsets.get(phase);
      if (observed !== undefined) {
        return { phase, availability: 'observed', offset_ms: observed };
      }
      return { phase, availability: 'not_proven', offset_ms: null };
    });

    const resources = [...this.resources.values()].sort((left, right) =>
      left.id.localeCompare(right.id),
    );

    return {
      schema_version: 'vscode_client_measurement.v1',
      subject: { ...this.subject },
      phases,
      resources,
      instrument_complete: phases.every((phase) => phase.availability === 'observed'),
    };
  }
}

export function resourceReturnedToBaseline(
  before: ClientResourceMeasurement,
  after: ClientResourceMeasurement,
): boolean | null {
  if (
    before.availability !== 'observed' ||
    after.availability !== 'observed' ||
    before.value === null ||
    after.value === null
  ) {
    return null;
  }
  return after.value <= before.value;
}
