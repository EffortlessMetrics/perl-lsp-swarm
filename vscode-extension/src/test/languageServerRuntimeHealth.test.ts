import { languageServerRuntimeHealth } from '../languageServerRuntimeHealth';

describe('languageServerRuntimeHealth', () => {
  test('requires the current generation to be running', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'running',
        generation: 7,
        error: undefined,
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: true,
      status: 'ok',
      detail: 'Language server running (generation 7).',
    });
  });

  test('reports startup failure even when the resolved binary remains available', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'failed',
        generation: 8,
        error: new Error('simulated client.start failure'),
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail: 'Language server failed to start (generation 8): simulated client.start failure',
    });
  });

  test('does not claim runtime health before a generation starts', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'stopped',
        generation: 0,
        error: undefined,
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail: 'Language server is not running (state stopped, generation 0).',
    });
  });
});
