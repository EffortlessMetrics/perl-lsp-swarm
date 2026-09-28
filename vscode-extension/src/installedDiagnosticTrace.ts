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
}

export interface InstalledDiagnosticTraceSnapshot {
  schema_version: 'installed_diagnostic_trace.v1';
  observation_boundary: 'client_notification_sent_and_converted_diagnostics_no_raw_response';
  events: InstalledDiagnosticTraceEvent[];
  dropped_events: number;
}

const MAX_EVENTS = 64;
let ownedUri: string | undefined;
let events: InstalledDiagnosticTraceEvent[] = [];
let droppedEvents = 0;

export function startInstalledDiagnosticTrace(uri: string): void {
  ownedUri = uri;
  events = [];
  droppedEvents = 0;
}

export function stopInstalledDiagnosticTrace(): void {
  ownedUri = undefined;
  events = [];
  droppedEvents = 0;
}

export function installedDiagnosticTraceSnapshot(): InstalledDiagnosticTraceSnapshot {
  return {
    schema_version: 'installed_diagnostic_trace.v1',
    observation_boundary: 'client_notification_sent_and_converted_diagnostics_no_raw_response',
    events: events.map((event) => ({ ...event, codes: event.codes && [...event.codes] })),
    dropped_events: droppedEvents,
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
