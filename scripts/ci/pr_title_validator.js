// The existing title rules, evaluated once per proved native member.
// G uses only reads. Proposed comments/label changes belong to the trusted
// owning session's existing publisher; recording them grants no write authority.
'use strict';

async function legacyTitleRules({github, core, context}, pr, actor) {
  const title = pr.title;
  const login = pr.user.login;

  if (actor === 'dependabot[bot]' || login.includes('dependabot')) {
    core.info('Exempt: dependabot PR');
    return;
  }

  const labels = pr.labels.map(l => l.name);
  if (labels.includes('skip-title-check')) {
    core.info('Exempt: skip-title-check label present');
    return;
  }

  if (/^v?\d+\.\d+\.\d+$/.test(title.trim())) {
    core.info('Exempt: release stamp title');
    return;
  }

  const BOT_MARKER = '<!-- pr-title-check-bot -->';
  const issueGuidance = [
    'For Codex or other agent-authored PRs: prefer filing the plan as a GitHub issue first, then reference it in the title (e.g. `(#1234)`).',
    '',
    'The issue should preserve the plan context: problem, scope, constraints, files or surfaces, validation, and explicit non-goals.',
    '',
    'If you genuinely cannot determine the issue number, use the placeholder `(#0000)` — do NOT guess a real issue number, because a wrong-but-real reference silently pollutes an unrelated issue. `(#0000)` passes this check (non-blocking) but flags the PR with `needs-issue-link`; a real issue must be linked before merge.',
    '',
    '`skip-title-check` is reserved for maintainer-owned exceptions.',
  ].join('\n');
  const upsertTitleCheckComment = async (commentBody) => {
    // Deduplicate: find and update existing bot comment instead of posting a new one.
    const { data: comments } = await github.rest.issues.listComments({
      owner: context.repo.owner,
      repo: context.repo.repo,
      issue_number: pr.number,
    });

    const existing = comments.find(c =>
      c.user.type === 'Bot' && c.body && c.body.includes(BOT_MARKER)
    );

    if (existing) {
      await github.rest.issues.updateComment({
        owner: context.repo.owner,
        repo: context.repo.repo,
        comment_id: existing.id,
        body: commentBody,
      });
      core.info('Prepared update to existing bot comment for the trusted owning session.');
    } else {
      await github.rest.issues.createComment({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: pr.number,
        body: commentBody,
      });
      core.info('Prepared new bot comment for the trusted owning session.');
    }
  };
  const issueMatches = [...title.matchAll(/\B#(\d+)\b/g)];

  if (issueMatches.length === 0) {
    core.setFailed(
      `PR title must reference an issue (e.g. "feat: add thing (#42)"). ` +
      `Codex/agent PRs should file the plan as an issue first. Got: "${title}"`
    );

    const commentBody = [
      '## \u26a0\ufe0f PR Title Check Failed',
      '',
      BOT_MARKER,
      '',
      'This PR\'s title does not include an **issue reference** (`#<number>` outside of version strings).',
      '',
      '**Expected format:** `type: description (#1234)`',
      '',
      issueGuidance,
      '',
      `Current title: \`${title}\``,
    ].join('\n');

    await upsertTitleCheckComment(commentBody);
    return;
  }

  // Zero-valued references (#0 / #0000) are the sanctioned placeholder
  // for "issue number not yet known" (issue #724). They are NON-BLOCKING:
  // the check passes, but the PR is flagged with `needs-issue-link` and a
  // real issue must be linked before merge (ops is the backstop). This
  // removes any incentive to guess a real issue number \u2014 a wrong-but-real
  // reference silently pollutes an unrelated issue's thread.
  const nonZeroMatch = issueMatches.find((match) => parseInt(match[1], 10) !== 0);
  const zeroIssueMatch = issueMatches.find((match) => parseInt(match[1], 10) === 0);

  if (!nonZeroMatch && zeroIssueMatch) {
    core.info(
      `Placeholder issue reference "${zeroIssueMatch[0]}" accepted (non-blocking). ` +
      `Preparing trusted-session needs-issue-link reconciliation.`
    );
    const commentBody = [
      '## \u2139\ufe0f PR Title \u2014 placeholder issue reference',
      '',
      BOT_MARKER,
      '',
      `This PR title uses the placeholder issue reference \`${zeroIssueMatch[0]}\`. The title check **passes** (non-blocking), but a **real issue must be linked before merge** \u2014 this PR is labeled \`needs-issue-link\`.`,
      '',
      'If you know the issue, update the title to reference it, e.g. `(#1234)`. Never guess a real issue number.',
      '',
      issueGuidance,
      '',
      `Current title: \`${title}\``,
    ].join('\n');

    await upsertTitleCheckComment(commentBody);
    try {
      await github.rest.issues.addLabels({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: pr.number,
        labels: ['needs-issue-link'],
      });
    } catch (e) {
      core.warning(`Could not add needs-issue-link label: ${e.message}`);
    }
    core.info('PR title passes validation (placeholder reference).');
    return;
  }

  // Validate the first real (non-placeholder) issue reference.
  const issueNumber = parseInt(nonZeroMatch[1], 10);
  try {
    await github.rest.issues.get({
      owner: context.repo.owner,
      repo: context.repo.repo,
      issue_number: issueNumber,
    });
    core.info(`Referenced issue #${issueNumber} exists.`);
  } catch (e) {
    core.warning(
      `Referenced issue #${issueNumber} was not found. Please verify the issue number.`
    );
    // Don't fail the check for this — it may be a private/internal reference
  }

  // Self-clearing needs-issue-link is prepared for the owning session.
  // Once a PR's title carries a real (non-placeholder)
  // issue reference, drop the label instead of leaving it for a manual
  // clear at merge time. Per #3356: the title was fixed (a real issue
  // ref added, this check went green) but `needs-issue-link` survived
  // and blocked an ops merge under the "no needs-* label may merge"
  // doctrine — the label-writer never reconciled its own label.
  if (labels.includes('needs-issue-link')) {
    try {
      await github.rest.issues.removeLabel({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: pr.number,
        name: 'needs-issue-link',
      });
      core.info(`Prepared removal of needs-issue-link: title now references real issue #${issueNumber}.`);
    } catch (e) {
      core.warning(`Could not remove needs-issue-link label: ${e.message}`);
    }
  }

  core.info('PR title passes validation.');
}

async function evaluateOne({github, core, context}, pr, {allowEventActor = false} = {}) {
  const housekeeping = [];
  let failed = false;
  const record = operation => async args => {
    housekeeping.push({operation, args});
    return {data: {}};
  };
  const readOnly = {rest: {issues: {
    get: args => github.rest.issues.get(args),
    listComments: args => github.rest.issues.listComments(args),
    createComment: record('createComment'),
    updateComment: record('updateComment'),
    addLabels: record('addLabels'),
    removeLabel: record('removeLabel'),
  }}};
  const diagnostics = {
    info: message => core.info(message),
    warning: message => core.warning(message),
    setFailed: message => { failed = true; core.setFailed(message); },
  };
  // A merge-group actor is not the actor for every constituent. The historical
  // event-actor exception remains available only to explicit authoring callers;
  // G retains each member's own current Dependabot-author exemption.
  await legacyTitleRules({github: readOnly, core: diagnostics, context}, pr,
                        allowEventActor ? context.actor : '');
  if (housekeeping.length) {
    core.info(`PR #${pr.number}: ${housekeeping.length} title housekeeping operation(s) prepared for the trusted owning session; G performed no writes.`);
  }
  return {pull_number: pr.number, verdict: failed ? 'failure' : 'success', housekeeping};
}

async function validateMembers(dependencies, receipt) {
  if (!receipt || receipt.schema_id !== 'perl.native-queue-title.v1'
      || receipt.schema_version !== 1 || !Array.isArray(receipt.members)
      || receipt.members.length < 1 || receipt.members.length > 100) {
    throw new Error('Missing proved native title subject');
  }
  const seen = new Set();
  const results = [];
  for (const member of receipt.members) {
    if (!member || !Number.isSafeInteger(member.pull_number) || member.pull_number <= 0
        || seen.has(member.pull_number) || member.pull?.number !== member.pull_number
        || !/^[0-9a-f]{40}$/.test(member.head_sha)) {
      throw new Error('Ambiguous native title member');
    }
    seen.add(member.pull_number);
    results.push(await evaluateOne(dependencies, member.pull));
  }
  return results;
}

module.exports = {evaluateOne, validateMembers};
