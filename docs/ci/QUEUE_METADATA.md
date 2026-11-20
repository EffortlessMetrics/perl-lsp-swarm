# Merge-group metadata compilation

`workflow-trigger-lint.yml` owns the `queue-metadata` job for native
`merge_group: checks_requested` events. It builds the default-feature, development
profile `xtask` and `action-pin-provenance` binaries together on Ubuntu 24.04.
The trigger lint, methodology gate and capability gate then use those binaries in
one checkout and target directory. No executable archive or verdict cache crosses
jobs. The ordinary PR/push paths remain in their existing workflows; retiring those
paths belongs to the queue lifecycle cutover (#19326).

The original trigger-lint job, methodology job and capability router do not run for
merge groups. Their workflow registrations remain, but the G executions are owned
by the shared job. This changes the advisory job presentation, not any of the five
required integration contexts. The Non-Rust policy evaluator remains a separate
trusted-source execution and is not compiled or executed in this cluster.

The shared job retains these distinct checks:

- Whole-tree setup-perl action-pin Python regression.
- One locked Cargo build of both binaries.
- Workflow trigger lint, with its native JSON receipt.
- Immutable action-pin provenance against the event's exact base, with its native
  JSON receipt.
- Methodology gate using the merge-group event fixture (the same non-PR behavior
  as its original workflow), with its native JSON receipt.
- Agent capability boundary, retaining its native exit status and console output.

Each command has a separate execution record with G, B, repository, workflow ref
and source SHA, run, attempt, job, argv, status, exit code and elapsed seconds.
Native receipt bytes are retained and hashed, not rewritten as an aggregate PASS.
These are candidate-produced diagnostics, not trusted finalizer or admission
receipts. They do not establish native stack membership or proof semantics.

A failed check does not prevent the remaining independent checks from running.
Build failure records the binary-dependent checks as `not_run`; it cannot use an
old executable. Missing native receipts, command-start failure and timeout fail the
job. Per-command timeout kills the command process group. Workflow cancellation
or timeout can leave partial evidence and must never be read as success. Uploads
are bound to exact G/run/attempt; an existing attempt directory is refused before
commands run, preventing old receipts from filling a missing result.

The job checks event/checkout agreement before doing work. Artifact names include
G/run/attempt. The trusted queue lifecycle must still validate native Actions job
results and expected membership independently; this helper does not authorize
landing or replace the full verification portfolio.

The expected saving is removal of two independent `xtask` dependency builds and
the merge-group capability runner-routing step. Actual elapsed time and raw runner
minutes require a new native G observation, including setup/upload/failure costs.
Local command fixtures prove routing and failure semantics, not compilation cost.
The 25-minute job cap is a failure bound, not a claimed performance result.

Rollback: restore the three original merge-group job conditions and remove the
shared job, helper and its tests. Never leave both the old jobs and the shared job
eligible for the same G.
