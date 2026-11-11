"use strict";

// Execute the actual workflow receipt consumer and actual title module. Native
// observe/recheck and Git composition are exercised by test_native_queue_title.py.
// This harness is not evidence of installed authority or live GitHub qualification.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const root = path.resolve(__dirname, '../..');

function loadWorkflowScript(raw = fs.readFileSync(path.join(root, '.github/workflows/pr-title-check.yml'), 'utf8')) {
  const lines = raw.split(/\r?\n/);
  const marker = '          script: |';
  assert.equal(lines.filter(line => line === marker).length, 1, 'one actual title script body');
  const body = [];
  for (const line of lines.slice(lines.indexOf(marker) + 1)) {
    if (line.trim() === '') { body.push(''); continue; }
    if (!line.startsWith('            ')) break;
    body.push(line.slice(12));
  }
  return body.join('\n');
}

async function runReceipt({receipt, comments = [], unavailable = false, actor = 'dependabot[bot]'}) {
  const failures = [], warnings = [], infos = [], calls = [], writes = [];
  const workspace = '/fixture/workspace', temporary = '/fixture/temp';
  const receiptPath = path.join(temporary, 'native-queue-title.json');
  const sourcePath = path.join(workspace, 'trusted-title-source/scripts/ci/pr_title_validator.js');
  const fileSystem = {
    readFileSync: (file, encoding) => {
      assert.equal(file, receiptPath); assert.equal(encoding, 'utf8');
      return JSON.stringify(receipt);
    },
    writeFileSync: (file, body) => {
      assert.equal(file, receiptPath + '.titles.json');
      writes.push(JSON.parse(body));
    },
  };
  const allow = object => new Proxy(object, {get(target, key) {
    if (!(key in target)) {
      calls.push({operation: 'forbidden', key: String(key)});
      throw new Error('Unexpected GitHub operation: ' + String(key));
    }
    return target[key];
  }});
  const github = allow({rest: allow({issues: allow({
    get: async args => {
      calls.push({operation: 'issues.get', args});
      if (unavailable) throw new Error('not found');
      return {data: {number: args.issue_number}};
    },
    listComments: async args => {
      calls.push({operation: 'issues.listComments', args});
      return {data: comments};
    },
  })})});
  const fixtureRequire = name => {
    if (name === 'node:fs') return fileSystem;
    if (name === 'node:path') return path;
    assert.equal(name, sourcePath, 'the workflow must load the selected source module');
    return require(path.join(root, 'scripts/ci/pr_title_validator.js'));
  };
  const core = {setFailed: x => failures.push(x), warning: x => warnings.push(x), info: x => infos.push(x)};
  const context = {eventName: 'merge_group', actor,
    repo: {owner: 'EffortlessMetrics', repo: 'perl-lsp-swarm'}};
  const execute = new AsyncFunction('core', 'github', 'context', 'require', 'process', loadWorkflowScript());
  await execute(core, github, context, fixtureRequire,
    {env: {GITHUB_WORKSPACE: workspace, RUNNER_TEMP: temporary}});
  assert.equal(writes.length, 1, 'the actual workflow must serialize its validator result once');
  assert.ok(calls.every(call => call.operation !== 'forbidden'), 'no write or membership inference API');
  return {results: writes[0], failures, warnings, infos, calls};
}

function member(number, title, login = 'contributor', labels = []) {
  return {pull_number: number, head_sha: number.toString(16).padStart(40, '0'),
    pull: {number, title, user: {login}, labels: labels.map(name => ({name}))}};
}
const receipt = members => ({schema_id: 'perl.native-queue-title.v1', schema_version: 1, members});

if (process.argv.includes('--receipt-stdin')) {
  runReceipt(JSON.parse(fs.readFileSync(0, 'utf8'))).then(
    result => process.stdout.write(JSON.stringify(result)),
    error => { console.error(error); process.exitCode = 1; },
  );
} else {
  test('extractor retains blank lines and stops before later workflow steps', () => {
    assert.equal(loadWorkflowScript('          script: |\n            first();\n\n            second();\n      - name: next\n            notScript();'),
      'first();\n\nsecond();');
  });
  test('the actual workflow validates a singleton receipt without association lookup', async () => {
    const result = await runReceipt({receipt: receipt([member(101, 'feat: one member (#42)')])});
    assert.deepEqual(result.failures, []);
    assert.deepEqual(result.results.map(row => [row.pull_number, row.verdict]), [[101, 'success']]);
  });
  test('every member is evaluated and result identities are the actual PR numbers', async () => {
    const result = await runReceipt({receipt: receipt([
      member(101, 'feat: first (#42)'), member(102, 'bad title'), member(103, 'fix: third (#42)'),
    ])});
    assert.equal(result.failures.length, 1);
    assert.deepEqual(result.results.map(row => [row.pull_number, row.verdict]),
      [[101, 'success'], [102, 'failure'], [103, 'success']]);
    assert.doesNotMatch(JSON.stringify(result.results), /undefined/);
  });
  test('exact Dependabot passes but lookalike and group actor cannot exempt another member', async () => {
    const result = await runReceipt({receipt: receipt([
      member(101, 'no issue', 'dependabot[bot]'),
      member(102, 'no issue', 'not-dependabot-person'),
    ])});
    assert.equal(result.failures.length, 1);
    assert.deepEqual(result.results.map(row => row.verdict), ['success', 'failure']);
  });
  test('a fresh receipt with an edited bad title fails after an earlier success', async () => {
    const input = {receipt: receipt([member(101, 'feat: good (#42)')])};
    assert.deepEqual((await runReceipt(input)).failures, []);
    input.receipt.members[0].pull.title = 'edited to remove issue';
    assert.equal((await runReceipt(input)).failures.length, 1);
  });
  test('placeholder succeeds and prepares comment and label without GitHub writes', async () => {
    const result = await runReceipt({receipt: receipt([member(101, 'feat: pending (#0000)')])});
    assert.deepEqual(result.failures, []);
    const proposed = result.results[0].housekeeping;
    assert.deepEqual(proposed.map(row => row.operation), ['createComment', 'addLabels']);
    assert.match(proposed[0].args.body, /placeholder issue reference/);
    assert.deepEqual(proposed[1].args.labels, ['needs-issue-link']);
    assert.deepEqual(result.calls.map(call => call.operation), ['issues.listComments']);
  });
  test('placeholder reuses the existing bot comment without GitHub writes', async () => {
    const result = await runReceipt({receipt: receipt([member(101, 'feat: pending (#0000)')]),
      comments: [{id: 42, user: {type: 'Bot'}, body: '<!-- pr-title-check-bot --> old'}]});
    assert.deepEqual(result.failures, []);
    const proposed = result.results[0].housekeeping;
    assert.deepEqual(proposed.map(row => row.operation), ['updateComment', 'addLabels']);
    assert.equal(proposed[0].args.comment_id, 42);
    assert.deepEqual(result.calls.map(call => call.operation), ['issues.listComments']);
  });
  test('missing empty duplicate mismatched and oversize subjects cannot silently pass', async () => {
    const valid = member(101, 'feat: proof (#42)');
    for (const candidate of [null, receipt([]), {...receipt([valid]), schema_version: 2},
      receipt([valid, valid]), receipt([{...valid, pull_number: 102}]),
      receipt([{...valid, head_sha: 'not-a-head'}]), receipt(Array(101).fill(valid))]) {
      await assert.rejects(runReceipt({receipt: candidate}));
    }
  });
}
