import * as fs from 'fs';
import * as path from 'path';

const extensionRoot = path.resolve(__dirname, '../..');
const packageJson = JSON.parse(
  fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'),
) as Record<string, unknown>;
const extensionSource = fs.readFileSync(path.join(extensionRoot, 'src/extension.ts'), 'utf8');

function configurationProperties(): Record<string, unknown> {
  const contributes = packageJson.contributes as Record<string, unknown> | undefined;
  const configuration = contributes?.configuration;
  const groups = Array.isArray(configuration) ? configuration : [configuration];
  const properties: Record<string, unknown> = {};
  for (const group of groups) {
    if (!group || typeof group !== 'object') {
      continue;
    }
    const groupProperties = (group as { properties?: Record<string, unknown> }).properties ?? {};
    for (const [key, value] of Object.entries(groupProperties)) {
      properties[key] = value;
    }
  }
  return properties;
}

describe('generic MCP passthrough complete removal (#7119)', () => {
  test('active manifest has no MCP server definition provider contribution', () => {
    const contributes = packageJson.contributes as Record<string, unknown> | undefined;
    expect(contributes?.mcpServerDefinitionProviders).toBeUndefined();
  });

  test('active manifest has no perl-lsp.mcp.servers setting definition', () => {
    expect(configurationProperties()['perl-lsp.mcp.servers']).toBeUndefined();
  });

  test('does not ship a generic MCP implementation module', () => {
    expect(fs.existsSync(path.join(extensionRoot, 'src/mcpSupport.ts'))).toBe(false);
  });

  test('extension activation does not register generic MCP support or measure an mcp domain', () => {
    expect(extensionSource).not.toMatch(/registerMcpSupport/);
    expect(extensionSource).not.toMatch(/measure\(\s*['"]mcp['"]/);
    expect(extensionSource).not.toMatch(/from '\.\/mcpSupport'/);
  });

  test('recurrence: reintroducing the provider contribution fails the absence contract', () => {
    const contributes = packageJson.contributes as Record<string, unknown> | undefined;
    const providers = contributes?.mcpServerDefinitionProviders;
    if (Array.isArray(providers) && providers.length > 0) {
      throw new Error('generic MCP provider contribution was reintroduced');
    }
    expect(providers).toBeUndefined();
  });
});
