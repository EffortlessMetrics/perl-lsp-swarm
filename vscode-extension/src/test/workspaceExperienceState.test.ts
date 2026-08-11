import {
  presentIndexReadinessReason,
  presentWorkspaceExperience,
  projectWorkspaceLifecycle,
  type WorkspaceExperienceSnapshot,
} from '../workspaceExperienceState';

describe('workspace experience presentation', () => {
  test.each([
    ['ParseStorm { pending_parses: 4 }', 'Frequent changes limited workspace coverage'],
    ['IoError { message: "permission denied" }', 'Some workspace files could not be read'],
    ['ScanTimeout { elapsed_ms: 30000 }', 'Workspace indexing reached its time budget'],
    ['ResourceLimit { kind: MaxFiles }', 'Workspace file limit reached'],
    ['ResourceLimit { kind: MaxSymbols }', 'Workspace symbol limit reached'],
    ['ResourceLimit { kind: MaxCacheBytes }', 'Workspace cache limit reached'],
    ['Cancelled', 'Workspace indexing was cancelled'],
  ])('maps known readiness reason %s to bounded text', (reason, expected) => {
    expect(presentIndexReadinessReason(reason)).toBe(expected);
  });

  test('maps unknown readiness reasons to a generic bounded label', () => {
    expect(presentIndexReadinessReason('FutureReason { private: "value" }')).toBe(
      'Limited workspace coverage',
    );
  });

  test('does not classify known tokens nested inside unknown reasons', () => {
    expect(presentIndexReadinessReason('FutureReason { detail: MaxFiles, note: ParseStorm }')).toBe(
      'Limited workspace coverage',
    );
    expect(presentIndexReadinessReason('IoError { message: "ParseStorm" }')).toBe(
      'Some workspace files could not be read',
    );
  });

  test('preserves environment resolution as a distinct user-facing state', () => {
    expect(projectWorkspaceLifecycle('resolving')).toBe('resolving_environment');
    expect(projectWorkspaceLifecycle('starting')).toBe('starting');
    expect(projectWorkspaceLifecycle('running')).toBe('ready');
    expect(projectWorkspaceLifecycle('failed')).toBe('failed');
  });

  test('keeps operation outcomes outside the workspace snapshot contract', () => {
    const snapshot: WorkspaceExperienceSnapshot = { lifecycle: 'ready' };
    // @ts-expect-error Provider outcomes describe one operation, not workspace health.
    snapshot.providerOutcome = 'not_ready';
    expect(snapshot.lifecycle).toBe('ready');
  });

  test('renders a healthy ready state without operation-scoped result data', () => {
    const presentation = presentWorkspaceExperience(
      { lifecycle: 'ready' },
      { version: '0.18.0', fileCount: 12, errorCount: 0 },
    );

    expect(presentation.mode).toBe('running');
    expect(presentation.text).toBe('$(check) perl-lsp v0.18.0: 12 files');
    expect(presentation.background).toBeUndefined();
  });

  test('shows active-document readiness only when the lifecycle authority supplies it', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'indexing_active_context',
      reasonCode: 'active_document_pending',
    });

    expect(presentation.mode).toBe('indexing');
    expect(presentation.text).toContain('preparing active file');
    expect(presentation.tooltip).toContain('active_document_pending');
  });

  test('shows ready-limited only when the readiness authority supplies it', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'ready_limited',
      detail: 'Some workspace files could not be read.',
      reasonCode: 'index_ready_limited',
    });

    expect(presentation.mode).toBe('running');
    expect(presentation.text).toContain('ready (limited)');
    expect(presentation.tooltip).toContain('Some workspace files could not be read.');
  });

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

  test('reports failure only when the lifecycle authority supplies failure', () => {
    const presentation = presentWorkspaceExperience({
      lifecycle: 'failed',
      detail: 'The provider transport failed and stopped the server.',
    });

    expect(presentation.mode).toBe('stopped');
    expect(presentation.text).toContain('failed');
    expect(presentation.background).toBe('error');
  });

  test('tells a stopped user how to recover instead of offering generic options', () => {
    const presentation = presentWorkspaceExperience({ lifecycle: 'failed' });

    expect(presentation.mode).toBe('stopped');
    expect(presentation.background).toBe('error');
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
