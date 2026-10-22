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
Builtin routes require Python 3.10+ and Cargo 1.95+; staged Clippy requires Python 3.11+ and the exact installed repository pin 1.95.0. Keep the qualification
profiles unchanged. Default jobs remain two (accepted range one to four), with
incremental compilation disabled. This is a capacity policy, not a speed claim.

Host profiles for the jobs knob (#17673): the default of two protects
constrained and shared hosts. On a dedicated host with at least eight logical
cores and memory headroom for parallel rustc processes, setting
`CARGO_BUILD_JOBS=4` in the shell environment (never in tracked config,
#5204) is the supported faster profile. Measured on an 8-core/16-thread,
80 GiB host (fresh target, shared cargo-home, `check -p perl-parser`,
one run per setting): cold 80.2s at jobs=2 versus 31.9s at jobs=4; warm and
single-crate edited checks are under two seconds at either setting, so the
win is concentrated in cold and multi-crate work. Peak memory at jobs=4
was not captured, so treat the profile as unqualified on memory-tight
hosts and on hosted CI runners, which have different core/memory envelopes
and should keep the default.

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

For a focused `xtask` gate, use the supported builtin `run` route from the same
canonical worktree and with the same admitted `DEVPLANE`, Cargo home and temporary-storage
settings as the earlier producer:

```bash
scripts/cargo-admitted run -p xtask --locked -- gates --gate perl_token_leaf_contract --receipt --base origin/main
```

The `xtask` alias itself is refused. A raw `cargo run` retry without the admitted
target/build settings can rebuild under the repository's `target/`.
Verify the earlier producer's settlement and released lease before retrying;
preserve gate profiles, features and timeouts. On constrained hosts, prepare a
budget and preflight for this exact command as described below. Reusing the same
paths does not guarantee freshness across different dependency/feature graphs.

The wrapper refuses aliases, arbitrary external subcommands (including nextest), +toolchain,
clean, manifest/config/path overrides (including output/artifact/build directory
flags) and job overrides. The staged Linux Clippy route below binds direct installed
drivers rather than Cargo alias/external discovery. Before `--`, joined single-dash tokens are refused except pure verbosity
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
Builtin Cargo on Linux and staged Clippy release only after their owned Linux
kernel child scope proves settlement
and matching lease release succeeds. Exit 0/101, leader/session absence and a
disconnect never establish that proof. Ambiguity or deadline expiry retains it.
Cleanup requires the original directory identity and unique ownership marker;
replacement leases, including copied markers, remain for owner verification.
Linux uses the existing exclusive single-thread CPython subreaper/pidfd owner;
missing capabilities refuse without a leader-only fallback. Builtin Cargo
session and controlling-terminal behavior are preserved: unlike finite Clippy,
builtin launch inherits the caller's session/group. Cancellation never signals
that shared group; the existing tree owner validates and signals bound child
pidfds before proving kernel closure and releasing the matching lease. Builtin
launch receipts identify the inherited group without claiming ownership of it.
Builtin command, PATH/toolchain selection, argument order and environment
projection are unchanged.
Original lease identity/marker and operation-labelled Cargo launch/product/settlement
receipts make product exit independent of terminality. Any product status can be
returned after proven closure and matching release; cancellation returns 130.
Incomplete closure or failed release returns 75 and retains ownership.

### Explicit Linux proc-parent/pidfd discovery

`CARGO_ADMITTED_CHILD_DISCOVERY=proc-parent-pidfd` explicitly selects a bounded
alternative for Linux hosts lacking `CONFIG_PROC_CHILDREN`. The default
`proc-children` profile is unchanged; no missing-table detection silently selects
a namespace scan. The alternate profile reads only visible numeric PID `stat`
records, including their parent and kernel starttime. It does not read process
environments, command lines, descriptors, mappings or executable paths.

`child_discovery_capability(mode)` validates the live calling owner before any
lease or allocation. The profile requires one native CPython owner thread, default
SIGCHLD, matching singleton procfs/native PID identities, the same current and
child PID namespaces, and one full procfs root without hidepid restrictions or
numeric/task-path overmounts. It revalidates these observations on discovery
turns. Unreadable or malformed records, changed roots, more than 32,768 numeric
entries, or a discovery turn exceeding one second refuse. This elapsed-time
threshold includes metadata and the final observation; it does not preempt an
individual blocking procfs syscall. Vanished entries may
be skipped; no incomplete snapshot claims closure.

PPID only nominates a direct-child candidate. `track()` must bind a pidfd and
validate kernel child waitability with `__WALL` before that handle may receive a
signal. An exited Popen leader's stale numeric ledger key is removed before a
new child is bound; its completed status never hides a reused PID. The exclusive
single owner does not reap between discovery and binding, so a genuinely owned
unreaped child retains its PID. A stale or foreign nomination cannot become
signal authority through PPID alone.

Cancellation repeats discovery between kernel drains to cover new adoption and
detached generations. Final ECHILD remains the sole closure authority; bounds,
observation failures and unsettled descendants retain the original lease. The
selected profile is captured in the native budget environment digest, resource
descriptor and settlement receipt. Cgroups shared with provider/other tasks do
not supply exclusive cancellation authority.

A separate cohort consumer must explicitly qualify and select the same admitted
profile at preflight and owner construction. The public constructor supports
`ClippyTree(child_discovery=mode)`; a capability descriptor is an observation,
not a permanent completeness attestation or release permission. This extension
does not automatically activate a cohort branch or qualify its disk budget.

Other platforms retain the legacy builtin behavior: exit 0/101 releases its
matching lease, without a descendant-settlement guarantee; abnormal statuses and
interruption retain it. This Linux correction does not qualify native Windows,
macOS, WSL as native Windows, or canonical executor completion (#11659/#11660).
Native Windows remains owned by #17482. Independently owned consumers still need
separate proof, and old attempts cannot be retroactively adopted.

An unlaunched preparation failure releases only its own lease. Storage paths are
revalidated for links before allocation and launch; this is not an adversarial
filesystem sandbox. An unproven Linux tree or failed matching release retains it
for owner verification regardless of leader status. Exit 101 can also
represent a Cargo panic: an exit status is not proof that all descendants ended.
The root must verify independent consumers before assigning the slot again.
No age/PID-based stealing or automatic deletion exists.

## Cohort lifecycle prerequisite (#17617)

`scripts/cargo_cohort.py` provides a dedicated-process `Cohort` API beneath an
already admitted Linux outer owner. It neither acquires nor releases the outer
lease. The original resource descriptor must belong to a live ancestor and match
the native resource plan, original lease directory, and unique ownership marker.
The cohort reuses `ClippyTree`; it requires readable child discovery for its
actual live thread before launching work, then requires kernel `ECHILD (__WALL)`
before reclamation. A successful producer or consumer exit alone is insufficient.
Unavailable discovery refuses without starting the producer.

The trusted producer supplies complete source/profile manifests with absolute
worktree file paths and SHA256 digests, exact expected compiler target/feature/
profile records, pinned paths, and a new evidence path outside both executable
roots. The API rechecks manifest digests and each listed file. Manifest
completeness, command provenance, effective compiler configuration, test selection,
and absence of independent consumers remain the admitted caller's obligations;
this library does not authenticate a fabricated descriptor or Cargo JSON. Its
single producer and exact executable consumers run within one owned child scope.
The original outer owner must keep both private resources exclusive throughout.

Only newly emitted, non-fresh test-harness compiler artifacts matching the
expected records can become cleanup candidates. Each executable is bound to its
canonical path, parent directory, device/inode, file metadata, and SHA256. The
cohort executes that exact path and revalidates it before unlinking. The default
API refuses preexisting and fresh products. The native producer explicitly enables
retained consumption: exact fresh or preexisting Cargo harnesses can run under the
same identity, hash, source/profile and runtime checks, but never acquire cleanup
authority. A rebuilt preexisting path or any preexisting alias also retains the
whole group. Receipts distinguish retained products from reclaimed groups. Pinned,
foreign, replaced and symlinked files still refuse. Reclamation requires a complete
newly produced private inode group, described below. Successful
reclamation consumes that path's cleanup authority; repeated cleanup cannot adopt
a replacement. Before reclamation, producer failure, consumer failure,
interruption, uncertain settlement, or changed bindings prevents reclamation.
An error during sequential per-path reclamation stops further deletion and
preserves remaining artifacts; cleanup is not transactional. Product status and
effective status are separate: uncertain closure is 75; settled cancellation is 130.
Producer JSON evidence remains retained. Before deletion, a flushed and synced
sidecar records captured file identities/hashes, source/profile bindings,
consumption, and kernel settlement; successful per-path reclamation is appended.
Failure to persist authorization prevents deletion. The unlink and completion
record are not an atomic transaction. Inherited diagnostic and consumer streams
must also be captured by the admitted caller.

The staged native producer and complete private alias support described below extend this prerequisite. The routed gate remains on its original command. No gate package, feature, profile, reserve, timeout, or quarantine changes. The API is not an adversarial filesystem
sandbox: exclusive cooperative ownership is required, and final unlink is not an
atomic compare-and-delete against hostile concurrent writers. Tests distinguish
filesystem/interface doubles from actual native process proof:

```bash
python3 -m unittest discover -s scripts/tests -p 'test_cargo_cohort.py' -v
```

In the default discovery profile, a host without readable
`/proc/<owner-pid>/task/<owner-tid>/children` must demonstrate prelaunch refusal.
Known-child pidfds and eventual
subreaper closure do not supply discovery of unknown live detached children.
End-to-end cohort cancellation and reclamation require a supported native host;
portable fault-injected controls cannot replace that qualification.

Nested host/test runners that promise reaping before return must own and reap
their inner child scope before reporting that boundary. Later cohort or outer
Cargo settlement cannot retroactively satisfy it. A terminated but unreaped
zombie does not satisfy a synchronous-reaping contract, and changing its liveness
oracle to report dead would weaken that proof. Inner runner failures remain
product failures even when the cohort subsequently settles successfully.

## Staged routed producer and exact Cargo alias groups

`xtask/src/bin/routed-test-cohorts.rs` is a dedicated native producer, reached only
through the explicitly staged outer-owner route:

```text
scripts/cargo-admitted --routed-test-cohorts [--budget-file ABSOLUTE_FILE] run --locked -p xtask --bin routed-test-cohorts -- -p PACKAGE [-p PACKAGE ...] --evidence-dir ABSOLUTE_NEW_WORKTREE_DIRECTORY
```

Use an already authorized, exact host/task budget; this command provides no new
reserve or growth allowance. The canonical owner remains `cargo-admitted` and
`ClippyTree`. The staged selector verifies live-thread child discovery **before
compiling the native producer**, then holds one original lease through setup,
public server build, eligibility observation, every cohort, consumer, and cleanup.
The original descriptor is forced into command-local child configuration; the
caller cannot replace it with an inherited value. Non-staged invocation remains
unchanged. The thin Python CLI bridge neither acquires a nested lease nor falls
back to raw unowned Cargo. It rechecks the original capacity allowance before
each child launch. A separate observation operation can run metadata/check/build
setup, but has no consumption or reclamation authority.

The producer builds `perllsp` with the original default build intent once, captures
its actual private executable from compiler JSON, and retains both that source and
an exclusive immutable proof copy. Copying uses bounded reads/writes, original
file descriptor identity, before/after source validation, file/directory fsync,
and a retained hash/identity receipt. Each subsequent cohort verifies the copied
product's exact inode metadata and bytes and uses it as `PERL_LSP_BIN`.

Cargo metadata supplies target inventory, not activated-feature authority. An
admitted `cargo check --locked --tests --profile test` with **every original
package and feature switch** observes actual eligible harness targets, activated
features, and effective profiles without bulk test executable codegen. Cargo 1.95
includes check test units in `ArtifactProfile.test`. Every codegen cohort uses
`cargo test --no-run` with that same complete selection before a target selector;
check and build target/feature/profile records must agree exactly. Duplicate
names are grouped across packages. Selector collisions with `test=false` or
feature-disabled targets, missing unconditionally eligible targets, or unsupported
kinds refuse rather than changing the original suite. The newly added native bin
is included in ordinary `xtask --tests` inventory; it is not exempted.

For each captured executable, the bridge retains the canonical package working
directory and Cargo's target-process metadata environment. The staged outer owner
freezes a finite set of original Cargo/loader fields before native bootstrap and
both producer and consumer launches restore them before Cargo or the consumer applies package context. Registry credentials
are excluded, and receipts expose only the environment digest. Runtime context
follows Cargo's order: package build-script environment, `CARGO` and complete
package metadata, package cwd, and configuration. Native loader paths follow
Cargo's ordered root/deps/sysroot construction and original loader environment;
package build-script outputs must match the observed check context. Cargo's final
`UnitOutput.env` layer adds `CARGO_BIN_EXE_<name>` paths for package binaries to
integration and benchmark processes; the runtime map models this separately from
the earlier `fill_env` context. Library, binary and example harnesses preserve the
original environment without that extra layer. Cargo metadata omits
`required-features` for an unconditionally enabled binary; absence means no
requirement, while malformed present values are refused.
[Cargo documents `CARGO_BIN_EXE_<name>` for integration tests and benchmarks](https://doc.rust-lang.org/cargo/reference/environment-variables.html)
and says that a binary with unmet `required-features` is not built. Cargo 1.95's
final unit environment still assigns each package bin its target executable path;
for a feature-disabled bin that path can intentionally name a file which does not exist.
The checked-in resolver-2 fixture covers enabled and disabled bins in both
integration-test and benchmark targets, comparing compile-time and run-time paths
and checking the disabled file remains absent. Pinned Cargo 1.95 executed all three
fixture commands successfully on the supported hosted runner with the existing
40 GiB reserve. The producer now derives each package binary's runtime path from
the admitted Cargo target root. The hosted fixture confirmed that Cargo's
`CARGO_BIN_EXE_*` paths can be under `target/debug` while test harnesses are under a
separate `build/debug/deps` root. The bridge matches enabled paths to exact Cargo
compiler-artifact records and represents disabled bins as absent-path placeholders;
it refuses path or artifact disagreement. Full lifecycle and exact-suite equivalence
remain required before route activation. Custom runner,
external/parent Cargo configuration, configuration includes, cross-target requests,
custom Cargo environment beyond this repository's `RUST_BACKTRACE`, and compiler/
loader injection selectors remain unsupported and refuse. The existing native
compression flags in the source-bound root Cargo configuration remain effective.

The native wrapper preserves inner effective status 75 for unproven closure and 130 for settled cancellation; later outer settlement cannot upgrade that failure. Original product status remains in retained bridge evidence.

The bridge inventories all and ignored cases using the real standard libtest
`--list` interface, then executes the default unfiltered harness. Successful
execution must report each listed case exactly once with the same ignored
population. Unsupported harness output or hidden omissions/duplicates prevents
cleanup. Inventory and actual consumer stdout remain durable alongside compiler
JSON; stderr retains the original inherited diagnostic stream. The admitted caller
must retain that stderr stream as well.

Compiler JSON authorizes an inode candidate, not alias names. The lifecycle owner
finds all exact device/inode matches only within the two canonical private roots,
requires observed group count equal to kernel `st_nlink` and rejects pinned
aliases. Cleanup additionally requires every path absent from the pre-production
census and a non-fresh compiler artifact; retained consumption cannot satisfy or
override those cleanup requirements.
An external or otherwise unobserved hardlink rejects the entire group. It holds
the original inode and parent descriptors through consumption and closure. After
each exact relative unlink, it checks the held inode's permitted ctime/nlink change
and revalidates every remaining alias; content, replacement, new-link, or parent
changes stop cleanup. The durable pre-delete receipt includes the entire alias
group, identities, hash, binding, coverage, and kernel settlement. Each successful
owned unlink is appended and synced. Cleanup remains sequential and nontransactional:
a later error preserves remaining aliases and evidence, not previously removed
names. Shared libraries and metadata are never selected by filename patterns.

This source candidate does **not** activate `unit_routed_full`. Local Python
controls distinguish filesystem/interface doubles and a synthetic finite resolver
model from real Cargo/native proof. The checked-in two-crate resolver-2 fixture
contains a development dependency that activates another package's required test
feature, duplicate test names, an enabled bin, an unbuilt required-feature-disabled
bin, and integration/benchmark runtime checks for both binary paths; its actual Cargo execution passed in hosted run 38229918719 at candidate
8604007faaa0ad9dc21d73c8d9b55cef6c1a32a1. On an admitted Linux host with the repository's
existing reserve, run the exact fixture with:

```bash
cd scripts/tests/fixtures/routed-resolver-2/b
../../../../cargo-admitted test --locked --offline -p cohort-b --test shared
../../../../cargo-admitted test --locked --offline -p cohort-b --features enabled-bin --test runtime_env -- --nocapture
../../../../cargo-admitted bench --locked --offline -p cohort-b --features enabled-bin --bench runtime_env
```

The first invocation checks paths for two feature-disabled bins, the second checks
compile-time and runtime paths for one built bin and one disabled bin, and the third
does the same in Cargo's benchmark environment. These are fixture commands only;
they do not activate or replace the production route.
Native producer compilation and all eleven current unit tests passed on
`52c7a004ef58f8e80fa18de5c1c26ebca004546b` in hosted run `38241188886`. Exact original-suite case/feature equivalence, configured runtime contexts,
full-route storage/timing measurements, and reserve qualification remain pending.

`scripts/measure_cargo_storage.py` now supplies the previously missing sampled
allocation measurement path. It reuses `storage-doctor`'s GNU `du` allocated-block
semantics: unique `(device, inode)` identities contribute `st_blocks * 512`,
including directories and symlinks without following links. One traversal across
every supplied root counts shared hardlinks once. Vanishing entries during an
active build are explicitly counted as a scan limitation; other I/O errors make
the sample invalid. Evidence is exclusive, outside measured roots, flushed and
synced; unreadable/partial or interrupted scans are explicit invalid observations.
Each sample records scan duration and per-filesystem available/total bytes; the
header records the requested interval. Per-root hardlink attribution follows root
order; the aggregate is deduplicated across all roots. Command-only timing is
recorded separately from the final retained scan. Missing roots contribute zero only at the
observation time. A sampled peak is a lower bound on transient peak, never a
verified growth bound: scans are non-atomic, sampling can miss short-lived output,
and filesystem free space includes independent writers. Allocated blocks are not
exact overlay/reflink physical accounting. The optional `--min-free-bytes` guard
signals the admitted owner on an invalid scan, observed reserve loss or
evidence-persistence failure; it
cannot guarantee continuous reserve preservation between samples. It acquires no
lease, grants no admission or cleanup authority, and does not replace native
child settlement. Command failure remains failure; measurement failure is not
reported as a successful disk qualification.

The campaign-only ordinary/cohort jobs in CI Gate Self-Tests measure the original
six-package selection (`perl-dap`, `perl-lsp-rs`, `perl-parser-core`,
`perl-semantic-analyzer`, `perl-workspace`, `xtask`) on separate fresh hosted
runners. They retain the 1,500-second route watchdog, default features/profile,
two build jobs and 40 GiB reserve, and preserve measurements and diagnostics as
artifacts. The measured roots include admitted target/build, Cargo home, temporary
storage, the immutable public proof copy and cohort evidence, and command logs.
The observer journal is outside these roots; its own small allocation and later
report/artifact packaging overhead are excluded and must not be called total host
allocation. Per-filesystem free-space samples still observe those bytes.
`report_routed_storage_qualification.py` publishes source-bound unit/features/profile
and case/ignored-inventory hashes, package counts, sample gaps and settlement into
the hosted log. It rejects conflicting unit contexts, extra packages and ambiguous
or incomplete libtest boundaries; six-package completeness requires all six
packages and settled compiler observation. Partial inventories cannot establish
full equivalence. Successful complete case inventory also requires every native
receipt to succeed and settle, and the overall command to succeed; failed observed
populations remain inspectable separately. They do not activate or replace the production gate. The existing policy permits
one retry; qualification uses one separately measured warm retry only after a
timeout with proven kernel closure and matching lease release. Each attempt keeps
the 1,500-second bound. First-attempt artifacts and journals stay preserved, and
there is no retry after an ownership refusal or new cleanup authority over
preexisting products. Full equivalence
and measured fit require completed exact-candidate results; job presence is not
proof.

```bash
python3 -m unittest discover -s scripts/tests -p 'test_measure_cargo_storage.py' -v
python3 scripts/measure_cargo_storage.py --root /absolute/private/target \
  --root /absolute/private/build --interval 1 --min-free-bytes 42949672960 \
  --evidence /absolute/new/storage.jsonl -- scripts/cargo-admitted test --locked -p PACKAGE
```
Core lifecycle evidence from another candidate cannot substitute for these new
alias and producer qualifications. Required test gates and existing quarantines
remain unchanged, as do DAP and inner synchronous-reaping owners.

```bash
python3 -m unittest discover -s scripts/tests -p 'test_cargo_cohort.py' -v
python3 -m unittest discover -s scripts/tests -p 'test_routed_test_cohorts.py' -v
```

The resolver and runtime boundaries follow pinned Cargo 1.95 source:
[resolution before target filters](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/ops/cargo_compile/mod.rs#L271),
[test/check profile identity](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/build_config.rs#L245),
[alias production](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/mod.rs#L589),
[final unit environment](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/artifact.rs#L17),
and [target process context](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/compiler/compilation.rs#L329).

## Staged native Clippy

The existing owner admits these finite Linux requests (optional `--offline`):

```text
python3 scripts/cargo_admitted.py clippy -p PACKAGE --all-targets --profile agent --locked -- -D warnings
python3 scripts/cargo_admitted.py clippy -p PACKAGE --test NAME --profile agent --locked -- -D warnings
```

Select exactly one of `--all-targets` or `--test NAME`. A named test uses one literal
ASCII name beginning with a letter or digit and containing only letters, digits,
underscores or hyphens; patterns, paths, joined forms, multiple selectors and other
target/feature selectors refuse before allocation. Cargo still checks prerequisites
of that target and reports a missing target as a product failure. The selected name
remains in the request digest, so its budget cannot transfer to another target or
all-targets work. A focused result covers only the selected compilation graph; it
does not replace a required all-targets gate or qualify an unvisited target.

Python 3.11+ and the already-installed repository pin 1.95.0 are required. No
installation occurs. The route fingerprints direct Cargo/rustc/rustdoc,
cargo-clippy and sibling clippy-driver plus compiler runtimes, repository pin,
owner source and discovered Cargo/Clippy configuration into the budget scope;
it revalidates tools/config before launch and checks matching native versions
under the same lease. These observations bind local file identity, not signed
distribution provenance. It refuses wrappers, compiler/lint/loader selectors,
Cargo config includes and configured loader environment entries. Bounded TOML
validation covers quoted/dotted keys; there is no partial text parser. Forced
Cargo child environment cannot replace projected tools, lint policy or resource
paths. The renderer enforces one explicit native target and private target/build
directories. Raw flags, `--fix`, help/version and other no-work shapes refuse.

The direct driver gets the literal `clippy` token, exact child Cargo and compiler
identities. Cargo JSON artifacts can witness compilation work; setup version
success never qualifies package linting. Cancellation signals the owned native
session group. Before the first setup/product child, the process-local Linux
subreaper requires a single native CPython thread, no existing unrelated children, a known
default SIGCHLD action without auto-reaping, and supported pidfd child waits and
signals. The same owner reaps adopted descendants, including setsid/double-fork
helpers, until kernel ECHILD with __WALL proves all its child types have ended.
Only that proof plus matching lease release permits reuse; no whole-host process
or environment scan is required. The recorded leader result remains separate
from tree settlement. Proven settlement and release after cancellation returns
status 130, preserving the product exit separately. Failed settlement or release
returns admission status 75, including cancellation with incomplete closure.
This staged route does not close the canonical
typed executor/parity contracts #11660/#11663 or cover independent external
services/artifact consumers. The guarantee covers descendants of newly launched
setup and product children in this exclusive Linux scope, including descendants
that change sessions. Work delegated to an unrelated existing service is outside
that tree and requires separate ownership proof. Native Windows Clippy remains
unqualified; other platforms are refused.

Cancellation uses bound pidfds validated as this owner's waitable children.
The current driver's unreaped native session can receive TERM; owned handles
receive TERM then KILL after2seconds, with at most30seconds of post-leader drain.
In the default discovery profile, only the owner's immediate-child table discovers adopted cancellation
candidates. If that table is unavailable, kernel waits can still prove eventual
settlement, but unknown surviving detached helpers cannot safely be signaled:
deadline/error retains the lease. There is no automatic broad /proc fallback;
the separately selected proc-parent/pidfd profile above supplies discovery only.
Missing
capabilities, existing children/threads, lost subreaper ownership, ambiguous waits,
owner death and incomplete cancellation never become release authority.

The opt-in `scripts/tests/qualify_admitted_clippy.py --proof-root ABSOLUTE_DIR`
creates a dependency-free offline fail/clean fixture. Its scoped reserve 4 GiB and
growth 2 GiB apply only to that fixture, with every destination checked. The
default 40 GiB reserve remains unchanged. It preserves logs, source hashes,
budget/volume observations and original-owner kernel settlement/release receipts.
It independently records exact-group absence; that additional observation does
not authorize arbitrary workload cleanup or cover independent consumers.

### Recovering a retained Clippy lease

Capture the `cargo-admitted resources:` JSON before setup starts, together with
launch/product/cancellation logs and the exact owner revision. It records the
original `lease`, `lease_identity` (device/inode) and unique `lease_marker`;
`cargo-admitted Clippy launch:` records the native host and process group. These
fields are evidence for recovery, not automatic release authority. New owned
operations emit `cargo-admitted Clippy settlement:` with kernel proof and release
postcondition. Previously retained attempts launched without that subreaper scope
cannot be adopted or retroactively proven by a later owner.

For new operations, a trustworthy matching settlement receipt with
`tree_settled:true` and `proof:"kernel ECHILD (__WALL)"` replaces any whole-host
scan requirement for the launched tree. Verify original ownership, any separately
admitted artifact consumers, and the release postcondition. Failed/missing proof
preserves the lease. The manual independent verification below applies to older
or interrupted operations with no kernel completion receipt:

1. Recover executor connectivity and verify the same native host and complete
   process visibility. A connection loss, age, missing leader PID, exit 0/101 or
   absent process group alone does not prove settlement. Do not signal a reused
   numeric PID/group or adopt ownership from current directory contents.
2. Independently verify no active descendant, detached helper, build script,
   binary consumer or separate Cargo operation uses the recorded source/output/
   temporary roots. Check native process ancestry/group/session, executable,
   cwd, open descriptors and mappings within authorized task visibility. Do not
   retry blocked privileged scans or inspect unrelated process environments.
   Account explicitly for known pre-existing provider processes and observation
   limits; unfamiliar active processes or incomplete consumer visibility refuse.
   Native Windows requires the separately qualified launcher/tree authority;
   Linux observations do not substitute for it.
3. Preserve the verification receipt and artifact inventory/identities. Confirm
   the original lease device/inode and exactly its recorded empty unique marker
   still match. Missing original ownership evidence, replaced/linked directories,
   extra markers/files, host mismatch or changed resource identities refuse.
4. Only after those checks, the root owner calls this revision's existing
   `release_lease(Path(receipt["lease"]), tuple(receipt["lease_identity"]),
   Path(receipt["lease_marker"]))`. This removes only the matching empty marker
   and lease directories. Verify `os.path.lexists(lease)` is false; a failed
   postcondition remains retained for investigation. Never use recursive deletion,
   unlink a replacement, clear outputs/caches or use age-only reclamation.

Already-absent leases are an observed released state, not permission to release a
new directory at the same path. If any consumer/evidence is uncertain, preserve
the lease and artifacts with a stated blocker. This is a practical manual owner
procedure; no generic automatic process-tree proof or cleanup service is added.
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

The exact-head comparison in run `38241188886` observed the same 1,228 eligible
unit/features/profile hash in both routes. Ordinary warm execution returned 101
after 750.493 seconds, with a sampled peak of 26,471,247,872 allocated bytes and
26,471,211,008 retained bytes. Cohort warm execution refused a fresh retained
product with status 75 after 322.321 seconds, with a sampled peak of
3,473,424,384 bytes and 3,440,812,032 retained bytes. Neither completed cases; these
are partial-work measurements, not completed-work savings or a disk-fit proof.
The first composition used owner repair #17640
(`7124afe5543c6003a02a00f72c3fe14d0bc1e51d`) and refreshed main. Its positive
qualification was withdrawn after PID-publication and cwd-dependent-import test
defects. Corrected dependency `e0da55faaaafd62632bc4512678809a5f8881a1c`
repairs one test file, with production/consumer bytes unchanged; it passed root,
alternate-cwd and independent affected tests, and the owner suite. It merged with all five required contexts as main
`e38d936e9ba9333858a631cd6c6aff55a8a8a431`; the candidate incorporates that
main version. Its cohort waits
service `reap_adopted()` with SIGINT/SIGTERM blocked in the same owning thread;
that service neither signals live children nor proves closure or releases a lease.
Final `finish()` remains responsible for ECHILD. Missing service refuses before
launch. The bridge claims its exclusive result path before constructing the scope
or running producer/consumers; a collision preserves prior evidence and refuses.

Adversarial review of that composition found two interactions, both repaired in
the next candidate: an externally interrupted scan must not send a second SIGINT
during orderly child shutdown, and an empty/malformed exclusive bridge result must
preserve the child’s typed 75/130 failure rather than become generic status 1.
The real single-signal control asserts exactly one delivered SIGINT and both child
and measurement status 130. A native result-decoding control retains malformed
evidence and checks typed 75/130/101 propagation. The repository-supported `xtask --all-targets --profile agent` Clippy shape is
a named hosted check through the original admitted owner, without lint overrides.

### Reserve provenance and native sizing

The 42,949,672,960-byte threshold in this campaign is 40 GiB. Its source is the
legacy `MIN_FREE_GB` default in `scripts/cargo-safe:25`, carried into
`scripts/cargo_admitted.py:626` (percent mode). Environment byte-budget mode also
requires at least 40 GiB at line 634. The campaign observer explicitly retains
that threshold for comparable hosted measurements; it is not a field in
`unit_routed_full`. That gate's policy specifies a 1,500-second watchdog, one
retry and `budgets.max_duration_ms: 1320000`, with no disk-reserve field.

These inherited thresholds do not establish a universal reserve for every
filesystem. The constrained-host route introduced in #17478 supports an explicit
operator-owned budget file bound to the complete request scope. Its
`reserve_bytes` and `expected_growth_bytes` are independent positive byte counts;
execution requires their sum at every destination and rechecks inside the lease.
It leaves the legacy defaults and unrelated gates unchanged. A fit obtained from
invented growth or copied test-fixture values is not qualified sizing. See
[the budget schema and sizing obligations](../how-to/CONSTRAINED_CARGO_BUILDS.md#supply-an-explicit-host-budget-when-the-default-does-not-apply).

Report observed demand, the fixed reserve of the actually selected policy and
available capacity separately. A sampled peak is not a conservative growth bound.
The selected local filesystem has 33,770,192,896 total bytes, but comparing that
number with a default 40 GiB threshold alone does not prove native execution
impossible under a supported scoped declaration. Current partial measurements
do not yet qualify a complete cold/warm/edit growth envelope for a new native
budget. No smaller host declaration is silently selected from free space.

Independently, this selected runtime lacks its own live-thread proc children
entry. Actual staged `--preflight` refuses at that capability check before
budget scope, lease acquisition or allocation. A read-only escalated probe
confirms the same missing entry. Budget sizing does not remove that capability
requirement; an alternative ownership/cancellation route would require its own
qualification rather than dropping the guard.

### Terminal comparison and native unit calibration

Run [38248428802](https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/38248428802)
measured candidate `d1f450b0505343b53f6e9055d94c1a6415f30ba0` through terminal
status. Both routes reported the same 1,228 eligible compiler units and
unit/features/profile SHA256
`2a76b7f053ad37e28e556b2912ca92e71d11a6dd48e425db7ff0f6595d07c74f`.
Ordinary cold execution timed out after 1,500.070 seconds with sampled peak and
retained bytes 12,225,478,656. Its settled warm retry returned 101 after 753.632
seconds, with sampled peak and retained bytes 26,476,785,664, and minimum observed
free bytes 64,957,505,536. It observed 1,799 cases, four ignored, across 27
harnesses before `perl-dap --test dap_exact_session_matrix` failed.

Cohort cold execution timed out after 1,500.059 seconds, with sampled peak
3,399,639,040 and retained bytes 3,372,441,600. It observed 60 cases across seven
harnesses, with minimum observed free bytes 88,031,166,464. The settled warm
retry timed out after 1,500.213 seconds, with sampled peak 3,603,644,416 and
retained bytes 3,592,376,320. Its reporter encountered an interrupted empty
preclaimed result; no warm case count is claimed. The reporter repair preserves
partial measurement evidence, emits receipt errors and refuses complete coverage
for missing, malformed or unsettled receipts. Both attempts ended with ECHILD and
lease release. The hosted observer's 40 GiB reserve remained observed throughout.
These are partial-work measurements: no completed-work savings, complete case
equivalence or native full-route budget is proven.

Evidence artifacts: [ordinary 11748391967](https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/38248428802/artifacts/11748391967),
SHA256 `702e2b642ce355d70792d6be98b7672446c923efd8256a59a599201eda03861f`;
[cohort 11748214280](https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/38248428802/artifacts/11748214280),
SHA256 `94c92c594e88b5c7b5ac9ab1112e74c1f452b2b97b4bef30bb0108dcc0b27dc9`.

The isolated native unit request at candidate `4f95fbc7d661b3fde0d212fd9359533a43379f9b`
passed all 12 routed-producer Rust tests in 592.618 seconds. Its scoped operator
budget declared 6,442,450,944 reserve bytes and 8,589,934,592 forecast growth
bytes. Actual sampled peak and retained bytes were 1,805,938,688; minimum observed
free bytes were 29,741,273,088, with 491 samples and maximum sample gap 1.797
seconds. Measurement completed, reserve remained observed, and kernel ECHILD
preceded lease release. The declaration remains an operator forecast, not an
empirical upper bound, and applies only to this unit request on the selected
33,770,192,896-byte filesystem. It is not transferred to full-route sizing.

### Bounded-selector performance candidate

After main `b03bb0e1cdc10dae1c60668ecfa878f948d3d2d7` integrated #17618, the
producer combines already qualified global selector groups into batches of at
most eight observed harness targets. A group is indivisible; empty plans/groups
and oversized groups refuse. Every batch keeps the complete original package
and feature flags. Expected targets are the exact selector union, still checked
against actual compiler receipts before any consumption. A failed consumer keeps
all batch executables, including previously consumed ones. This bounds selected
harness count, not bytes, companion binaries, retained warm files or peak growth.
Original capacity admission, diagnostics and watchdog remain authoritative.

A real isolated Cargo 1.95 resolver-2 fixture compared original `--tests`, the
union of separate selectors, and combined `--lib`, repeated `--bin` and repeated
`--test` selectors. All three produced identical eight-harness/thirteen-artifact
package/target/features/profile graphs. Omitting a selector was distinguishable;
dropping the feature-activating package removed the shared `extra` feature and
failed with 101. All ordinary-owner operations settled and released their leases.
Fixture report SHA256:
`f91c1ef42d1a387dfa81484b8903ca6dc03b3f122a2bd65e968e45214640f4f8`.
The fixture budget is not the native full-workload sizing proof.

Separately, source manifest/file path checks use one fresh `lstat` per current
component instead of repeated link stats and a second resolve traversal. Every
ancestor is checked on every invocation; source hashes and before/after fd/path
identity checks retain their original frequency. No cached directory or source
facts authorize another file, launch or reclaim. Independent differential,
replacement, reparse and permission controls pass; ancestor-omission and cached
result mutants fail. This preserves the existing cooperative filesystem contract,
not an atomic snapshot against hostile concurrent writers.

Three alternating warm paired complete-entry component measurements on
`f0783dcf33` plus the helper diff checked 14,267 files and 189,847,416 logical
bytes. Original/new medians were 1.725/1.073 seconds, a 37.8 percent reduction.
Helper SHA256 `28d9df2f8993aea0cab926edcb4c28406cc54e3f0fdaca75238f241d278c0152`;
benchmark report SHA256
`68c1125437fa4abd382da1de32f7ba5fba21ded274f115fbba72d6f7d2b20f18`.
These component results are not whole-route timing or disk-fit qualification.

### Repeated native case registrations

The terminal warm cohort retry in run `38261171453` reached 437 cases across 32
harnesses, then refused `xtask`'s main test binary because its list contained
repeated case names. Rust 1.95 permits repeated `#[test]` attributes; the existing
`module_train_live_tests.rs` source contains such registrations. An isolated real
Cargo fixture listed each regular and ignored name twice, executed the regular
function twice in its default four-test run, and executed the ignored function
twice under `--ignored`. Each operation settled and released its lease. Fixture
report SHA256:
`6b5b1a1f94ec21b656df20a730ce58fe46d9af34bb15b348de497fd90ec9670a`.

Inventory now preserves case-name multiplicity within each exact target binding.
The all, ignored and completed populations use multisets; ignored counts must be
contained in all counts, and successful execution must match both populations
exactly. Serialized arrays and hashes retain repeated names. Dropped, added or
changed ignored occurrences refuse and preserve executable groups. This does
not deduplicate or alter existing tests, invent distinct occurrence IDs, change
binary bindings/default arguments, or grant cleanup after partial success.
Ordinary reporting also preserves legitimate repeated names while refusing
missing/excess output relative to the native `running N tests` count. Complete
ordinary/cohort fixture reports agree on 24 occurrences and 12 ignored across
six targets; ignored multiplicity overflow fails verification.
