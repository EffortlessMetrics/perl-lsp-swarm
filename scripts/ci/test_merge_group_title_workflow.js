'use strict';

// Execute the real actions/github-script body with synthetic API responses.
// This deliberately reads no policy copy and performs no network access.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

async function loadWorkflowScript(workflowFile) {
  const workflow = fs.readFileSync(
    path.join(__dirname, '..', '..', '.github', 'workflows', workflowFile),
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

// Captured from native squash-queue run 43158183620. PR #19295 was open when
// this event ran; its current merged state is deliberately not replayed here.
const BASE_SHA = '5dfecf1602bbcd83494893840c0f8795dba492e0';
const GROUP_SHA = '3b7cb31996c2c75a6c96dc22d365a70aedd8d138';
const PR_HEAD_SHA = 'd1ea95fd0c2d7166c6943d9cd1f870e7bc304fb1';
const PR_NUMBER = 19295;
const HEAD_REF = `refs/heads/gh-readonly-queue/main/pr-${PR_NUMBER}-${BASE_SHA}`;

const title = (text, login = 'author') => ({ title: text, user: { login }, labels: [] });

function queuePr(overrides = {}) {
  return {
    number: PR_NUMBER,
    state: 'open',
    ...title('perf(release): reuse Domain6 listing object ids (#19262)'),
    ...overrides,
    base: { ref: 'main', sha: BASE_SHA, ...(overrides.base || {}) },
    head: { sha: PR_HEAD_SHA, ...(overrides.head || {}) },
  };
}

async function runQueue({ mergeGroup = {}, currentPr = queuePr(), pullsGet = null } = {}) {
  const failures = [];
  const readPrs = [];
  const legacyCalls = [];
  const infos = [];
  const github = {
    rest: {
      repos: {
        // The native control has one synthetic G commit and no associated PR.
        // The repaired path must neither call nor depend on these legacy APIs.
        compareCommitsWithBasehead: async () => {
          legacyCalls.push('compare');
          return {
            data: {
              status: 'ahead',
              total_commits: 1,
              commits: [{ sha: GROUP_SHA, parents: [{ sha: BASE_SHA }] }],
            },
          };
        },
        listPullRequestsAssociatedWithCommit: async () => {
          legacyCalls.push('association');
          return { data: [] };
        },
      },
      pulls: {
        get: async ({ pull_number: number }) => {
          readPrs.push(number);
          if (pullsGet) return pullsGet(number);
          if (!currentPr) throw new Error('PR not found');
          return { data: currentPr };
        },
      },
    },
  };
  const context = {
    eventName: 'merge_group',
    repo: { owner: 'owner', repo: 'repo' },
    payload: {
      merge_group: {
        base_sha: BASE_SHA,
        head_sha: GROUP_SHA,
        base_ref: 'refs/heads/main',
        head_ref: HEAD_REF,
        ...mergeGroup,
      },
    },
  };
  const core = {
    setFailed: (message) => failures.push(message),
    info: (message) => infos.push(message),
  };

  // actions/github-script invokes this exact body as an async function.
  const execute = new AsyncFunction('core', 'github', 'context', await loadWorkflowScript('merge-group-title-check.yml'));
  await execute(core, github, context);
  return { failures, readPrs, legacyCalls, infos };
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

  const execute = new AsyncFunction('core', 'github', 'context', await loadWorkflowScript('pr-title-check.yml'));
  await execute(core, github, context);
  return { failures, warnings, created, updated, addedLabels, removedLabels };
}

test('the captured native squash group passes even when its synthetic commit has no associated PR', async () => {
  assert.notEqual(PR_HEAD_SHA, GROUP_SHA, 'the native PR head and synthetic group head differ');
  const result = await runQueue({
    mergeGroup: { head_ref: HEAD_REF.replace(/^refs\/heads\//, '') },
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
  assert.deepEqual(result.legacyCalls, []);
  const passed = result.infos.find((line) => line.includes('title validation passed'));
  assert.ok(passed, 'expected a success log line');
  assert.match(passed, /#19295/);
});

test('the canonical full queue ref form is accepted', async () => {
  const result = await runQueue();
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
  assert.deepEqual(result.legacyCalls, []);
});

test('a queued PR keeps its recorded base SHA when the group builds on an earlier synthetic commit', async () => {
  const result = await runQueue({
    mergeGroup: {
      base_sha: GROUP_SHA,
      head_ref: `refs/heads/gh-readonly-queue/main/pr-${PR_NUMBER}-${GROUP_SHA}`,
    },
    currentPr: queuePr({ base: { sha: BASE_SHA } }),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
  assert.deepEqual(result.legacyCalls, []);
});

test('a base branch with regex metacharacters is matched literally', async () => {
  const baseRef = 'release/1.0';
  const result = await runQueue({
    mergeGroup: {
      base_ref: `refs/heads/${baseRef}`,
      head_ref: `refs/heads/gh-readonly-queue/${baseRef}/pr-${PR_NUMBER}-${BASE_SHA}`,
    },
    currentPr: queuePr({ base: { ref: baseRef } }),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
});

test('missing, malformed, ambiguous, and contradictory queue identities fail before a PR read', async (t) => {
  const queuePrefix = 'refs/heads/gh-readonly-queue/main';
  const cases = [
    ['missing head_ref', { head_ref: null }],
    ['missing base_sha', { base_sha: null }],
    ['missing head_sha', { head_sha: null }],
    ['missing base_ref', { base_ref: null }],
    ['malformed base_sha', { base_sha: 'base' }],
    ['malformed head_sha', { head_sha: 'group' }],
    ['double ref prefix', { head_ref: `refs/heads/${HEAD_REF}` }],
    ['missing PR number', { head_ref: `${queuePrefix}/pr--${BASE_SHA}` }],
    ['zero PR number', { head_ref: `${queuePrefix}/pr-0-${BASE_SHA}` }],
    ['unsafe PR number', { head_ref: `${queuePrefix}/pr-9007199254740992-${BASE_SHA}` }],
    ['multiple PR components', { head_ref: `${queuePrefix}/pr-19295-pr-19296-${BASE_SHA}` }],
    ['trailing data', { head_ref: `${HEAD_REF}-extra` }],
    ['base branch contradiction', { head_ref: `refs/heads/gh-readonly-queue/other/pr-${PR_NUMBER}-${BASE_SHA}` }],
    ['base SHA contradiction', { head_ref: `${queuePrefix}/pr-${PR_NUMBER}-${GROUP_SHA}` }],
    ['regex is not a base branch wildcard', {
      base_ref: 'refs/heads/release/1.0',
      head_ref: `refs/heads/gh-readonly-queue/release/1x0/pr-${PR_NUMBER}-${BASE_SHA}`,
    }],
  ];
  for (const [name, mergeGroup] of cases) {
    await t.test(name, async () => {
      const result = await runQueue({ mergeGroup });
      assert.equal(result.failures.length, 1);
      assert.match(result.failures[0], /merge_group/);
      assert.deepEqual(result.readPrs, []);
      assert.deepEqual(result.legacyCalls, []);
    });
  }
});

test('an absent PR fails closed instead of producing a zero-member success', async () => {
  const result = await runQueue({ currentPr: null });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /#19295.*could not re-read current title/);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
  assert.deepEqual(result.legacyCalls, []);
});

test('contradictory current PR identity fails closed', async (t) => {
  const cases = [
    ['wrong number', queuePr({ number: PR_NUMBER + 1 })],
    ['closed state', queuePr({ state: 'closed' })],
    ['edited base branch', queuePr({ base: { ref: 'other' } })],
    ['missing current head SHA', queuePr({ head: { sha: null } })],
    ['malformed current head SHA', queuePr({ head: { sha: 'head' } })],
  ];
  for (const [name, currentPr] of cases) {
    await t.test(name, async () => {
      const result = await runQueue({ currentPr });
      assert.equal(result.failures.length, 1);
      assert.match(result.failures[0], /#19295.*changed state, base, or identity/);
      assert.deepEqual(result.readPrs, [PR_NUMBER]);
      assert.deepEqual(result.legacyCalls, []);
    });
  }
});

test('a title edited after an earlier green validation fails the next run', async () => {
  const beforeEdit = await runQueue();
  assert.deepEqual(beforeEdit.failures, []);

  const afterEdit = await runQueue({
    currentPr: queuePr(title('edited to remove the issue reference')),
  });
  assert.equal(afterEdit.failures.length, 1);
  assert.match(afterEdit.failures[0], /#19295: no issue reference/);
  assert.deepEqual(afterEdit.readPrs, [PR_NUMBER]);
});

test('a dependabot lookalike author gets no exemption', async () => {
  const result = await runQueue({
    currentPr: queuePr(title('no issue reference', 'not-dependabot-person')),
  });
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /#19295: no issue reference/);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
});

test('the exact dependabot identity remains exempt', async () => {
  const result = await runQueue({
    currentPr: queuePr(title('dependency: automated update', 'dependabot[bot]')),
  });
  assert.deepEqual(result.failures, []);
  assert.deepEqual(result.readPrs, [PR_NUMBER]);
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
