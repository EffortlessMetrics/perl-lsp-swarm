import type { ChildProcess } from 'node:child_process';
import { LanguageClient } from 'vscode-languageclient/node';

/**
 * Keeps the process created for this client generation observable after
 * vscode-languageclient clears its private handle during shutdown.
 *
 * The lifecycle uses this getter only as a stop witness. Launch options and
 * restart ownership remain in LanguageClient and LanguageClientLifecycle.
 */
export class ProcessBoundLanguageClient extends LanguageClient {
  private retainedServerProcess: ChildProcess | undefined;

  protected override async createMessageTransports(encoding: string) {
    try {
      return await super.createMessageTransports(encoding);
    } finally {
      this.retainedServerProcess = super.serverProcess;
    }
  }

  override get serverProcess(): ChildProcess | undefined {
    return this.retainedServerProcess ?? super.serverProcess;
  }
}
