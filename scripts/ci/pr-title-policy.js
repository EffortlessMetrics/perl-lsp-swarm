'use strict';

// Shared PR-title policy for the pull_request_target and merge_group producers.
//
// Both jobs call this module so the exemptions and placeholder semantics cannot
// drift between the PR path and the queue path. The module is pure: it reads a
// PR-shaped object and returns a verdict. It never mutates GitHub state.

const RELEASE_STAMP = /^v?\d+\.\d+\.\d+$/;
const ISSUE_REFERENCE = /\B#(\d+)\b/g;

function labelsOf(pr) {
  const labels = pr.labels || [];
  return labels.map((label) => (typeof label === 'string' ? label : label.name));
}

function loginOf(pr) {
  return (pr.user && pr.user.login) || (pr.author && pr.author.login) || '';
}

/**
 * Apply the repository title policy to one PR.
 *
 * Returns:
 *   { ok: true,  kind: 'exempt',      reason }
 *   { ok: true,  kind: 'placeholder', reason, placeholder }
 *   { ok: true,  kind: 'issue',       reason, issueNumber }
 *   { ok: false, kind: 'missing-issue', reason }
 */
function validateTitle(pr) {
  const title = pr.title || '';
  const login = loginOf(pr);
  const labels = labelsOf(pr);

  if (login.includes('dependabot')) {
    return { ok: true, kind: 'exempt', reason: 'dependabot' };
  }
  if (labels.includes('skip-title-check')) {
    return { ok: true, kind: 'exempt', reason: 'skip-title-check' };
  }
  if (RELEASE_STAMP.test(title.trim())) {
    return { ok: true, kind: 'exempt', reason: 'release stamp' };
  }

  const matches = [...title.matchAll(ISSUE_REFERENCE)];
  if (matches.length === 0) {
    return { ok: false, kind: 'missing-issue', reason: 'no issue reference' };
  }

  const nonZero = matches.find((match) => parseInt(match[1], 10) !== 0);
  if (!nonZero) {
    const placeholder = matches.find((match) => parseInt(match[1], 10) === 0);
    return {
      ok: true,
      kind: 'placeholder',
      reason: 'placeholder issue reference',
      placeholder: placeholder[0],
    };
  }

  const issueNumber = parseInt(nonZero[1], 10);
  return { ok: true, kind: 'issue', reason: `issue #${issueNumber}`, issueNumber };
}

module.exports = { validateTitle };
