import { ActivationTransaction } from '../activationTransaction';

describe('ActivationTransaction', () => {
  test('rolls back resources in deterministic reverse registration order', async () => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction('attempt-1');
    transaction.registerResource({
      id: 'status',
      phase: 'base',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('status');
      },
    });
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('commands');
      },
    });
    transaction.registerResource({
      id: 'client',
      phase: 'language_client',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('client');
      },
    });

    const receipt = await transaction.rollback();

    expect(cleanupOrder).toEqual(['client', 'commands', 'status']);
    expect(receipt.cleaned_resources).toEqual(['client', 'commands', 'status']);
    expect(receipt.cleanup_failures).toEqual([]);
    expect(transaction.currentState()).toBe('activation_failed');
  });

  test('attempts later cleanup even when one resource cleanup fails', async () => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction('attempt-2');
    transaction.registerResource({
      id: 'base',
      phase: 'base',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('base');
      },
    });
    transaction.registerResource({
      id: 'broken',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('broken');
        throw new Error('cleanup exploded\nprivate detail');
      },
    });
    transaction.registerResource({
      id: 'client',
      phase: 'language_client',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('client');
      },
    });

    const receipt = await transaction.rollback();

    expect(cleanupOrder).toEqual(['client', 'broken', 'base']);
    expect(receipt.cleaned_resources).toEqual(['client', 'base']);
    expect(receipt.cleanup_failures).toEqual([
      {
        resource_id: 'broken',
        phase: 'commands',
        reason: 'cleanup exploded',
      },
    ]);

    const second = await transaction.rollback();
    expect(second).toBe(receipt);
  });

  test('may deliberately retain only approved support surfaces after failure', async () => {
    const supportCleanup = jest.fn();
    const mandatoryCleanup = jest.fn();
    const transaction = new ActivationTransaction('attempt-3');
    transaction.registerResource({
      id: 'output-support',
      phase: 'support',
      resource_class: 'support_surface_allowed_after_failure',
      cleanup: supportCleanup,
    });
    transaction.registerResource({
      id: 'client',
      phase: 'language_client',
      resource_class: 'mandatory_for_activation',
      cleanup: mandatoryCleanup,
    });

    const receipt = await transaction.rollback({ retain_support_surfaces: true });

    expect(mandatoryCleanup).toHaveBeenCalledTimes(1);
    expect(supportCleanup).not.toHaveBeenCalled();
    expect(receipt.retained_support_resources).toEqual(['output-support']);
  });

  test('commit transfers the same resource graph to normal deactivation', async () => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction('attempt-4');
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('commands');
      },
    });
    transaction.registerResource({
      id: 'watchdog',
      phase: 'background',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('watchdog');
      },
    });

    const runtime = transaction.commit();
    expect(transaction.currentState()).toBe('active');
    expect(runtime.currentState()).toBe('active');

    const first = await runtime.deactivate();
    const second = await runtime.deactivate();

    expect(cleanupOrder).toEqual(['watchdog', 'commands']);
    expect(first).toBe(second);
    expect(first.terminal_state).toBe('deactivated');
    expect(transaction.currentState()).toBe('deactivated');
  });

  test('prevents duplicate resource ownership and registration after commit', () => {
    const transaction = new ActivationTransaction('attempt-5');
    transaction.registerResource({
      id: 'commands',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: jest.fn(),
    });

    expect(() =>
      transaction.registerResource({
        id: 'commands',
        phase: 'commands',
        resource_class: 'optional_degradable',
        cleanup: jest.fn(),
      }),
    ).toThrow('duplicate activation resource id: commands');

    transaction.commit();
    expect(() =>
      transaction.registerResource({
        id: 'late',
        phase: 'optional_ui',
        resource_class: 'optional_degradable',
        cleanup: jest.fn(),
      }),
    ).toThrow('cannot register activation resource while state=active');
  });

  test('rejects resource identities that could encode filesystem ownership', () => {
    const transaction = new ActivationTransaction('attempt-6');
    expect(() =>
      transaction.registerResource({
        id: '../workspace-resource',
        phase: 'base',
        resource_class: 'mandatory_for_activation',
        cleanup: jest.fn(),
      }),
    ).toThrow('activation resource id must be bounded and path-independent');
  });
});
