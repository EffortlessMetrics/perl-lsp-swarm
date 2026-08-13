import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { assertCandidateBoundInstallSource } from './runPublishedSmoke';

void test('candidate-bound Marketplace latest is refused before installation', () => {
  assert.throws(
    () =>
      assertCandidateBoundInstallSource({
        source: 'marketplace',
        version: '',
        vsixPath: '',
        candidateBound: true,
      }),
    /refuses Marketplace latest.*exact VSIX path and observed digest/,
  );
});

void test('scheduled unbound Marketplace smoke remains allowed', () => {
  assert.doesNotThrow(() =>
    assertCandidateBoundInstallSource({
      source: 'marketplace',
      version: '',
      vsixPath: '',
      candidateBound: false,
    }),
  );
});
