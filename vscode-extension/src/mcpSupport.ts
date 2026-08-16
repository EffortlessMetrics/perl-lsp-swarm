import type * as vscode from 'vscode';

/**
 * The historical generic MCP passthrough is deliberately disabled.
 *
 * `perl-lsp.mcp.servers` allowed arbitrary local commands to be published under
 * the Perl extension's identity even though perl-lsp did not own those server
 * schemas or processes. Public-beta compatibility does not justify preserving
 * that execution surface. A future first-party MCP integration must be a
 * separately reviewed `perllsp mcp --stdio` contract.
 */
export const GENERIC_MCP_PASSTHROUGH_ENABLED = false as const;

export function registerMcpSupport(
  outputChannel: Pick<vscode.OutputChannel, 'appendLine'>,
): vscode.Disposable | undefined {
  outputChannel.appendLine(
    '[mcp] Generic configured-command MCP passthrough is disabled; no MCP process definitions were registered.',
  );
  return undefined;
}
