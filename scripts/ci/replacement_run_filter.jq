# Select the run that replaced the one under evaluation, or select nothing.
#
# Input is the body of
#   GET /repos/{owner}/{repo}/actions/workflows/{workflow_id}/runs?head_sha=…
# which already constrains two identities: the workflow, and the head SHA.
#
# Three more are not constrained by that endpoint, and #16186's review found
# each of them load-bearing — "chooses the first same-workflow run for the SHA
# without checking event, PR association, or whether it is newer […] the
# evidence claim remains too strong". They are checked here.
#
# Arguments:
#   $pr     number   this pull request's number      (--argjson)
#   $since  string   the evaluated run's created_at  (--arg)
#
# Output: one run id, or nothing at all. Nothing is the safe answer — the
# caller exports no REPLACEMENT_RUN_ID and the gate says only that every
# blocking lane was cancelled and no newer run was identified.
[ .workflow_runs[]?

  # 1. Event. A `push` or `workflow_dispatch` run can share a head SHA with a
  #    pull request and proves nothing about the pull request's candidate.
  #    Measured 2026-09-20: `push` runs on this repository carry an empty
  #    `pull_requests`, so this and the next check are not redundant — this one
  #    states the requirement, that one enforces it.
  | select(.event == "pull_request")

  # 2. Association. A commit can be the head of more than one pull request.
  #    A run of a different pull request is not this candidate's replacement,
  #    however new it is. `// []` because the field can be absent.
  | select([(.pull_requests // [])[].number] | index($pr))

  # 3. Chronology. The same SHA may have been tested before this run existed —
  #    a branch reset, a revert, a cherry-pick that lands the same tree. A
  #    historical run is not a replacement for a run that came after it.
  #    Strict `>`: a tie to the second is not demonstrably newer, and failing
  #    closed costs only the narrower sentence.
  | select(.created_at > $since)

  | .id
]

# The endpoint returns newest first, so the first survivor is the newest
# qualifying run. `// empty` yields no output at all rather than `null`, which
# the caller's `^[0-9]+$` guard would reject anyway — this makes the intent
# explicit rather than relying on that guard.
| first // empty
