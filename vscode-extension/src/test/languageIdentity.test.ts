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
    grammars: Array<{ language: string }>;
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

  test('the alias is never a second language contribution or grammar', () => {
    const pkg = readExtensionManifest();
    const contributedIds = pkg.contributes.languages.map((language) => language.id);
    expect(contributedIds).toContain('perl');
    expect(contributedIds).not.toContain('perl5');

    const grammarLanguages = pkg.contributes.grammars.map((grammar) => grammar.language);
    for (const language of grammarLanguages) {
      expect(contributedIds).toContain(language);
    }
    expect(grammarLanguages).not.toContain('perl5');
  });

  test('the language client selector is built from the authority, not inline literals', () => {
    const extensionSource = fs.readFileSync(path.join(SRC_ROOT, 'extension.ts'), 'utf8');
    expect(extensionSource).toContain('documentSelector: perlDocumentSelector()');
    expect(extensionSource).not.toMatch(/language:\s*'perl5?'/);
  });

  test('production sources route language identity through the authority (no scattered equality)', () => {
    const scattered =
      /\.languageId\s*(===|!==)\s*'(?:perl5?)'|'(?:perl5?)'\s*(===|!==)\s*[\w.]*languageId/;
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
