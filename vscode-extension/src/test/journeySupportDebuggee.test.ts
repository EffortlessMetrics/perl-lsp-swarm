import * as assert from 'assert';
import * as path from 'path';
import { debuggeeCreationTimeFromProbe, requirePackagedBreakpointStop } from './published/journeySupport';
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

describe('installed editor breakpoint trace', () => {
  const program = path.resolve('fixture', 'program.pl');
  const line = 8;
  const set = {
    type: 'request',
    command: 'setBreakpoints',
    seq: 10,
    arguments: { source: { path: program }, breakpoints: [{ line }] },
  };
  const pending = {
    type: 'response',
    command: 'setBreakpoints',
    request_seq: 10,
    success: true,
    body: { breakpoints: [{ id: 7, verified: false }] },
  };
  const verified = {
    type: 'event',
    event: 'breakpoint',
    body: { reason: 'changed', breakpoint: { id: 7, verified: true } },
  };
  const stop = {
    type: 'event',
    event: 'stopped',
    body: { reason: 'breakpoint', threadId: 1, hitBreakpointIds: [7] },
  };
  const incoming = (message: object) => ({ direction: 'in', message });
  const outgoing = (message: object) => ({ direction: 'out', message });
  const trace = [incoming(set), outgoing(pending), outgoing(verified), outgoing(stop)];

  it('joins a pending editor request, verification and the first hit', () => {
    assert.deepEqual(requirePackagedBreakpointStop(trace, program, line), {
      breakpoint_id: 7,
      thread_id: 1,
    });
    const installed = { ...pending, body: { breakpoints: [{ id: 7, verified: true }] } };
    assert.deepEqual(
      requirePackagedBreakpointStop([incoming(set), outgoing(installed), outgoing(stop)], program, line),
      { breakpoint_id: 7, thread_id: 1 },
    );
  });

  it('rejects an entry stop even when the correct breakpoint stop follows', () => {
    const entryStop = outgoing({ ...stop, body: { ...stop.body, reason: 'entry' } });
    assert.throws(() => requirePackagedBreakpointStop([entryStop, ...trace], program, line), /first DAP stop/);
  });

  it('rejects a missing stop, unverified breakpoint, or a stop attributed to another ID', () => {
    assert.throws(() => requirePackagedBreakpointStop(trace.slice(0, -1), program, line), /no stopped event/);
    assert.throws(
      () => requirePackagedBreakpointStop([incoming(set), outgoing(pending), outgoing(stop)], program, line),
      /not verified/,
    );
    const wrongHit = outgoing({ ...stop, body: { ...stop.body, hitBreakpointIds: [42] } });
    assert.throws(() => requirePackagedBreakpointStop([...trace.slice(0, -1), wrongHit], program, line), /wrong breakpoint hit identity/);
  });

  it('rejects the wrong source, line, request sequence, and a superseding remove', () => {
    const otherSource = path.resolve('other', path.basename(program));
    assert.throws(() => requirePackagedBreakpointStop(trace, otherSource, line), /not verified/);
    assert.throws(() => requirePackagedBreakpointStop(trace, program, line + 1), /not verified/);
    const wrongResponse = outgoing({ ...pending, request_seq: 11 });
    assert.throws(() => requirePackagedBreakpointStop([incoming(set), wrongResponse, ...trace.slice(2)], program, line), /not verified/);
    const remove = incoming({ ...set, seq: 11, arguments: { ...set.arguments, breakpoints: [] } });
    assert.throws(() => requirePackagedBreakpointStop([...trace.slice(0, -1), remove, outgoing(stop)], program, line), /not verified/);
  });
});
