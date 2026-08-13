import {
  type ActivationPhase,
  ActivationTransaction,
} from '../activationTransaction';

const PHASES: ActivationPhase[] = [
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

function registerThroughPhase(
  transaction: ActivationTransaction,
  failedPhase: ActivationPhase,
  cleanupOrder: string[],
): string[] {
  const registered: string[] = [];
  for (const phase of PHASES) {
    const id = `${phase}-resource`;
    transaction.registerResource({
      id,
      phase,
      resource_class:
        phase === 'support'
          ? 'support_surface_allowed_after_failure'
          : 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push(id);
      },
    });
    registered.push(id);
    if (phase === failedPhase) {
      break;
    }
  }
  return registered;
}

describe('activation transaction deterministic fault matrix', () => {
  test.each(PHASES)('rolls back every owned resource when %s phase fails', async (failedPhase) => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction(`failure-${failedPhase}`);
    const registered = registerThroughPhase(transaction, failedPhase, cleanupOrder);

    const receipt = await transaction.rollback();

    expect(transaction.currentState()).toBe('activation_failed');
    expect(receipt.cleanup_failures).toEqual([]);
    expect(receipt.retained_support_resources).toEqual([]);
    expect(cleanupOrder).toEqual([...registered].reverse());
    expect(receipt.cleaned_resources).toEqual([...registered].reverse());
  });

  test('retains only explicitly approved support surface while mandatory resources roll back', async () => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction('failure-after-support');
    registerThroughPhase(transaction, 'background', cleanupOrder);

    const receipt = await transaction.rollback({ retain_support_surfaces: true });

    expect(receipt.retained_support_resources).toEqual(['support-resource']);
    expect(receipt.cleaned_resources).not.toContain('support-resource');
    expect(cleanupOrder).not.toContain('support-resource');
    expect(cleanupOrder).toEqual([
      'background-resource',
      'optional_ui-resource',
      'debugger-resource',
      'testing-resource',
      'document_providers-resource',
      'language_client-resource',
      'workspace_listeners-resource',
      'commands-resource',
      'base-resource',
    ]);
  });

  test('cleanup failure cannot prevent remaining earlier resources from being attempted', async () => {
    const cleanupOrder: string[] = [];
    const transaction = new ActivationTransaction('cleanup-failure');
    transaction.registerResource({
      id: 'base-resource',
      phase: 'base',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('base-resource');
      },
    });
    transaction.registerResource({
      id: 'commands-resource',
      phase: 'commands',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('commands-resource');
        throw new Error('commands cleanup failed');
      },
    });
    transaction.registerResource({
      id: 'client-resource',
      phase: 'language_client',
      resource_class: 'mandatory_for_activation',
      cleanup: () => {
        cleanupOrder.push('client-resource');
      },
    });

    const receipt = await transaction.rollback();

    expect(cleanupOrder).toEqual(['client-resource', 'commands-resource', 'base-resource']);
    expect(receipt.cleaned_resources).toEqual(['client-resource', 'base-resource']);
    expect(receipt.cleanup_failures).toEqual([
      {
        resource_id: 'commands-resource',
        phase: 'commands',
        reason: 'commands cleanup failed',
      },
    ]);
  });

  test('failed-attempt resource graph cannot accept late registration or commit', async () => {
    const transaction = new ActivationTransaction('failed-attempt-closed');
    transaction.registerResource({
      id: 'client-resource',
      phase: 'language_client',
      resource_class: 'mandatory_for_activation',
      cleanup: jest.fn(),
    });
    await transaction.rollback();

    expect(() =>
      transaction.registerResource({
        id: 'late-resource',
        phase: 'optional_ui',
        resource_class: 'optional_degradable',
        cleanup: jest.fn(),
      }),
    ).toThrow('cannot register activation resource while state=activation_failed');
    expect(() => transaction.commit()).toThrow(
      'cannot commit activation while state=activation_failed',
    );
  });
});
