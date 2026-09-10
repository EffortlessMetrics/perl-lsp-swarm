import type * as vscode from 'vscode';
import { ExtensionActivationOwner } from '../activationOwner';

function makeHostContext(): vscode.ExtensionContext {
  return { subscriptions: [] } as unknown as vscode.ExtensionContext;
}

function makeDisposable(disposed: string[], id: string): vscode.Disposable {
  return {
    dispose: () => {
      disposed.push(id);
    },
  };
}

describe('extension activation owner (#7854)', () => {
  test('census reports the resources the owner actually holds, across commit and shutdown (#14678)', async () => {
    const disposed: string[] = [];
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);

    // Registered the way production does: a disposable through own(), a
    // non-disposable teardown through ownCleanup(), and a scoped-context push.
    owner.own('commands', 'mandatory_for_activation', makeDisposable(disposed, 'commands'));
    owner.ownCleanup('module-projections', 'base', 'mandatory_for_activation', () => {
      disposed.push('module-projections');
    });
    owner
      .scopedContext('debugger', 'optional_degradable')
      .subscriptions.push(makeDisposable(disposed, 'debugger'));

    const activating = owner.resourceCensus();
    expect(activating.live_total).toBe(3);
    expect(activating.live_by_class.mandatory_for_activation).toBe(2);
    expect(activating.live_by_class.optional_degradable).toBe(1);

    // Committing transfers ownership but releases nothing.
    owner.commit();
    expect(owner.resourceCensus().live_total).toBe(3);

    // Deactivating the committed runtime must be visible through the owner.
    await owner.deactivate();
    expect(owner.resourceCensus().live_total).toBe(0);
    expect(disposed).toContain('commands');
  });

  test('census keeps a resource the owner failed to release (#14678)', async () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);

    owner.own('commands', 'mandatory_for_activation', {
      dispose: () => {
        throw new Error('dispose exploded');
      },
    });
    owner.own('workspace_listeners', 'mandatory_for_activation', makeDisposable([], 'listeners'));

    owner.commit();
    const receipt = await owner.deactivate();
    expect(receipt?.cleanup_failures).toHaveLength(1);

    // The healthy resource drops out; the one whose dispose() threw was never
    // confirmed released, so the owner still reports it as held.
    expect(owner.resourceCensus().live_total).toBe(1);
  });

  test('a rolled-back attempt still reports the support surfaces it retained (#14678)', async () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);

    owner.own('base', 'support_surface_allowed_after_failure', makeDisposable([], 'output'));
    owner.own('commands', 'mandatory_for_activation', makeDisposable([], 'commands'));

    await owner.rollback();

    const census = owner.resourceCensus();
    expect(census.live_total).toBe(1);
    expect(census.live_by_class.support_surface_allowed_after_failure).toBe(1);
    expect(census.live_by_class.mandatory_for_activation).toBe(0);
  });

  test('registers scoped-context pushes as attempt resources while activating', () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const scoped = owner.scopedContext('debugger', 'mandatory_for_activation');

    const first = makeDisposable([], 'first');
    const second = makeDisposable([], 'second');
    scoped.subscriptions.push(first);
    scoped.subscriptions.push(second);

    expect(owner.resourceIds()).toEqual(['debugger-1', 'debugger-2']);
    // While the attempt is activating the resources are attempt-owned only;
    // the host array stays untouched until commit mirrors them.
    expect(host.subscriptions).toEqual([]);
    expect(owner.currentState()).toBe('activating');

    owner.commit();
    expect(host.subscriptions).toEqual([first, second]);
  });

  test('scoped context delegates every other property to the host context', () => {
    const host = {
      subscriptions: [],
      extensionPath: '/extension/path',
      extensionUri: { fsPath: '/extension/path' },
    } as unknown as vscode.ExtensionContext;
    const owner = new ExtensionActivationOwner(host);
    const scoped = owner.scopedContext('document_providers', 'optional_degradable');

    expect(scoped.extensionPath).toBe('/extension/path');
    expect(scoped).not.toBe(host);
    expect(Object.getPrototypeOf(scoped)).toBe(host);
  });

  test('post-commit scoped-context pushes fall through to host disposal', () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const scoped = owner.scopedContext('document_providers', 'optional_degradable');
    owner.commit();

    // A lazily created resource (for example a POD preview panel dispose hook)
    // is not owned by the closed attempt; it must reach the host net.
    const lazy = makeDisposable([], 'lazy');
    scoped.subscriptions.push(lazy);

    expect(owner.resourceIds()).toEqual([]);
    expect(host.subscriptions).toEqual([lazy]);
  });

  test('commit transfers ownership exactly once and mirrors in creation order', () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const disposed: string[] = [];
    const base = owner.own('base', 'mandatory_for_activation', makeDisposable(disposed, 'base'));
    const command = owner.own(
      'commands',
      'mandatory_for_activation',
      makeDisposable(disposed, 'command'),
    );

    const runtime = owner.commit();
    expect(runtime.currentState()).toBe('active');
    expect(host.subscriptions).toEqual([base, command]);
    // A second commit returns the same committed runtime and does not mirror
    // the disposables twice.
    expect(owner.commit()).toBe(runtime);
    expect(host.subscriptions).toEqual([base, command]);
  });

  test('rollback disposes mandatory resources in reverse order and retains support surfaces', async () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const disposed: string[] = [];
    owner.ownCleanup('module-projections', 'base', 'mandatory_for_activation', () => {
      disposed.push('module-projections');
    });
    const retained = owner.own(
      'support',
      'support_surface_allowed_after_failure',
      makeDisposable(disposed, 'retained'),
    );
    owner.own('commands', 'mandatory_for_activation', makeDisposable(disposed, 'command'));
    owner.own('language_client', 'mandatory_for_activation', makeDisposable(disposed, 'client'));

    const receipt = await owner.rollback();

    expect(owner.currentState()).toBe('activation_failed');
    expect(disposed).toEqual(['client', 'command', 'module-projections']);
    expect(receipt.cleaned_resources).toEqual([
      'language_client-1',
      'commands-1',
      'module-projections',
    ]);
    expect(receipt.retained_support_resources).toEqual(['support-1']);
    // The retained support surface stays explicitly owned: the failed attempt
    // hands it to the host net so shutdown still cleans it.
    expect(host.subscriptions).toEqual([retained]);
  });

  test('cleanup failure during rollback cannot prevent remaining cleanups or retention', async () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const disposed: string[] = [];
    owner.own('base', 'mandatory_for_activation', makeDisposable(disposed, 'base'));
    owner.own('commands', 'mandatory_for_activation', {
      dispose: () => {
        throw new Error('command cleanup exploded');
      },
    });
    owner.own(
      'support',
      'support_surface_allowed_after_failure',
      makeDisposable(disposed, 'retained'),
    );

    const receipt = await owner.rollback();

    expect(disposed).toEqual(['base']);
    expect(receipt.cleanup_failures).toHaveLength(1);
    expect(receipt.cleanup_failures[0]).toMatchObject({
      resource_id: 'commands-1',
      phase: 'commands',
    });
    expect(owner.currentState()).toBe('activation_failed');
  });

  test('deactivate uses the committed runtime and is idempotent', async () => {
    const host = makeHostContext();
    const owner = new ExtensionActivationOwner(host);
    const disposed: string[] = [];
    owner.own('base', 'mandatory_for_activation', makeDisposable(disposed, 'base'));
    owner.own('language_client', 'mandatory_for_activation', makeDisposable(disposed, 'client'));
    owner.commit();

    const first = await owner.deactivate();
    expect(first?.terminal_state).toBe('deactivated');
    expect(first?.cleaned_resources).toEqual(['language_client-1', 'base-1']);
    expect(disposed).toEqual(['client', 'base']);

    // The committed runtime guards repeated deactivation with its receipt.
    const second = await owner.deactivate();
    expect(second).toBe(first);
    expect(disposed).toEqual(['client', 'base']);
  });

  test('deactivate without a committed runtime returns null for the fallback path', async () => {
    const owner = new ExtensionActivationOwner(makeHostContext());
    await owner.rollback();
    expect(await owner.deactivate()).toBeNull();
  });

  test('attempt ids are bounded, unique, and monotonically numbered', () => {
    const first = new ExtensionActivationOwner(makeHostContext());
    const second = new ExtensionActivationOwner(makeHostContext());
    expect(first.attemptId).not.toBe(second.attemptId);
    expect(first.attemptId).toMatch(/^extension-activation-\d+$/);
  });
});
