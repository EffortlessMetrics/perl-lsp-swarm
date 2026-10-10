# Cargo storage admission

`scripts/cargo-admitted build|check|test|run|bench|doc ...` is the bounded
agent route for explicit staged rollout. The existing `scripts/cargo-safe` stays
unchanged for caller compatibility; it does **not** provide the guarantees below.
Root orchestration must explicitly choose `cargo-admitted` after host admission.
Existing callers and running processes are not automatically protected. The admitted
route sets `RUSTUP_AUTO_INSTALL=0` before even the version probe, preserves it for
Cargo, and forces it in Cargo's child environment; a missing pinned toolchain
refuses instead of automatically installing. This changes no global rustup setting.
See the [rustup environment reference](https://rust-lang.github.io/rustup/environment-variables.html).
Python 3.10+ and Cargo 1.95+ are required. Keep the qualification
profiles unchanged. Default jobs remain two (accepted range one to four), with
incremental compilation disabled. This is a capacity policy, not a speed claim.

One admission/lease slot is keyed by canonical Git common directory, hostname and
OS, under `DEVPLANE` (default `~/.cache/devplane`). Sibling worktrees share this
exclusion domain; independent clones and Windows/WSL do not. Within that slot,
`worktrees/<sha256-of-canonical-worktree-path>/target` and `build` are private to
each worktree. Invoking from a nested directory resolves the same worktree root.
Git path output is read as bytes and removes exactly its final LF; valid POSIX
trailing spaces, tabs, carriage returns and newlines remain part of path identity.
POSIX non-UTF-8 path bytes also remain lossless when hashing the common directory
and worktree. Inherited `GIT_DIR`, `GIT_WORK_TREE` or `GIT_COMMON_DIR` overrides
refuse before allocation, including empty values: Cargo would still execute from
the invocation directory while Git could identify another source. Unset the
selector explicitly and invoke from the intended worktree; the route does not
silently retarget an inherited subject.
Both paths must be isolated: separate final output with shared intermediates still
reuses incompatible libraries. Cargo home remains independently reusable.

Cargo's workspace-relative fingerprints and mtime checks can consider a sibling's
different source fresh when both worktrees predate the build. Serialization,
disabling incremental compilation, a passing test feature variant, and an outer
Git version stamp do not establish ordinary-library source identity. This is
reproduced on Cargo 1.95 and 1.99; see [#11650](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11650)
and [Cargo #12516](https://github.com/rust-lang/cargo/issues/12516).

The common slot and its existing lease stay unchanged. Old shared target/build
paths are retained without reading them as candidate proof, moving them, or
cleaning them. Explicit target/build environment overrides are accepted only when
they equal the resolved private paths; stale common-slot or sibling paths refuse.
Machine `DEVPLANE`, Cargo home, temporary-storage settings, and repository toolchain
pins retain their existing semantics. Each worktree reuses its own paths across
normal edits, not one new target per task or commit. Storage therefore grows with
retained worktrees, and the capacity gate remains necessary. Do not repurpose a
worktree path or restore backdated source over its build state without a separately
verified fresh qualification; this path partition is not content-addressed state.

The wrapper refuses aliases, external subcommands (including clippy and nextest), +toolchain,
clean, manifest/config/path overrides (including output/artifact/build directory
flags) and job overrides. Clippy requires a separately admitted route because Cargo
can resolve it through an alias or external executable. Before `--`, joined single-dash tokens are refused except pure verbosity
(`-vv`, `-vvv`, etc.); `-vj8`, `-pfoo` and `-Ffoo` are refused.
Spell other permitted short options and values separately (`-p foo`, `-F foo`);
job/configuration overrides remain refused. Arguments after `--` are passed to
the test/program unchanged. The wrapper forces command-local
`unstable.unstable-options=false`; inherited configuration or environment cannot
enable unstable artifact-directory copying through that switch. Such configured
output requests can fail rather than create extra output. The wrapper does not
support unstable output options. Use native absolute paths
for storage; Git Bash `/c/...` is translated on Windows. WSL must use its own
Linux filesystem, not `/mnt/...` Windows storage. Literal `{` or `}` in expanded
resource paths are refused: Cargo path templates are unsupported because expansion
would make effective output differ from the admitted descriptor. Linked/junction storage paths
are refused. Compiler wrappers are not automatically enabled: their additional
cache volumes need a separately admitted route. Unsupported commands must be
explicitly admitted by the root, not silently retried as raw Cargo.

Before allocating any directory, admission checks the nearest existing ancestor
of the slot, both target/build resources, Cargo home and temporary storage. The
default `CARGO_STORAGE_POLICY=percent` preserves 40 GiB minimum free AND less
than 85% used. The 85% figure is an inherited legacy heuristic, not a measured
requirement for this workload. The 40 GiB reserve is a preserved safety floor,
not a proven universal reserve. `MIN_FREE_GB`/`MAX_USED_PCT` remain explicit host
policy in the unchanged percentage mode; do not tune them simply to pass a build.

For constrained hosts, the existing route also accepts an exact-command
`--preflight` and an explicit scope-bound `--budget-file`. See
[Cargo work on a constrained host](../how-to/CONSTRAINED_CARGO_BUILDS.md) for the
schema and operator sizing obligations. The no-build Python preflight reports
host/cwd/worktree, argument and captured-environment digests, effective paths and
jobs without allocating resources or acquiring a lease. A generic storage doctor
or writer preflight does not assess Cargo capacity.

A declared budget may select a smaller reserve only with an operator-owned sizing
basis for the exact request. `basis_verified: false` remains explicit: no measured
small-cloud profile is supplied. Execution checks reserve plus total additional
peak growth on every destination again inside the lease. This does not reserve
bytes, enforce runtime quotas, attest all source/config/cache state, apportion
separate volumes, admit compiler caches or account for cross-repository consumers.
Without this option, the policies below retain their existing thresholds.

For a host with an evidenced byte budget, the root may explicitly select
`CARGO_STORAGE_POLICY=byte-budget`. This requires both:

- `CARGO_EXPECTED_GROWTH_GB`: a finite positive conservative upper bound on the
  **total additional storage** across target, build, dependency downloads and
  temporary files for the admitted work, in GiB; and
- `CARGO_STORAGE_BUDGET_EVIDENCE`: a nonempty reference to the measurement or
  approved basis for that bound, not secret contents. The wrapper records the
  reference but does not read it or independently validate the estimate.

This mode checks `free >= reserve + total expected growth` at every effective
destination's nearest existing ancestor. Reserve defaults to 40 GiB and cannot
be lowered below 40 GiB in this mode. A shared volume can be checked repeatedly,
but the growth bound is never added once per output path. The entire total bound
is conservatively required on each distinct destination volume. Valid explicit
byte-budget mode replaces the percentage check; it does not modify `cargo-safe`,
release/security gates, or choose a policy automatically from host capacity.
Missing or invalid inputs refuse before allocation. The resource descriptor
reports policy, reserve, budget, evidence reference, observed free bytes and
remaining headroom for each destination.

This is admission, not a disk reservation. Root/native task ownership must hold
one heavy-build lane and account for concurrent consumers; no new task manager
is introduced. No real-build growth measurement accompanies this option, so the
current host is not admitted merely because it has enough bytes for the reserve.

All TEMP/TMP/TMPDIR values are unified for the child, including command-local
overrides of Cargo `[env]` forced values. Configured compiler wrappers are disabled
with command-local empty values. Arbitrary build scripts and
programs can write elsewhere; this is not an OS sandbox or a disk reservation.

The atomic `cargo-active` directory is a conservative lease. Contenders refuse.
Cargo exit 0 or 101 releases only the invocation's matching lease and retains artifacts.
Cleanup requires the original directory identity and unique ownership marker;
replacement leases, including copied markers, remain for owner verification.
An unlaunched preparation failure releases only its own lease. Storage paths are
revalidated for links before allocation and launch; this is not an adversarial
filesystem sandbox. Recognized abnormal
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

When OpenClaw allocates the work, reuse its existing native task ID and node ID
or worker identity. Attach or reference the current worktree, both resolved Cargo
target/build paths from the emitted descriptor, and each resource's disposition
and evidence-retention reason in the existing native terminal status. Success or
cancellation does not authorize cleanup. Do not introduce a competing task registry
or status manager: `cargo-active` is only a local exclusion primitive, not an
ownership database. The bridge owner owns the concrete adapter; its native
interface remains to be established. This contract does not invent API fields or
claim that an adapter is already installed.

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

Proof: `python3 -m unittest discover -s scripts/tests -p 'test_cargo_admitted*.py'`
runs both admission suites. The storage suite covers admission,
worktree-private paths with a common lease, same-name and trailing-whitespace
worktree distinction, exact
and stale override handling, lock refusal, cancellation retention, path
translation/junction refusal and non-destructive failure. Set
`CARGO_ADMITTED_REAL_BUILD_TEST=1` for the offline two-crate linked-worktree A/B/A
behavior regression; use an already installed toolchain. It executes real Cargo
through the production Python entrypoint, but uses a fixture capacity observation
and makes no host-budget claim. Its TemporaryDirectory has one sequential owner;
no next build starts until the current behavior assertion returns. It does not
prove concurrent or independent post-lease binary consumption. Production callers
must still hold ownership through consumers or capture immutable artifacts before
releasing that ownership. The cleanup sweep fixture checks command-local fetch
maintenance suppression; it does not change global Git configuration.

Parser preparation (#17479) consumes `CARGO_ADMITTED_RESOURCES` from the live
admitted parent: exact worktree, final/intermediate pair and lease marker. It
reuses those roots sequentially before integration libtest; it allocates no nested
slot, changes no capacity policy, and cleans no outputs. The descriptor is a trusted
parent observation, not a cryptographic admission token or a host-wide reservation.
Root admission must include every parser preparation command (strict Clippy and
nonzero library tests included) and all other selected nested workloads. Other
suite build fallbacks remain separately unqualified; private outer roots alone
do not authorize concurrent nested compilation. Raw parser preparation without
the live parent descriptor refuses. Native validation must execute the owning
runner and resource controls before claiming the full canonical runtime.

`scripts/ci/pr_smoke_resources.py` is a read-only consumer of the existing
`resource_plan`: it writes both canonical output paths to `GITHUB_ENV`, allocating
no directories and granting no lease or capacity admission. It rejects paths that
cannot be represented as one environment-file line. This helper is not activated
in PR Smoke yet. Exporting a live descriptor from an outer admitted `cargo run`
does not admit every command that its program or tests might launch. In particular,
the parser's existing strict Clippy shape differs from the finite Linux route in
#17506. Supported nested command admission and Linux settlement #17507 are
prerequisites to activating the proposed hosted handoff; unsupported descendants
must refuse before product preparation starts. Compiled-output cache restoration
also remains unqualified by this storage policy. Dependency downloads can be
cached separately without restoring final or intermediate outputs.
