import type * as vscode from 'vscode';

// Installed-journey evidence only. The owned URI is held for matching but never serialized.
export interface InstalledDiagnosticTraceEvent {
  step: string;
  version?: number | undefined;
  count?: number | undefined;
  codes?: string[] | undefined;
  outcome?: 'ok' | 'error' | 'cancelled' | 'unchanged';
  active?: boolean;
  visible?: boolean;
  ordinal?: number;
  previous_result_id_present?: boolean;
  raw_kind?: 'full' | 'unchanged' | 'null' | 'other';
  error_code?: number | undefined;
}

export interface InstalledDiagnosticTraceSnapshot {
  schema_version: 'installed_diagnostic_trace.v1';
  observation_boundary: 'client_notification_sent_raw_pull_response_and_converted_diagnostics';
  events: InstalledDiagnosticTraceEvent[];
  dropped_events: number;
}

const MAX_EVENTS = 64;
let ownedUri: string | undefined;
let events: InstalledDiagnosticTraceEvent[] = [];
let droppedEvents = 0;
let rawRequestOrdinal = 0;

export function startInstalledDiagnosticTrace(uri: string): void {
  ownedUri = uri;
  events = [];
  droppedEvents = 0;
  rawRequestOrdinal = 0;
}

export function stopInstalledDiagnosticTrace(): void {
  ownedUri = undefined;
  events = [];
  droppedEvents = 0;
  rawRequestOrdinal = 0;
}

export function installedDiagnosticTraceSnapshot(): InstalledDiagnosticTraceSnapshot {
  return {
    schema_version: 'installed_diagnostic_trace.v1',
    observation_boundary: 'client_notification_sent_raw_pull_response_and_converted_diagnostics',
    events: events.map((event) => ({ ...event, codes: event.codes && [...event.codes] })),
    dropped_events: droppedEvents,
  };
}

export function nextOwnedRawDiagnosticOrdinal(uri: string): number | undefined {
  if (ownedUri !== uri) return undefined;
  return ++rawRequestOrdinal;
}

export function rawDiagnosticTraceSummary(
  result: unknown,
): Pick<InstalledDiagnosticTraceEvent, 'raw_kind' | 'count' | 'codes'> {
  if (result === null) return { raw_kind: 'null' };
  if (typeof result !== 'object' || result === undefined) return { raw_kind: 'other' };
  const report = result as { kind?: unknown; items?: unknown };
  if (report.kind === 'unchanged') return { raw_kind: 'unchanged' };
  if (report.kind !== 'full' || !Array.isArray(report.items)) return { raw_kind: 'other' };
  return {
    raw_kind: 'full',
    count: report.items.length,
    codes: report.items.slice(0, 16).map((item: unknown) => {
      const code = item && typeof item === 'object' ? (item as { code?: unknown }).code : undefined;
      return typeof code === 'string' && /^PL\d{3}$/.test(code) ? code : 'other';
    }),
  };
}

export function recordInstalledDiagnosticTrace(
  uri: vscode.Uri | string,
  event: InstalledDiagnosticTraceEvent,
): void {
  if (ownedUri === undefined || String(uri) !== ownedUri) return;
  if (events.length >= MAX_EVENTS) {
    droppedEvents++;
    return;
  }
  events.push(event);
}

export function diagnosticTraceCodes(items: readonly vscode.Diagnostic[]): string[] {
  return items.slice(0, 16).map((item) => {
    const raw = typeof item.code === 'object' && item.code !== null ? item.code.value : item.code;
    return typeof raw === 'string' && /^PL\d{3}$/.test(raw) ? raw : 'other';
  });
}
