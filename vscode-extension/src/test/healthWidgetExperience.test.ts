import { ThemeColor } from 'vscode';
import type { StatusBarItem } from 'vscode';
import { ClientState, HealthWidget } from '../healthWidget';

function makeWidget() {
  const item = {
    text: '',
    tooltip: '' as string | undefined,
    command: '',
    backgroundColor: undefined as ThemeColor | undefined,
    show: jest.fn(),
    hide: jest.fn(),
    dispose: jest.fn(),
  };
  return {
    item,
    widget: new HealthWidget(item as unknown as StatusBarItem),
  };
}

describe('HealthWidget workspace experience adapter', () => {
  test('distinguishes environment resolution from generic startup', () => {
    const { item, widget } = makeWidget();

    widget.setWorkspaceLifecycleState('resolving_environment');

    expect(widget.lifecycleState).toBe('resolving_environment');
    expect(widget.mode).toBe('starting');
    expect(item.text).toContain('resolving environment');
  });

  test('distinguishes active-document readiness from workspace indexing', () => {
    const { item, widget } = makeWidget();

    widget.setWorkspaceLifecycleState('indexing_active_context');

    expect(widget.lifecycleState).toBe('indexing_active_context');
    expect(widget.mode).toBe('indexing');
    expect(item.text).toContain('preparing active file');
  });

  test('shows bounded fallback as ready-limited without an error background', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);

    widget.setProviderOutcome('bounded_fallback', {
      detail: 'Using a bounded text fallback.',
      reasonCode: 'compiler_fact_unavailable',
    });

    expect(widget.lifecycleState).toBe('ready');
    expect(widget.providerOutcome).toBe('bounded_fallback');
    expect(widget.mode).toBe('running');
    expect(item.text).toContain('ready (limited)');
    expect(item.tooltip).toContain('compiler_fact_unavailable');
    expect(item.backgroundColor).toBeUndefined();
  });

  test('keeps legitimate empty results in the healthy ready state', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);

    widget.setProviderOutcome('legitimate_empty');

    expect(widget.mode).toBe('running');
    expect(item.text).toBe('$(check) perl-lsp');
  });

  test('shows an actionable configuration state with warning emphasis', () => {
    const { item, widget } = makeWidget();

    widget.setWorkspaceLifecycleState('configuration_action_required', {
      detail: 'The configured server path does not exist.',
      action: 'Fix or clear perl-lsp.serverPath.',
    });

    expect(widget.mode).toBe('stopped');
    expect(item.text).toContain('action required');
    expect(item.tooltip).toContain('Fix or clear perl-lsp.serverPath.');
    expect(item.backgroundColor).toBeInstanceOf(ThemeColor);
    expect(item.backgroundColor?.id).toBe('statusBarItem.warningBackground');
  });

  test('shows provider instrument failure as failure rather than limited success', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);

    widget.setProviderOutcome('product_or_instrument_error', {
      detail: 'Provider evidence could not be decoded.',
    });

    expect(widget.mode).toBe('stopped');
    expect(item.text).toContain('failed');
    expect(item.backgroundColor?.id).toBe('statusBarItem.errorBackground');
  });

  test('clears stale provider outcomes when a new startup begins', () => {
    const { widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    widget.setProviderOutcome('safe_refusal');

    widget.onStateChange(ClientState.Starting);

    expect(widget.lifecycleState).toBe('starting');
    expect(widget.providerOutcome).toBeUndefined();
  });

  test('running projection must not clobber active workspace indexing', () => {
    const { item, widget } = makeWidget();
    widget.onProgress('token-1', { kind: 'begin', title: 'Indexing workspace' });
    widget.onStateChange(ClientState.Running);

    expect(widget.mode).toBe('indexing');
    expect(item.text).toContain('Indexing');

    // Regression guard: extension wiring must not unconditionally project `ready`
    // after onStateChange when progress tokens remain active.
    widget.setWorkspaceLifecycleState('ready');
    expect(widget.mode).toBe('running');
    expect(item.text).not.toContain('Indexing');
  });

  test('stopped detail must not be wiped by a bare lifecycle projection', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Stopped);

    expect(item.tooltip).toContain('client_stopped');
    expect(item.tooltip).toContain('reconnect');

    // Regression guard: extension wiring must not project starting/ready after
    // onStateChange(Stopped) without carrying the WorkspaceExperienceUpdate.
    widget.setWorkspaceLifecycleState('starting');
    expect(item.tooltip).not.toContain('client_stopped');
  });
});
