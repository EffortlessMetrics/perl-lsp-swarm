import type * as vscode from 'vscode';

/** Document identity captured when a provider middleware call begins. */
export interface ProviderRequestSubject {
  readonly uri: string;
  readonly version: number;
}

export function captureProviderRequestSubject(
  document: vscode.TextDocument,
): ProviderRequestSubject {
  return {
    uri: document.uri.toString(),
    version: document.version,
  };
}

/** True when the live document still matches the captured request subject. */
export function isProviderRequestSubjectCurrent(
  subject: ProviderRequestSubject,
  document: vscode.TextDocument,
): boolean {
  return subject.uri === document.uri.toString() && subject.version === document.version;
}
