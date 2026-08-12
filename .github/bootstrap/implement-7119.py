from __future__ import annotations

import json
from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one replacement in {path}, found {count}: {old[:100]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


package_path = Path("vscode-extension/package.json")
package = json.loads(package_path.read_text(encoding="utf-8"))
contributes = package["contributes"]
providers = contributes.pop("mcpServerDefinitionProviders", None)
if providers != [{"id": "perl-lsp.mcp-servers", "label": "Perl LSP MCP Servers"}]:
    raise SystemExit(f"unexpected MCP provider contribution: {providers!r}")

removed_setting = 0
for section in contributes.get("configuration", []):
    properties = section.get("properties", {})
    if "perl-lsp.mcp.servers" in properties:
        del properties["perl-lsp.mcp.servers"]
        removed_setting += 1
if removed_setting != 1:
    raise SystemExit(f"expected one perl-lsp.mcp.servers setting, removed {removed_setting}")

scripts = package["scripts"]
scripts["check:no-generic-mcp"] = "node scripts/check-no-generic-mcp-surface.js"
prepublish = scripts["vscode:prepublish"]
if "check:no-generic-mcp" in prepublish:
    raise SystemExit("prepublish already includes no-generic-MCP guard")
scripts["vscode:prepublish"] = f"{prepublish} && npm run check:no-generic-mcp"
summary = scripts["test:receipt-summary"]
if "check-no-generic-mcp-surface" in summary:
    raise SystemExit("receipt-summary already includes no-generic-MCP guard")
scripts["test:receipt-summary"] = (
    f"{summary} && node --test scripts/check-no-generic-mcp-surface.test.js "
    "&& node scripts/check-no-generic-mcp-surface.js"
)
package_path.write_text(json.dumps(package, indent=2) + "\n", encoding="utf-8")

extension = Path("vscode-extension/src/extension.ts")
replace_once(extension, "import { registerMcpSupport } from './mcpSupport';\n", "")
replace_once(
    extension,
    """  const mcpDisposable = featureActivationMetrics.measure('mcp', true, () =>
    registerMcpSupport(outputChannel),
  );
""",
    "",
)
replace_once(extension, "    ...(mcpDisposable ? [mcpDisposable] : []),\n", "")

metrics = Path("vscode-extension/src/featureActivationMetrics.ts")
replace_once(metrics, "  | 'mcp'\n", "")

metrics_test = Path("vscode-extension/src/test/featureActivationMetrics.test.ts")
replace_once(metrics_test, "      metrics.measure('mcp', false, () => {", "      metrics.measure('configuration', false, () => {")
replace_once(metrics_test, "      domain: 'mcp',", "      domain: 'configuration',")

mcp_source = Path("vscode-extension/src/mcpSupport.ts")
if not mcp_source.is_file():
    raise SystemExit("generic MCP provider source is already absent")
mcp_source.unlink()

readme = Path("vscode-extension/README.md")
readme_lines = readme.read_text(encoding="utf-8").splitlines()
filtered = [line for line in readme_lines if "`perl-lsp.mcp.servers`" not in line]
if len(filtered) != len(readme_lines) - 1:
    raise SystemExit("expected one README configuration row for perl-lsp.mcp.servers")
readme.write_text("\n".join(filtered) + "\n", encoding="utf-8")

extension_doc = Path("docs/EXTENSION.md")
replace_once(
    extension_doc,
    "including deprecated `perl-lsp.perlcritic.*` settings, AI completion streaming, MCP servers, Linux libc selection, and update intervals",
    "including deprecated `perl-lsp.perlcritic.*` settings, AI completion streaming, Linux libc selection, and update intervals",
)

guard = Path("vscode-extension/scripts/check-no-generic-mcp-surface.js")
guard.write_text(
    r'''#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..');

const ACTIVE_TEXT_SURFACES = [
  'vscode-extension/package.json',
  'vscode-extension/src/extension.ts',
  'vscode-extension/src/featureActivationMetrics.ts',
  'vscode-extension/README.md',
  'docs/EXTENSION.md',
];

const FORBIDDEN_TEXT = [
  { needle: 'mcpServerDefinitionProviders', description: 'generic MCP provider contribution' },
  { needle: 'perl-lsp.mcp.servers', description: 'arbitrary MCP command setting' },
  { needle: 'perl-lsp.mcp-servers', description: 'generic MCP provider identifier' },
  { needle: 'registerMcpSupport', description: 'generic MCP registration path' },
];

function collectFindings(root = repositoryRoot) {
  const findings = [];
  const retiredSource = path.join(root, 'vscode-extension/src/mcpSupport.ts');
  if (fs.existsSync(retiredSource)) {
    findings.push('vscode-extension/src/mcpSupport.ts: retired generic MCP provider source exists');
  }

  for (const relative of ACTIVE_TEXT_SURFACES) {
    const file = path.join(root, relative);
    if (!fs.existsSync(file)) {
      findings.push(`${relative}: required active surface is missing`);
      continue;
    }
    const text = fs.readFileSync(file, 'utf8');
    for (const forbidden of FORBIDDEN_TEXT) {
      if (text.includes(forbidden.needle)) {
        findings.push(`${relative}: ${forbidden.description} (${forbidden.needle})`);
      }
    }
  }
  return findings;
}

function main() {
  const findings = collectFindings();
  if (findings.length > 0) {
    console.error('Generic VS Code MCP surface check failed:');
    for (const finding of findings) console.error(`- ${finding}`);
    process.exitCode = 1;
    return;
  }
  console.log('Generic VS Code MCP surface check: retired');
}

if (require.main === module) main();

module.exports = { collectFindings };
''',
    encoding="utf-8",
)

guard_test = Path("vscode-extension/scripts/check-no-generic-mcp-surface.test.js")
guard_test.write_text(
    r'''#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { collectFindings } = require('./check-no-generic-mcp-surface');

const activeFiles = [
  'vscode-extension/package.json',
  'vscode-extension/src/extension.ts',
  'vscode-extension/src/featureActivationMetrics.ts',
  'vscode-extension/README.md',
  'docs/EXTENSION.md',
];

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'perl-lsp-mcp-surface-'));
  for (const relative of activeFiles) {
    const file = path.join(root, relative);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, '{}\n', 'utf8');
  }
  return root;
}

test('clean active surfaces pass', () => {
  const root = fixture();
  assert.deepEqual(collectFindings(root), []);
});

test('provider source and each public command surface are load-bearing', () => {
  const mutations = [
    { relative: 'vscode-extension/src/mcpSupport.ts', content: 'export const registerMcpSupport = true;' },
    { relative: 'vscode-extension/package.json', content: '{"mcpServerDefinitionProviders":[]}' },
    { relative: 'vscode-extension/package.json', content: '{"perl-lsp.mcp.servers":[]}' },
    { relative: 'vscode-extension/src/extension.ts', content: 'registerMcpSupport(outputChannel);' },
    { relative: 'vscode-extension/README.md', content: '`perl-lsp.mcp.servers`' },
  ];

  for (const mutation of mutations) {
    const root = fixture();
    const file = path.join(root, mutation.relative);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, mutation.content, 'utf8');
    assert.notDeepEqual(collectFindings(root), [], mutation.relative);
  }
});
''',
    encoding="utf-8",
)
