// CA02B: use the extension's existing TypeScript compiler API. No text-marker
// fallback exists when that tool is unavailable. This never executes adapters.
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const root = process.argv[2];
const compiler = process.argv[3];
if (!root || !compiler) throw new Error('usage: node check_high_risk_client_bindings.cjs ROOT TYPESCRIPT_MODULE');
const { pathToFileURL } = require('node:url');
async function main() {
const packageRoot = path.resolve(compiler);
const { API } = await import(pathToFileURL(path.join(packageRoot, 'dist/api/sync/api.js')).href);
const ts = await import(pathToFileURL(path.join(packageRoot, 'dist/ast/index.js')).href);
const virtual = new Map();
const key = file => file.replaceAll('\\', '/').toLowerCase();
let sequence = 0;
const api = new API({ cwd: path.resolve(root), fs: {
  readFile(file) { return virtual.get(key(file)); },
  fileExists(file) { return virtual.has(key(file)) ? true : undefined; },
  directoryExists(directory) { return directory.replaceAll('\\', '/').includes('/ca02b-virtual') ? true : undefined; },
} });
try {
function parse(text) {
  const file = path.resolve(root, 'ca02b-virtual', `binding${sequence++}.ts`).replaceAll('\\', '/');
  virtual.set(key(file), text);
  const snapshot = api.updateSnapshot({ openFiles: [file], fileChanges: { created: [file] } });
  try {
    const project = snapshot.getDefaultProjectForFile(file);
    const source = project?.program.getSourceFile(file);
    if (!source) throw new Error('TypeScript parser returned no source file');
    if (project.program.getSyntacticDiagnostics(file).length) throw new Error('invalid TypeScript syntax');
    return source;
  } finally { snapshot[Symbol.dispose](); }
}
// Compare syntax tree identity, including literal/identifier text and operator
// child nodes. Source positions and comments are deliberately not evidence.
function canonical(node) {
  const children = [];
  node.forEachChild(child => { children.push(canonical(child)); });
  return { kind: node.kind, text: node.text ?? node.escapedText, children };
}
function check(sourceText, witness) {
  const source = parse(sourceText);
  if (witness.function.startsWith('@table:')) {
    const name = witness.function.slice('@table:'.length);
    const declarations = source.statements.filter(ts.isVariableStatement)
      .flatMap(node => node.declarationList.declarations)
      .filter(node => node.name?.text === name);
    const expectedFile = parse(`const expected = ${witness.expression};`);
    const expected = expectedFile.statements[0]?.declarationList?.declarations[0]?.initializer;
    if (declarations.length !== 1 || !declarations[0].initializer || !expected ||
        JSON.stringify(canonical(declarations[0].initializer)) !== JSON.stringify(canonical(expected))) {
      throw new Error(`client binding table drift: ${name}`);
    }
    return;
  }
  const expectedFile = parse(`function expected() ${witness.expression}`);
  const expected = expectedFile.statements[0]?.body;
  if (!expected) throw new Error('witness must contain a function body');
  const render = node => JSON.stringify(canonical(node));
  const expectedText = render(expected);
  const owners = source.statements.filter(node => ts.isFunctionDeclaration(node) && node.name?.text === witness.function);
  if (owners.length !== 1 || !owners[0].body || render(owners[0].body) !== expectedText) {
    throw new Error(`client binding drift: ${witness.function}`);
  }
}
// Coupled read-to-payload control: another function/comment cannot satisfy it.
const control = { function: 'payload', expression: '{ const enabled = config.get("enabled"); return { enabled }; }' };
const source = 'function payload() { const enabled = config.get("enabled"); return { enabled }; }';
check(source, control);
for (const changed of [source.replace('get("enabled")', 'get("other")'), source.replace('return { enabled }', 'return { enabled: false }'), source.replace('function payload', 'function elsewhere')]) {
  let rejected = false;
  try { check(`${changed}\n// ${source}`, control); } catch { rejected = true; }
  if (!rejected) throw new Error('client AST negative control did not reject');
}
const projection = JSON.parse(fs.readFileSync(path.join(root, 'fixtures/configuration_authority/high_risk_bindings.v1.json'), 'utf8'));
const table = { function: '@table:SPECS', expression: '[{ key: "enabled", property: "enabled" }]' };
const tableSource = `const SPECS = ${table.expression};`;
check(tableSource, table);
for (const changed of [tableSource.replace('key: "enabled"', 'key: "other"'), tableSource.replace('property: "enabled"', 'property: "other"')]) {
  let rejected = false;
  try { check(`${changed}\n// ${tableSource}\nconst OTHER = ${table.expression};`, table); } catch { rejected = true; }
  if (!rejected) throw new Error('client table negative control did not reject');
}
for (const [id, witness] of Object.entries(projection.witnesses)) {
  if (!witness.path.endsWith('.ts')) continue;
  try { check(fs.readFileSync(path.join(root, witness.path), 'utf8'), witness); }
  catch (error) { throw new Error(`${id}: ${error.message}`); }
}
} finally { api.close(); }
}
main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
