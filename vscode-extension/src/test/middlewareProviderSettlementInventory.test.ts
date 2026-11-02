import * as fs from 'node:fs';
import * as path from 'node:path';
import { evaluateMiddlewareProviderSettlementInventory } from '../middlewareProviderSettlementInventory';

const extensionSource = fs.readFileSync(
  path.join(__dirname, '..', '..', 'src', 'extension.ts'),
  'utf8',
);

describe('middleware provider settlement inventory', () => {
  test('requires typed settlement for every middleware provider hook', () => {
    const result = evaluateMiddlewareProviderSettlementInventory(extensionSource);
    expect(result.failures).toEqual([]);
    expect(result.ok).toBe(true);
  });
});
