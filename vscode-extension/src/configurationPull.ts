import * as vscode from 'vscode';
import { buildLanguageClientConfigurationPayload } from './languageClientConfiguration';

/**
 * Folder-owned answers for the server's `workspace/configuration` pull (#14447).
 *
 * The server asks for one unscoped item plus one item per workspace folder,
 * every one of them `section: "perl"`:
 *
 * ```rust
 * let mut items = vec![json!({ "section": "perl" })];
 * items.extend(folder_uris.iter().map(|uri| json!({ "scopeUri": uri, "section": "perl" })));
 * ```
 *
 * `vscode-languageclient`'s default handler resolves a dotless section as
 * `workspace.getConfiguration(undefined, resource).get('perl')` — that is, the
 * `perl.*` VS Code namespace. This extension contributes its settings under
 * `perl-lsp.*`, so without an adapter every item resolves to `null` and the
 * server's per-folder configuration authority never observes a user value. The
 * only values that reached it came from the unscoped `didChangeConfiguration`
 * push, which the server necessarily applies session-globally.
 *
 * This module is that adapter: it answers each requested item, in order, from
 * the `perl-lsp` namespace resolved at the item's own `scopeUri`, so folder A's
 * values cannot answer for folder B.
 */

/** The section name the Perl language server requests. */
export const PERL_CONFIGURATION_SECTION = 'perl';

/**
 * Structurally identical to `vscode-languageserver-protocol`'s
 * `ConfigurationItem`, restated locally so this module stays testable without
 * standing up a language client.
 */
export type ConfigurationItemLike = {
  scopeUri?: string;
  section?: string;
};

export type ConfigurationParamsLike = {
  items: ConfigurationItemLike[];
};

/**
 * The value of the `perl` section for one scope.
 *
 * Returns the *unwrapped* section content (`{ workspace, critic, perlcritic }`),
 * which is what `apply_workspace_config_layer` on the server consumes for a
 * `workspace/configuration` result. This deliberately reuses the same builder
 * as the `didChangeConfiguration` push so pull and push cannot describe the
 * same scope differently.
 */
export function buildPerlSectionValue(scope?: vscode.Uri): Record<string, unknown> {
  const payload = buildLanguageClientConfigurationPayload(scope);
  const settings = payload.settings as { perl?: Record<string, unknown> } | undefined;
  return settings?.perl ?? {};
}

function parseScopeUri(scopeUri: string | undefined): vscode.Uri | undefined {
  if (typeof scopeUri !== 'string' || scopeUri.length === 0) {
    return undefined;
  }

  try {
    return vscode.Uri.parse(scopeUri);
  } catch {
    // A scopeUri we cannot parse must not silently borrow another folder's
    // configuration; fall back to the unscoped view for that item only.
    return undefined;
  }
}

function isPerlItem(item: ConfigurationItemLike): boolean {
  return item.section === PERL_CONFIGURATION_SECTION;
}

/**
 * Answer a `workspace/configuration` request.
 *
 * Every `section: "perl"` item is answered from the `perl-lsp` namespace at
 * that item's scope. Items for any other section are delegated to `next` (the
 * language client's default resolution) and spliced back at their original
 * index, so ordering and arity of the response always match the request.
 */
export async function resolvePerlConfiguration<T>(
  params: ConfigurationParamsLike,
  token: T,
  next: (params: ConfigurationParamsLike, token: T) => unknown,
): Promise<unknown[]> {
  const items = params.items ?? [];
  const result: unknown[] = new Array(items.length).fill(null);

  const delegatedIndexes: number[] = [];
  for (const [index, item] of items.entries()) {
    if (isPerlItem(item)) {
      result[index] = buildPerlSectionValue(parseScopeUri(item.scopeUri));
    } else {
      delegatedIndexes.push(index);
    }
  }

  if (delegatedIndexes.length > 0) {
    const delegated = await next(params, token);
    if (Array.isArray(delegated)) {
      for (const index of delegatedIndexes) {
        result[index] = delegated[index] ?? null;
      }
    }
  }

  return result;
}

/**
 * `middleware.workspace.configuration` for the Perl language client.
 *
 * Installed in `extension.ts` so the server's per-folder pull is answered from
 * the contributed `perl-lsp.*` namespace rather than the unrelated `perl.*` one.
 */
export function perlConfigurationMiddleware() {
  return <T>(
    params: ConfigurationParamsLike,
    token: T,
    next: (params: ConfigurationParamsLike, token: T) => unknown,
  ): Promise<unknown[]> => resolvePerlConfiguration(params, token, next);
}
