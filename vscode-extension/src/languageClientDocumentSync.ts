import { lspAttachmentEligible } from './documentEligibility';
import { isPerlLanguageId } from './languageIdentity';

export interface OpenTextDocumentSnapshot {
  readonly uri: string;
  readonly languageId: string;
  readonly version: number;
  readonly text: string;
  readonly fileName?: string;
}

/** v0.18 selected text/position envelope (#8129). vscode-languageclient follows advertised Full + UTF-16. */
export const V0_18_TEXT_SYNC_ENVELOPE = {
  decision: 'full_document_utf16',
  textSyncKind: 'Full',
  positionEncoding: 'utf-16',
} as const;

export interface TextDocumentNotificationClient {
  sendNotification(method: string, params: unknown): Promise<void>;
}

export interface ReadyTextDocumentNotificationClient extends TextDocumentNotificationClient {
  readonly state: number;
  onDidChangeState(listener: (event: { readonly newState: number }) => void): { dispose(): void };
}

export class StaleDocumentReplayError extends Error {
  constructor() {
    super('Language-client generation became stale during open-document replay.');
    this.name = 'StaleDocumentReplayError';
  }
}

async function waitForClientState(
  client: ReadyTextDocumentNotificationClient,
  runningState: number,
  timeoutMs: number,
): Promise<void> {
  if (client.state === runningState) {
    return;
  }

  await new Promise<void>((resolve, reject) => {
    let subscription: { dispose(): void } | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let settled = false;
    const finish = (error?: Error): void => {
      if (settled) {
        return;
      }
      settled = true;
      if (timeout !== undefined) {
        clearTimeout(timeout);
      }
      subscription?.dispose();
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    };
    subscription = client.onDidChangeState((event) => {
      if (event.newState !== runningState) {
        return;
      }
      finish();
    });
    timeout = setTimeout(
      () => finish(new Error(`Language client did not reach Running within ${timeoutMs}ms.`)),
      timeoutMs,
    );
    // Close the registration race if Running arrived between the first state
    // read and listener installation.
    if (client.state === runningState) {
      finish();
    }
  });
}

function assertCurrentGeneration(isCurrent: () => boolean): void {
  if (!isCurrent()) {
    throw new StaleDocumentReplayError();
  }
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
function snapshotEligible(document: OpenTextDocumentSnapshot): boolean {
  if (!isPerlLanguageId(document.languageId)) {
    return false;
  }
  const fileName = document.fileName ?? documentUriFileName(document.uri);
  return lspAttachmentEligible({
    uri: { scheme: documentUriScheme(document.uri), fsPath: documentUriFsPath(document.uri) },
    languageId: document.languageId,
    fileName,
  });
}

function documentUriScheme(uri: string): string {
  const match = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(uri);
  return match?.[1] ?? 'file';
}

function documentUriFsPath(uri: string): string {
  try {
    const url = new URL(uri);
    if (url.protocol !== 'file:') {
      return uri;
    }
    let fsPath = decodeURIComponent(url.pathname);
    if (/^\/[A-Za-z]:\//.test(fsPath)) {
      fsPath = fsPath.slice(1);
    }
    return fsPath;
  } catch {
    return uri;
  }
}

function documentUriFileName(uri: string): string {
  const fsPath = documentUriFsPath(uri);
  const slash = fsPath.lastIndexOf('/');
  const backslash = fsPath.lastIndexOf('\\');
  const index = Math.max(slash, backslash);
  return index >= 0 ? fsPath.slice(index + 1) : fsPath;
}

export async function replayOpenPerlDocuments(
  client: TextDocumentNotificationClient,
  documents: readonly OpenTextDocumentSnapshot[],
): Promise<void> {
  for (const document of documents) {
    if (!snapshotEligible(document)) {
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

/** Wait for observable client readiness, then replay only while this generation owns the client. */
export async function replayOpenPerlDocumentsWhenReady(
  client: ReadyTextDocumentNotificationClient,
  documents: readonly OpenTextDocumentSnapshot[],
  runningState: number,
  isCurrent: () => boolean,
  timeoutMs: number,
): Promise<void> {
  assertCurrentGeneration(isCurrent);
  await waitForClientState(client, runningState, timeoutMs);
  assertCurrentGeneration(isCurrent);

  for (const document of documents) {
    if (!snapshotEligible(document)) {
      continue;
    }
    assertCurrentGeneration(isCurrent);
    await replayOpenPerlDocuments(client, [document]);
    assertCurrentGeneration(isCurrent);
  }
}
