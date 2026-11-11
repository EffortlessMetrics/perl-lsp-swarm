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

function associatedPr(number, sha, overrides = {}) {
  return {
    number,
    state: 'open',
    base: { ref: 'main' },
    head: { sha },
    ...overrides,
  };
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
  const headShas = new Map();
  const associatedPulls = new Map(
    commits.map((commit, index) => [commit.sha, associatedPr(index + 101, commit.sha)]),
  );
  const github = {
    rest: {
      repos: {
        compareCommitsWithBasehead: async () => ({
          data: { status, total_commits: totalCommits, commits },
        }),
        listPullRequestsAssociatedWithCommit: async ({ commit_sha: sha }) => {
          const pulls = pullsFor ? pullsFor(sha) : [associatedPulls.get(sha)];
          for (const pr of pulls) headShas.set(pr.number, pr.head?.sha);
          return { data: pulls };
        },
      },
      pulls: {
        get: async ({ pull_number: number }) => {
          readTitles.push(number);
          const current = currentTitles.has(number) ? currentTitles.get(number) : {};
          return {
            data: {
              state: 'open',
              base: { ref: 'main' },
              head: { sha: headShas.get(number) },
              ...current,
            },
          };
        },
      },
    },
  };
  const context = {
    eventName: 'merge_group',
    repo: { owner: 'owner', repo: 'repo' },
    payload: {
      merge_group: { base_sha: 'base', head_sha: 'group', base_ref: 'refs/heads/main' },
    },
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

// PR-event path: the same body, driven by a pull_request payload. Records the
// mutations the placeholder branch is supposed to make so the non-blocking
// contract (pass, comment, label) is asserted rather than assumed.
async function runPr({ pr, existingComments = [] }) {
  const failures = [];
  const warnings = [];
  const created = [];
  const updated = [];
  const addedLabels = [];
  const removedLabels = [];
  const github = {
    rest: {
      issues: {
        listComments: async () => ({ data: existingComments }),
        createComment: async ({ body }) => {
          created.push(body);
          return { data: { id: 1 } };
        },
        updateComment: async ({ comment_id: id, body }) => {
          updated.push({ id, body });
          return { data: { id } };
        },
        addLabels: async ({ labels }) => {
          addedLabels.push(...labels);
          return { data: [] };
        },
        removeLabel: async ({ name }) => {
          removedLabels.push(name);
          return { data: [] };
        },
        get: async () => ({ data: {} }),
      },
    },
  };
  const context = {
    eventName: 'pull_request_target',
    actor: 'author',
    repo: { owner: 'owner', repo: 'repo' },
    payload: { pull_request: pr },
  };
  const core = {
    setFailed: (message) => failures.push(message),
    warning: (message) => warnings.push(message),
    info: () => {},
  };

  const execute = new AsyncFunction('core', 'github', 'context', await loadWorkflowScript());
  await execute(core, github, context);
  return { failures, warnings, created, updated, addedLabels, removedLabels };
}

const title = (text, login = 'author') => ({ title: text, user: { login }, labels: [] });

test('a nonempty but incomplete association set fails before title validation', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }, { sha: 'two' }],
    totalCommits: 2,
    pullsFor: (sha) => (sha === 'one' ? [associatedPr(101, sha)] : []),
    currentTitles: new Map([[101, title('feat: visible member (#18255)')]]),
  });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /two.*incomplete association set/);
  assert.deepEqual(result.readTitles, []);
});

test('unrelated associations cannot reject exact-H members or replace their titles', async () => {
  const unrelated = [
    associatedPr(999, 'one', { state: 'closed', base: { ref: 'other' } }),
    associatedPr(999, 'one', { base: { ref: 'other' } }),
    associatedPr(999, 'different'),
  ];
  for (const candidate of unrelated) {
    const result = await runQueue({
      commits: [{ sha: 'one' }],
      totalCommits: 1,
      pullsFor: () => [associatedPr(101, 'one'), candidate],
      currentTitles: new Map([
        [101, title('feat: admitted member (#18255)')],
        [999, title('unrelated title has no issue reference')],
      ]),
    });
    assert.deepEqual(result.failures, []);
    assert.deepEqual(result.readTitles, [101]);
  }
});

test('a dependabot lookalike author gets no exemption', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 1,
    currentTitles: new Map([[101, title('no issue reference', 'not-dependabot-person')]]),
  });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /#101: no issue reference/);
  assert.deepEqual(result.readTitles, [101]);
});

test('the exact dependabot identity remains exempt', async () => {
  const result = await runQueue({
    commits: [{ sha: 'one' }],
    totalCommits: 1,
    currentTitles: new Map([[101, title('dependency: automated update', 'dependabot[bot]')]]),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readTitles, [101]);
});

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
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /one.*incomplete association set/);
  assert.deepEqual(result.readTitles, []);
});

test('a PR-event placeholder title passes, comments, and labels needs-issue-link', async () => {
  const result = await runPr({
    pr: { number: 7, title: 'feat: pending issue (#0000)', user: { login: 'author' }, labels: [] },
  });
  assert.deepEqual(result.failures, []);
  assert.equal(result.created.length, 1);
  assert.match(result.created[0], /placeholder issue reference/);
  assert.match(result.created[0], /<!-- pr-title-check-bot -->/);
  assert.deepEqual(result.addedLabels, ['needs-issue-link']);
  assert.deepEqual(result.removedLabels, []);
});

test('a PR-event placeholder title reuses the existing bot comment', async () => {
  const result = await runPr({
    pr: { number: 7, title: 'feat: pending issue (#0000)', user: { login: 'author' }, labels: [] },
    existingComments: [
      { id: 42, user: { type: 'Bot' }, body: '<!-- pr-title-check-bot -->\nold' },
    ],
  });
  assert.deepEqual(result.failures, []);
  assert.equal(result.created.length, 0);
  assert.equal(result.updated.length, 1);
  assert.equal(result.updated[0].id, 42);
  assert.deepEqual(result.addedLabels, ['needs-issue-link']);
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
