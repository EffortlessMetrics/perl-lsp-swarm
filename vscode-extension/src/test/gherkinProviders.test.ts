import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { readBoundedFile } from '../gherkinStepDefinitions';
import * as gherkinStepDefinitions from '../gherkinStepDefinitions';
import {
  collectStepDefinitionDocuments,
  provideGherkinDocumentSymbols,
  provideGherkinFoldingRanges,
  provideGherkinStepDefinitionLinks,
  registerGherkinProviders,
} from '../gherkinProviders';
import { classifyStepDefinitionStatus, parseGherkinStepLine } from '../gherkinStepDefinitions';

function makeEnvelopeWorkspace(name: string): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), `gherkin-envelope-${name}-`));
}

function cancelled(isCancellationRequested: boolean): vscode.CancellationToken {
  return { isCancellationRequested } as vscode.CancellationToken;
}

describe('gherkin outline providers', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  test('registers document symbol, folding, and definition providers for gherkin', () => {
    registerGherkinProviders();

    expect(vscode.languages.registerDocumentSymbolProvider).toHaveBeenCalledTimes(1);
    expect(vscode.languages.registerFoldingRangeProvider).toHaveBeenCalledTimes(1);
    expect(vscode.languages.registerDefinitionProvider).toHaveBeenCalledTimes(1);
    expect((vscode.languages.registerDocumentSymbolProvider as jest.Mock).mock.calls[0][0]).toEqual(
      [{ language: 'gherkin' }],
    );
  });

  test('builds hierarchical document symbols for feature structure', () => {
    const text = [
      '@smoke',
      'Feature: Checkout flow',
      '  Background: signed-in user',
      '    Given I am logged in',
      '  Scenario Outline: buying a product',
      '    When I add <item> to the cart',
      '    Then the total should be <total>',
      '    Examples: happy path',
      '      | item   | total |',
      '      | Widget | 10    |',
    ].join('\n');

    const symbols = provideGherkinDocumentSymbols(text);
    expect(symbols).toHaveLength(1);
    const feature = symbols[0];
    if (!feature) return;

    expect(feature.name).toBe('Feature: Checkout flow');
    expect(feature.children).toHaveLength(2);
    expect(feature.children.map((child) => child.name)).toEqual([
      'Background: signed-in user',
      'Scenario Outline: buying a product',
    ]);

    const background = feature.children[0];
    if (!background) return;

    expect(background.children.map((child) => child.name)).toEqual(['Given I am logged in']);

    const outline = feature.children[1];
    if (!outline) return;

    expect(outline.children.map((child) => child.name)).toEqual([
      'When I add <item> to the cart',
      'Then the total should be <total>',
      'Examples: happy path',
    ]);
  });

  test('returns folding ranges for feature sections', () => {
    const text = [
      'Feature: Checkout flow',
      '  Background: signed-in user',
      '    Given I am logged in',
      '  Scenario: buying a product',
      '    When I add the item to the cart',
      '    Then the total should be 10',
    ].join('\n');

    const ranges = provideGherkinFoldingRanges(text);
    expect(ranges).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ start: 0, end: 5 }),
        expect.objectContaining({ start: 1, end: 2 }),
        expect.objectContaining({ start: 3, end: 5 }),
      ]),
    );
  });

  test('finds regex-based Perl step definitions for Given steps', () => {
    const featureText = [
      'Feature: Login',
      '  Scenario: Successful login',
      '    Given a user exists with username "alice"',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/user_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/a user exists with username "([^"]+)"/, sub {',
            '    my $username = $1;',
            '};',
          ].join('\n'),
        },
      ],
    );

    expect(links).toHaveLength(1);
    const link = links?.[0];
    if (!link) return;

    expect(link.targetUri.fsPath).toBe('/project/features/step_definitions/user_steps.pm');
    expect(link.targetSelectionRange?.start.line).toBe(2);
  });

  test('skips potentially expensive regex step definitions', () => {
    const featureText = [
      'Feature: Login',
      '  Scenario: Successful login',
      '    Given aaaaaaaaaaaaaaaaaaaa!',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/user_steps.pm'),
          text: ['use Test::BDD::Cucumber::StepFile;', '', 'Given qr/^(a+)+!$/, sub {', '};'].join(
            '\n',
          ),
        },
      ],
    );

    expect(links).toHaveLength(0);
  });

  test('skips quantified alternation regex step definitions', () => {
    const featureText = [
      'Feature: Login',
      '  Scenario: Successful login',
      '    Given aaaaaaaaaaaaaaaaaaaa!',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/user_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/^(a|aa)+!$/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );

    expect(links).toHaveLength(0);
  });

  test('skips named backreference regex step definitions', () => {
    const links = provideGherkinStepDefinitionLinks(
      ['Feature: Named backreference', '  Scenario: Unsafe', '    Given a'].join('\n'),
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/steps.pm'),
          text: 'Given qr/^(?<value>a)\\k<value>$/, sub { return; };',
        },
      ],
    );

    expect(links).toHaveLength(0);
  });

  test.each([
    ['an escaped pipe', '^(foo\\|bar)+$'],
    ['a character-class pipe', '^(foo[|]bar)+$'],
  ])('does not skip quantified literal pipes (%s)', (_description, pattern) => {
    const featureText = [
      'Feature: Login',
      '  Scenario: Literal pipe',
      '    Given foo|barfoo|bar',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/user_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/' + pattern + '/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );

    expect(links).toHaveLength(1);
  });

  test('skips step definitions with bounded inner quantifiers in quantified groups (#953)', () => {
    // `([a-z]{2,5})+` — bounded inner quantifier, outer group quantifier
    const featureA = [
      'Feature: Validation',
      '  Scenario: Code check',
      '    Given aaaaaaaaaaaaaaaaaaaa!',
    ].join('\n');

    const linksA = provideGherkinStepDefinitionLinks(
      featureA,
      { line: 2, character: 10 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/validation_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/^([a-z]{2,5})+!$/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );
    expect(linksA).toHaveLength(0);

    // `(\d{1,3}){4}` — bounded inner quantifier, exact outer repetition
    const featureB = [
      'Feature: IP check',
      '  Scenario: Address match',
      '    Given 192168001001',
    ].join('\n');

    const linksB = provideGherkinStepDefinitionLinks(
      featureB,
      { line: 2, character: 10 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/ip_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/^(\\d{1,3}){4}$/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );
    expect(linksB).toHaveLength(0);
  });

  test('#859 safe cases still resolve links after bounded-quantifier extension', () => {
    // `[^"]+` and `[0-9]+` are linear-time — they must not be blocked.
    const featureText = [
      'Feature: Cart',
      '  Scenario: Price check',
      '    Given the total is 42',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/cart_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/^the total is [0-9]+$/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );
    expect(links).toHaveLength(1);
  });

  test('supports the existing case-insensitive regex flag', () => {
    const links = provideGherkinStepDefinitionLinks(
      ['Feature: Flags', '  Scenario: Case', '    Given STATUS: PASS'].join('\n'),
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/steps.pm'),
          text: 'Given qr/^status: pass$/i, sub { return; };',
        },
      ],
    );

    expect(links).toHaveLength(1);
  });

  test('rejects case-folded adjacent repetition through both matching consumers', () => {
    const pattern = '^a+A+$';
    const step = parseGherkinStepLine('Given aaAA', 0);
    expect(step).not.toBeNull();
    expect(classifyStepDefinitionStatus(step!, [`Given qr/${pattern}/i, sub { return; };`])).toBe(
      'ambiguous',
    );

    expect(
      provideGherkinStepDefinitionLinks(
        ['Feature: Flags', '  Scenario: Folded overlap', '    Given aaAA'].join('\n'),
        { line: 2, character: 12 } as vscode.Position,
        [
          {
            uri: vscode.Uri.file('/project/steps.pm'),
            text: `Given qr/${pattern}/i, sub { return; };`,
          },
        ],
      ),
    ).toEqual([]);
  });

  test('fails closed when the shared match budget is exhausted', () => {
    const featureText = [
      'Feature: Budget',
      '  Scenario: Exhausted',
      '    Given an unmatched step',
    ].join('\n');
    const definitions = Array.from(
      { length: 20_001 },
      (_unused, index) => `Given qr/^definition ${index}$/, sub { return; };`,
    ).join('\n');

    expect(
      provideGherkinStepDefinitionLinks(
        featureText,
        { line: 2, character: 15 } as vscode.Position,
        [{ uri: vscode.Uri.file('/project/steps.pm'), text: definitions }],
      ),
    ).toBeNull();
  });

  test('keeps provider and step-definition matching parity for policy limits', () => {
    const pattern = '^a$';
    const stepText = 'a'.repeat(513);
    const step = parseGherkinStepLine(`Given ${stepText}`, 2);
    expect(step).not.toBeNull();
    expect(classifyStepDefinitionStatus(step!, [`Given qr/${pattern}/, sub { return; };`])).toBe(
      'ambiguous',
    );

    const featureText = ['Feature: Limits', '  Scenario: Oversized', `    Given ${stepText}`].join(
      '\n',
    );
    expect(
      provideGherkinStepDefinitionLinks(
        featureText,
        { line: 2, character: 15 } as vscode.Position,
        [
          {
            uri: vscode.Uri.file('/project/steps.pm'),
            text: `Given qr/${pattern}/, sub { return; };`,
          },
        ],
      ),
    ).toEqual([]);

    const oversizedPattern = `^${'a'.repeat(257)}$`;
    const shortStep = parseGherkinStepLine('Given a', 2);
    expect(shortStep).not.toBeNull();
    expect(
      classifyStepDefinitionStatus(shortStep!, [`Given qr/${oversizedPattern}/, sub { return; };`]),
    ).toBe('ambiguous');
    expect(
      provideGherkinStepDefinitionLinks(
        ['Feature: Limits', '  Scenario: Oversized regex', '    Given a'].join('\n'),
        { line: 2, character: 15 } as vscode.Position,
        [
          {
            uri: vscode.Uri.file('/project/steps.pm'),
            text: `Given qr/${oversizedPattern}/, sub { return; };`,
          },
        ],
      ),
    ).toEqual([]);
  });

  test('does not skip named-capture group step definitions (no false positive)', () => {
    const featureText = [
      'Feature: Cart',
      '  Scenario: Add items',
      '    Given I have 3 items in the cart',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 15 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/features/step_definitions/cart_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/I have (?<count>\\d+) items in the cart/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );

    // Named captures are safe — must NOT be filtered by the expensive-regex guard
    expect(links).toHaveLength(1);
  });

  test('resolves And steps using the previous Given/When/Then context', () => {
    const featureText = [
      'Feature: Checkout',
      '  Scenario: Purchase',
      '    Given I am authenticated',
      '    And the cart is empty',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 3, character: 12 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/t/steps/cart_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/the cart is empty/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );

    expect(links).toHaveLength(1);
    const link = links?.[0];
    if (!link) return;

    expect(link.targetSelectionRange?.start.line).toBe(2);
  });

  test('does not infer a definition keyword for an initial And step', () => {
    const featureText = [
      'Feature: Checkout',
      '  Scenario: Purchase',
      '    And the cart is empty',
    ].join('\n');

    const links = provideGherkinStepDefinitionLinks(
      featureText,
      { line: 2, character: 12 } as vscode.Position,
      [
        {
          uri: vscode.Uri.file('/project/t/steps/cart_steps.pm'),
          text: [
            'use Test::BDD::Cucumber::StepFile;',
            '',
            'Given qr/the cart is empty/, sub {',
            '};',
          ].join('\n'),
        },
      ],
    );

    expect(links).toHaveLength(0);
  });

  test('uses the registered definition provider to scan workspace step files', async () => {
    registerGherkinProviders();

    const provider = (vscode.languages.registerDefinitionProvider as jest.Mock).mock.calls[0][1];
    const workspaceRoot = makeEnvelopeWorkspace('provider-scan');
    const stepFile = path.join(workspaceRoot, 'features', 'step_definitions', 'login_steps.pm');
    fs.mkdirSync(path.dirname(stepFile), { recursive: true });
    fs.writeFileSync(
      stepFile,
      [
        'use Test::BDD::Cucumber::StepFile;',
        '',
        'When qr/the user logs in with valid credentials/, sub {',
        '};',
      ].join('\n'),
    );
    (vscode.workspace.findFiles as jest.Mock)
      .mockResolvedValueOnce([vscode.Uri.file(stepFile)])
      .mockResolvedValue([]);

    try {
      const links = await provider.provideDefinition(
        {
          getText: () =>
            [
              'Feature: Login',
              '  Scenario: Happy path',
              '    When the user logs in with valid credentials',
            ].join('\n'),
        } as vscode.TextDocument,
        { line: 2, character: 12 } as vscode.Position,
        { isCancellationRequested: false } as vscode.CancellationToken,
      );

      expect(vscode.workspace.findFiles).toHaveBeenCalled();
      expect(links).toHaveLength(1);
      expect(links[0].targetUri.fsPath).toBe(stepFile);
    } finally {
      fs.rmSync(workspaceRoot, { recursive: true, force: true });
    }
  });
});

/**
 * The workspace scan bounds are a security claim (#9773), so they get a direct
 * seam and real files: `openTextDocument` bounded nothing, and a mock cannot
 * fail when the cap is removed.
 */
describe('gherkin step-definition workspace envelope', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    (vscode.workspace.decode as jest.Mock).mockImplementation(async (content: Uint8Array) =>
      Buffer.from(content).toString('utf8'),
    );
  });

  test('skips candidates over the per-file byte cap', async () => {
    const root = makeEnvelopeWorkspace('per-file-cap');
    const oversized = path.join(root, 'big.pm');
    const small = path.join(root, 'small.pm');
    fs.writeFileSync(oversized, Buffer.alloc(600 * 1024, 0x61));
    fs.writeFileSync(small, 'Given qr/^ok$/, sub { return; };\n');

    try {
      const scan = await collectStepDefinitionDocuments(
        [vscode.Uri.file(oversized), vscode.Uri.file(small)],
        cancelled(false),
      );

      expect(scan.documents.map((document) => document.uri.fsPath)).toEqual([small]);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('stops reading at the aggregate byte cap instead of reading everything', async () => {
    const root = makeEnvelopeWorkspace('aggregate-cap');
    // Each file is under the 512 KiB per-file cap, so only the aggregate cap
    // can stop the scan: 42 x 400 KiB = 16.4 MiB > 16 MiB.
    const chunk = Buffer.alloc(400 * 1024, 0x61);
    const candidates = Array.from({ length: 42 }, (_unused, index) => {
      const candidate = path.join(root, `part_${index}.pm`);
      fs.writeFileSync(candidate, chunk);
      return vscode.Uri.file(candidate);
    });

    try {
      const scan = await collectStepDefinitionDocuments(candidates, cancelled(false));

      // 40 files fit under 16 MiB; the 41st would push past the cap, so the
      // scan must stop rather than read the rest. Without the aggregate cap
      // this scan would return all 42 documents.
      expect(scan.documents).toHaveLength(40);
      expect(scan.refusal).toBe('read_budget_exhausted');
      const acceptedBytes = scan.documents.reduce(
        (total, document) => total + Buffer.byteLength(document.text, 'utf8'),
        0,
      );
      expect(acceptedBytes).toBeLessThanOrEqual(16 * 1024 * 1024);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('honours an already-cancelled token before reading any file', async () => {
    const root = makeEnvelopeWorkspace('cancelled');
    const candidate = path.join(root, 'steps.pm');
    fs.writeFileSync(candidate, 'Given qr/^ok$/, sub { return; };\n');

    try {
      const scan = await collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(true),
      );

      expect(scan.documents).toHaveLength(0);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('skips a symlinked candidate and reads the regular file on every platform', async () => {
    const root = makeEnvelopeWorkspace('symlink');
    const regular = path.join(root, 'steps.pm');
    fs.writeFileSync(regular, 'Given qr/^ok$/, sub { return; };\n');
    const linked = path.join(root, 'linked.pm');
    let symlinkConstructed = false;
    try {
      fs.symlinkSync(regular, linked, 'file');
      symlinkConstructed = true;
    } catch (error) {
      // Windows without developer mode and hardened sandboxes refuse symlink
      // creation. The regular-file leg still runs everywhere, and the
      // link-mechanism test below discriminates without a real link; the
      // refusal code must still be one of the known platform refusals.
      const code = (error as NodeJS.ErrnoException).code;
      expect(['EPERM', 'EACCES', 'ENOTSUP', 'UNKNOWN', 'ENOENT']).toContain(code);
    }

    try {
      const scan = await collectStepDefinitionDocuments(
        symlinkConstructed
          ? [vscode.Uri.file(linked), vscode.Uri.file(regular)]
          : [vscode.Uri.file(regular)],
        cancelled(false),
      );

      expect(scan.documents.map((document) => document.uri.fsPath)).toEqual([regular]);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('rejects a candidate whose path entry is a symlink even where symlinks cannot be constructed', async () => {
    const root = makeEnvelopeWorkspace('symlink-mechanism');
    const regular = path.join(root, 'steps.pm');
    fs.writeFileSync(regular, 'Given qr/^ok$/, sub { return; };\n');
    const lstat = jest.spyOn(fs.promises, 'lstat').mockResolvedValue({
      isFile: () => false,
      isSymbolicLink: () => true,
    } as unknown as fs.Stats);

    try {
      const read = await readBoundedFile(regular, 512 * 1024);

      expect(read).toBeNull();
    } finally {
      lstat.mockRestore();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('rejects a nonregular path before opening a descriptor', async () => {
    const root = makeEnvelopeWorkspace('special-entry');
    const candidate = path.join(root, 'steps.pm');
    const lstat = jest.spyOn(fs.promises, 'lstat').mockResolvedValue({
      isFile: () => false,
      isSymbolicLink: () => false,
    } as unknown as fs.Stats);
    const open = jest.spyOn(fs.promises, 'open');

    try {
      expect(await readBoundedFile(candidate, 512 * 1024)).toBeNull();
      expect(open).not.toHaveBeenCalled();
    } finally {
      open.mockRestore();
      lstat.mockRestore();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('refuses the scan when attempted reads exhaust the read budget', async () => {
    const root = makeEnvelopeWorkspace('read-budget');
    // The review's falsifier: 100 x 600 KiB candidates are each rejected by
    // the per-file cap only AFTER reading 512 KiB + 1 byte, so a scan that
    // counts only retained bytes streams ~50 MB while its retained total
    // stays 0. The read budget must stop it far short of that.
    const hostile = Buffer.alloc(600 * 1024, 0x61);
    const candidates = Array.from({ length: 100 }, (_unused, index) => {
      const candidate = path.join(root, `hostile_${index}.pm`);
      fs.writeFileSync(candidate, hostile);
      return vscode.Uri.file(candidate);
    });

    try {
      const scan = await collectStepDefinitionDocuments(candidates, cancelled(false));

      expect(scan.documents).toHaveLength(0);
      expect(scan.refusal).toBe('read_budget_exhausted');
      expect(scan.attemptedBytes).toBeLessThanOrEqual(16 * 1024 * 1024);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('prefers an already-open clean document buffer over its stale disk contents', async () => {
    const root = makeEnvelopeWorkspace('open-buffer');
    const candidate = path.join(root, 'steps.pm');
    fs.writeFileSync(candidate, 'Given qr/^stale$/, sub { return; };\n');
    const openDocument = {
      uri: vscode.Uri.file(candidate),
      isDirty: false,
      getText: () => 'Given qr/^buffer$/, sub { return; };\n',
    } as unknown as vscode.TextDocument;
    (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [openDocument];

    try {
      const scan = await collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(false),
      );

      // The disk copy says ^stale$; the open buffer says ^buffer$. Links must
      // follow the buffer, the way the previous openTextDocument-based scan
      // happened to see it.
      expect(scan.documents.map((document) => document.text)).toEqual([
        'Given qr/^buffer$/, sub { return; };\n',
      ]);
    } finally {
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('decodes bounded bytes with the configured VS Code encoding', async () => {
    const root = makeEnvelopeWorkspace('utf16');
    const candidate = path.join(root, 'steps.pm');
    const text = 'Given qr/^café$/, sub { return; };\n';
    fs.writeFileSync(candidate, Buffer.from(`\uFEFF${text}`, 'utf16le'));
    const decode = jest
      .spyOn(vscode.workspace, 'decode')
      .mockImplementation(async (bytes: Uint8Array) =>
        Buffer.from(bytes)
          .toString('utf16le')
          .replace(/^\uFEFF/, ''),
      );

    try {
      const uri = vscode.Uri.file(candidate);
      const scan = await collectStepDefinitionDocuments([uri], cancelled(false));

      expect(decode).toHaveBeenCalledWith(expect.any(Uint8Array), { uri });
      expect(scan.documents.map((document) => document.text)).toEqual([text]);
      const links = provideGherkinStepDefinitionLinks(
        'Feature: Café\n  Scenario: Visit\n    Given café',
        { line: 2, character: 12 } as vscode.Position,
        scan.documents,
      );
      expect(links).toHaveLength(1);
      expect(links?.[0]?.targetUri.toString()).toBe(uri.toString());
      expect(links?.[0]?.targetSelectionRange?.start).toEqual(new vscode.Position(0, 0));
    } finally {
      decode.mockRestore();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('returns no definition result when the production scan refuses the read budget', async () => {
    const root = makeEnvelopeWorkspace('provider-refusal');
    const matchingDefinition = 'When qr/^the user logs in$/, sub { return; };\n#';
    const featureText = 'Feature: Login\n  Scenario: Happy path\n    When the user logs in';
    const candidates = Array.from({ length: 42 }, (_unused, index) => {
      const candidate = path.join(root, `part_${index}.pm`);
      const contents = Buffer.alloc(400 * 1024, 0x61);
      if (index === 0) {
        contents.write(matchingDefinition, 'utf8');
      }
      fs.writeFileSync(candidate, contents);
      return vscode.Uri.file(candidate);
    });

    try {
      expect(
        provideGherkinStepDefinitionLinks(
          featureText,
          { line: 2, character: 12 } as vscode.Position,
          [{ uri: vscode.Uri.file(path.join(root, 'part_0.pm')), text: matchingDefinition }],
        ),
      ).toHaveLength(1);
      registerGherkinProviders();
      (vscode.workspace.findFiles as jest.Mock)
        .mockResolvedValueOnce(candidates)
        .mockResolvedValue([]);
      const provider = (vscode.languages.registerDefinitionProvider as jest.Mock).mock.calls[0][1];
      const token = cancelled(false);

      const links = await provider.provideDefinition(
        {
          getText: () => featureText,
        } as vscode.TextDocument,
        { line: 2, character: 12 } as vscode.Position,
        token,
      );

      expect(vscode.workspace.findFiles).toHaveBeenCalledWith(
        expect.any(String),
        expect.any(String),
        1000,
        token,
      );
      expect(links).toBeUndefined();
      expect(vscode.window.showWarningMessage).toHaveBeenCalledWith(
        'Gherkin step-definition discovery stopped before the workspace was fully scanned; no definition result is available.',
      );
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('prefers a buffer that turns dirty while the disk read is pending', async () => {
    const root = makeEnvelopeWorkspace('dirty-race');
    const candidate = path.join(root, 'steps.pm');
    const diskText = 'Given qr/^disk$/, sub { return; };\n';
    fs.writeFileSync(candidate, diskText);

    let releaseDiskRead: (
      value: { bytes: Uint8Array; text: string; byteLength: number } | null,
    ) => void = () => undefined;
    const pendingDiskRead = new Promise<{
      bytes: Uint8Array;
      text: string;
      byteLength: number;
    } | null>((resolve) => {
      releaseDiskRead = resolve;
    });
    const diskReadSpy = jest
      .spyOn(gherkinStepDefinitions, 'readBoundedFile')
      .mockImplementation(async () => pendingDiskRead);

    const dirty = {
      uri: vscode.Uri.file(candidate),
      isDirty: false,
      getText: () => 'Given qr/^buffer$/, sub { return; };\n',
    } as unknown as vscode.TextDocument;
    (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];

    try {
      const scanPromise = collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(false),
      );
      // The edit lands while the disk read is pending, so only the post-await
      // reconcile can see it.
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [dirty];
      (dirty as unknown as { isDirty: boolean }).isDirty = true;
      releaseDiskRead({
        bytes: Buffer.from(diskText, 'utf8'),
        text: diskText,
        byteLength: Buffer.byteLength(diskText, 'utf8'),
      });

      const scan = await scanPromise;

      expect(scan.documents.map((document) => document.text)).toEqual([
        'Given qr/^buffer$/, sub { return; };\n',
      ]);
    } finally {
      diskReadSpy.mockRestore();
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('rechecks a buffer that turns dirty after a failed disk read', async () => {
    const root = makeEnvelopeWorkspace('dirty-null-race');
    const candidate = path.join(root, 'steps.pm');
    const dirtyText = 'Given qr/^buffer$/, sub { return; };\n';
    fs.writeFileSync(candidate, 'Given qr/^disk$/, sub { return; };\n');

    let releaseDiskRead: (
      value: { bytes: Uint8Array; text: string; byteLength: number } | null,
    ) => void = () => undefined;
    const pendingDiskRead = new Promise<{
      bytes: Uint8Array;
      text: string;
      byteLength: number;
    } | null>((resolve) => {
      releaseDiskRead = resolve;
    });
    const diskReadSpy = jest
      .spyOn(gherkinStepDefinitions, 'readBoundedFile')
      .mockImplementation(async () => pendingDiskRead);

    const dirty = {
      uri: vscode.Uri.file(candidate),
      isDirty: false,
      getText: () => dirtyText,
    } as unknown as vscode.TextDocument;
    (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];

    try {
      const scanPromise = collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(false),
      );
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [dirty];
      (dirty as unknown as { isDirty: boolean }).isDirty = true;
      releaseDiskRead(null);

      const scan = await scanPromise;

      expect(scan.documents.map((document) => document.text)).toEqual([dirtyText]);
    } finally {
      diskReadSpy.mockRestore();
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('rechecks an editor buffer that appears while configured decoding is pending', async () => {
    const root = makeEnvelopeWorkspace('decode-race');
    const candidate = path.join(root, 'steps.pm');
    const diskText = 'Given qr/^disk$/, sub { return; };\n';
    fs.writeFileSync(candidate, diskText);

    let markDecodeStarted = (): void => undefined;
    const decodeStarted = new Promise<void>((resolve) => {
      markDecodeStarted = resolve;
    });
    let releaseDecode: (value: string) => void = () => undefined;
    const pendingDecode = new Promise<string>((resolve) => {
      releaseDecode = resolve;
    });
    const decode = jest.spyOn(vscode.workspace, 'decode').mockImplementation(async () => {
      markDecodeStarted();
      return pendingDecode;
    });
    const openDocument = {
      uri: vscode.Uri.file(candidate),
      isDirty: true,
      getText: () => 'Given qr/^buffer$/, sub { return; };\n',
    } as unknown as vscode.TextDocument;

    try {
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      const scanPromise = collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(false),
      );
      await decodeStarted;
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [openDocument];
      releaseDecode(diskText);

      const scan = await scanPromise;

      expect(scan.documents.map((document) => document.text)).toEqual([
        'Given qr/^buffer$/, sub { return; };\n',
      ]);
    } finally {
      decode.mockRestore();
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('rechecks an editor buffer when configured decoding fails', async () => {
    const root = makeEnvelopeWorkspace('decode-error-race');
    const candidate = path.join(root, 'steps.pm');
    fs.writeFileSync(candidate, 'Given qr/^disk$/, sub { return; };\n');

    let markDecodeStarted = (): void => undefined;
    const decodeStarted = new Promise<void>((resolve) => {
      markDecodeStarted = resolve;
    });
    let releaseDecode: () => void = () => undefined;
    const pendingDecode = new Promise<never>((_resolve, reject) => {
      releaseDecode = () => reject(new Error('decode failed'));
    });
    const decode = jest.spyOn(vscode.workspace, 'decode').mockImplementation(async () => {
      markDecodeStarted();
      return pendingDecode;
    });
    const openDocument = {
      uri: vscode.Uri.file(candidate),
      isDirty: true,
      getText: () => 'Given qr/^buffer$/, sub { return; };\n',
    } as unknown as vscode.TextDocument;

    try {
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      const scanPromise = collectStepDefinitionDocuments(
        [vscode.Uri.file(candidate)],
        cancelled(false),
      );
      await decodeStarted;
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [openDocument];
      releaseDecode();

      const scan = await scanPromise;

      expect(scan.documents.map((document) => document.text)).toEqual([
        'Given qr/^buffer$/, sub { return; };\n',
      ]);
    } finally {
      decode.mockRestore();
      (vscode.workspace as unknown as { textDocuments: unknown[] }).textDocuments = [];
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test('skips candidates whose URI scheme is not a local file', async () => {
    const root = makeEnvelopeWorkspace('scheme');
    const regular = path.join(root, 'steps.pm');
    const virtual = path.join(root, 'virtual.pm');
    fs.writeFileSync(regular, 'Given qr/^ok$/, sub { return; };\n');
    fs.writeFileSync(virtual, 'Given qr/^ok$/, sub { return; };\n');

    try {
      const scan = await collectStepDefinitionDocuments(
        [
          { scheme: 'git', fsPath: virtual, toString: () => virtual } as unknown as vscode.Uri,
          vscode.Uri.file(regular),
        ],
        cancelled(false),
      );

      // A non-file fsPath names no local file this scan may read, so the
      // virtual candidate must not be opened even though a local file exists
      // at that path.
      expect(scan.documents.map((document) => document.uri.fsPath)).toEqual([regular]);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
});
