# Perl LSP Swarm Rust Small Proof

This repository is the high-volume same-repo PR workspace for
`EffortlessMetrics/perl-lsp`.

The first protected swarm lane is `Perl LSP Rust Small Result`. Branch
protection must require that normalized result, not the conditional
implementation jobs for CX53, CX43, or GitHub-hosted fallback.

Initial proof captured:

- same-repo PR fallback route: `26146166886`;
- forced CX43 backfill route: `26146635076`;
- forced CX53 primary route: `26147069092`.

Release, publish, signing, extension, and secrets-heavy workflows remain owned
by the source repository until a separate deliberate migration.

## Lane receipt

`cargo run -p xtask --locked -- rust-small-proof` produces a versioned receipt
(`rust_small_proof.v1`) at `target/receipts/rust-small-proof.json`, or at
`--receipt <path>`, after admitting the destination and establishing a clean
candidate. The schema binds the candidate SHA, `rustc`/`cargo` versions, and
scorecard profile/features from the pinned argv to all nine selected steps.
Outcomes are `ok`, `product_failure`, `not_completed`, `instrument_failure`,
and `not_run`.

Exact candidate production and `--verify-receipt <path>` require a clean
checkout. The required `worktree_dirty` field remains in the v1 schema for
compatibility and diagnostic shape validation; matching dirty flags do not
identify source content and cannot certify a candidate. No dirty-tree digest
is claimed.

### Destination admission and ownership

Before deleting or overwriting any file, the command resolves the destination
and checks its receipt, lock, and staging paths. It rejects links (including
Windows reparse points), ambiguous traversal through missing directories,
paths tracked in either `HEAD` or the index (including staged deletions),
unrelated existing files, and existing lock or staging sidecars. Existing
parent-relative paths such as `../receipts/proof.json` are supported when the
traversed directory exists. An existing destination must be a coherent v1
receipt; an older-candidate or failed receipt is still a recognized prior
artifact.

After nonmutating preflight, missing parent directories may be created. An
atomic `create_new` sibling lock then establishes exclusive ownership, and
admission is rechecked before invalidating the recognized prior receipt.
The lock remains held through subject capture, execution, and publication.
Contention fails closed; an old or uncertain lock is never automatically
broken. Reconcile its owner before manually removing it. Empty directories
created before contention or refusal may remain, because their ownership may
be shared.

Preflight refusal preserves existing files and creates no new receipt evidence.
After admission and ownership, the prior receipt is removed before capture or
proof. A subsequent failed capture therefore leaves no prior green artifact.
Publication creates a new staging file exclusively and renames it only after
checking that the destination remains absent. The lock, rather than rename,
serializes cooperating producers. Normal returns attempt to remove the owned
lock and report cleanup errors. Cleanup failure or interruption can leave a
lock or staging file requiring owner reconciliation.
This protocol assumes a governed filesystem without another process replacing
artifact names behind the owner; it does not claim hostile filesystem race
protection.

### Proof and consumption boundaries

A lane failure after subject capture emits a coherent failure receipt when the
subject remains unchanged and publication succeeds. The failing step retains
its classification, and every unreached step is `not_run`. Publication errors
are reported on stderr while the original proof failure remains the command's
error. Absence means no receipt evidence; it does not identify whether capture,
execution, interruption, or publication prevented the artifact.

Both success and failure paths recapture the subject before publication. Clean
start/end observations require a governed checkout with no concurrent source
writer. They do not prove continuous immutability or detect a source change
that was restored between observations. Tool versions likewise assume the
governed PATH and environment; they are not executable provenance.

Only the admitted receipt and its lock/staging paths are excluded from Git's
dirty signal. Canonical paths are compared against the Git repository root,
including when invoked from a subdirectory. Tracked source cannot become an
artifact exclusion through the producer or verifier entry points.

`--verify-receipt <path>` runs no proof steps. It refuses malformed schemas or
unknown fields; missing, extra, reordered, or renamed steps; argv drift;
inconsistent census/outcome/exit-code combinations; execution after the first
failure; and a subject differing from the clean candidate, toolchain, or
scorecard profile. All nine steps and their typed failure rules remain the
canonical lane contract.

**Verification checks consistency and candidate identity, not success.** A
coherent failed receipt verifies successfully. A consumer deciding whether the
lane passed must inspect `result`. Verification also does not establish
latest-attempt freshness: a preflight refusal can preserve an older receipt
for the same candidate. The #8408 route consumer owns attempt association and
must not treat a preserved artifact as evidence that a refused invocation ran.
This command does not change workflow routing or artifact consumption.
