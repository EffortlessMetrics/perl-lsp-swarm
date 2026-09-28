import type * as vscode from 'vscode';
import {
  diagnosticTraceCodes,
  installedDiagnosticTraceSnapshot,
  nextOwnedRawDiagnosticOrdinal,
  rawDiagnosticTraceSummary,
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

  test('summarizes raw reports without source text, paths, or arbitrary codes', () => {
    expect(rawDiagnosticTraceSummary(null)).toEqual({ raw_kind: 'null' });
    expect(rawDiagnosticTraceSummary({ kind: 'full', items: [] })).toEqual({
      raw_kind: 'full',
      count: 0,
      codes: [],
    });
    const raw = {
      kind: 'full',
      items: [
        { code: 'PL100', message: 'secret', data: { uri: 'file:///private/owned.pl' } },
        { code: 'private-value', message: 'secret' },
      ],
    };
    expect(rawDiagnosticTraceSummary(raw)).toEqual({
      raw_kind: 'full',
      count: 2,
      codes: ['PL100', 'other'],
    });
    expect(JSON.stringify(rawDiagnosticTraceSummary(raw))).not.toContain('secret');
    expect(JSON.stringify(rawDiagnosticTraceSummary(raw))).not.toContain('/private/');
    startInstalledDiagnosticTrace('file:///private/owned.pl');
    expect(nextOwnedRawDiagnosticOrdinal('file:///private/foreign.pl')).toBeUndefined();
    expect(nextOwnedRawDiagnosticOrdinal('file:///private/owned.pl')).toBe(1);
    expect(nextOwnedRawDiagnosticOrdinal('file:///private/owned.pl')).toBe(2);
  });
});
