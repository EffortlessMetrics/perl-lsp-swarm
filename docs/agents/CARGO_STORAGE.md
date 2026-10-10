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
missing capabilities refuse without a leader-only fallback. Builtin Cargo command,
PATH/toolchain selection, argument order and environment projection are unchanged.
Original lease identity/marker and operation-labelled Cargo launch/product/settlement
receipts make product exit independent of terminality. Any product status can be
returned after proven closure and matching release; cancellation returns 130.
Incomplete closure or failed release returns 75 and retains ownership.

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

### Staged native Clippy

The existing owner admits only this finite Linux request (optional `--offline`):

```text
python3 scripts/cargo_admitted.py clippy -p PACKAGE --all-targets --profile agent --locked -- -D warnings
```

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
Only the owner's immediate-child table may discover adopted cancellation
candidates. If that table is unavailable, kernel waits can still prove eventual
settlement, but unknown surviving detached helpers cannot safely be signaled:
deadline/error retains the lease. There is no broad /proc fallback. Missing
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

## Bounded nested request prerequisite (#17479)

On the qualified Linux owner, `--nested-plan PATH` (before existing admission
options) binds a finite JSON request `{"schema_version":1,"rows":["parser-check",
"parser-build","parser-clippy","parser-lib"]}` to the outer request, installed
tools/config/exact committed source and effective environment before launch.
The request is capped at 64 KiB, accepts only unique maintained row identifiers,
and does not certify that these rows cover a full consumer workload.

Migrated children call `nested_command(row, env)` from `cargo_admitted.py` or
invoke `python scripts/cargo_admitted.py --nested-row ROW`. The renderer checks
current plan membership, kernel ancestry, original live lease/marker identity,
canonical roots and bound inputs. It does not reacquire or release the lease;
the child CLI replaces itself with the direct identified tool. Existing
consumer logging, watchdogs, work-count and product-result checks remain owners.
Compiler/profile flags are frozen; unsupported flags/selectors refuse, and
parser docs alone gets its explicit missing-docs flag exception.

A live `CARGO_ADMITTED_RESOURCES` descriptor by itself admits no nested command.
No generic Cargo/alias fallback is added. Hosted activation still requires
complete selected inventory, measured aggregate capacity/lifetimes, migrated
callers and current-source binary handoff. The retained snapshot is owned
metadata, not a cache or a product-success receipt. Native non-Linux support and
hermetic external/build-script inputs remain unqualified.
