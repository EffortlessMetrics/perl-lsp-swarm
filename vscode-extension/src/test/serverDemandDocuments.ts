/**
 * Shared mock documents for the server-demand contract (#8180).
 *
 * Three suites need to put an eligible Perl buffer in front of the extension so
 * activation carries real demand. They previously each built their own mock,
 * which mirrored `isServerDependentDocument` in three places: a change to the
 * eligible language ids or uri schemes needed three edits, and a copy that fell
 * behind would keep passing for the wrong reason.
 */

import * as vscode from 'vscode';

export interface FakeDocument {
  readonly languageId: string;
  readonly uri: { readonly scheme: string; toString(): string };
  readonly version: number;
  getText(): string;
}

/** Build one document with an explicit language id and uri scheme. */
export function fakeDocument(languageId: string, scheme = 'file'): FakeDocument {
  return {
    languageId,
    uri: { scheme, toString: () => `${scheme}:///workspace/demo` },
    version: 1,
    getText: () => '',
  };
}

/** Replace the mock workspace's open documents. */
export function setOpenDocuments(documents: readonly FakeDocument[]): void {
  (vscode.workspace as unknown as { textDocuments: readonly FakeDocument[] }).textDocuments =
    documents;
}

/**
 * Install one open Perl document and return a restore function.
 *
 * Callers must restore in `afterEach` or a `finally` block: a mock document left
 * installed gives every later test in the file unintended server demand, so one
 * failure cascades into unrelated ones.
 */
export function openPerlDocument(): () => void {
  const workspaceMock = vscode.workspace as unknown as { textDocuments: readonly FakeDocument[] };
  const original = workspaceMock.textDocuments;
  setOpenDocuments([fakeDocument('perl')]);
  return () => {
    workspaceMock.textDocuments = original;
  };
}
