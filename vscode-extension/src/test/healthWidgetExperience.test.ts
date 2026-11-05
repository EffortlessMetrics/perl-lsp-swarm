import { ThemeColor } from 'vscode';
import type { StatusBarItem } from 'vscode';
import { ClientState, HealthWidget } from '../healthWidget';

function makeWidget() {
  const item = {
    text: '',
    tooltip: '' as string | undefined,
    command: 'perl-lsp.showWorkspaceStatus',
    accessibilityInformation: { label: 'Perl Language Server', role: 'button' },
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
  test('updates accessible recovery guidance instead of retaining the static product name', () => {
    const { item, widget } = makeWidget();

    widget.setWorkspaceLifecycleState('configuration_action_required', {
      detail: 'The configured server path does not exist.',
      action: 'Fix or clear perl-lsp.serverPath.',
      reasonCode: 'configured_server_missing',
    });

    expect(item.accessibilityInformation.label).toMatch(/action required/i);
    expect(item.accessibilityInformation.label).toContain(
      'The configured server path does not exist.',
    );
    expect(item.accessibilityInformation.label).toContain('Fix or clear perl-lsp.serverPath.');
    expect(item.accessibilityInformation.label).not.toContain('configured_server_missing');
    expect(item.accessibilityInformation.role).toBe('button');
    expect(item.command).toBe('perl-lsp.showWorkspaceStatus');
  });

  test.each([
    ['dormant', /not started/i],
    ['starting', /starting/i],
    ['resolving_environment', /resolving.*(environment|configuration)/i],
    ['indexing_active_context', /preparing.*(file|document)/i],
    ['indexing_workspace', /indexing.*workspace/i],
    ['ready', /ready|running/i],
    ['ready_limited', /ready.*limited|limited.*coverage/i],
    ['configuration_action_required', /action required/i],
    ['failed', /failed|stopped/i],
  ] as const)('exposes %s as readable accessible state', (lifecycle, state) => {
    const { item, widget } = makeWidget();

    widget.setWorkspaceLifecycleState(lifecycle, { reasonCode: 'internal_state_transition' });

    expect(item.accessibilityInformation.label).toMatch(state);
    expect(item.accessibilityInformation.label).toContain('Perl Language Server');
    expect(item.accessibilityInformation.label).not.toMatch(/\$\(|internal_state_transition/);
    expect(item.accessibilityInformation.role).toBe('button');
    expect(item.command).toBe('perl-lsp.showWorkspaceStatus');
  });

  test('clears accessible failure and repair text during restart and recovery', () => {
    const { item, widget } = makeWidget();
    widget.setWorkspaceLifecycleState('failed', {
      detail: 'The server exited unexpectedly.',
      action: 'Restart the server or inspect Output.',
      reasonCode: 'unexpected_exit',
    });
    expect(item.accessibilityInformation.label).toContain('The server exited unexpectedly.');
    expect(item.accessibilityInformation.label).toContain('Restart the server or inspect Output.');

    widget.onStateChange(ClientState.Starting);
    expect(item.accessibilityInformation.label).toMatch(/starting/i);
    expect(item.accessibilityInformation.label).not.toMatch(
      /exited|inspect Output|unexpected_exit/,
    );

    widget.onStateChange(ClientState.Running);
    expect(item.accessibilityInformation.label).toMatch(/ready|running/i);
    expect(item.accessibilityInformation.label).not.toMatch(
      /starting|exited|inspect Output|unexpected_exit/,
    );
  });

  test('keeps accessible counts current and distinguishes a bounded scan from an exact count', () => {
    const { item, widget } = makeWidget();
    widget.setFileCount(1);
    widget.setErrorCount(1);
    widget.onStateChange(ClientState.Running);
    expect(item.accessibilityInformation.label).toMatch(/1 file\b/);
    expect(item.accessibilityInformation.label).toMatch(/1 error\b/);
    expect(item.accessibilityInformation.label).not.toContain('at least');

    widget.setFileCount(50_000, true);
    widget.setErrorCount(0);
    expect(item.accessibilityInformation.label).toContain('at least 50000 files');
    expect(item.accessibilityInformation.label).not.toMatch(/50000\+|1 error/);
    expect(item.text).toContain('50000+ files');

    widget.setFileCount(undefined);
    expect(item.accessibilityInformation.label).not.toMatch(/50000|at least/);
  });

  test('exposes current indexing progress and clears it when indexing ends', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    widget.onProgress('scan', { kind: 'begin', title: 'Indexing', message: 'Scanning modules' });
    expect(item.accessibilityInformation.label).toMatch(/indexing.*workspace/i);
    expect(item.accessibilityInformation.label).toContain('Scanning modules');

    widget.onProgress('scan', { kind: 'report', percentage: 52.6 });
    expect(item.text).toContain('53%');
    expect(item.accessibilityInformation.label).toContain('53 percent');
    widget.setFileCount(1, true);
    expect(item.accessibilityInformation.label).toContain('53 percent');
    expect(item.accessibilityInformation.label).toContain('at least 1 file');
    expect(item.accessibilityInformation.label).not.toContain('Scanning modules');
    expect(item.text).toContain('1+ files');
    expect(item.text).toContain('53%');

    widget.onProgress('scan', { kind: 'end' });
    expect(item.accessibilityInformation.label).toMatch(/ready|running/i);
    expect(item.accessibilityInformation.label).not.toMatch(/indexing|Scanning modules|53 percent/);
  });

  test('includes the bounded readiness explanation without internal transport reasons', () => {
    const { item, widget } = makeWidget();

    widget.onIndexReadinessState('ready_limited', 'ResourceLimit { kind: MaxFiles }');

    expect(item.accessibilityInformation.label).toMatch(/ready.*limited|limited.*coverage/i);
    expect(item.accessibilityInformation.label).toContain('Workspace file limit reached');
    expect(item.accessibilityInformation.label).not.toMatch(
      /ResourceLimit|MaxFiles|index_ready_limited/,
    );
  });

  test('does not reclassify accessible workspace health after a provider outcome and later render', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    widget.setProviderOutcome('safe_refusal', {
      detail: 'Rename was refused.',
      action: 'Review the rename refusal.',
      reasonCode: 'rename_refused',
    });

    // setProviderOutcome itself does not render; telemetry forces the next render.
    widget.setFileCount(4);

    expect(widget.providerOutcome).toBe('safe_refusal');
    expect(item.accessibilityInformation.label).toMatch(/ready|running/i);
    expect(item.accessibilityInformation.label).toContain('4 files');
    expect(item.accessibilityInformation.label).not.toMatch(/Rename|rename|refused|refusal/);
  });

  test('keeps observed identity and guidance readable without codicon markup', () => {
    const { item, widget } = makeWidget();
    widget.setName('$(beaker) custom-perl-server');
    widget.setVersion('0.18.0');
    widget.onStateChange(ClientState.Running);
    expect(item.accessibilityInformation.label).toContain('custom-perl-server');
    expect(item.accessibilityInformation.label).toContain('0.18.0');
    expect(item.accessibilityInformation.label).not.toContain('$(');

    widget.setWorkspaceLifecycleState('configuration_action_required', {
      detail: '$(warning) Workspace trust is required.',
      action: '$(gear) Review Workspace Trust.',
    });
    expect(item.accessibilityInformation.label).toContain('Workspace trust is required.');
    expect(item.accessibilityInformation.label).toContain('Review Workspace Trust.');
    expect(item.accessibilityInformation.label).not.toContain('$(');
  });

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

  test('records bounded fallback without changing workspace status', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    const readyText = item.text;
    const readyTooltip = item.tooltip;

    widget.setProviderOutcome('bounded_fallback', {
      detail: 'Using a bounded text fallback.',
      reasonCode: 'compiler_fact_unavailable',
    });

    expect(widget.lifecycleState).toBe('ready');
    expect(widget.providerOutcome).toBe('bounded_fallback');
    expect(widget.providerDetail).toBe('Using a bounded text fallback.');
    expect(widget.providerReasonCode).toBe('compiler_fact_unavailable');
    expect(widget.mode).toBe('running');
    expect(item.text).toBe(readyText);
    expect(item.tooltip).toBe(readyTooltip);
    expect(item.text).not.toContain('ready (limited)');
    expect(item.backgroundColor).toBeUndefined();
  });

  test('records legitimate empty results without changing workspace status', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    const readyText = item.text;

    widget.setProviderOutcome('legitimate_empty');

    expect(widget.providerOutcome).toBe('legitimate_empty');
    expect(widget.mode).toBe('running');
    expect(item.text).toBe(readyText);
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

  test('records provider failure without reporting the workspace as stopped', () => {
    const { item, widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    const readyText = item.text;

    widget.setProviderOutcome('product_or_instrument_error', {
      detail: 'Provider evidence could not be decoded.',
      action: 'Inspect the provider output.',
      reasonCode: 'provider_receipt_decode_failed',
    });

    expect(widget.providerOutcome).toBe('product_or_instrument_error');
    expect(widget.providerDetail).toBe('Provider evidence could not be decoded.');
    expect(widget.providerAction).toBe('Inspect the provider output.');
    expect(widget.providerReasonCode).toBe('provider_receipt_decode_failed');
    expect(widget.mode).toBe('running');
    expect(item.text).toBe(readyText);
    expect(item.backgroundColor).toBeUndefined();
  });

  test('clears stale provider outcomes when a new startup begins', () => {
    const { widget } = makeWidget();
    widget.onStateChange(ClientState.Running);
    widget.setProviderOutcome('safe_refusal', {
      detail: 'Rename was refused.',
      action: 'Review the refusal.',
      reasonCode: 'rename_refused',
    });

    widget.onStateChange(ClientState.Starting);

    expect(widget.lifecycleState).toBe('starting');
    expect(widget.providerOutcome).toBeUndefined();
    expect(widget.providerDetail).toBeUndefined();
    expect(widget.providerAction).toBeUndefined();
    expect(widget.providerReasonCode).toBeUndefined();
  });

  test('clears operation state immediately when readiness changes during active progress', () => {
    const { item, widget } = makeWidget();
    widget.onProgress('token-1', { kind: 'begin', title: 'Indexing workspace' });
    widget.setProviderOutcome('bounded_fallback', {
      detail: 'Prior readiness generation used a fallback.',
      action: 'Wait for current readiness.',
      reasonCode: 'prior_generation_fallback',
    });
    const indexingText = item.text;

    widget.onIndexReadinessState('ready_limited', 'ScanTimeout { elapsed_ms: 30000 }');

    expect(widget.readinessState).toBe('ready_limited');
    expect(widget.providerOutcome).toBeUndefined();
    expect(widget.providerDetail).toBeUndefined();
    expect(widget.providerAction).toBeUndefined();
    expect(widget.providerReasonCode).toBeUndefined();
    expect(widget.mode).toBe('indexing');
    expect(item.text).toBe(indexingText);

    widget.onProgress('token-1', { kind: 'end' });
    expect(widget.mode).toBe('running');
    expect(item.text).toContain('ready (limited)');
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
