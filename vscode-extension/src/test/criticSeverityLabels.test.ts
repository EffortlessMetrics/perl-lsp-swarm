import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
jest.mock('vscode-languageclient/node', () => ({
  LanguageClient: class {},
  Trace: { Off: 'off', Messages: 'messages', Verbose: 'verbose' },
  TransportKind: { stdio: 0 },
}));
import { setPerlCriticSeverity } from '../extension';

/**
 * `perl-lsp.critic.severity` is a MINIMUM-severity-to-report threshold. The
 * server gate is `severity as u8 >= config.severity`
 * (`crates/perl-lsp-rs-core/src/tooling/perl_critic/native/native_registry.rs`),
 * which matches perlcritic's own `--severity` scale: 1 is `--brutal` and
 * reports everything, 5 is `--gentle` and reports only the most severe.
 *
 * So a LOWER number yields MORE diagnostics. The dropdown once described `1`
 * as "very permissive" and `5` as "very strict" — exactly backwards, and
 * directly contradicting the `description` string sitting beside it in the
 * same settings block. A user wanting strict linting picked `5` and got the
 * fewest diagnostics.
 *
 * These tests pin the direction so the labels cannot drift back.
 */
describe('critic severity labels', () => {
  const extensionRoot = path.resolve(__dirname, '..', '..');
  const packageJson = JSON.parse(
    fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
  ) as {
    contributes: {
      configuration:
        | { properties: Record<string, unknown> }
        | Array<{ properties: Record<string, unknown> }>;
    };
  };

  const configBlocks = Array.isArray(packageJson.contributes.configuration)
    ? packageJson.contributes.configuration
    : [packageJson.contributes.configuration];

  const severitySettings = configBlocks
    .flatMap((block) => Object.entries(block.properties ?? {}))
    .filter(([key]) => /critic\.severity$/.test(key)) as Array<
    [string, { enum?: number[]; enumDescriptions?: string[]; description?: string }]
  >;

  it('declares at least one critic severity setting', () => {
    expect(severitySettings.length).toBeGreaterThan(0);
  });

  for (const [key, value] of severitySettings) {
    describe(key, () => {
      it('pairs every enum value with a description', () => {
        expect(value.enum).toBeDefined();
        expect(value.enumDescriptions).toBeDefined();
        expect(value.enumDescriptions).toHaveLength(value.enum?.length ?? -1);
      });

      it('does not describe the lowest value as the most permissive', () => {
        // Severity 1 reports the MOST. Calling it "permissive" inverts the
        // meaning relative to how the server consumes the value.
        const lowest = value.enumDescriptions?.[0] ?? '';
        expect(lowest.toLowerCase()).not.toContain('permissive');
      });

      it('does not describe the highest value as the most strict', () => {
        // Severity 5 reports the LEAST — it is the permissive end.
        const highest = value.enumDescriptions?.[(value.enum?.length ?? 1) - 1] ?? '';
        expect(highest.toLowerCase()).not.toContain('strict');
      });

      it('keeps the prose description consistent with the threshold direction', () => {
        // The description was already correct when the labels were not; guard
        // it so the two halves cannot diverge again.
        expect(value.description).toMatch(/1 = least severe\/reports more/);
        expect(value.description).toMatch(/5 = most severe\/reports less/);
      });
    });
  }
});

describe('setPerlCriticSeverity quick pick', () => {
  it('offers labels whose wording matches the reporting direction', async () => {
    const showQuickPick = vscode.window.showQuickPick as unknown as jest.Mock;
    showQuickPick.mockClear();
    showQuickPick.mockResolvedValueOnce(undefined);

    await setPerlCriticSeverity(undefined);

    const items = showQuickPick.mock.calls[0]?.[0] as
      | Array<{ label: string; description?: string }>
      | undefined;
    expect(items).toBeDefined();
    expect(items?.map((item) => item.label)).toEqual(['1', '2', '3', '4', '5']);

    const described = new Map(
      (items ?? []).map((item) => [item.label, (item.description ?? '').toLowerCase()]),
    );

    // 1 reports the MOST, so it must not be sold as the permissive option.
    expect(described.get('1')).not.toContain('permissive');
    // 5 reports the LEAST, so it must not be sold as the strict option.
    expect(described.get('5')).not.toContain('strict');
  });
});
