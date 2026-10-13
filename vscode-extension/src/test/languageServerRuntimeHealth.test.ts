import { languageServerRuntimeHealth } from '../languageServerRuntimeHealth';

describe('languageServerRuntimeHealth', () => {
  test('requires the current generation to be running', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'running',
        generation: 7,
        error: undefined,
        serverPath: '/perllsp',
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: true,
      status: 'ok',
      detail: 'Language server is running.',
    });
  });

  test('reports startup failure even when the resolved binary remains available', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'failed',
        generation: 8,
        error: new Error('simulated client.start failure'),
        serverPath: '/perllsp',
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail: 'Language server failed to start: simulated client.start failure',
    });
  });

  test('reports an unknown startup error when failure has no detail', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'failed',
        generation: 9,
        error: null,
        serverPath: '/perllsp',
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail: 'Language server failed to start: unknown startup error',
    });
  });

  test('does not claim runtime health before a generation starts', () => {
    expect(
      languageServerRuntimeHealth({
        state: 'stopped',
        generation: 0,
        error: undefined,
        serverPath: null,
      }),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail: 'Language server is not running.',
    });
  });

  test('rejects setup evidence from an older server path after a retry', () => {
    expect(
      languageServerRuntimeHealth(
        { state: 'running', generation: 2, error: undefined, serverPath: '/server-b' },
        '/server-a',
      ),
    ).toEqual({
      label: 'LSP runtime',
      ok: false,
      status: 'error',
      detail:
        'The language server changed while health checks were running. Run Health Check again.',
    });
  });
});
