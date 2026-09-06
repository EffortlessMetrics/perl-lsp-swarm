import * as vscode from 'vscode';
import {
  classifyConfigurationChange,
  type ConfigurationChangeEventLike,
} from './languageClientConfiguration';

export type WorkspaceEventHandlers = {
  /**
   * Every configuration change, before classification.
   *
   * `classifyConfigurationChange` classifies *current* settings by the subsystem they
   * drive. A consumer whose subject is not a current setting — the legacy-setting
   * migration reader (#14966), whose keys were removed and drive no subsystem — would
   * never be reached by any class, so it observes the raw event here rather than adding a
   * second `onDidChangeConfiguration` owner.
   */
  onAnyConfigurationChanged?: (event: ConfigurationChangeEventLike) => void | Promise<void>;
  onLiveConfigurationChanged: (event: ConfigurationChangeEventLike) => void | Promise<void>;
  onReconstructConfigurationChanged: (event: ConfigurationChangeEventLike) => void | Promise<void>;
  onRestartRequired: (event: ConfigurationChangeEventLike) => void | Promise<void>;
  onError?: (error: unknown) => void;
};

function invoke(
  callback: (event: ConfigurationChangeEventLike) => void | Promise<void>,
  event: ConfigurationChangeEventLike,
  onError: ((error: unknown) => void) | undefined,
): void {
  const reportError = (error: unknown): void => {
    if (onError) {
      onError(error);
      return;
    }
    process.stderr.write(`[workspace-configuration] handler failed: ${String(error)}\n`);
  };

  try {
    Promise.resolve(callback(event)).catch((error: unknown) => {
      reportError(error);
    });
  } catch (error: unknown) {
    reportError(error);
  }
}

export function registerWorkspaceConfigurationEvents(
  handlers: WorkspaceEventHandlers,
): vscode.Disposable {
  return vscode.workspace.onDidChangeConfiguration((event) => {
    if (handlers.onAnyConfigurationChanged) {
      invoke(handlers.onAnyConfigurationChanged, event, handlers.onError);
    }
    const classes = classifyConfigurationChange(event);
    if (classes.includes('live')) {
      invoke(handlers.onLiveConfigurationChanged, event, handlers.onError);
    }
    if (classes.includes('reconstruct')) {
      invoke(handlers.onReconstructConfigurationChanged, event, handlers.onError);
    }
    if (classes.includes('restart')) {
      invoke(handlers.onRestartRequired, event, handlers.onError);
    }
  });
}
