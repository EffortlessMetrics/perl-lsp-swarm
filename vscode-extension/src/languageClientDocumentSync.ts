export interface OpenTextDocumentSnapshot {
  readonly uri: string;
  readonly languageId: string;
  readonly version: number;
  readonly text: string;
}

export interface TextDocumentNotificationClient {
  sendNotification(method: string, params: unknown): Promise<void>;
}

/**
 * Replay open Perl buffers after replacing a language-client generation.
 *
 * vscode-languageclient normally registers open-document synchronization
 * asynchronously. A restart can therefore accept provider requests before
 * the new server has received didOpen for a document that was already open
 * before the old client stopped. Replaying the current snapshots makes the
 * restart contract explicit without changing readiness semantics.
 */
export async function replayOpenPerlDocuments(
  client: TextDocumentNotificationClient,
  documents: readonly OpenTextDocumentSnapshot[],
): Promise<void> {
  for (const document of documents) {
    if (document.languageId !== 'perl') {
      continue;
    }

    await client.sendNotification('textDocument/didOpen', {
      textDocument: {
        uri: document.uri,
        languageId: document.languageId,
        version: document.version,
        text: document.text,
      },
    });
  }
}
