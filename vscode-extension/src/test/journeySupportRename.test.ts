import * as assert from 'assert';
import { assertDailyDriverRenameEdits } from './published/journeySupport';

const source = 'use strict;\nuse warnings;\n\nmy $value = 42;\nprint $value;\n';
function edits(includeSigil = true) {
  const occurrence = (line: number) => {
    const start = line === 3 ? 3 : 6;
    return {
      range: {
        start: { line, character: start + (includeSigil ? 0 : 1) },
        end: { line, character: start + 6 },
      },
      newText: includeSigil ? '$renamed_value' : 'renamed_value',
    };
  };
  return [occurrence(3), occurrence(4)] as const;
}

describe('packaged fixture rename oracle', () => {
  it('accepts either legal sigil representation and arbitrary edit order', () => {
    assertDailyDriverRenameEdits(source, edits());
    assertDailyDriverRenameEdits(source, [...edits(false)].reverse());
  });

  it('rejects partial, duplicate, and extra occurrences', () => {
    const candidate = edits();
    assert.throws(() => assertDailyDriverRenameEdits(source, candidate.slice(0, 1)));
    assert.throws(() => assertDailyDriverRenameEdits(source, [candidate[0], candidate[0]]));
    assert.throws(() => assertDailyDriverRenameEdits(source, [...candidate, candidate[1]]));
  });

  it('rejects wrong replacement text and wrong ranges', () => {
    const wrongText = edits();
    wrongText[1].newText = '$other';
    assert.throws(() => assertDailyDriverRenameEdits(source, wrongText));
    const wrongRange = edits();
    wrongRange[1].range.end.character += 1;
    assert.throws(() => assertDailyDriverRenameEdits(source, wrongRange));
    const wrongStart = edits();
    wrongStart[0].range.start.character -= 1;
    assert.throws(() => assertDailyDriverRenameEdits(source, wrongStart));
  });

  it('rejects stale fixture text before applying edits', () => {
    assert.throws(() => assertDailyDriverRenameEdits(source.replace('42', '43'), edits()));
  });
});
