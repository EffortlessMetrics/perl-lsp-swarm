/**
 * Server-demand ownership: the boundary between *extension* activation and
 * *language-server* activation.
 *
 * VS Code activates this extension for several unrelated surfaces — Perl,
 * Gherkin, the getting-started walkthrough, and Perl debug configuration. Only
 * some of those surfaces need `perllsp` running. Before this module the shared
 * activation path started the language client for every trigger, so opening a
 * `.feature` file or clicking the walkthrough paid the full server-startup
 * cost and reported a `starting` server nobody had asked for.
 *
 * This module is deliberately UI-free and VS Code-free. The extension supplies
 * the actual start function and presentation callbacks when it composes the
 * coordinator, which keeps the whole demand decision unit-testable.
 *
 * Two facts drive the design:
 *
 * 1. `activate()` runs at most once. Returning early on a non-LSP trigger is
 *    therefore not enough — a Perl document opened *later* in the same session
 *    must still start exactly one client generation.
 * 2. Demand can arrive concurrently from several places (a document open, a
 *    server command, a restored editor). All of them must join one start.
 */

/** Whether a trigger needs a given managed process, and when. */
export type ServerDemandDisposition = 'immediate' | 'on-first-use' | 'never';

/** Activation events shipped in `contributes`/`activationEvents`. */
export type ActivationTriggerId =
  | 'onLanguage:perl'
  | 'onLanguage:perl5'
  | 'onLanguage:gherkin'
  | 'onWalkthrough:perl-lsp.gettingStarted'
  | 'onDebugResolve:perl'
  | 'onDebugInitialConfigurations';

/**
 * One checked disposition row.
 *
 * `perllsp` and `perlDap` are recorded separately on purpose: DAP activation is
 * not evidence that the language server must run, and the two processes keep
 * separate identities.
 */
export interface ActivationTriggerRow {
  readonly trigger: ActivationTriggerId;
  readonly perllsp: ServerDemandDisposition;
  readonly perlDap: ServerDemandDisposition;
  readonly note: string;
}

/**
 * The activation-trigger ledger.
 *
 * Every shipped `activationEvents` entry appears exactly once. A trigger that
 * is not in this table has no checked disposition, which the composition test
 * treats as a failure rather than defaulting it to "start the server".
 */
export const ACTIVATION_TRIGGER_LEDGER: readonly ActivationTriggerRow[] = [
  {
    trigger: 'onLanguage:perl',
    perllsp: 'immediate',
    perlDap: 'never',
    note: 'A Perl document is the canonical server-dependent surface.',
  },
  {
    trigger: 'onLanguage:perl5',
    perllsp: 'immediate',
    perlDap: 'never',
    note:
      'Retained alias trigger. This extension contributes no `perl5` language id, ' +
      'so the event only fires when another extension contributes one; treat such a ' +
      'document as an ordinary eligible Perl buffer rather than silently ignoring it.',
  },
  {
    trigger: 'onLanguage:gherkin',
    perllsp: 'never',
    perlDap: 'never',
    note: 'Gherkin providers and step definitions are extension-local and need no perllsp.',
  },
  {
    trigger: 'onWalkthrough:perl-lsp.gettingStarted',
    perllsp: 'on-first-use',
    perlDap: 'never',
    note: 'The walkthrough renders static media; a step that runs a server command starts it.',
  },
  {
    trigger: 'onDebugResolve:perl',
    perllsp: 'on-first-use',
    perlDap: 'immediate',
    note: 'Debugging owns its own adapter process; the language server is not required to debug.',
  },
  {
    trigger: 'onDebugInitialConfigurations',
    perllsp: 'never',
    perlDap: 'never',
    note: 'Producing launch.json stanzas is pure data generation.',
  },
];

/** Server-facing entry points that may demand `perllsp` after activation. */
export interface ServerEntryPointRow {
  readonly command: string;
  readonly perllsp: ServerDemandDisposition;
  readonly note: string;
}

/**
 * Commands whose implementation touches the language client.
 *
 * `on-first-use` means the command routes through `ensureStarted` and may start
 * a dormant server, because the user explicitly asked for something that needs
 * it. `never` means the command reports state and must not create demand — a
 * status read that starts a server would make the dormant state unobservable.
 */
export const SERVER_ENTRY_POINT_LEDGER: readonly ServerEntryPointRow[] = [
  {
    command: 'perl-lsp.restart',
    perllsp: 'on-first-use',
    note: 'Explicit user request for a running server.',
  },
  {
    command: 'perl-lsp.runHealthCheck',
    perllsp: 'on-first-use',
    note: 'Health check resolves and exercises the managed binary.',
  },
  {
    command: 'perl-lsp.showVersion',
    perllsp: 'on-first-use',
    note: 'Reports the resolved server version, which requires a resolved binary.',
  },
  {
    command: 'perl-lsp.showWorkspaceStatus',
    perllsp: 'never',
    note: 'Pure state read. Starting a server here would hide the dormant state.',
  },
  {
    command: 'perl-lsp.showStatusMenu',
    perllsp: 'never',
    note: 'Pure state read; individual menu actions carry their own disposition.',
  },
];

/** Typed demand states exposed to UI and metrics. */
export type ServerDemandState =
  | 'not_started'
  | 'starting'
  | 'running'
  | 'failed'
  | 'action_required';

/** Why the server is currently dormant or blocked. */
export type ServerDemandReasonCode = 'no_server_demand' | 'workspace_untrusted' | 'startup_failure';

/** Minimal document shape needed to decide eligibility. */
export interface ServerDemandDocument {
  readonly languageId: string;
  readonly uriScheme: string;
}

export interface ServerDemandSnapshot {
  readonly state: ServerDemandState;
  readonly reasonCode: ServerDemandReasonCode | undefined;
  readonly generation: number;
  readonly error: unknown;
}

export interface ServerDemandHooks {
  /**
   * Perform the real start. The coordinator guarantees this is never called
   * concurrently with itself and never while the gate is closed.
   */
  startServer(): Promise<void>;
  onStateChange?(snapshot: ServerDemandSnapshot): void;
  log?(message: string): void;
}

/** Language ids that make a document a server-dependent surface. */
const ELIGIBLE_LANGUAGE_IDS: ReadonlySet<string> = new Set(['perl', 'perl5']);

/** Schemes we are willing to synchronize to the server. */
const ELIGIBLE_URI_SCHEMES: ReadonlySet<string> = new Set(['file', 'untitled']);

/**
 * Whether a document is a server-dependent surface.
 *
 * Kept narrow on purpose: a `git:`/`output:`/diff-view buffer that happens to
 * be highlighted as Perl is not a reason to spawn a language server.
 */
export function isServerDependentDocument(document: ServerDemandDocument): boolean {
  return (
    ELIGIBLE_LANGUAGE_IDS.has(document.languageId) && ELIGIBLE_URI_SCHEMES.has(document.uriScheme)
  );
}

/** Look up a trigger's checked `perllsp` disposition. */
export function perllspDispositionFor(trigger: ActivationTriggerId): ServerDemandDisposition {
  const row = ACTIVATION_TRIGGER_LEDGER.find((entry) => entry.trigger === trigger);
  // Fail closed: an unclassified trigger must not silently imply demand.
  return row ? row.perllsp : 'never';
}

/**
 * The single `ensureLanguageServerStarted` owner.
 *
 * Every server-dependent path goes through {@link ensureStarted}. Nothing else
 * may call the underlying start function, so "did we start, and why" has one
 * answer instead of one per call site.
 */
export class ServerDemandCoordinator {
  private state: ServerDemandState = 'not_started';
  private reasonCode: ServerDemandReasonCode | undefined = 'no_server_demand';
  private generation = 0;
  private error: unknown = undefined;
  private inFlight: Promise<void> | undefined;
  private gateOpen = true;
  private pendingDemand = false;
  private disposed = false;

  constructor(private readonly hooks: ServerDemandHooks) {}

  get snapshot(): ServerDemandSnapshot {
    return {
      state: this.state,
      reasonCode: this.reasonCode,
      generation: this.generation,
      error: this.error,
    };
  }

  /** True once a start has been demanded and not yet invalidated. */
  get hasDemand(): boolean {
    return this.state === 'starting' || this.state === 'running' || this.pendingDemand;
  }

  /**
   * Close the start gate (for example, an untrusted workspace).
   *
   * Demand that arrives while the gate is closed is remembered, not dropped, so
   * granting trust starts the server without asking the user to re-trigger it.
   */
  closeGate(reasonCode: ServerDemandReasonCode): void {
    this.gateOpen = false;
    if (this.state === 'not_started' || this.state === 'action_required') {
      this.publish('action_required', reasonCode);
    }
  }

  /** Re-open the gate and honour any demand recorded while it was closed. */
  async openGate(): Promise<void> {
    if (this.disposed || this.gateOpen) {
      return;
    }
    this.gateOpen = true;
    if (this.state === 'action_required') {
      this.publish('not_started', 'no_server_demand');
    }
    if (this.pendingDemand) {
      this.pendingDemand = false;
      await this.ensureStarted('gate-opened');
    }
  }

  /**
   * Start the language server if it is not already starting or running.
   *
   * Concurrent callers join the in-flight start rather than creating a second
   * client generation. A previous failure is *not* retried automatically: an
   * open editor must not re-spawn a server that just crashed on every keystroke.
   * Explicit user actions pass `retry` to override that.
   */
  async ensureStarted(reason: string, options: { retry?: boolean } = {}): Promise<void> {
    if (this.disposed) {
      return;
    }

    if (this.state === 'running') {
      return;
    }

    if (this.inFlight) {
      this.hooks.log?.(`[server-demand] joining in-flight start (${reason})`);
      return this.inFlight;
    }

    if (this.state === 'failed' && !options.retry) {
      this.hooks.log?.(
        `[server-demand] ignoring ${reason}: previous start failed; explicit retry required`,
      );
      return;
    }

    if (!this.gateOpen) {
      this.pendingDemand = true;
      this.hooks.log?.(`[server-demand] recorded demand while gated (${reason})`);
      return;
    }

    const startGeneration = ++this.generation;
    this.hooks.log?.(`[server-demand] starting language server (${reason})`);
    this.publish('starting', undefined);

    const attempt = this.runStart(startGeneration);
    this.inFlight = attempt;
    try {
      await attempt;
    } finally {
      if (this.inFlight === attempt) {
        this.inFlight = undefined;
      }
    }
  }

  private async runStart(startGeneration: number): Promise<void> {
    try {
      await this.hooks.startServer();
      if (!this.isCurrent(startGeneration)) {
        return;
      }
      this.error = undefined;
      this.publish('running', undefined);
    } catch (error: unknown) {
      if (!this.isCurrent(startGeneration)) {
        return;
      }
      this.error = error;
      this.publish('failed', 'startup_failure');
      const message = error instanceof Error ? error.message : String(error);
      this.hooks.log?.(`[server-demand] start failed: ${message}`);
    }
  }

  /**
   * Handle a document that just opened or became active.
   *
   * This is the bounded listener's only job: it converts "a Perl buffer exists"
   * into demand, and ignores everything else.
   */
  async observeDocument(document: ServerDemandDocument): Promise<void> {
    if (this.disposed || !isServerDependentDocument(document)) {
      return;
    }
    await this.ensureStarted(`document:${document.languageId}`);
  }

  /**
   * Apply an activation trigger's checked disposition.
   *
   * `immediate` starts now; `on-first-use` and `never` leave the server dormant
   * and let a later real demand decide.
   */
  async noteActivationTrigger(trigger: ActivationTriggerId): Promise<void> {
    if (perllspDispositionFor(trigger) !== 'immediate') {
      this.hooks.log?.(`[server-demand] ${trigger} does not require perllsp`);
      return;
    }
    await this.ensureStarted(`trigger:${trigger}`);
  }

  /**
   * Record that a running generation now exists without going through
   * {@link ensureStarted}.
   *
   * Restart is the one legitimate case: it owns its own stop-then-start
   * sequence, and afterwards the demand state must agree that a server is
   * running — otherwise a stale `failed` state would suppress later demand.
   */
  noteRunning(): void {
    if (this.disposed) {
      return;
    }
    this.generation += 1;
    this.inFlight = undefined;
    this.error = undefined;
    this.pendingDemand = false;
    this.publish('running', undefined);
  }

  /**
   * Invalidate the current generation after an external stop.
   *
   * The next demand then starts a fresh generation instead of believing a
   * server that is no longer running.
   */
  noteStopped(): void {
    this.generation += 1;
    this.inFlight = undefined;
    this.error = undefined;
    this.publish('not_started', 'no_server_demand');
  }

  dispose(): void {
    this.disposed = true;
    // Bump the generation so an in-flight start cannot publish a running state
    // into a disposed coordinator.
    this.generation += 1;
    this.inFlight = undefined;
  }

  private isCurrent(generation: number): boolean {
    return !this.disposed && this.generation === generation;
  }

  private publish(state: ServerDemandState, reasonCode: ServerDemandReasonCode | undefined): void {
    this.state = state;
    this.reasonCode = reasonCode;
    this.hooks.onStateChange?.(this.snapshot);
  }
}
