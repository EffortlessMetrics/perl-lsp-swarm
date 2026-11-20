import { describe, expect, test } from '@jest/globals';
import * as fs from 'fs';
import * as path from 'path';
import {
  LIFECYCLE_REPLACEMENT_GATE_MARKER,
  REPLACEMENT_ADMISSION_ROUTES,
  REPLACEMENT_FUNNEL_FUNCTION,
} from '../replacementAdmissionLedger';

const extensionSource = fs.readFileSync(
  path.join(__dirname, '..', '..', 'src', 'extension.ts'),
  'utf8',
);

function countNonCommentOccurrences(source: string, needle: string): number {
  const lines = source.split('\n');
  let count = 0;
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('//') || trimmed.startsWith('*')) {
      continue;
    }
    if (line.includes(needle)) {
      count += 1;
    }
  }
  return count;
}

describe('replacement admission ledger (#14448)', () => {
  test('every ledgered production route remains wired in extension.ts', () => {
    for (const route of REPLACEMENT_ADMISSION_ROUTES) {
      expect(extensionSource).toContain(route.sourceMarker);
    }
    expect(new Set(REPLACEMENT_ADMISSION_ROUTES.map((row) => row.id)).size).toBe(
      REPLACEMENT_ADMISSION_ROUTES.length,
    );
  });

  test('replacement construction uses exactly one lifecycle.restart gate', () => {
    expect(countNonCommentOccurrences(extensionSource, LIFECYCLE_REPLACEMENT_GATE_MARKER)).toBe(1);
    expect(extensionSource).toContain(REPLACEMENT_FUNNEL_FUNCTION);
    const funnelStart = extensionSource.indexOf(REPLACEMENT_FUNNEL_FUNCTION);
    const funnelEnd = extensionSource.indexOf('\nasync function restartServerFromExplicitRecovery', funnelStart);
    expect(funnelStart).toBeGreaterThanOrEqual(0);
    expect(funnelEnd).toBeGreaterThan(funnelStart);
    const funnelBody = extensionSource.slice(funnelStart, funnelEnd);
    expect(funnelBody).toContain(LIFECYCLE_REPLACEMENT_GATE_MARKER);
  });

  test('explicit recovery wrappers delegate to restartServer rather than lifecycle directly', () => {
    const wrapperStart = extensionSource.indexOf(
      'async function restartServerFromExplicitRecovery',
    );
    expect(wrapperStart).toBeGreaterThanOrEqual(0);
    const wrapperEnd = extensionSource.indexOf('\nfunction shouldFormatOnSave', wrapperStart);
    const wrapperBody = extensionSource.slice(wrapperStart, wrapperEnd);
    expect(wrapperBody).toContain('await restartServer(context)');
    expect(wrapperBody).not.toContain(LIFECYCLE_REPLACEMENT_GATE_MARKER);
  });
});
