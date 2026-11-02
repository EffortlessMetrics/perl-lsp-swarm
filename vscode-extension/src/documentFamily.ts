import * as path from 'path';

import { isPerlLanguageId, isSupportedPerlUriScheme } from './languageIdentity';

/** Product disposition for a VS Code Perl-associated document family (#14450). */
export type DocumentFamilyDisposition =
  | 'plain_perl_full_lsp'
  | 'perl_dsl_bounded_lsp'
  | 'embedded_perl_regions'
  | 'syntax_only'
  | 'documentation_only'
  | 'other_language_owner'
  | 'unsupported'
  | 'not_proven';

export type LspAttachmentLevel = 'full' | 'bounded' | 'none';

export type CapabilityCeiling = 'allowed' | 'bounded' | 'refused';

/** Stable family id for registry rows and proof fixtures. */
export type DocumentFamilyId =
  | 'plain_perl'
  | 'extensionless_shebang_perl'
  | 'untitled_perl'
  | 'manual_language_perl'
  | 'pod'
  | 'xs_boundary'
  | 'include_generated'
  | 'template'
  | 'perl_build_dsl'
  | 'cpan_dist_config';

export interface DocumentFamilyRow {
  readonly id: DocumentFamilyId;
  readonly extensions?: readonly string[];
  readonly exactFilenames?: readonly string[];
  readonly disposition: DocumentFamilyDisposition;
  readonly lspAttachment: LspAttachmentLevel;
  readonly readProviders: CapabilityCeiling;
  readonly editProviders: CapabilityCeiling;
  readonly externalExecution: CapabilityCeiling;
  readonly debugTest: CapabilityCeiling;
}

/**
 * Canonical VS Code document-family registry (#14450).
 *
 * Syntax associations in `package.json` remain the highlighting source; this
 * table owns LSP/read/edit/external-process/debug ceilings. Dispositions marked
 * `not_proven` in the issue tracker are resolved here with conservative,
 * documented defaults and can be revised without changing consumer call sites.
 */
export const DOCUMENT_FAMILY_REGISTRY: readonly DocumentFamilyRow[] = [
  {
    id: 'plain_perl',
    extensions: ['.pl', '.PL', '.pm', '.t', '.psgi', '.cgi', '.fcgi'],
    disposition: 'plain_perl_full_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'allowed',
    externalExecution: 'allowed',
    debugTest: 'allowed',
  },
  {
    id: 'extensionless_shebang_perl',
    disposition: 'plain_perl_full_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'allowed',
    externalExecution: 'allowed',
    debugTest: 'allowed',
  },
  {
    id: 'untitled_perl',
    disposition: 'plain_perl_full_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'allowed',
    externalExecution: 'allowed',
    debugTest: 'allowed',
  },
  {
    id: 'manual_language_perl',
    disposition: 'plain_perl_full_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'allowed',
    externalExecution: 'allowed',
    debugTest: 'allowed',
  },
  {
    id: 'perl_build_dsl',
    exactFilenames: ['Makefile.PL', 'Build.PL'],
    disposition: 'plain_perl_full_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'allowed',
    externalExecution: 'allowed',
    debugTest: 'allowed',
  },
  {
    id: 'cpan_dist_config',
    exactFilenames: ['cpanfile', 'cpanfile.snapshot', 'dist.ini'],
    disposition: 'perl_dsl_bounded_lsp',
    lspAttachment: 'full',
    readProviders: 'allowed',
    editProviders: 'refused',
    externalExecution: 'refused',
    debugTest: 'refused',
  },
  {
    id: 'pod',
    extensions: ['.pod'],
    disposition: 'documentation_only',
    lspAttachment: 'none',
    readProviders: 'refused',
    editProviders: 'refused',
    externalExecution: 'refused',
    debugTest: 'refused',
  },
  {
    id: 'xs_boundary',
    extensions: ['.xs', '.xsi'],
    disposition: 'other_language_owner',
    lspAttachment: 'none',
    readProviders: 'refused',
    editProviders: 'refused',
    externalExecution: 'refused',
    debugTest: 'refused',
  },
  {
    id: 'include_generated',
    extensions: ['.i'],
    disposition: 'syntax_only',
    lspAttachment: 'none',
    readProviders: 'refused',
    editProviders: 'refused',
    externalExecution: 'refused',
    debugTest: 'refused',
  },
  {
    id: 'template',
    extensions: ['.mason', '.mas', '.tt', '.tt2', '.ep'],
    disposition: 'embedded_perl_regions',
    lspAttachment: 'none',
    readProviders: 'refused',
    editProviders: 'refused',
    externalExecution: 'refused',
    debugTest: 'refused',
  },
] as const;

const EXTENSION_TO_ROW: ReadonlyMap<string, DocumentFamilyRow> = buildExtensionIndex();
const FILENAME_TO_ROW: ReadonlyMap<string, DocumentFamilyRow> = buildFilenameIndex();

function buildExtensionIndex(): ReadonlyMap<string, DocumentFamilyRow> {
  const map = new Map<string, DocumentFamilyRow>();
  for (const row of DOCUMENT_FAMILY_REGISTRY) {
    for (const extension of row.extensions ?? []) {
      map.set(extension.toLowerCase(), row);
    }
  }
  return map;
}

function buildFilenameIndex(): ReadonlyMap<string, DocumentFamilyRow> {
  const map = new Map<string, DocumentFamilyRow>();
  for (const row of DOCUMENT_FAMILY_REGISTRY) {
    for (const filename of row.exactFilenames ?? []) {
      map.set(filename, row);
    }
  }
  return map;
}

export interface DocumentFamilyInput {
  readonly uri?: { readonly scheme?: string; readonly fsPath?: string; readonly toString?: () => string };
  readonly languageId: string;
  readonly fileName?: string;
}

function manifestAssociatedFilename(fileName: string): DocumentFamilyRow | undefined {
  return FILENAME_TO_ROW.get(path.basename(fileName));
}

/**
 * Classify a Perl-language buffer into one registry row using path/family facts,
 * not language ID alone (#14450).
 */
function documentUriScheme(document: DocumentFamilyInput): string {
  const scheme = document.uri?.scheme;
  return scheme && scheme.length > 0 ? scheme : 'file';
}

function documentFileName(document: DocumentFamilyInput): string {
  const explicit = document.fileName ?? '';
  if (explicit.length > 0) {
    return explicit;
  }
  const fsPath = document.uri?.fsPath ?? '';
  if (fsPath.length > 0) {
    return path.basename(fsPath);
  }
  let uriText: string | undefined;
  if (document.uri !== undefined && typeof document.uri.toString === 'function') {
    uriText = document.uri.toString();
  }
  if (uriText?.startsWith('file://')) {
    try {
      return path.basename(decodeURIComponent(uriText.replace(/^file:\/\//, '')));
    } catch {
      return path.basename(uriText.replace(/^file:\/\//, ''));
    }
  }
  return explicit;
}

export function classifyDocumentFamily(document: DocumentFamilyInput): DocumentFamilyRow {
  if (!isPerlLanguageId(document.languageId) || !isSupportedPerlUriScheme(documentUriScheme(document))) {
    return unsupportedFamilyRow();
  }

  const fileName = documentFileName(document);

  if (documentUriScheme(document) === 'untitled') {
    return requireRow('untitled_perl');
  }

  const basename = path.basename(fileName);
  const byFilename = manifestAssociatedFilename(basename);
  if (byFilename) {
    return byFilename;
  }

  const extension = path.extname(fileName).toLowerCase();
  if (extension.length > 0) {
    const byExtension = EXTENSION_TO_ROW.get(extension);
    if (byExtension) {
      return byExtension;
    }
    return requireRow('manual_language_perl');
  }

  return requireRow('extensionless_shebang_perl');
}

function requireRow(id: DocumentFamilyId): DocumentFamilyRow {
  const row = DOCUMENT_FAMILY_REGISTRY.find((entry) => entry.id === id);
  if (!row) {
    return unsupportedFamilyRow();
  }
  return row;
}

const UNSUPPORTED_FAMILY_ROW: DocumentFamilyRow = {
  id: 'manual_language_perl',
  disposition: 'unsupported',
  lspAttachment: 'none',
  readProviders: 'refused',
  editProviders: 'refused',
  externalExecution: 'refused',
  debugTest: 'refused',
};

function unsupportedFamilyRow(): DocumentFamilyRow {
  return {
    ...UNSUPPORTED_FAMILY_ROW,
  };
}

/** Every manifest extension/filename maps to exactly one registry row. */
export function manifestAssociationFamilyIds(): {
  readonly extensions: readonly string[];
  readonly filenames: readonly string[];
} {
  const extensions: string[] = [];
  const filenames: string[] = [];
  for (const row of DOCUMENT_FAMILY_REGISTRY) {
    for (const extension of row.extensions ?? []) {
      extensions.push(extension);
    }
    for (const filename of row.exactFilenames ?? []) {
      filenames.push(filename);
    }
  }
  return { extensions, filenames };
}
