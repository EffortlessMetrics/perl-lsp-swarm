import * as assert from 'assert';
import { debuggeeCreationTimeFromProbe } from './published/journeySupport';
import type { BoundedProcessResult } from '../testAdapter';

const success: BoundedProcessResult = {
  outcome: 'completed',
  stdout: '133700000000000000\r\n',
  stderr: '',
  exitCode: 0,
  signal: null,
  capturedOutputBytes: 20,
};

describe('installed debuggee probe interpretation', () => {
  it('preserves the exact creation identity without numeric rounding', () => {
    assert.equal(debuggeeCreationTimeFromProbe(4242, success), '133700000000000000');
  });

  it('recognizes absence only after a successful empty probe', () => {
    assert.equal(debuggeeCreationTimeFromProbe(4242, { ...success, stdout: '' }), null);
    for (const outcome of ['timed_out', 'termination_failed', 'spawn_error'] as const) {
      assert.throws(() => debuggeeCreationTimeFromProbe(4242, { ...success, outcome, stdout: '' }));
    }
    assert.throws(() =>
      debuggeeCreationTimeFromProbe(4242, { ...success, exitCode: 1, stdout: '' }),
    );
  });

  it('retains the PID and full bounded failure evidence', () => {
    const failure: BoundedProcessResult = {
      outcome: 'termination_failed',
      stdout: 'partial identity',
      stderr: 'probe stderr marker',
      exitCode: null,
      signal: 'SIGKILL',
      capturedOutputBytes: 37,
      diagnostic: 'tree cleanup exceeded its bound',
    };
    assert.throws(
      () => debuggeeCreationTimeFromProbe(4242, failure),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        const prefix = 'owned debuggee scan failed: ';
        assert.ok(error.message.startsWith(prefix));
        assert.deepEqual(JSON.parse(error.message.slice(prefix.length)), { pid: 4242, ...failure });
        return true;
      },
    );
  });

  it('rejects malformed identity output and invalid PIDs', () => {
    assert.throws(() =>
      debuggeeCreationTimeFromProbe(4242, { ...success, stdout: 'not a timestamp' }),
    );
    assert.throws(() => debuggeeCreationTimeFromProbe(-1, success));
  });
});
