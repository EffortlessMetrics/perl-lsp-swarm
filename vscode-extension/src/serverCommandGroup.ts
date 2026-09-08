import * as vscode from 'vscode';
import { HealthCheckStatus, type HealthCheckResult } from './onboarding';
import type { LanguageServerRuntimeSnapshot } from './languageServerRuntimeHealth';
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
  /** Runtime proof supplied by the lifecycle owner after implicit startup. */
  readonly runtimeHealthCheck: (resolvedPath: string | null) => HealthCheckResult;
  /** Read-only lifecycle identity used to reject stale setup probe results. */
  readonly currentRuntimeSnapshot: () => LanguageServerRuntimeSnapshot;
  /** Report only a known matching runtime failure for explicit-path diagnostics. */
  readonly runtimeFailureCheck?: (requestedPath: string | null) => HealthCheckResult | undefined;
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
      const initialSnapshot =
        serverPath === undefined ? dependencies.currentRuntimeSnapshot() : null;
      const firstProbePath =
        serverPath === undefined ? initialSnapshot!.serverPath : resolvedPath;
      let results = [...(await dependencies.runHealthCheck(firstProbePath))];
      let runtimeResult: HealthCheckResult | undefined;
      if (serverPath === undefined) {
        const afterProbeSnapshot = dependencies.currentRuntimeSnapshot();
        if (
          initialSnapshot?.generation !== afterProbeSnapshot.generation ||
          initialSnapshot?.serverPath !== afterProbeSnapshot.serverPath
        ) {
          results = [...(await dependencies.runHealthCheck(afterProbeSnapshot.serverPath))];
          const finalSnapshot = dependencies.currentRuntimeSnapshot();
          if (
            afterProbeSnapshot.generation !== finalSnapshot.generation ||
            afterProbeSnapshot.serverPath !== finalSnapshot.serverPath
          ) {
            results = [];
            runtimeResult = {
              label: 'LSP runtime',
              ok: false,
              status: HealthCheckStatus.Error,
              detail:
                'The language server changed during the health check. Run Health Check again.',
            };
          } else {
            runtimeResult = dependencies.runtimeHealthCheck?.(finalSnapshot.serverPath);
          }
        } else {
          runtimeResult = dependencies.runtimeHealthCheck?.(afterProbeSnapshot.serverPath);
        }
      } else {
        runtimeResult = dependencies.runtimeFailureCheck?.(resolvedPath);
      }
      if (serverPath === undefined && runtimeResult === undefined) {
        runtimeResult = {
          label: 'LSP runtime',
          ok: false,
          status: HealthCheckStatus.Error,
          detail: 'Runtime health evidence is unavailable.',
        };
      }
      if (runtimeResult) {
        results.push(runtimeResult);
      }
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
        const successMessage =
          serverPath === undefined
            ? 'Perl LSP health check passed.'
            : 'Perl LSP setup check passed.';
        void vscode.window
          .showInformationMessage(successMessage, 'Show Output')
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
