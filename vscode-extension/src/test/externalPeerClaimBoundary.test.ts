import type * as vscode from 'vscode';
import { buildLaunchJsonContent, externalDebuggerConfigurationError } from '../debugAdapter';

function asDebugConfiguration(value: Record<string, unknown>): vscode.DebugConfiguration {
  return value as unknown as vscode.DebugConfiguration;
}

const invalidExternalConfigurations: Array<[Record<string, unknown>, string]> = [
  [{ externalPeer: 'host --flag:9000' }, 'externalPeer must use a hostname or IPv4 address'],
  [
    {
      debuggerBackend: 'external',
      externalDebugger: { mode: 'connect', control: 'mirror', port: 0 },
    },
    'connect mode requires a port',
  ],
  [
    {
      debuggerBackend: 'external',
      externalDebugger: { mode: 'launchPeer', control: 'mirror', port: 13604 },
    },
    'launchPeer',
  ],
  [
    {
      debuggerBackend: 'external',
      externalDebugger: { mode: 'connect', control: 'cooperative', port: 13604 },
    },
    'Only mirror control',
  ],
  [
    {
      debuggerBackend: 'external',
      externalDebugger: { kind: 'ptkdb2', mode: 'connect', control: 'mirror', port: 13604 },
    },
    'Unsupported externalDebugger.kind',
  ],
  [
    {
      debuggerBackend: 'external',
      externalDebugger: {
        kind: 'ptkdb',
        mode: 'listen',
        control: 'mirror',
        host: '127.0.0.1',
        port: 0,
      },
    },
    'mode="listen" is not wired yet',
  ],
  [{ debuggerBackend: 'ptkdb-bootstrap' }, 'does not yet wire'],
  [{ externalDebugger: { mode: 'connect', port: 13604 } }, 'requires debuggerBackend="external"'],
  [
    {
      debuggerBackend: 'native',
      externalDebugger: { mode: 'connect', port: 13604 },
    },
    'silently use native debugging',
  ],
];

describe('external debugger claim boundary', () => {
  test('native and behavior-backed peer configurations validate', () => {
    expect(
      externalDebuggerConfigurationError(
        asDebugConfiguration({ debuggerBackend: 'native', program: '/tmp/script.pl' }),
      ),
    ).toBeUndefined();

    expect(
      externalDebuggerConfigurationError(asDebugConfiguration({ externalPeer: '127.0.0.1:13604' })),
    ).toBeUndefined();

    expect(
      externalDebuggerConfigurationError(
        asDebugConfiguration({
          debuggerBackend: 'external',
          externalDebugger: {
            kind: 'ptkdb',
            mode: 'connect',
            control: 'mirror',
            host: '127.0.0.1',
            port: 13604,
          },
        }),
      ),
    ).toBeUndefined();
  });

  test.each(invalidExternalConfigurations)(
    'rejects unsupported explicit selection %#',
    (configuration, expected) => {
      expect(externalDebuggerConfigurationError(asDebugConfiguration(configuration))).toContain(
        expected,
      );
    },
  );

  test('rejects ambiguous flat and structured peer selections', () => {
    expect(
      externalDebuggerConfigurationError(
        asDebugConfiguration({
          externalPeer: '127.0.0.1:13604',
          debuggerBackend: 'external',
          externalDebugger: { mode: 'connect', port: 13604 },
        }),
      ),
    ).toContain('not both');
  });

  test('wizard labels the peer template as experimental', () => {
    const parsed = JSON.parse(buildLaunchJsonContent('external-peer')) as {
      configurations: Array<{ name?: string; externalPeer?: string }>;
    };
    expect(parsed.configurations).toHaveLength(1);
    expect(parsed.configurations[0]?.name).toContain('experimental');
    expect(parsed.configurations[0]?.externalPeer).toBe('127.0.0.1:13604');
  });
});
