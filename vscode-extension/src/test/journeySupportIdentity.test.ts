import * as assert from 'assert';
import { isLinuxProcessGoneError, parseLinuxProcessStat } from './published/journeySupport';

function statWithStart(start: string): string {
  return `123 (perl (dap)) ${['S', ...Array(18).fill('0'), start].join(' ')}`;
}

describe('Linux packaged-process identity parsing', () => {
  it('uses the final closing parenthesis and binds boot identity to start ticks', () => {
    assert.deepEqual(parseLinuxProcessStat(statWithStart('42'), 123, 'boot-id-1'), {
      pid: 123,
      creationIdentity: 'boot-id-1:42',
    });
  });

  it('rejects malformed stat and boot identities', () => {
    assert.throws(() => parseLinuxProcessStat('123 (broken', 123, 'boot'));
    assert.throws(() => parseLinuxProcessStat(statWithStart('not-a-number'), 123, 'boot'));
    assert.throws(() => parseLinuxProcessStat(statWithStart('42'), 123, ''));
  });

  it('distinguishes a replacement PID by its start ticks', () => {
    const first = parseLinuxProcessStat(statWithStart('42'), 123, 'boot');
    const replacement = parseLinuxProcessStat(statWithStart('43'), 123, 'boot');
    assert.notEqual(first.creationIdentity, replacement.creationIdentity);
  });

  it('rejects a different PID and retains an unreaped zombie identity', () => {
    assert.throws(() => parseLinuxProcessStat(statWithStart('42'), 124, 'boot'));
    const identity = parseLinuxProcessStat(
      statWithStart('42').replace(') S ', ') Z '),
      123,
      'boot',
    );
    assert.equal(identity.creationIdentity, 'boot:42');
  });

  it('only classifies confirmed process absence as gone', () => {
    assert.equal(isLinuxProcessGoneError({ code: 'ENOENT' }), true);
    assert.equal(isLinuxProcessGoneError({ code: 'ESRCH' }), true);
    assert.equal(isLinuxProcessGoneError({ code: 'EACCES' }), false);
    assert.equal(isLinuxProcessGoneError(new Error('malformed stat')), false);
    assert.equal(isLinuxProcessGoneError(null), false);
    assert.equal(isLinuxProcessGoneError(undefined), false);
    assert.equal(isLinuxProcessGoneError('ENOENT'), false);
    assert.equal(isLinuxProcessGoneError(13), false);
  });
});
