/**
 * #16804 — one typed owner for managed update outcomes and rendering.
 * This slice covers admission refusals and manual check terminal results.
 */
import {
  composeCheckSkippedOutcome,
  composeUpToDateOutcome,
  composeUpdatedReloadRequiredOutcome,
  composeUpdatedReloadUnverifiedOutcome,
  ManagedUpdateOutcomeDedupe,
  projectManagedUpdateOutcome,
  type ManagedUpdatePresentation,
} from '../managedUpdateOutcome';
import { decideManagedUpdateCheck } from '../managedUpdateCheckAdmission';

const HOST_KEY = 'linux-x86_64-gnu';
const SCOPED_KEY = `perl-lsp.lastUpdateCheck.${HOST_KEY}`;
const NOW = 1_700_000_000_000;

function baseFacts(overrides: Record<string, unknown> = {}) {
  return {
    intent: 'activation_background' as const,
    channel: 'latest',
    versionTag: '',
    downloadBaseUrlConfigured: false,
    updateCheckIntervalHours: 24,
    autoUpdate: false,
    promptSuppressed: false,
    configuredServerPath: '',
    hostCompatibilityProven: true,
    managedBinaryPath: true,
    localBinaryPresent: true,
    scopedStateKey: SCOPED_KEY,
    lastCheckMs: 0,
    nowMs: NOW,
    ...overrides,
  };
}

function presentation(
  outcome: ReturnType<typeof composeCheckSkippedOutcome>,
): ManagedUpdatePresentation {
  return projectManagedUpdateOutcome(outcome);
}

describe('managedUpdateOutcome admission refusals (#16804)', () => {
  test('background pinned tag stays output-only with stable dedup identity', () => {
    const decision = decideManagedUpdateCheck(baseFacts({ channel: 'tag', versionTag: 'v0.12.0' }));
    expect(decision.kind).not.toBe('run');
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = presentation(outcome);

    expect(view.notification.visibility).toBe('none');
    expect(view.outputChannel.line).toContain('pinned_exact_tag');
    expect(view.outputChannel.line).toContain('v0.12.0');
    expect(view.dedupIdentity).toBe(
      'check_skipped:skip:pinned_exact_tag:perl-lsp.channel is pinned to the exact tag v0.12.0; automatic update checks are disabled by that pin.',
    );
  });

  test('manual pinned tag is information, not mistaken for up to date', () => {
    const decision = decideManagedUpdateCheck(
      baseFacts({ intent: 'manual_force', channel: 'tag', versionTag: 'v0.12.0' }),
    );
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = projectManagedUpdateOutcome(outcome);

    expect(view.notification.visibility).toBe('explicit-command');
    expect(view.notification.severity).toBe('information');
    expect(view.notification.summary).toContain('Binary update check skipped');
    expect(view.notification.summary).toContain('v0.12.0');
    expect(view.notification.summary).not.toMatch(/up to date/i);
  });

  test('manual configured serverPath uses the dedicated copy (#16530/#16804)', () => {
    const decision = decideManagedUpdateCheck(
      baseFacts({ intent: 'manual_force', configuredServerPath: '/custom/perllsp' }),
    );
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = projectManagedUpdateOutcome(outcome);

    expect(view.notification.summary).toBe(
      'Binary update checks are unavailable while perl-lsp.serverPath is configured.',
    );
  });

  test('manual missing managed binary warns with reinstall guidance', () => {
    const decision = decideManagedUpdateCheck(
      baseFacts({ intent: 'manual_force', localBinaryPresent: false }),
    );
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = projectManagedUpdateOutcome(outcome);

    expect(view.notification.severity).toBe('warning');
    expect(view.notification.summary).toContain('missing');
    expect(view.notification.summary).toContain('Reinstall');
  });

  test('manual invalid channel reports remediation, not a generic skip', () => {
    const decision = decideManagedUpdateCheck(
      baseFacts({ intent: 'manual_force', channel: 'nightly' }),
    );
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = projectManagedUpdateOutcome(outcome);

    expect(view.notification.severity).toBe('warning');
    expect(view.notification.summary).toContain('Binary update check skipped');
    expect(view.notification.summary).toContain('stable');
  });
});

describe('managedUpdateOutcome manual terminal results (#16804)', () => {
  test('manual up to date is always visible to the user', () => {
    const view = projectManagedUpdateOutcome(composeUpToDateOutcome('0.12.0', true));

    expect(view.notification.visibility).toBe('explicit-command');
    expect(view.notification.summary).toBe('You are up to date (0.12.0).');
    expect(view.dedupIdentity).toBe('up_to_date:0.12.0');
  });

  test('successful staged update offers reload only', () => {
    const view = projectManagedUpdateOutcome(composeUpdatedReloadRequiredOutcome('0.13.0'));

    expect(view.notification.visibility).toBe('explicit-command');
    expect(view.notification.summary).toContain('0.13.0');
    expect(view.notification.summary).toContain('Reload');
    expect(view.actions).toEqual(['Reload Window']);
  });

  test('unverified staged install warns without claiming success', () => {
    const installedPath = '/tmp/perllsp/current/perllsp';
    const view = projectManagedUpdateOutcome(composeUpdatedReloadUnverifiedOutcome(installedPath));

    expect(view.notification.severity).toBe('warning');
    expect(view.notification.summary).toContain('could not be verified');
    expect(view.outputChannel.line).toContain(installedPath);
    expect(view.actions).toEqual(['View Logs']);
  });
});

describe('ManagedUpdateOutcomeDedupe', () => {
  test('suppresses repeated identical background refusals in one session', () => {
    const dedupe = new ManagedUpdateOutcomeDedupe();
    const decision = decideManagedUpdateCheck(baseFacts({ channel: 'tag', versionTag: 'v0.12.0' }));
    const outcome = composeCheckSkippedOutcome(
      decision as Exclude<typeof decision, { kind: 'run' }>,
    );
    const view = projectManagedUpdateOutcome(outcome);

    expect(dedupe.shouldEmitOutput(view)).toBe(true);
    expect(dedupe.shouldEmitOutput(view)).toBe(false);
    dedupe.clear();
    expect(dedupe.shouldEmitOutput(view)).toBe(true);
  });

  test('never dedupes explicit-command presentations', () => {
    const dedupe = new ManagedUpdateOutcomeDedupe();
    const view = projectManagedUpdateOutcome(composeUpToDateOutcome('1.0.0', true));

    expect(dedupe.shouldEmitOutput(view)).toBe(true);
    expect(dedupe.shouldEmitOutput(view)).toBe(true);
  });

  test('background refusals with same reason but different explanation dedupe separately', () => {
    const dedupe = new ManagedUpdateOutcomeDedupe();
    const decisionV12 = decideManagedUpdateCheck(
      baseFacts({ channel: 'tag', versionTag: 'v0.12.0' }),
    );
    const viewV12 = projectManagedUpdateOutcome(
      composeCheckSkippedOutcome(decisionV12 as Exclude<typeof decisionV12, { kind: 'run' }>),
    );
    const decisionV13 = decideManagedUpdateCheck(
      baseFacts({ channel: 'tag', versionTag: 'v0.13.0' }),
    );
    const viewV13 = projectManagedUpdateOutcome(
      composeCheckSkippedOutcome(decisionV13 as Exclude<typeof decisionV13, { kind: 'run' }>),
    );

    expect(viewV12.dedupIdentity).not.toBe(viewV13.dedupIdentity);
    expect(dedupe.shouldEmitOutput(viewV12)).toBe(true);
    expect(dedupe.shouldEmitOutput(viewV12)).toBe(false);
    expect(dedupe.shouldEmitOutput(viewV13)).toBe(true);
  });
});
