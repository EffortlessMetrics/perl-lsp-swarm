/**
 * `decideManagedUpdateCheck` is the single owner of managed update-check
 * admission (#16803).
 *
 * These tests are pure: no `vscode`, no clock, no filesystem. Each one is
 * written to *discriminate* — a wrong implementation that kept the old
 * branch chain, or one that collapsed two distinct dispositions into one,
 * fails a named test rather than passing incidentally.
 */

import {
  decideManagedUpdateCheck,
  type ManagedUpdateCheckFacts,
} from '../managedUpdateCheckAdmission';

const HOUR = 1000 * 60 * 60;
const NOW = 1_800_000_000_000;
const STATE_KEY = 'perl-lsp.lastUpdateCheck.x86_64-unknown-linux-gnu';

/** A healthy, due, background check. Every test perturbs one fact. */
function facts(overrides: Partial<ManagedUpdateCheckFacts> = {}): ManagedUpdateCheckFacts {
  return {
    intent: 'activation_background',
    channel: 'latest',
    versionTag: '',
    updateCheckIntervalHours: 24,
    autoUpdate: false,
    promptSuppressed: false,
    configuredServerPath: '',
    hostCompatibilityProven: true,
    managedBinaryPath: true,
    localBinaryPresent: true,
    scopedStateKey: STATE_KEY,
    lastCheckMs: NOW - 48 * HOUR,
    nowMs: NOW,
    ...overrides,
  };
}

describe('managed update-check admission', () => {
  describe('the healthy path runs', () => {
    test('a due background check runs and owns the scoped cadence key', () => {
      const decision = decideManagedUpdateCheck(facts());

      expect(decision).toEqual({
        kind: 'run',
        reason: 'scheduled_due',
        action: 'prompt',
        stateKey: STATE_KEY,
      });
    });

    test('the cadence key is the compatibility-scoped one, never the legacy row (#9847)', () => {
      const decision = decideManagedUpdateCheck(facts());

      // Falsifier 9/10: every entrypoint must agree on one state key. A legacy
      // key here would re-establish the unscoped row as a current authority.
      expect(decision.kind === 'run' && decision.stateKey).toBe(STATE_KEY);
      expect(decision.kind === 'run' && decision.stateKey).not.toBe('perl-lsp.lastUpdateCheck');
    });
  });

  describe('manual force bypasses cadence but nothing else', () => {
    test('a fresh timestamp does not suppress a manual check', () => {
      const decision = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', lastCheckMs: NOW - 1_000 }),
      );

      expect(decision).toMatchObject({ kind: 'run', reason: 'manual_forced' });
    });

    test('a zero interval disables scheduled checks but not the manual command', () => {
      const background = decideManagedUpdateCheck(facts({ updateCheckIntervalHours: 0 }));
      const manual = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', updateCheckIntervalHours: 0 }),
      );

      expect(background).toMatchObject({
        kind: 'skip',
        reason: 'background_checking_disabled',
        userVisibility: 'silent',
      });
      expect(manual).toMatchObject({ kind: 'run', reason: 'manual_forced' });
    });

    test('a negative interval is treated as disabled, not as always-due', () => {
      const decision = decideManagedUpdateCheck(facts({ updateCheckIntervalHours: -1 }));

      expect(decision).toMatchObject({ kind: 'skip', reason: 'background_checking_disabled' });
    });

    test('manual force still reports a pinned tag rather than checking', () => {
      const decision = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', channel: 'tag', versionTag: 'v0.12.0' }),
      );

      // Falsifier 5: "no automatic update check" must never read as "up to date".
      expect(decision).toMatchObject({
        kind: 'skip',
        reason: 'pinned_exact_tag',
        userVisibility: 'report',
      });
      expect(decision.kind === 'skip' && decision.detail).toContain('v0.12.0');
    });

    test('manual force still reports an invalid configuration', () => {
      const decision = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', channel: 'not-a-channel' }),
      );

      expect(decision).toMatchObject({ kind: 'invalid', userVisibility: 'report' });
    });

    test('manual force still reports a user-owned binary', () => {
      const decision = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', configuredServerPath: '/opt/perl/bin/perllsp' }),
      );

      expect(decision).toMatchObject({
        kind: 'skip',
        reason: 'explicit_user_binary_selected',
        userVisibility: 'report',
      });
    });
  });

  describe('prompt suppression is not cadence', () => {
    test('a suppressed prompt does not block a background check', () => {
      const decision = decideManagedUpdateCheck(facts({ promptSuppressed: true }));

      // Falsifier 4 (first half): suppressing a prompt must not suppress a check.
      expect(decision).toMatchObject({ kind: 'run', reason: 'scheduled_due' });
    });

    test('a suppressed prompt changes the action, not the admission', () => {
      const decision = decideManagedUpdateCheck(facts({ promptSuppressed: true }));

      expect(decision).toMatchObject({ kind: 'run', action: 'prompt_suppressed' });
    });

    test('a suppressed prompt never masks an explicitly enabled autoUpdate', () => {
      const decision = decideManagedUpdateCheck(
        facts({ promptSuppressed: true, autoUpdate: true }),
      );

      // Falsifier 4 (second half): "Don't ask again" is prompt-scoped only.
      expect(decision).toMatchObject({ kind: 'run', action: 'auto_update' });
    });

    test('manual force is offered the update even when prompts are suppressed', () => {
      const decision = decideManagedUpdateCheck(
        facts({ intent: 'manual_force', promptSuppressed: true }),
      );

      expect(decision).toMatchObject({ kind: 'run', reason: 'manual_forced', action: 'prompt' });
    });
  });

  describe('exact tag pinning and configuration validity', () => {
    test('channel=tag with a tag is a named pin, not silence', () => {
      const decision = decideManagedUpdateCheck(facts({ channel: 'tag', versionTag: 'v0.12.0' }));

      expect(decision).toMatchObject({ kind: 'skip', reason: 'pinned_exact_tag' });
    });

    test('channel=tag without a tag is invalid, not an empty pin', () => {
      const decision = decideManagedUpdateCheck(facts({ channel: 'tag', versionTag: '' }));

      // Falsifier 6's sibling: an unusable pin is repairable, so it must be
      // reported as invalid rather than skipped.
      expect(decision).toMatchObject({ kind: 'invalid', reason: 'missing_or_invalid_tag' });
    });

    test('versionTag under a non-tag channel is a typed invalid, never silently ignored', () => {
      const decision = decideManagedUpdateCheck(
        facts({ channel: 'latest', versionTag: 'v0.12.0' }),
      );

      // Falsifier 6. The pre-#16803 chain never read versionTag unless the
      // channel was already 'tag', so this configuration checked the wrong
      // release while the user believed a tag was pinned.
      expect(decision.kind).toBe('invalid');
      if (decision.kind === 'invalid') {
        expect(decision.reason).toBe('version_tag_ignored_for_channel');
        expect(decision.remediation).toContain('v0.12.0');
      }
    });

    test('a whitespace-only versionTag is still ignored-on-channel, not a pin', () => {
      const decision = decideManagedUpdateCheck(facts({ channel: 'stable', versionTag: '   ' }));

      expect(decision).toMatchObject({ kind: 'run' });
    });

    test('an unknown channel is invalid and never reaches transport', () => {
      const decision = decideManagedUpdateCheck(facts({ channel: 'nightly' }));

      // Falsifier 7: pre-#16803 this only surfaced as a throw from
      // getLatestRelease after the check had already run.
      expect(decision).toMatchObject({ kind: 'invalid', reason: 'unknown_channel' });
    });

    test('a non-numeric interval is invalid rather than silently coerced', () => {
      const decision = decideManagedUpdateCheck(facts({ updateCheckIntervalHours: Number.NaN }));

      expect(decision).toMatchObject({ kind: 'invalid', reason: 'invalid_update_check_interval' });
    });

    test('stable and latest channels are both admissible', () => {
      expect(decideManagedUpdateCheck(facts({ channel: 'stable' }))).toMatchObject({ kind: 'run' });
      expect(decideManagedUpdateCheck(facts({ channel: 'latest' }))).toMatchObject({ kind: 'run' });
    });
  });

  describe('binary ownership', () => {
    test('a user-configured serverPath is a named refusal', () => {
      const decision = decideManagedUpdateCheck(
        facts({ configuredServerPath: '  /opt/perl/bin/perllsp  ' }),
      );

      expect(decision).toMatchObject({ kind: 'skip', reason: 'explicit_user_binary_selected' });
    });

    test('a bundled, extension-owned binary is not update-checkable', () => {
      const decision = decideManagedUpdateCheck(facts({ managedBinaryPath: false }));

      expect(decision).toMatchObject({ kind: 'skip', reason: 'binary_not_managed' });
    });

    test('a missing managed binary points at reinstall', () => {
      const decision = decideManagedUpdateCheck(facts({ localBinaryPresent: false }));

      expect(decision).toMatchObject({ kind: 'skip', reason: 'local_binary_missing' });
      expect(decision.kind === 'skip' && decision.detail).toContain('Reinstall');
    });
  });

  describe('host identity', () => {
    test('an unproven compatibility key is refused before any state is touched', () => {
      const decision = decideManagedUpdateCheck(
        facts({ hostCompatibilityProven: false, scopedStateKey: '' }),
      );

      // Falsifier 8's sibling: the pre-#16803 code fell back to the unscoped
      // legacy row here, letting one host write cadence for another target.
      expect(decision).toMatchObject({ kind: 'skip', reason: 'unsupported_or_unproven_host' });
    });

    test('host identity outranks configuration validity', () => {
      const decision = decideManagedUpdateCheck(
        facts({ hostCompatibilityProven: false, scopedStateKey: '', channel: 'nonsense' }),
      );

      // No state may be attributed to a target this host cannot name, so the
      // host refusal is reported first rather than a config that cannot be
      // acted on without a state namespace.
      expect(decision).toMatchObject({ kind: 'skip', reason: 'unsupported_or_unproven_host' });
    });
  });

  describe('timestamp handling', () => {
    test('an unelapsed interval is its own reason, distinct from a disabled interval', () => {
      const decision = decideManagedUpdateCheck(facts({ lastCheckMs: NOW - 1 * HOUR }));

      expect(decision).toMatchObject({ kind: 'skip', reason: 'interval_not_elapsed' });
    });

    test('a legacy seed that has elapsed admits the check', () => {
      const decision = decideManagedUpdateCheck(facts({ lastCheckMs: 0 }));

      expect(decision).toMatchObject({ kind: 'run', reason: 'scheduled_due' });
    });

    test('a timestamp in the future is not treated as evidence the interval elapsed', () => {
      // A clock that moved backwards must not silently suppress every future
      // check, so an uninterpretable stamp reads as "never checked".
      const decision = decideManagedUpdateCheck(facts({ lastCheckMs: NOW + 10 * HOUR }));

      expect(decision).toMatchObject({ kind: 'run', reason: 'scheduled_due' });
    });

    test('a non-numeric timestamp is not trusted as a recent check', () => {
      const decision = decideManagedUpdateCheck(
        facts({ lastCheckMs: Number.NaN as unknown as number }),
      );

      expect(decision).toMatchObject({ kind: 'run', reason: 'scheduled_due' });
    });
  });

  describe('every refusal is named and zero-effect', () => {
    test('no refusal is silent about why it refused', () => {
      const refusals = [
        facts({ channel: 'tag', versionTag: 'v1' }),
        facts({ configuredServerPath: '/x' }),
        facts({ managedBinaryPath: false }),
        facts({ localBinaryPresent: false }),
        facts({ updateCheckIntervalHours: 0 }),
        facts({ lastCheckMs: NOW }),
        facts({ hostCompatibilityProven: false, scopedStateKey: '' }),
        facts({ channel: 'tag', versionTag: '' }),
        facts({ channel: 'nope' }),
        facts({ channel: 'latest', versionTag: 'v1' }),
      ];

      for (const refusal of refusals) {
        const decision = decideManagedUpdateCheck(refusal);
        if (decision.kind === 'run') {
          throw new Error(`expected a refusal, got run: ${decision.reason}`);
        }
        const explanation = decision.kind === 'skip' ? decision.detail : decision.remediation;
        expect(explanation.length).toBeGreaterThan(0);
        expect(explanation).toMatch(/\S/);
      }
    });

    test('no refusal can carry a cadence key to write', () => {
      const decision = decideManagedUpdateCheck(facts({ updateCheckIntervalHours: 0 }));

      // Falsifier 7: a skip must not be able to mutate managed state, so no
      // refusal exposes a writable stateKey at all.
      expect(decision).not.toHaveProperty('stateKey');
    });
  });
});
