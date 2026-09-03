export type ActivationPhase =
  | 'base'
  | 'commands'
  | 'workspace_listeners'
  | 'language_client'
  | 'document_providers'
  | 'testing'
  | 'debugger'
  | 'optional_ui'
  | 'support'
  | 'background';

/**
 * Runtime authority for {@link ActivationPhase}: the literal union and this
 * list must stay in lockstep so test-only phase targeting (#7855, #7856) can
 * validate a phase name before arming an injection.
 */
export const ACTIVATION_PHASES: readonly ActivationPhase[] = [
  'base',
  'commands',
  'workspace_listeners',
  'language_client',
  'document_providers',
  'testing',
  'debugger',
  'optional_ui',
  'support',
  'background',
];

export type ActivationResourceClass =
  | 'mandatory_for_activation'
  | 'optional_degradable'
  | 'lazy_user_triggered'
  | 'support_surface_allowed_after_failure';

/**
 * Zeroed census buckets, one per declared {@link ActivationResourceClass}.
 *
 * Written as an explicit typed literal rather than built from a name list and
 * cast: the annotation makes exhaustiveness a compile error in both directions
 * — a class added to the union without a bucket here fails to typecheck, and so
 * does a bucket for a class the union does not declare. A cast would instead
 * let a missing bucket survive to runtime and turn that class's count into
 * `NaN` on the first increment.
 */
function emptyClassCounts(): Record<ActivationResourceClass, number> {
  return {
    mandatory_for_activation: 0,
    optional_degradable: 0,
    lazy_user_triggered: 0,
    support_surface_allowed_after_failure: 0,
  };
}

/**
 * Runtime authority for {@link ActivationResourceClass}, derived from the
 * census buckets so the two cannot drift, letting the extension-owned resource
 * census (#14678) report one deterministic bucket per class instead of only the
 * classes the current attempt happens to populate.
 */
export const ACTIVATION_RESOURCE_CLASSES: readonly ActivationResourceClass[] = Object.keys(
  emptyClassCounts(),
) as ActivationResourceClass[];

/**
 * Bounded count of the resources this attempt still owns.
 *
 * "Live" means registered and not yet cleaned. {@link
 * ActivationTransaction.resourceIds} reports every resource ever registered,
 * including cleaned ones, so it cannot express current ownership and cannot
 * distinguish a deactivated attempt from a leaked one. The census is the
 * ownership-aware source for the `extension_owned_*` counters in
 * `vscode_client_measurement.v1`: it counts only resources this extension
 * registered, never host-wide listeners or disposables (#7866).
 */
export interface ActivationResourceCensus {
  live_total: number;
  live_by_class: Record<ActivationResourceClass, number>;
}

export type ActivationAttemptState =
  | 'inactive'
  | 'activating'
  | 'active'
  | 'activation_failed'
  | 'deactivating'
  | 'deactivated';

export interface ActivationResourceSpec {
  id: string;
  phase: ActivationPhase;
  resource_class: ActivationResourceClass;
  cleanup: () => void | Promise<void>;
}

export interface ActivationCleanupFailure {
  resource_id: string;
  phase: ActivationPhase;
  reason: string;
}

export interface ActivationCleanupReceipt {
  attempt_id: string;
  terminal_state: 'activation_failed' | 'deactivated';
  cleaned_resources: string[];
  retained_support_resources: string[];
  cleanup_failures: ActivationCleanupFailure[];
}

interface OwnedActivationResource extends ActivationResourceSpec {
  cleaned: boolean;
  /**
   * Set when this resource's cleanup threw. The attempt still marks such a
   * resource `cleaned` so it is never retried, but the throw means release was
   * never confirmed — so for census purposes it stays owned.
   */
  releaseFailed: boolean;
}

function boundedErrorReason(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const firstLine = raw.split(/\r\n|[\n\r\u2028\u2029]/, 1)[0]?.trim() ?? 'cleanup failed';
  return firstLine.length <= 160 ? firstLine : `${firstLine.slice(0, 157)}...`;
}

function validateResourceSpec(spec: ActivationResourceSpec): void {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._:-]{0,127}$/.test(spec.id)) {
    throw new Error(`activation resource id must be bounded and path-independent: ${spec.id}`);
  }
}

function censusOf(resources: readonly OwnedActivationResource[]): ActivationResourceCensus {
  const liveByClass = emptyClassCounts();
  let liveTotal = 0;
  for (const resource of resources) {
    // A resource is owned until its release is *confirmed*. A cleanup that
    // threw is marked cleaned so the attempt never retries it, but the throw
    // leaves the resource possibly still held — counting it as released would
    // let the very case most likely to be a leak report a clean baseline.
    if (resource.cleaned && !resource.releaseFailed) {
      continue;
    }
    liveByClass[resource.resource_class] += 1;
    liveTotal += 1;
  }

  return { live_total: liveTotal, live_by_class: liveByClass };
}

async function cleanResources(
  resources: OwnedActivationResource[],
  retainSupportAfterFailure: boolean,
): Promise<
  Pick<
    ActivationCleanupReceipt,
    'cleaned_resources' | 'retained_support_resources' | 'cleanup_failures'
  >
> {
  const cleanedResources: string[] = [];
  const retainedSupportResources: string[] = [];
  const cleanupFailures: ActivationCleanupFailure[] = [];

  for (const resource of [...resources].reverse()) {
    if (resource.cleaned) {
      continue;
    }
    if (
      retainSupportAfterFailure &&
      resource.resource_class === 'support_surface_allowed_after_failure'
    ) {
      retainedSupportResources.push(resource.id);
      continue;
    }

    try {
      await resource.cleanup();
      resource.cleaned = true;
      cleanedResources.push(resource.id);
    } catch (error: unknown) {
      resource.cleaned = true;
      resource.releaseFailed = true;
      cleanupFailures.push({
        resource_id: resource.id,
        phase: resource.phase,
        reason: boundedErrorReason(error),
      });
    }
  }

  return {
    cleaned_resources: cleanedResources,
    retained_support_resources: retainedSupportResources,
    cleanup_failures: cleanupFailures,
  };
}

export class ActivationTransaction {
  private state: ActivationAttemptState = 'activating';
  private readonly resources: OwnedActivationResource[] = [];
  private committedRuntime: CommittedActivation | null = null;
  private rollbackReceipt: ActivationCleanupReceipt | null = null;

  public constructor(public readonly attempt_id: string) {
    if (!/^[a-zA-Z0-9][a-zA-Z0-9._:-]{0,127}$/.test(attempt_id)) {
      throw new Error('activation attempt id must be bounded and path-independent');
    }
  }

  public currentState(): ActivationAttemptState {
    return this.state;
  }

  public registerResource(spec: ActivationResourceSpec): void {
    if (this.state !== 'activating') {
      throw new Error(`cannot register activation resource while state=${this.state}`);
    }
    validateResourceSpec(spec);
    if (this.resources.some((resource) => resource.id === spec.id)) {
      throw new Error(`duplicate activation resource id: ${spec.id}`);
    }
    this.resources.push({ ...spec, cleaned: false, releaseFailed: false });
  }

  public resourceIds(): string[] {
    return this.resources.map((resource) => resource.id);
  }

  /** Ownership-aware count of the resources this attempt still holds. */
  public resourceCensus(): ActivationResourceCensus {
    return censusOf(this.resources);
  }

  public commit(): CommittedActivation {
    if (this.state !== 'activating') {
      throw new Error(`cannot commit activation while state=${this.state}`);
    }
    this.state = 'active';
    this.committedRuntime = new CommittedActivation(this.attempt_id, this.resources, () => {
      this.state = 'deactivated';
    });
    return this.committedRuntime;
  }

  public async rollback(
    options: { retain_support_surfaces?: boolean } = {},
  ): Promise<ActivationCleanupReceipt> {
    if (this.rollbackReceipt !== null) {
      return this.rollbackReceipt;
    }
    if (this.state !== 'activating') {
      throw new Error(`cannot rollback activation while state=${this.state}`);
    }

    const cleanup = await cleanResources(this.resources, options.retain_support_surfaces === true);
    this.state = 'activation_failed';
    this.rollbackReceipt = {
      attempt_id: this.attempt_id,
      terminal_state: 'activation_failed',
      ...cleanup,
    };
    return this.rollbackReceipt;
  }

  public activeRuntime(): CommittedActivation | null {
    return this.committedRuntime;
  }
}

export class CommittedActivation {
  private state: ActivationAttemptState = 'active';
  private receipt: ActivationCleanupReceipt | null = null;

  public constructor(
    public readonly attempt_id: string,
    private readonly resources: OwnedActivationResource[],
    private readonly onDeactivated: () => void,
  ) {}

  public currentState(): ActivationAttemptState {
    return this.state;
  }

  /**
   * Ownership-aware count of the resources the committed runtime still holds.
   *
   * Shares the attempt's resource ledger, so a resource released during
   * deactivation leaves the census here and in the originating transaction
   * together. A resource whose cleanup threw was never confirmed released and
   * stays counted, as does one deliberately retained by a rollback
   * (`retain_support_surfaces`).
   */
  public resourceCensus(): ActivationResourceCensus {
    return censusOf(this.resources);
  }

  public async deactivate(): Promise<ActivationCleanupReceipt> {
    if (this.receipt !== null) {
      return this.receipt;
    }
    if (this.state !== 'active') {
      throw new Error(`cannot deactivate activation runtime while state=${this.state}`);
    }

    this.state = 'deactivating';
    const cleanup = await cleanResources(this.resources, false);
    this.state = 'deactivated';
    this.receipt = {
      attempt_id: this.attempt_id,
      terminal_state: 'deactivated',
      ...cleanup,
    };
    this.onDeactivated();
    return this.receipt;
  }
}
