'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { verifyPublisherTools } = require('./check-publisher-tools');

void test('publisher CLIs resolve from exact local dependencies without network access', () => {
  const tools = verifyPublisherTools();

  assert.deepEqual(
    tools.map(({ packageName, executable }) => ({ packageName, executable })),
    [
      { packageName: '@vscode/vsce', executable: '@vscode/vsce' },
      { packageName: 'ovsx', executable: 'ovsx' },
    ],
  );
  for (const tool of tools) assert.equal(tool.reportedVersion, tool.expectedVersion);
  for (const tool of tools) assert.match(tool.packagePath, /node_modules/);
});
