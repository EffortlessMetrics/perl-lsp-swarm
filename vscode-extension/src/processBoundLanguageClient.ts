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

  // vscode-languageclient may invoke stop without awaiting it; observe that rejection while returning the same promise so the lifecycle still owns cleanup failure handling.
  override stop(timeout?: number): Promise<void> {
    const operation = super.stop(timeout);
    void operation.catch(() => undefined);
    return operation;
  }

  protected override async createMessageTransports(encoding: string) {
    try {
      return await super.createMessageTransports(encoding);
    } finally {
      this.retainedServerProcess = super.serverProcess ?? this.retainedServerProcess;
    }
  }

  override get serverProcess(): ChildProcess | undefined {
    return this.retainedServerProcess ?? super.serverProcess;
  }
}
