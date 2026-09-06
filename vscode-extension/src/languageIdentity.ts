/**
 * Perl language identity for VS Code language IDs (#7699).
 *
 * VS Code language IDs and Perl file classification are different objects.
 * This package *contributes* exactly one language (`perl`, in
 * `package.json` → `contributes.languages`); `.pl`/`.pm`/`.t`/shebang Perl
 * buffers are classified by that contribution and stay classified by it. The
 * `perl5` language ID is a *supported alias*, not a contributed language: it
 * exists so buffers classified as `perl5` by some other extension's language
 * contribution, or by an explicit user classification (`files.associations`,
 * "Change Language Mode"), attach to the same single language client instead
 * of activating this extension and then being silently ignored.
 *
 * The alias boundary (issue #7699):
 *
 * - `onLanguage:perl5` remains an activation event only because client
 *   selection ({@link perlDocumentSelector}) and every extension-owned
 *   language gate ({@link isPerlLanguageId}) accept the alias. An activation
 *   event without a support disposition is forbidden; the manifest
 *   composition test fails on that drift.
 * - The alias maps onto the one canonical client, TextMate grammar, semantic
 *   model, configuration namespace, and server process. No second anything.
 * - Perl *file/path* classification (extensions, first-line shebang,
 *   filenames) is a different object and stays owned by
 *   `contributes.languages`; it must not be widened merely because two
 *   language IDs are supported.
 *
 * Every extension-owned "is this buffer Perl?" decision goes through
 * {@link isPerlLanguageId} or {@link perlDocumentSelector} rather than a
 * scattered `languageId === 'perl'` comparison; a source contract test keeps
 * the scattered form from returning.
 */

/** The language ID this package contributes and advertises. */
export const CANONICAL_PERL_LANGUAGE_ID = 'perl';

/**
 * Language IDs accepted as Perl language identity, canonical first.
 *
 * `perl5` is retained as a compatibility alias for buffers classified by
 * another extension or by an explicit user classification. This package never
 * assigns or contributes it.
 */
export const SUPPORTED_PERL_LANGUAGE_IDS: readonly string[] = [CANONICAL_PERL_LANGUAGE_ID, 'perl5'];

const SUPPORTED_PERL_LANGUAGE_ID_SET: ReadonlySet<string> = new Set(SUPPORTED_PERL_LANGUAGE_IDS);

/**
 * URI schemes the language client attaches to and the server demand boundary
 * is willing to synchronize.
 */
export const SUPPORTED_PERL_URI_SCHEMES: readonly string[] = ['file', 'untitled'];

const SUPPORTED_PERL_URI_SCHEME_SET: ReadonlySet<string> = new Set(SUPPORTED_PERL_URI_SCHEMES);

/** Whether a VS Code language ID identifies a Perl buffer for this extension. */
export function isPerlLanguageId(languageId: string | undefined): boolean {
  return languageId !== undefined && SUPPORTED_PERL_LANGUAGE_ID_SET.has(languageId);
}

/** Whether a URI scheme is one the language client attaches to. */
export function isSupportedPerlUriScheme(scheme: string): boolean {
  return SUPPORTED_PERL_URI_SCHEME_SET.has(scheme);
}

/** One `LanguageClientOptions.documentSelector` entry. */
export interface PerlDocumentSelectorEntry {
  readonly scheme: string;
  readonly language: string;
}

/**
 * The document selector for the one language client.
 *
 * Covers every supported scheme × supported language ID, so an activation
 * event (`onLanguage:perl5`) can never outpace client selection: any language
 * ID the extension activates for is a language ID the client attaches to.
 */
export function perlDocumentSelector(): PerlDocumentSelectorEntry[] {
  return SUPPORTED_PERL_URI_SCHEMES.flatMap((scheme) =>
    SUPPORTED_PERL_LANGUAGE_IDS.map((language) => ({ scheme, language })),
  );
}
