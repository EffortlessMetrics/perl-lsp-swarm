import * as fs from 'fs';
import * as path from 'path';

import {
  classifyDocumentFamily,
  DOCUMENT_FAMILY_REGISTRY,
  manifestAssociationFamilyIds,
} from '../documentFamily';
import {
  debugTestEligible,
  editProviderEligible,
  externalExecutionEligible,
  lspAttachmentEligible,
  perlFullLspDocumentSelector,
  readProviderEligible,
} from '../documentEligibility';

function doc(
  fileName: string,
  languageId = 'perl',
  scheme = 'file',
): {
  uri: { scheme: string; fsPath: string };
  languageId: string;
  fileName: string;
} {
  return {
    uri: { scheme, fsPath: `/workspace/${fileName}` },
    languageId,
    fileName,
  };
}

describe('document family registry (#14450)', () => {
  test('covers every Perl manifest extension and filename', () => {
    const manifestPath = path.resolve(__dirname, '../../package.json');
    const packageJson = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
      contributes?: {
        languages?: Array<{ id?: string; extensions?: string[]; filenames?: string[] }>;
      };
    };
    const perlLanguage = packageJson.contributes?.languages?.find((language) => language.id === 'perl');
    expect(perlLanguage).toBeDefined();

    const { extensions, filenames } = manifestAssociationFamilyIds();
    for (const extension of perlLanguage?.extensions ?? []) {
      expect(extensions.map((entry) => entry.toLowerCase())).toContain(extension.toLowerCase());
    }
    for (const filename of perlLanguage?.filenames ?? []) {
      expect(filenames).toContain(filename);
    }
  });

  test('classifies plain Perl separately from syntax-only families', () => {
    expect(classifyDocumentFamily(doc('lib/Foo.pm')).id).toBe('plain_perl');
    expect(classifyDocumentFamily(doc('script.pl')).id).toBe('plain_perl');
    expect(classifyDocumentFamily(doc('note.pod')).id).toBe('pod');
    expect(classifyDocumentFamily(doc('typemap.xs')).id).toBe('xs_boundary');
    expect(classifyDocumentFamily(doc('inline.i')).id).toBe('include_generated');
    expect(classifyDocumentFamily(doc('view.mason')).id).toBe('template');
  });

  test('path classification survives perl5 alias identity mutation', () => {
    const pod = doc('note.pod', 'perl5');
    expect(classifyDocumentFamily(pod).id).toBe('pod');
    expect(editProviderEligible(pod)).toBe(false);
    expect(debugTestEligible(pod)).toBe(false);
  });

  test('cpan/dist exact filenames keep read LSP but refuse edit and external execution', () => {
    const cpanfile = doc('cpanfile');
    expect(classifyDocumentFamily(cpanfile).disposition).toBe('perl_dsl_bounded_lsp');
    expect(lspAttachmentEligible(cpanfile)).toBe(true);
    expect(readProviderEligible(cpanfile)).toBe(true);
    expect(editProviderEligible(cpanfile)).toBe(false);
    expect(externalExecutionEligible(cpanfile)).toBe(false);
    expect(debugTestEligible(cpanfile)).toBe(false);
  });

  test('syntax-only and documentation families do not attach full LSP', () => {
    expect(lspAttachmentEligible(doc('note.pod'))).toBe(false);
    expect(lspAttachmentEligible(doc('typemap.xs'))).toBe(false);
    expect(lspAttachmentEligible(doc('view.tt2'))).toBe(false);
  });

  test('full LSP selector excludes syntax-only extensions but keeps plain Perl', () => {
    const selector = perlFullLspDocumentSelector();
    const patterns = selector.map((entry) => entry.pattern).filter(Boolean);
    expect(patterns.some((pattern) => pattern?.includes('.pm'))).toBe(true);
    expect(patterns.some((pattern) => pattern?.includes('.pod'))).toBe(false);
    expect(patterns.some((pattern) => pattern?.includes('.tt2'))).toBe(false);
    expect(selector).toEqual(
      expect.arrayContaining([{ scheme: 'file', language: 'perl5' }]),
    );
  });

  test('registry rows stay internally consistent', () => {
    for (const row of DOCUMENT_FAMILY_REGISTRY) {
      if (row.lspAttachment === 'none') {
        expect(row.readProviders === 'refused' || row.disposition === 'documentation_only').toBe(
          true,
        );
      }
      if (row.editProviders === 'allowed') {
        expect(row.lspAttachment === 'full' || row.lspAttachment === 'bounded').toBe(true);
      }
    }
  });
});
