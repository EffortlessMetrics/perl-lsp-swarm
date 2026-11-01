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

### Native Windows setup

Check the actual checkout, common Git directory, current base, hook paths and
storage destinations before provisioning. A stale coordination checkout must
not supply an old admission script. Preserve dirty or independently owned
checkouts; use a current isolated task clone with a linked writer worktree when
the existing checkout cannot safely be refreshed. The hook installer provisions
every registered worktree in that clone and changes its repository-local
`core.hooksPath`; invoking it from a linked tree does not confine those writes to
that tree.

Select an owned absolute native `$storageRoot` on a volume with capacity, then
set these variables in the same PowerShell task shell that will run admission:

```powershell
$env:DEVPLANE = Join-Path $storageRoot 'devplane'
$env:CARGO_HOME = Join-Path $storageRoot 'cargo-home'
$env:TMPDIR = Join-Path $storageRoot 'tmp'
$env:TEMP = $env:TMPDIR
$env:TMP = $env:TMPDIR
$env:RUSTUP_AUTO_INSTALL = '0'
```

Selecting only `DEVPLANE` can still leave Cargo home or temporary files on an
overfull system volume. Keep existing caches intact. Do not export target/build
paths or enable compiler wrappers: the admitted route owns both private output
paths. A copied `.cargo/config.local.toml` is not automatically loaded by Cargo's
[configuration discovery](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure)
and cannot configure Python's pre-admission resolver. The adjacent example no
longer recommends shared output paths or sccache for this route.

For missing hooks, use the existing lightweight installer owner (#17861), from
the intended worktree with the repository-pinned native toolchain installed:

```powershell
python scripts/cargo_admitted.py --preflight run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- install-githooks
python scripts/cargo_admitted.py run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- install-githooks
python scripts/cargo_admitted.py --preflight run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- check-githooks
python scripts/cargo_admitted.py run -p perl-ci-hygiene --bin perl-ci-hygiene --profile agent --locked -- check-githooks
```

Run each operation only after its preceding command succeeds; a preflight refusal
is not permission to continue. This invokes the authoritative installer without
the outer product-linked `xtask` bootstrap or a second hook generator. It leaves
installed commit/push gates unchanged. The PowerShell writer preflight no longer
creates a legacy target or tells callers to export it. Its `TargetRoot` parameter
is accepted for older callers but unused; pass the actual clone's canonical and
worktree roots when overriding its historical layout defaults. It still checks
checkout safety and installed hook currency, separately from Cargo admission.
Run the repository writer preflight using native Git Bash
(`bash scripts/agent-preflight.sh`) before editing; do not
accidentally select WSL's `bash.exe` for a native Windows checkout. Installation,
hook currency and writer safety do not qualify later Cargo commands, hook gate
execution, process-tree settlement on Windows, or a smaller cloud reserve. Keep
the default reserve and usage threshold; obtain a fresh exact-command admission
for subsequent work.

### Git context in hook-driven tests

Git hooks export repository-local variables that override both a child's working
directory and `git -C`. Fixture Git commands must use
`xtask::git_environment::isolate(Command::new("git"))` before adding their own
arguments or deliberate fixture overrides. Identity-pinning tests should continue
using the stronger `HermeticGit` fixture contract in `xtask/tests/git_test_support.rs`.
The shared Vim checkout verifier also selects its own repository through this
boundary. See [Git's foreign-repository hook guidance](https://git-scm.com/docs/githooks).

The gate runner isolates only Cargo-test-class child processes, including chained
setup/test commands recognized by its existing classifier. The parent hook and
current-repository staged commit checks retain their Git context and index. No
process-global environment changes, test exclusions or watchdog changes are needed.
The native inventory, disposable-caller probes and staged-index control guard this
distinction. This does not qualify a shell command that never reaches its intended
tests; inspect test-execution evidence as well as its exit status.

### Capacity policy

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
alternative for Linux hosts lacking `CONFIG_PROC_CHILDREN`. Linux builtin Cargo and
staged Clippy preflight and execution check the selected profile's discovery capability
before resource planning or lease acquisition. An unavailable default `proc-children`
interface therefore refuses even a build that might otherwise complete normally; select
the alternate explicitly only when its capability checks pass. No missing-table
detection silently selects a namespace scan. The alternate profile reads only visible numeric PID `stat`
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

### Cohort lifecycle prerequisite (#17617)

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
cohort executes that exact path and revalidates it before unlinking. Preexisting,
pinned, foreign, replaced, symlinked, and multiply linked files refuse. Successful
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

This is a lifecycle prerequisite, not the routed-test producer. Cargo's hardlinked
target/build aliases are deliberately refused until a separately qualified exact
alias-ownership extension exists. No gate command, package, feature, profile,
reserve, timeout, or quarantine changes. The API is not an adversarial filesystem
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

### Staged native Clippy

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

#### Recovering a retained Clippy lease

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
For bounded read-only inventory, save that single JSON object locally and run
`bash scripts/storage-doctor --descriptor record.json --json` from its original
working directory (`--resources-descriptor` is an equivalent spelling). The
existing repo-local target guard still runs; its failure makes this invocation
non-success. No-argument `storage-doctor` reports only its repo-local guard and
marks external resource authority unknown, without guessing a basename devplane.

The descriptor reader uses the existing admission path rules and verifies the
same native host, common repository, worktree, invocation directory, target/build
slot and agreeing scope/resource fields. It rejects linked inputs and unknown or
malformed scope. This is **historical inventory**, not authentication of the
descriptor's origin or a live lease. It traverses only the four named roots with
entry, depth, elapsed-time and fingerprint-byte bounds. Missing, unreadable,
changed or clipped entries keep partial observed counts but cannot yield complete
totals. Fingerprints under target and separated build roots include custom/agent
profiles and explicit target triples. Public JSON retains fixed resource roles,
path/content hashes and selected integer fingerprint hashes, never raw descriptor
paths, arbitrary metadata or fingerprint environment/flag values.

Apparent bytes measure regular-file content; allocated bytes use POSIX `st_blocks`
and include directories. Native Windows allocated bytes remain unknown. Combined
totals deduplicate native device/inode identities across roots, including overlapping
roots and hardlinks. These are retained snapshots, not reclaimable or exclusive
ownership. Peak demand, build deltas, open-unlinked bytes, WSL guest free space,
VHDX allocation and backing-host capacity remain separate unknown fields. No
`df` minus `du` inference, cleanup or compaction is performed.

`observe-xtask-build.sh` captures the emitted descriptor in a bounded private
temporary file outside its output directory and emits only this sanitized
historical inventory after the command. Multiple descriptors or an incomplete
stderr capture invalidate that inventory. Its outer before/after environment
snapshots are hashed hints: a descriptor emitted later **cannot retroactively
bind the before sample**. Neither command exit nor an emitted settlement line
proves same-lease measurement. The existing command's exact argv and exit status
remain authoritative even when inventory fails. Full workload qualification and
same-lease sampling remain with #17623/#17655/#17658/#17668; this instrument never
changes admission or retires a storage guard.

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
