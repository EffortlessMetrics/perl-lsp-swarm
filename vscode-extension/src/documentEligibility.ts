import {
  classifyDocumentFamily,
  type DocumentFamilyInput,
  type DocumentFamilyRow,
  DOCUMENT_FAMILY_REGISTRY,
  type LspAttachmentLevel,
} from './documentFamily';
import {
  isPerlLanguageId,
  PERL_ALIAS_LANGUAGE_ID,
  type PerlDocumentSelectorEntry,
  SUPPORTED_PERL_LANGUAGE_IDS,
  SUPPORTED_PERL_URI_SCHEMES,
} from './languageIdentity';

export type DocumentEligibilityInput = DocumentFamilyInput;

function familyFor(document: DocumentEligibilityInput): DocumentFamilyRow {
  return classifyDocumentFamily(document);
}

function ceilingAllows(ceiling: DocumentFamilyRow['editProviders']): boolean {
  return ceiling === 'allowed' || ceiling === 'bounded';
}

/** Whether the language client should attach and synchronize this document. */
export function lspAttachmentEligible(document: DocumentEligibilityInput): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  const attachment = familyFor(document).lspAttachment;
  return attachment === 'full' || attachment === 'bounded';
}

/** Whether read/LSP-backed providers may run for this document. */
export function readProviderEligible(document: DocumentEligibilityInput): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  return ceilingAllows(familyFor(document).readProviders);
}

/** Whether edit-producing extension or LSP edit providers may run. */
export function editProviderEligible(document: DocumentEligibilityInput): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  return familyFor(document).editProviders === 'allowed';
}

/** Whether local `perl` / `prove` style commands may run. */
export function externalExecutionEligible(document: DocumentEligibilityInput): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  return familyFor(document).externalExecution === 'allowed';
}

/** Whether debug prompts, breakpoints, and test explorer actions may run. */
export function debugTestEligible(document: DocumentEligibilityInput): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  return familyFor(document).debugTest === 'allowed';
}

function globPatternForExtension(extension: string): string {
  const normalized = extension.startsWith('.') ? extension : `.${extension}`;
  return `**/*${normalized}`;
}

function globPatternForFilename(filename: string): string {
  return `**/${filename}`;
}

function selectorEntriesForAttachment(level: LspAttachmentLevel): PerlDocumentSelectorEntry[] {
  const entries: PerlDocumentSelectorEntry[] = [];
  for (const scheme of SUPPORTED_PERL_URI_SCHEMES) {
    if (scheme === 'untitled') {
      for (const language of SUPPORTED_PERL_LANGUAGE_IDS) {
        entries.push({ scheme, language });
      }
      continue;
    }
    for (const row of DOCUMENT_FAMILY_REGISTRY) {
      if (row.lspAttachment !== level) {
        continue;
      }
      for (const extension of row.extensions ?? []) {
        for (const language of SUPPORTED_PERL_LANGUAGE_IDS) {
          entries.push({
            scheme,
            language,
            pattern: globPatternForExtension(extension),
          });
        }
      }
      for (const filename of row.exactFilenames ?? []) {
        for (const language of SUPPORTED_PERL_LANGUAGE_IDS) {
          entries.push({
            scheme,
            language,
            pattern: globPatternForFilename(filename),
          });
        }
      }
    }
  }
  return entries;
}

/**
 * Narrow LanguageClient attachment: syntax-highlighted Perl-associated families
 * that are not LSP-eligible do not match this selector (#14450).
 */
export function perlFullLspDocumentSelector(): PerlDocumentSelectorEntry[] {
  const entries = [
    ...selectorEntriesForAttachment('full'),
    ...selectorEntriesForAttachment('bounded'),
  ];
  // #7699: `perl5` is never manifest syntax-associated; alias buffers are classified
  // explicitly and must not be dropped by path-only patterns (#14450 + #7699).
  for (const scheme of SUPPORTED_PERL_URI_SCHEMES) {
    if (scheme === 'untitled') {
      continue;
    }
    entries.push({ scheme, language: PERL_ALIAS_LANGUAGE_ID });
  }
  return entries;
}

/** Human-readable refusal when an edit action is unavailable for the document family. */
export function editRefusalMessage(document: DocumentEligibilityInput): string {
  const family = familyFor(document);
  return (
    `This command is not available for ${family.id} documents ` +
    `(disposition: ${family.disposition}).`
  );
}

/** Human-readable refusal when a read/LSP action is unavailable for the document family. */
export function readRefusalMessage(document: DocumentEligibilityInput): string {
  return editRefusalMessage(document);
}
