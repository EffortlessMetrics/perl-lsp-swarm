import { presentWorkspaceExperience, projectWorkspaceLifecycle } from '../workspaceExperienceState';

describe('workspace experience presentation', () => {
  test('preserves environment resolution as a distinct user-facing state', () => {
    expect(projectWorkspaceLifecycle('resolving')).toBe('resolving_environment');
    expect(projectWorkspaceLifecycle('starting')).toBe('starting');
    expect(projectWorkspaceLifecycle('running')).toBe('ready');
    expect(projectWorkspaceLifecycle('failed')).toBe('failed');
  });

  test('keeps exact current answers quiet in the ready state', () => {
    const presentation = presentWorkspaceExperience(
      { lifecycle: 'ready', providerOutcome: 'exact_current' },
      { version: '0.18.0', fileCount: 12, errorCount: 0 },
    );

    expect(presentation.mode).toBe('running');
    expect(presentation.text).toBe('$(check) perl-lsp v0.18.0: 12 files');
    expect(presentation.background).toBeUndefined();
  });

  test('does not misclassify a legitimate empty result as not ready', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'ready',
      providerOutcome: 'legitimate_empty',
    });

    expect(presentation.mode).toBe('running');
    expect(presentation.text).toBe('$(check) perl-lsp');
  });

  test('does not flatten not-ready into a successful empty state', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'ready',
      providerOutcome: 'not_ready',
      reasonCode: 'active_document_pending',
    });

    expect(presentation.mode).toBe('indexing');
    expect(presentation.text).toContain('preparing active file');
    expect(presentation.tooltip).toContain('active_document_pending');
  });

  test.each(['bounded_fallback', 'unsupported_or_dynamic', 'safe_refusal'] as const)(
    'presents %s as ready but limited',
    (providerOutcome) => {
      const presentation = presentWorkspaceExperience({
        lifecycle: 'ready',
        providerOutcome,
        detail: 'Exact source-backed evidence is unavailable.',
      });

      expect(presentation.mode).toBe('running');
      expect(presentation.text).toContain('ready (limited)');
      expect(presentation.tooltip).toContain('Exact source-backed evidence is unavailable.');
    },
  );

  test('surfaces configuration action without reporting a product crash', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'configuration_action_required',
      detail: 'Workspace trust is required before starting Perl.',
      action: 'Trust the workspace or keep Perl execution disabled.',
    });

    expect(presentation.text).toContain('action required');
    expect(presentation.background).toBe('warning');
    expect(presentation.tooltip).toContain('Trust the workspace');
  });

  test('promotes instrument errors to a failed state', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'ready',
      providerOutcome: 'product_or_instrument_error',
      detail: 'The provider receipt could not be read.',
    });

    expect(presentation.mode).toBe('stopped');
    expect(presentation.text).toContain('failed');
    expect(presentation.background).toBe('error');
  });

  test('tells a stopped user how to recover instead of offering generic options', () => {
    const presentation = presentWorkspaceExperience({ lifecycle: 'failed' });

    expect(presentation.mode).toBe('stopped');
    expect(presentation.background).toBe('error');
    // The status-bar item opens the status menu, where Restart Server is one
    // explicit action. The tooltip must not claim that the click invokes a
    // direct restart.
    expect(presentation.tooltip).toBe(
      'Perl Language Server has stopped; choose Restart Server from the status menu (click for restart options)',
    );
  });

  test('keeps the restart affordance when a failure carries diagnostic detail', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'failed',
      detail: 'The server exited during startup.',
      reasonCode: 'startup_exit',
    });

    expect(presentation.tooltip).toContain('The server exited during startup.');
    expect(presentation.tooltip).toContain('Reason: startup_exit');
    expect(presentation.tooltip.endsWith('(click for restart options)')).toBe(true);
  });

  test('leaves non-failure states on the generic options affordance', () => {
    const ready = presentWorkspaceExperience({ lifecycle: 'ready' });
    const actionRequired = presentWorkspaceExperience({
      lifecycle: 'configuration_action_required',
    });

    expect(ready.tooltip.endsWith('(click for options)')).toBe(true);
    expect(actionRequired.tooltip.endsWith('(click for options)')).toBe(true);
  });

  test('retains workspace indexing telemetry without changing semantic ownership', () => {
    const presentation = presentWorkspaceExperience(
      { lifecycle: 'indexing_workspace' },
      { fileCount: 200, indexingPercentage: 52.6 },
    );

    expect(presentation.text).toBe('$(sync~spin) perl-lsp: Indexing… (200 files) 53%');
  });
});
