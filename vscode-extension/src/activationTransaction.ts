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

export type ActivationResourceClass =
  | 'mandatory_for_activation'
  | 'optional_degradable'
  | 'lazy_user_triggered'
  | 'support_surface_allowed_after_failure';

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
    this.resources.push({ ...spec, cleaned: false });
  }

  public resourceIds(): string[] {
    return this.resources.map((resource) => resource.id);
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
