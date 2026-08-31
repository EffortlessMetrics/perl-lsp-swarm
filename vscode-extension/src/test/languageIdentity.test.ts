import * as fs from 'fs';
import * as path from 'path';
import {
  CANONICAL_PERL_LANGUAGE_ID,
  SUPPORTED_PERL_LANGUAGE_IDS,
  SUPPORTED_PERL_URI_SCHEMES,
  isPerlLanguageId,
  isSupportedPerlUriScheme,
  perlDocumentSelector,
} from '../languageIdentity';

const EXT_ROOT = path.resolve(__dirname, '..', '..');
const SRC_ROOT = path.join(EXT_ROOT, 'src');

function readExtensionManifest(): {
  activationEvents: string[];
  contributes: {
    languages: Array<{ id: string }>;
    grammars: Array<{ language: string; scopeName: string; path: string }>;
    breakpoints: Array<{ language: string }>;
    snippets: Array<{ language: string; path: string }>;
    menus: Record<string, Array<{ command: string; when?: string }>>;
    keybindings: Array<{ command: string; key: string; when?: string }>;
  };
} {
  return JSON.parse(fs.readFileSync(path.join(EXT_ROOT, 'package.json'), 'utf8'));
}

function listProductionSourceFiles(dir: string): string[] {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (entry.name === 'test' || entry.name === '__mocks__') {
        continue;
      }
      files.push(...listProductionSourceFiles(path.join(dir, entry.name)));
    } else if (entry.name.endsWith('.ts')) {
      files.push(path.join(dir, entry.name));
    }
  }
  return files;
}

describe('Perl language identity authority (#7699)', () => {
  test('perl is the canonical contributed language id', () => {
    expect(CANONICAL_PERL_LANGUAGE_ID).toBe('perl');
    expect(SUPPORTED_PERL_LANGUAGE_IDS[0]).toBe(CANONICAL_PERL_LANGUAGE_ID);
  });

  test('perl5 is the single supported alias, listed after the canonical id', () => {
    expect(SUPPORTED_PERL_LANGUAGE_IDS).toEqual(['perl', 'perl5']);
  });

  test('recognizes the canonical id and the alias', () => {
    expect(isPerlLanguageId('perl')).toBe(true);
    expect(isPerlLanguageId('perl5')).toBe(true);
  });

  test('rejects look-alike and unrelated language ids (exact-match contract)', () => {
    expect(isPerlLanguageId('PERL')).toBe(false);
    expect(isPerlLanguageId('Perl')).toBe(false);
    expect(isPerlLanguageId('perl50')).toBe(false);
    expect(isPerlLanguageId('perl6')).toBe(false);
    expect(isPerlLanguageId('perls')).toBe(false);
    expect(isPerlLanguageId('markdown')).toBe(false);
    expect(isPerlLanguageId('')).toBe(false);
    expect(isPerlLanguageId(undefined)).toBe(false);
  });

  test('supported schemes are the client-attached file and untitled schemes', () => {
    expect(SUPPORTED_PERL_URI_SCHEMES).toEqual(['file', 'untitled']);
    expect(isSupportedPerlUriScheme('file')).toBe(true);
    expect(isSupportedPerlUriScheme('untitled')).toBe(true);
    expect(isSupportedPerlUriScheme('git')).toBe(false);
    expect(isSupportedPerlUriScheme('output')).toBe(false);
  });

  test('document selector covers every supported scheme x supported language id', () => {
    expect(perlDocumentSelector()).toEqual([
      { scheme: 'file', language: 'perl' },
      { scheme: 'file', language: 'perl5' },
      { scheme: 'untitled', language: 'perl' },
      { scheme: 'untitled', language: 'perl5' },
    ]);
  });
});

describe('language-ID manifest contract (#7699)', () => {
  test('no activation-only language id exists: every onLanguage id is contributed or a supported alias', () => {
    const pkg = readExtensionManifest();
    const contributedIds = new Set(pkg.contributes.languages.map((language) => language.id));
    const activationLanguageIds = pkg.activationEvents
      .filter((event) => event.startsWith('onLanguage:'))
      .map((event) => event.slice('onLanguage:'.length));

    // The contract under test: an activation event for a language this
    // package does not contribute is exactly how a false-support route
    // (`onLanguage:perl5` with no client selection) ships. Such an id must be
    // an explicitly supported alias with client selection, never drift back
    // to an activation-only gesture.
    for (const id of activationLanguageIds) {
      if (!contributedIds.has(id)) {
        expect(SUPPORTED_PERL_LANGUAGE_IDS).toContain(id);
      }
    }
    expect(activationLanguageIds).toContain('perl5');
  });

  test('the alias is never a second language contribution and shares the one canonical grammar', () => {
    const pkg = readExtensionManifest();
    const contributedIds = pkg.contributes.languages.map((language) => language.id);
    expect(contributedIds).toContain('perl');
    expect(contributedIds).not.toContain('perl5');

    // The alias gets no second language contribution and no second grammar: a
    // `perl5`-classified buffer is bound to the *same* bundled TextMate grammar
    // (`source.perl`, `./syntaxes/perl.tmLanguage.json`) so the "same grammar"
    // support claim is true, not advertised.
    const perlGrammarEntries = pkg.contributes.grammars.filter((grammar) =>
      SUPPORTED_PERL_LANGUAGE_IDS.includes(grammar.language),
    );
    expect(perlGrammarEntries.map((grammar) => grammar.language)).toEqual([
      ...SUPPORTED_PERL_LANGUAGE_IDS,
    ]);
    const perlGrammarPaths = new Set(perlGrammarEntries.map((grammar) => grammar.path));
    expect(perlGrammarPaths).toEqual(new Set(['./syntaxes/perl.tmLanguage.json']));
    expect(new Set(perlGrammarEntries.map((grammar) => grammar.scopeName))).toEqual(
      new Set(['source.perl']),
    );

    // Grammar entries for other languages still bind contributed languages.
    const contributedIdSet = new Set(contributedIds);
    for (const grammar of pkg.contributes.grammars) {
      if (SUPPORTED_PERL_LANGUAGE_IDS.includes(grammar.language)) {
        continue;
      }
      expect(contributedIdSet.has(grammar.language)).toBe(true);
    }
  });

  test('breakpoint registration covers every supported Perl language id', () => {
    const pkg = readExtensionManifest();
    const breakpointLanguages = pkg.contributes.breakpoints.map((entry) => entry.language);
    for (const id of SUPPORTED_PERL_LANGUAGE_IDS) {
      expect(breakpointLanguages).toContain(id);
    }
  });

  test('the shared Perl snippet catalog binds to every supported Perl language id', () => {
    const pkg = readExtensionManifest();
    const perlSnippetLanguages = pkg.contributes.snippets
      .filter((entry) => entry.path.endsWith('perl.json'))
      .map((entry) => entry.language);
    for (const id of SUPPORTED_PERL_LANGUAGE_IDS) {
      expect(perlSnippetLanguages).toContain(id);
    }
  });

  test('debug-config resolution activates for the alias too', () => {
    const pkg = readExtensionManifest();
    expect(pkg.activationEvents).toContain('onDebugResolve:perl');
    expect(pkg.activationEvents).toContain('onDebugResolve:perl5');
  });

  test('every declarative language gate names the full supported Perl language-id set', () => {
    const pkg = readExtensionManifest();
    const langGatePattern = /(editorLangId|resourceLangId)\s*==\s*([A-Za-z0-9]+)/g;

    const declarativeGates: Array<{ surface: string; command: string; when: string }> = [];
    for (const [surface, entries] of Object.entries(pkg.contributes.menus)) {
      for (const entry of entries) {
        if (entry.when) {
          declarativeGates.push({
            surface: `menus.${surface}`,
            command: entry.command,
            when: entry.when,
          });
        }
      }
    }
    for (const keybinding of pkg.contributes.keybindings) {
      if (keybinding.when) {
        declarativeGates.push({
          surface: 'keybindings',
          command: keybinding.command,
          when: keybinding.when,
        });
      }
    }
    expect(declarativeGates.length).toBeGreaterThan(0);

    const failures: string[] = [];
    for (const gate of declarativeGates) {
      const gatedIds = [...gate.when.matchAll(langGatePattern)]
        .map((match) => match[2])
        .filter((id): id is string => id !== undefined);
      if (gatedIds.length === 0 || !gatedIds.some((id) => isPerlLanguageId(id))) {
        continue;
      }
      for (const id of SUPPORTED_PERL_LANGUAGE_IDS) {
        if (!gatedIds.includes(id)) {
          failures.push(
            `${gate.surface} '${gate.command}' gates on "${gate.when}" without supported alias id '${id}'`,
          );
        }
      }
      for (const id of gatedIds) {
        if (!isPerlLanguageId(id) && /perl/i.test(id)) {
          failures.push(
            `${gate.surface} '${gate.command}' gates on unsupported Perl-like language id '${id}'`,
          );
        }
      }
    }
    expect(failures).toEqual([]);
  });

  test('the language client selector is built from the authority, not inline literals', () => {
    const extensionSource = fs.readFileSync(path.join(SRC_ROOT, 'extension.ts'), 'utf8');
    expect(extensionSource).toContain('documentSelector: perlDocumentSelector()');
    expect(extensionSource).not.toMatch(/language:\s*'perl5?'/);
  });

  test('production sources route language identity through the authority (no scattered equality)', () => {
    // Strict (===/!==) and loose (==/!=) comparisons are both contract
    // violations: loose equality additionally coerces, so `languageId == 'perl'`
    // drift must not survive either form. Cross-line or renamed-local shapes
    // remain inherent blind spots of a textual scan; the authority's export
    // surface (`isPerlLanguageId`/`perlDocumentSelector`) is the durable guard.
    const scattered =
      /\.languageId\s*(===|!==|==|!=)\s*'(?:perl5?)'|'(?:perl5?)'\s*(===|!==|==|!=)\s*[\w.]*languageId/;
    const offenders: string[] = [];
    for (const file of listProductionSourceFiles(SRC_ROOT)) {
      if (path.basename(file) === 'languageIdentity.ts') {
        continue;
      }
      if (scattered.test(fs.readFileSync(file, 'utf8'))) {
        offenders.push(path.relative(SRC_ROOT, file));
      }
    }
    expect(offenders).toEqual([]);
  });
});
