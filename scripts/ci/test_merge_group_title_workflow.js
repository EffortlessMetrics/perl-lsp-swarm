'use strict';

// Execute the real actions/github-script body with synthetic API responses.
// This deliberately reads no policy copy and performs no network access.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

async function loadWorkflowScript() {
  const workflow = fs.readFileSync(
    path.join(__dirname, '..', '..', '.github', 'workflows', 'pr-title-check.yml'),
    'utf8',
  );
  const lines = workflow.split(/\r?\n/);
  const start = lines.indexOf('          script: |');
  assert.notEqual(start, -1, 'the workflow keeps its executable github-script body');
  // Keep blank lines inside the block and stop at the first non-empty line
  // that is not indented as script body. A filter would silently drop blank
  // lines and could keep lines after the block that share the indent.
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (line.trim() === '') {
      body.push('');
      continue;
    }
    if (!line.startsWith('            ')) break;
    body.push(line.slice(12));
  }
  return body.join('\n');
}

async function runQueue({
  commits,
  totalCommits,
  currentTitles,
  status = 'ahead',
  pullsFor = null,
}) {
  const failures = [];
  const readTitles = [];
  const associatedPulls = new Map(commits.map((commit, index) => [commit.sha, index + 101]));
  const github = {
    rest: {
      repos: {
        compareCommitsWithBasehead: async () => ({
          data: { status, total_commits: totalCommits, commits },
        }),
        listPullRequestsAssociatedWithCommit: async ({ commit_sha: sha }) => ({
          data: pullsFor ? pullsFor(sha) : [{ number: associatedPulls.get(sha) }],
        }),
      },
      pulls: {
        get: async ({ pull_number: number }) => {
          readTitles.push(number);
          return { data: currentTitles.get(number) };
        },
      },
    },
  };
  const context = {
    eventName: 'merge_group',
    repo: { owner: 'owner', repo: 'repo' },
    payload: { merge_group: { base_sha: 'base', head_sha: 'group' } },
  };
  const core = {
    setFailed: (message) => failures.push(message),
    info: () => {},
  };

  // actions/github-script invokes this exact body as an async function.
  const execute = new AsyncFunction('core', 'github', 'context', await loadWorkflowScript());
  await execute(core, github, context);
  return { failures, readTitles };
}

const title = (text) => ({ title: text, user: { login: 'author' }, labels: [] });

test('one-member group validates its complete commit graph', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 1,
    currentTitles: new Map([[101, title('feat: one member (#18255)')]]),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readTitles, [101]);
});

test('multi-member group re-reads and validates every member', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }, { sha: 'two' }],
    totalCommits: 2,
    currentTitles: new Map([
      [101, title('feat: first member (#18255)')],
      [102, title('fix: second member (#18255)')],
    ]),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readTitles, [101, 102]);
});

test('one bad current title fails a multi-member group', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }, { sha: 'two' }],
    totalCommits: 2,
    currentTitles: new Map([
      [101, title('feat: first member (#18255)')],
      [102, title('bad title without an issue')],
    ]),
  });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /#102: no issue reference/);
  assert.deepEqual(result.readTitles, [101, 102]);
});

test('truncated compare fails closed instead of inferring membership', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 2,
    currentTitles: new Map([[101, title('feat: visible member (#18255)')]]),
  });
  assert.deepEqual(result.failures, [
    'merge_group commit graph is truncated (1/2); refusing to infer membership',
  ]);
  assert.deepEqual(result.readTitles, []);
});

test('a title edited after an earlier green validation fails the next run', async () => {
  const fixture = {
    commits: [{ sha: 'one' }, { sha: 'two' }],
    totalCommits: 2,
    currentTitles: new Map([
      [101, title('feat: first member (#18255)')],
      [102, title('fix: initially valid (#18255)')],
    ]),
  };
  const beforeEdit = await runQueue(fixture);
  assert.deepEqual(beforeEdit.failures, []);

  fixture.currentTitles.set(102, title('edited to remove the issue reference'));
  const afterEdit = await runQueue(fixture);
  assert.equal(afterEdit.failures.length, 1);
  assert.match(afterEdit.failures[0], /#102: no issue reference/);
  assert.deepEqual(afterEdit.readTitles, [101, 102]);
});

test('a diverged base fails closed before membership', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 1,
    status: 'diverged',
    currentTitles: new Map([[101, title('feat: one member (#18255)')]]),
  });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /not an ancestor/);
  assert.deepEqual(result.readTitles, []);
});

test('a commit with no associated PR fails closed', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 1,
    pullsFor: () => [],
    currentTitles: new Map(),
  });
  assert.deepEqual(result.failures, [
    'no constituent pull requests resolved for merge group head group',
  ]);
  assert.deepEqual(result.readTitles, []);
});

test('the exact compare cap is complete and one over is truncated', async () => {
  const commits = Array.from({ length: 250 }, (_, index) => ({ sha: `c${index}` }));
  const currentTitles = new Map(
    commits.map((_, index) => [index + 101, title(`feat: member ${index} (#18255)`)]),
  );

  const complete = await runQueue({ commits, totalCommits: 250, currentTitles });
  assert.deepEqual(complete.failures, []);
  assert.equal(complete.readTitles.length, 250);

  const truncated = await runQueue({ commits, totalCommits: 251, currentTitles });
  assert.deepEqual(truncated.failures, [
    'merge_group commit graph is truncated (250/251); refusing to infer membership',
  ]);
  assert.deepEqual(truncated.readTitles, []);
});
