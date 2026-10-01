# Cargo storage admission

`scripts/cargo-admitted build|check|test|run|bench|doc|clippy ...` is the bounded
agent route for explicit staged rollout. The existing `scripts/cargo-safe` stays
unchanged for caller compatibility; it does **not** provide the guarantees below.
Root orchestration must explicitly choose `cargo-admitted` after host admission.
Existing callers and running processes are not automatically protected. Python 3.10+ and Cargo 1.95+ are required. Keep the qualification
profiles unchanged. Default jobs remain two (accepted range one to four), with
incremental compilation disabled. This is a capacity policy, not a speed claim.

One reusable slot is keyed by canonical Git common directory, hostname and OS,
under `DEVPLANE` (default `~/.cache/devplane`). Sibling worktrees share it;
independent clones and Windows/WSL do not. Cargo fingerprints separate compiler,
profile and feature artifacts, but the slot is serialized because final filenames
can collide. Do not create a new DEVPLANE or clone for each task. This bounds slot
count, not bytes; the capacity gate remains necessary.

The wrapper refuses aliases, external subcommands (including nextest), +toolchain,
clean, manifest/config/path overrides and job overrides. Use native absolute paths
for storage; Git Bash `/c/...` is translated on Windows. WSL must use its own
Linux filesystem, not `/mnt/...` Windows storage. Linked/junction storage paths
are refused. Compiler wrappers are not automatically enabled: their additional
cache volumes need a separately admitted route. Unsupported commands must be
explicitly admitted by the root, not silently retried as raw Cargo.

Before allocating any directory, admission checks the nearest existing ancestor
of the slot, both target/build resources, Cargo home and temporary storage. The
existing defaults remain 40 GiB minimum free AND less than 85% used. Hosts failing
either condition stay refused; do not lower thresholds to make a build pass.
`MIN_FREE_GB`/`MAX_USED_PCT` are explicit host policy, requiring an approved budget.
All TEMP/TMP/TMPDIR values are unified for the child, including command-local
overrides of Cargo `[env]` forced values. Configured compiler wrappers are disabled
with command-local empty values. Arbitrary build scripts and
programs can write elsewhere; this is not an OS sandbox or a disk reservation.

The atomic `cargo-active` directory is a conservative lease. Contenders refuse.
Cargo exit 0 or 101 releases the lease and retains artifacts. Recognized abnormal
statuses (signals and Windows termination), unfamiliar exit codes, parent
interruption and failed spawn retain it for owner verification. Exit 101 can also
represent a Cargo panic: an exit status is not proof that all descendants ended.
The root must verify independent consumers before assigning the slot again.
No age/PID-based stealing or automatic deletion exists.
The wrapper holds through `run` and `test` execution, not independent later
artifact consumers. The root must retain exclusive slot ownership for those
consumers or use a separately admitted route. Direct Cargo and older wrappers do
not honor this lease: quiesce those consumers before rollout. This is not a
system-wide concurrency limit across repositories.

The emitted JSON resource descriptor belongs in the existing root-held frame.
Every terminal task returns each worktree and external resource as `released`,
`retained with reason`, or `awaiting owner verification`. A reusable cache is
normally retained. Unknown/active resources, dirty/untracked/ignored source or
evidence and detached/unpushed commits must be preserved. Read-only reconciliation
produces exact-path proposals, never inferred deletion authority. The legacy
`target-gc.sh --apply` is retired because it does not share this lease protocol.

Cargo's global cache GC cleans dependency downloads, **not** target/build outputs.
Cargo 1.95 full/profile/doc clean does not acquire the build lock, and a separate
build directory can also be erased by clean. Do not use Cargo clean as a safety
primitive. Any future cleanup requires authorization and exclusive ownership of
BOTH canonical resources and every consumer. There is no automatic cleanup here.

Rollout: run focused fixtures; review the candidate; record host budget and existing
consumers; route new tasks through cargo-admitted; retain old caches pending individually
approved proposals. Do not move active target directories or restart workers.
Changing the wrapper does not retrofit currently running processes. Revert the
candidate to roll back code, preserving all retained resources for review.

Proof: `python scripts/tests/test_cargo_admitted_storage.py` covers admission, bounded
reuse, lock refusal, cancellation retention, path translation/junction refusal and
non-destructive failure. The cleanup sweep fixture checks command-local fetch
maintenance suppression; it does not change global Git configuration.
