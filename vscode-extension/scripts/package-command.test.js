const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const packageJson = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'));

void test('ordinary packaging checks the exact VSIX it just produced', () => {
  const command = packageJson.scripts.package;
  const implementation = fs.readFileSync(path.join(__dirname, 'package-vsix.js'), 'utf8');

  assert.equal(command, 'node scripts/package-vsix.js');
  assert.match(implementation, /vsceEntry/);
  assert.match(implementation, /\['package', '--out', vsixName\]/);
  assert.match(
    implementation,
    /check-vsix-inventory-transition\.js'[\s\S]*'--vsix'[\s\S]*vsixName/,
  );
  assert.ok(
    implementation.indexOf("['package', '--out', vsixName]") <
      implementation.indexOf('check-vsix-inventory-transition.js'),
    'archive validation must run after packaging',
  );
});
