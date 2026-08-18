import * as vscode from 'vscode';
import type { HealthCheckResult } from './onboarding';
import type {
  HealthCheckCommandResult,
  HealthCheckCommandStatus,
  ReinstallCommandResult,
} from './commandResults';
import { SHOW_BINARY_IDENTITY_COMMAND } from './binaryIdentityCommand';

/**
 * Read-only and callback-based dependencies for server-facing commands.
 *
 * The command group does not read entry-module globals or own lifecycle
 * transitions. `currentServerPath` is a projection, while restart, reinstall,
 * health probing, and binary-identity presentation remain owned by the
 * composition layer that supplies them.
 */
export interface ServerCommandContext {
  readonly outputChannel: vscode.LogOutputChannel;
  readonly currentServerPath: () => string | null;
  /** Resolve the managed path when startup is still in progress. */
  readonly resolveServerPath: () => Promise<string | null>;
  readonly reinstallServerBinary: () => Promise<ReinstallCommandResult>;
  readonly restartServer: () => Promise<void>;
  readonly runHealthCheck: (serverPath: string | null) => Promise<HealthCheckResult[]>;
  /** Optional until the negotiated identity protocol is available in composition. */
  readonly showBinaryIdentity?: () => Promise<unknown>;
}

function toHealthCheckCommandResult(results: HealthCheckResult[]): HealthCheckCommandResult {
  const checks = results.map((result) => ({
    label: result.label,
    status: result.status as HealthCheckCommandStatus,
    detail: result.detail,
  }));

  return {
    ok: checks.every((check) => check.status !== 'error'),
    checks,
  };
}

/**
 * Register commands concerned with server availability and installation.
 *
 * The returned disposables belong to the caller's extension context. Keeping
 * registration here makes the command surface independently testable and
 * gives later command groups the same explicit composition seam.
 */
export function registerServerCommandGroup(
  dependencies: ServerCommandContext,
): vscode.Disposable[] {
  const showOutputCommand = vscode.commands.registerCommand('perl-lsp.showOutput', () => {
    dependencies.outputChannel.show();
  });

  const reinstallCommand = vscode.commands.registerCommand('perl-lsp.reinstall', async () => {
    return dependencies.reinstallServerBinary();
  });

  const restartCommand = vscode.commands.registerCommand('perl-lsp.restart', async () => {
    await dependencies.restartServer();
  });

  const showBinaryIdentityCommand = vscode.commands.registerCommand(
    SHOW_BINARY_IDENTITY_COMMAND,
    async () => {
      if (dependencies.showBinaryIdentity === undefined) {
        await vscode.window.showInformationMessage(
          'Binary identity is unavailable until the running server negotiates the identity feature.',
        );
        return { status: 'unsupported' as const };
      }
      return dependencies.showBinaryIdentity();
    },
  );

  const runHealthCheckCommand = vscode.commands.registerCommand(
    'perl-lsp.runHealthCheck',
    async (serverPath?: string | null) => {
      const resolvedPath =
        serverPath !== undefined ? serverPath : await dependencies.resolveServerPath();
      const results = await dependencies.runHealthCheck(resolvedPath);
      const commandResult = toHealthCheckCommandResult(results);

      const errors = results.filter((result) => !result.ok && result.status === 'error');
      const warnings = results.filter((result) => !result.ok && result.status === 'warning');

      const lines = results.map((result) => {
        const icon = result.ok
          ? '$(check)'
          : result.status === 'warning'
            ? '$(warning)'
            : '$(error)';
        return `${icon} ${result.label}: ${result.detail}`;
      });

      dependencies.outputChannel.appendLine('[health-check] Results:');
      for (const line of lines) {
        dependencies.outputChannel.appendLine(`  ${line.replace(/\$\(\w[^)]*\)/g, '')}`);
      }

      if (errors.length > 0) {
        const message = `Health check failed: ${errors.map((error) => error.label).join(', ')}`;
        void vscode.window.showErrorMessage(message, 'Show Output').then((selection) => {
          if (selection === 'Show Output') {
            dependencies.outputChannel.show();
          }
        });
      } else if (warnings.length > 0) {
        const message = `Health check passed with warnings: ${warnings
          .map((warning) => warning.detail)
          .join(' | ')}`;
        void vscode.window.showWarningMessage(message, 'Show Output').then((selection) => {
          if (selection === 'Show Output') {
            dependencies.outputChannel.show();
          }
        });
      } else {
        void vscode.window
          .showInformationMessage('Perl LSP health check passed.', 'Show Output')
          .then((selection) => {
            if (selection === 'Show Output') {
              dependencies.outputChannel.show();
            }
          });
      }

      return commandResult;
    },
  );

  return [
    showOutputCommand,
    reinstallCommand,
    restartCommand,
    showBinaryIdentityCommand,
    runHealthCheckCommand,
  ];
}
