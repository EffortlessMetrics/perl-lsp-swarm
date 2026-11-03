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

  test('reports a hook that bypasses typed settlement', () => {
    const source = `middleware: {
      provideHover: async (d, p, t, next) => next(d, p, t),
      handleWorkDoneProgress: () => {},
    }`;
    const result = evaluateMiddlewareProviderSettlementInventory(source);
    expect(result.ok).toBe(false);
    expect(result.failures).toContain(
      'provideHover: does not route through typed provider settlement',
    );
  });

  test('reports raw try/catch in middleware', () => {
    const source = `middleware: {
      provideHover: async (d, p, t, next) => {
        try {
          return await settleMiddlewareProviderCall('Hover', d, () => next(d, p, t), null);
        } catch (e) {
          return null;
        }
      },
      handleWorkDoneProgress: () => {},
    }`;
    const result = evaluateMiddlewareProviderSettlementInventory(source);
    expect(result.ok).toBe(false);
    expect(result.failures).toContain(
      'LanguageClient middleware contains raw try/catch handlers; use typed provider settlement helpers instead',
    );
  });

  test('fails when the middleware block is missing', () => {
    expect(evaluateMiddlewareProviderSettlementInventory('const x = 1;').ok).toBe(false);
  });
});
