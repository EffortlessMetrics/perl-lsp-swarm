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
  return lines
    .slice(start + 1)
    .filter((line) => line.startsWith('            '))
    .map((line) => line.slice(12))
    .join('\n');
}

async function runQueue({ commits, totalCommits, currentTitles }) {
  const failures = [];
  const readTitles = [];
  const associatedPulls = new Map(commits.map((commit, index) => [commit.sha, index + 101]));
  const github = {
    rest: {
      repos: {
        compareCommitsWithBasehead: async () => ({
          data: { status: 'ahead', total_commits: totalCommits, commits },
        }),
        listPullRequestsAssociatedWithCommit: async ({ commit_sha: sha }) => ({
          data: [{ number: associatedPulls.get(sha) }],
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
