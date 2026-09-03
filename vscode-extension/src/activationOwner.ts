import type * as vscode from 'vscode';
import {
  ActivationTransaction,
  type ActivationAttemptState,
  type ActivationCleanupReceipt,
  type ActivationPhase,
  type ActivationResourceCensus,
  type ActivationResourceClass,
  type CommittedActivation,
} from './activationTransaction';

/**
 * Production wiring for the activation transaction substrate (#7854).
 *
 * {@link ActivationTransaction} owns the attempt state machine, the
 * registration ledger, and deterministic reverse-order cleanup. This module
 * adapts the production `activate()` path to it without changing any
 * resource-creation order:
 *
 * - every disposable created during activation is registered with the attempt
 *   immediately after creation, carrying an explicit phase and resource class;
 * - sub-modules that push into `context.subscriptions` themselves (debugger
 *   registrations, lazily created POD preview panels) receive a scoped facade
 *   context whose `subscriptions` route into the same attempt while it is
 *   activating and fall through to the host array once the attempt closed, so
 *   post-commit lazy resources keep ordinary host disposal;
 * - on failure the attempt rolls back in reverse registration order,
 *   retaining only `support_surface_allowed_after_failure` surfaces, and the
 *   retained surfaces are handed to the host net so shutdown still cleans
 *   them;
 * - on success `commit()` transfers ownership to normal deactivation: the
 *   owned disposables are mirrored into the host `context.subscriptions`
 *   (the pre-existing shutdown net) and `deactivate()` runs the committed
 *   runtime, which uses the same cleanup primitives as rollback.
 */
export class ExtensionActivationOwner {
  public readonly attemptId: string;

  private readonly transaction: ActivationTransaction;
  private readonly ownedDisposablesById = new Map<string, vscode.Disposable>();
  private readonly ownedDisposables: vscode.Disposable[] = [];
  private readonly phaseOrdinals = new Map<ActivationPhase, number>();
  private committedRuntime: CommittedActivation | null = null;
  private lastReceipt: ActivationCleanupReceipt | null = null;

  constructor(
    private readonly hostContext: vscode.ExtensionContext,
    private readonly log: (message: string) => void = () => undefined,
  ) {
    this.attemptId = nextExtensionActivationAttemptId();
    this.transaction = new ActivationTransaction(this.attemptId);
  }

  public currentState(): ActivationAttemptState {
    return this.transaction.currentState();
  }

  public resourceIds(): string[] {
    return this.transaction.resourceIds();
  }

  /**
   * Ownership-aware count of the resources this attempt still holds (#14678).
   *
   * Reads the committed runtime once the attempt commits so a deactivation
   * drains the census; both views share one resource ledger, so the value is
   * the same fact observed through the current owner.
   */
  public resourceCensus(): ActivationResourceCensus {
    return (this.committedRuntime ?? this.transaction).resourceCensus();
  }

  public lastCleanupReceipt(): ActivationCleanupReceipt | null {
    return this.lastReceipt;
  }

  /**
   * Registers one disposable with the current attempt immediately after its
   * creation and returns it unchanged so call sites keep their shape.
   */
  public own(
    phase: ActivationPhase,
    resourceClass: ActivationResourceClass,
    disposable: vscode.Disposable,
  ): vscode.Disposable {
    const ordinal = this.nextPhaseOrdinal(phase);
    const id = `${phase}-${ordinal}`;
    this.transaction.registerResource({
      id,
      phase,
      resource_class: resourceClass,
      cleanup: () => {
        disposable.dispose();
      },
    });
    this.ownedDisposablesById.set(id, disposable);
    this.ownedDisposables.push(disposable);
    maybeInjectPhaseBoundaryFailure({ phase, ordinal, resource_id: id });
    return disposable;
  }

  public ownDisposables(
    phase: ActivationPhase,
    resourceClass: ActivationResourceClass,
    disposables: vscode.Disposable[],
  ): void {
    for (const disposable of disposables) {
      this.own(phase, resourceClass, disposable);
    }
  }

  /**
   * Registers a non-disposable resource (module projection clearing, language
   * client teardown) whose cleanup runs in the same deterministic order.
   */
  public ownCleanup(
    id: string,
    phase: ActivationPhase,
    resourceClass: ActivationResourceClass,
    cleanup: () => void | Promise<void>,
  ): void {
    this.transaction.registerResource({ id, phase, resource_class: resourceClass, cleanup });
    maybeInjectPhaseBoundaryFailure({
      phase,
      ordinal: this.nextPhaseOrdinal(phase),
      resource_id: id,
    });
  }

  /**
   * A facade context whose `subscriptions` pushes are owned by the attempt
   * while it is activating. Once the attempt committed or rolled back, pushes
   * fall through to the host array: post-commit lazily created resources
   * (for example a POD preview webview panel) belong to ordinary host
   * disposal, never to a closed activation attempt.
   *
   * The facade uses the host context as its prototype, so every other
   * property — including `extensionPath` and `globalStorageUri` used long
   * after activation — keeps delegating to the real object.
   */
  public scopedContext(
    phase: ActivationPhase,
    resourceClass: ActivationResourceClass,
  ): vscode.ExtensionContext {
    const scoped = Object.create(this.hostContext) as vscode.ExtensionContext;
    Object.defineProperty(scoped, 'subscriptions', {
      value: this.createRoutedSubscriptions(phase, resourceClass),
    });
    return scoped;
  }

  /**
   * Commits the attempt and transfers the host safety net: the owned
   * disposables are mirrored into `context.subscriptions` in creation order,
   * exactly the array content the pre-transaction code produced by pushing at
   * creation time. Normal deactivation runs {@link deactivate}, which uses
   * the same cleanup primitives as rollback.
   */
  public commit(): CommittedActivation {
    if (this.committedRuntime !== null) {
      return this.committedRuntime;
    }
    const runtime = this.transaction.commit();
    this.hostContext.subscriptions.push(...this.ownedDisposables);
    this.committedRuntime = runtime;
    return runtime;
  }

  /**
   * Rolls the failed attempt back in reverse registration order, retaining
   * only explicitly approved support surfaces. Retained surfaces stay
   * reachable for failure reporting and are pushed into the host
   * `context.subscriptions` so shutdown still disposes them.
   */
  public async rollback(): Promise<ActivationCleanupReceipt> {
    const receipt = await this.transaction.rollback({ retain_support_surfaces: true });
    this.lastReceipt = receipt;
    for (const retainedId of receipt.retained_support_resources) {
      const retained = this.ownedDisposablesById.get(retainedId);
      if (retained !== undefined) {
        this.hostContext.subscriptions.push(retained);
      }
    }
    for (const failure of receipt.cleanup_failures) {
      this.log(
        `[activation] cleanup failed for ${failure.resource_id} (${failure.phase}): ${failure.reason}`,
      );
    }
    return receipt;
  }

  /**
   * Deactivates the committed runtime. Returns `null` when no runtime was
   * committed (activation never ran to commit or was rolled back) so the
   * caller can keep its pre-transaction fallback.
   */
  public async deactivate(): Promise<ActivationCleanupReceipt | null> {
    const runtime = this.committedRuntime;
    if (runtime === null) {
      return null;
    }
    const receipt = await runtime.deactivate();
    this.lastReceipt = receipt;
    for (const failure of receipt.cleanup_failures) {
      this.log(
        `[activation] cleanup failed for ${failure.resource_id} (${failure.phase}): ${failure.reason}`,
      );
    }
    return receipt;
  }

  private nextPhaseOrdinal(phase: ActivationPhase): number {
    const ordinal = (this.phaseOrdinals.get(phase) ?? 0) + 1;
    this.phaseOrdinals.set(phase, ordinal);
    return ordinal;
  }

  private createRoutedSubscriptions(
    phase: ActivationPhase,
    resourceClass: ActivationResourceClass,
  ): vscode.Disposable[] {
    const routed: vscode.Disposable[] = [];
    routed.push = (...items: vscode.Disposable[]): number => {
      if (this.transaction.currentState() === 'activating') {
        for (const item of items) {
          this.own(phase, resourceClass, item);
        }
        return routed.length;
      }
      return this.hostContext.subscriptions.push(...items);
    };
    return routed;
  }
}

let extensionActivationSequence = 0;

function nextExtensionActivationAttemptId(): string {
  extensionActivationSequence += 1;
  return `extension-activation-${extensionActivationSequence}`;
}

/**
 * One production resource boundary as the attempt crosses it (#7855): the
 * phase the registration belongs to, its per-phase ordinal (counting every
 * `own*` registration in that phase, in creation order), and the ledger id the
 * attempt recorded.
 */
export interface ActivationPhaseBoundary {
  phase: ActivationPhase;
  ordinal: number;
  resource_id: string;
}

/**
 * Test-only phase-boundary failure injection (#7855).
 *
 * The injector is consulted immediately AFTER a production resource boundary
 * completes — the resource was created by the production activation body and
 * registered with the attempt — so returning an Error fails the activation at
 * a named boundary through exactly the path any real mid-activation exception
 * takes: it propagates out of the registration call site, `activate()` rolls
 * the attempt back, and the error is rethrown. It must never be set outside
 * tests; production code never reads it.
 * @internal
 */
export type ActivationPhaseFailureInjector = (boundary: ActivationPhaseBoundary) => Error | null;

let activationPhaseFailureInjector: ActivationPhaseFailureInjector | null = null;

/**
 * Install (or clear) the test-only phase-boundary failure injector (#7855).
 * @internal
 */
export function _setActivationPhaseFailureInjectorForTest(
  injector: ActivationPhaseFailureInjector | null,
): void {
  activationPhaseFailureInjector = injector;
}

function maybeInjectPhaseBoundaryFailure(boundary: ActivationPhaseBoundary): void {
  const error = activationPhaseFailureInjector?.(boundary) ?? null;
  if (error !== null) {
    throw error;
  }
}
