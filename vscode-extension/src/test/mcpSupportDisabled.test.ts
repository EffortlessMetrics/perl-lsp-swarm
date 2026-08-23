import { GENERIC_MCP_PASSTHROUGH_ENABLED, registerMcpSupport } from '../mcpSupport';

describe('generic MCP passthrough removal', () => {
  test('keeps the historical arbitrary-command surface disabled', () => {
    expect(GENERIC_MCP_PASSTHROUGH_ENABLED).toBe(false);
  });

  test('registers no MCP provider or process definition', () => {
    const outputChannel = {
      appendLine: jest.fn(),
    };

    const disposable = registerMcpSupport(outputChannel);

    expect(disposable).toBeUndefined();
    expect(outputChannel.appendLine).toHaveBeenCalledWith(
      '[mcp] Generic configured-command MCP passthrough is disabled; no MCP process definitions were registered.',
    );
  });

  test('does not require configuration, command, cwd, env, or VS Code MCP API input', () => {
    const outputChannel = {
      appendLine: jest.fn(),
    };

    expect(() => registerMcpSupport(outputChannel)).not.toThrow();
    expect(Object.keys(outputChannel)).toEqual(['appendLine']);
  });
});
