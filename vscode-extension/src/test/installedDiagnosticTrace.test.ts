import type * as vscode from 'vscode';
import {
  diagnosticTraceCodes,
  installedDiagnosticTraceSnapshot,
  recordInstalledDiagnosticTrace,
  startInstalledDiagnosticTrace,
  stopInstalledDiagnosticTrace,
} from '../installedDiagnosticTrace';

describe('installed diagnostic trace', () => {
  afterEach(stopInstalledDiagnosticTrace);

  test('keeps only bounded owned-document protocol summaries', () => {
    const owned = 'file:///private/owned.pl';
    startInstalledDiagnosticTrace(owned);
    recordInstalledDiagnosticTrace('file:///private/other.pl', {
      step: 'didChange_client_processed',
      version: 7,
    });
    for (let version = 1; version <= 66; version++) {
      recordInstalledDiagnosticTrace(owned, { step: 'didChange_client_processed', version });
    }
    const snapshot = installedDiagnosticTraceSnapshot();
    expect(snapshot.events).toHaveLength(64);
    expect(snapshot.dropped_events).toBe(2);
    expect(snapshot.events[0]).toEqual({
      step: 'didChange_client_processed',
      version: 1,
      codes: undefined,
    });
    expect(JSON.stringify(snapshot)).not.toContain('/private/');
  });

  test('records known codes without messages or arbitrary diagnostic codes', () => {
    const diagnostics = [
      { code: { value: 'PL100', target: { toString: () => 'file:///secret' } }, message: 'secret' },
      { code: 'private-code', message: 'secret' },
    ] as unknown as vscode.Diagnostic[];
    expect(diagnosticTraceCodes(diagnostics)).toEqual(['PL100', 'other']);
  });
});
