import { presentWorkspaceExperience } from '../workspaceExperienceState';

describe('workspace experience presentation', () => {
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

  test('retains workspace indexing telemetry without changing semantic ownership', () => {
    const presentation = presentWorkspaceExperience(
      { lifecycle: 'indexing_workspace' },
      { fileCount: 200, indexingPercentage: 52.6 },
    );

    expect(presentation.text).toBe('$(sync~spin) perl-lsp: Indexing… (200 files) 53%');
  });
});
