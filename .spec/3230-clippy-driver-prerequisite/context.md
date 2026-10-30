# Clippy driver prerequisite for #3230

Status: finite staged Linux Clippy admission in the existing storage owner;
canonical typed executor completion and native Windows Clippy remain **NOT_PROVEN**.
The initial dispatch-only prerequisite was preserved at `75d64037c699b92dd48924ded9f360ff68a93880`.
Review of the existing CARGO_STORAGE root-admission exception distinguishes a
separately admitted finite route from a production substitute for #11660. This
increment changes that same owner, preserves its resources/capacity/lease and
initially retained every attempted Clippy lease for independent owner verification.
The follow-up kernel scope below makes new owned-tree settlement independent of
unrelated host-daemon visibility; earlier unproven attempts remain retained. No
parallel executor/model/reservation architecture is introduced.

## Current subjects and start gate

Inspected main: `ac383369c40494bd873d9657e8c881b7e8792000`.
Storage prerequisite/test base: `fd8aa27ce2ebf2a1e95b2eae4109be394252b729`
(`recovery/cargo-storage-17477-windows-tests`; production same as `4e0cae40`).
That writer branch remains untouched; Clippy changes are isolated on the separate recovery branch.
Qualification consumer only after admission:
`b5046c390bd6ea141b530bc896448c11c57f6cf2`, hook/resolver tree
`9f6958b3a3936a04fa5c35ec1597d052b20d925b`.

Live #11660 requires accepted #11642/#11647/#11650/#11653/#11659 APIs;
its [2026-08-21 checkpoint](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11660#issuecomment-5365930317)
permits test-local renderer tables/fake fixtures ahead of them, forbids production
substitutes, and freezes the plan before admission. The inspected xtask owner
surface exports no canonical Cargo executor. All five prerequisite issues remain
open; open/closed alone is not API acceptance. No accepted model/API packet was
found on the inspected owner surface or linked governing checkpoint.

The earliest train prerequisite remains #11639 -> #11640/#11641 -> #11642;
this packet does not implement those unrelated host/model leaves. #11647 then owns
pure request/operation vocabulary, #11650 state, #11653 reservation, #11659 exact
process supervision, #11660 composition, #11663 parity. A storage rollout candidate
does not satisfy this chain or establish native Windows Clippy support.

## Real driver, not arbitrary external Cargo

Primary oracle: [Clippy Rust 1.95 source](https://github.com/rust-lang/rust-clippy/blob/rust-1.95.0/src/main.rs).
`cargo-clippy` skips argv[0] AND one subcommand token, maps its normal operation to
child Cargo `check`, takes Cargo from `CARGO` (otherwise ambient PATH), selects
`clippy-driver` beside its own executable, and encodes lint arguments in
`CLIPPY_ARGS`. `--fix` changes the operation to Cargo `fix`. `--version`/help can
return before any child work. `--no-deps` is a driver input, not a Cargo flag.

The canonical contract should render the exact installed toolchain's direct
`cargo-clippy` binary with the literal `clippy` token and typed Cargo fields under
its accepted supervisor. The staged existing-owner route renders one finite
request shape; it does not claim those canonical APIs or supervision outcomes. Do not launch `cargo clippy`: that re-enters alias/external
discovery. Do not admit nextest, arbitrary external programs, aliases, `--fix`, or
raw argument/environment escape hatches as a consequence of this route.

### Renderer table to reconcile into #11660

| Typed authority | Exact projection / refusal |
| --- | --- |
| #11647 operation = Clippy | direct identified `cargo-clippy`, first argument `clippy`; child operation remains `check` |
| #11647 metadata package ID | exact metadata-backed package selection, no directory/name guess |
| target/features/host-target | exhaustive typed renderer; unknown fields refuse before allocation/admission |
| Cargo build profile | `--profile agent` for this consumer; runner profile absent, not reused |
| lock/network disposition | `--locked`, and offline/frozen only from the request |
| #11650 target AND build | target argument and build config after `clippy`, plus exact environment values |
| lint policy | finite typed lint levels/names after `--`; no compiler flags or `__CLIPPY_HACKERY__` injection |
| no-deps disposition | explicit driver input, retained in operation identity |
| #11659 executable subject | cargo-clippy + sibling clippy-driver + Cargo + rustc/toolchain identities, native suffix and host |

Example plan shape (illustrative fields, not an executable approval):

```text
<identified-toolchain>/bin/cargo-clippy clippy
  -p <metadata-backed-package> --all-targets --profile agent --locked
  --target-dir <allocated-private-target>
  --config <typed-build-dir-config> -- -D warnings
```

### Environment and setup

Set `CARGO` to identified direct Cargo, `RUSTC` to identified rustc, and no-install
toolchain inputs from provisioning authority. Refuse or remove ambient wrappers,
compiler/lint argument inputs and executable selectors through typed environment
projection. Retain private target/build/tmp/Cargo-home roles, jobs and incremental
policy from their existing owners. Explicitly account for `CLIPPY_ARGS`,
`CLIPPY_CONF_DIR`, `CLIPPY_DRIVER_PATH`, `RUSTC_WORKSPACE_WRAPPER`, `RUSTC_WRAPPER`,
`RUSTFLAGS`/encoded flags and `[env] force=true`; names are investigation inputs,
not a claim that each is active in the pinned driver. Admit project Clippy config
only as the request's config subject. No arbitrary compiler-cache wrapper.

[Cargo 1.95 `load_global_rustc` / `maybe_get_tool`](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/util/context/mod.rs)
reads the direct `RUSTC_WORKSPACE_WRAPPER` environment ahead of the config wrapper.
Therefore a command-local empty workspace-wrapper config does NOT by itself
neutralize the driver-set environment. Do not infer effective wrapper identity
from config alone. Forced Cargo child `[env]` entries must also be reconciled;
the dispatch spy proves the pre-Cargo environment, not Cargo's eventual child.
The real lint fail/clean pair is mandatory before production qualification.

Matching versions are prerequisite observations, not executable provenance proof.
Provisioning must supply the accepted exact identity, and #11659 must bind actual
executables/libraries and reject changes after planning/waiting. Missing tools,
wrong commit/host, unsupported target, proxy/path substitution and unproven
identity have setup outcomes with no product verdict and no implicit install.

## Fixed transaction and result boundary

Pure request validation -> #11650 allocation + immutable plan -> #11653 admission
-> #11659 supervised driver/Cargo/compiler tree -> work/product observations ->
terminal release -> typed final result. No resource-bearing subprocess before
reservation. A separately admitted bounded setup probe may observe identity only.
No fallback, silent profile/package/state switch, retry loop, lock stealing or
automatic cleanup. Cancellation and unfamiliar exits preserve ownership until
accepted tree terminality; a parent exit never suffices. Record setup, product,
work/instrument, cancellation, terminality, release and reporting independently.

Clippy product proof must positively observe the selected compilation/lint work
and its exact package/target/profile. A version probe, cached unrelated result or
Cargo `check` without the accepted Clippy driver cannot fill it. Work instrumentation
belongs to the accepted executor contract; this packet invents no count parser.

## Staged implementation and evidence ceiling

`scripts/cargo_admitted.py` admits only Linux, installed repository pin 1.95.0,
Python 3.11+, canonical worktree cwd, one explicit `-p NAME --all-targets --profile
agent --locked [--offline] -- -D warnings` shape. Native target is rendered from
the bound host. Toolchain executable/runtime subjects, semantic TOML pin,
configuration and renderer source are scope-bound; tools, pin and configuration
are revalidated before launch. Incoming
compiler/lint/loader selectors refuse. Parsed Cargo config includes and loader
entries refuse; forced child tool/lint/sysroot/resource entries are overridden.
No aliases, fixes, help/version, arbitrary flags, discovery fallback or install.

Version probes are lease-owned setup only. New ClippyTree scopes are enabled
before every setup/product child in the existing single-thread CPython owner.
Initial ECHILD with __WALL refuses unrelated children; prior subreaper0 and known
SIGCHLD default/non-autoreaping action are required. Linux adoption retains
setsid/double-fork descendants as this owner's children. Sequential leader waits
preserve product results; bounded adopted-child waits must reach kernel ECHILD
with __WALL before original matching lease release. Group absence never suffices.
Pidfd cancellation requires kernel child waitability. Only an owner-only children
table discovers additional candidates; no broad process/environment scan. This
host lacks that table, so an unknown surviving detached helper conservatively
retains its lease at deadline, while eventual kernel closure can still settle.
Missing capabilities, ownership/wait errors, timeout, process death or failed
release never become completion (CLI75 even if product0). Earlier scopes without
subreaper provenance cannot be retroactively adopted. Independent external
service/artifact consumers remain separately owned and outside this guarantee.
Canonical typed model/operation/result/queue APIs, work counters, broad supervision
and cross-host parity remain future contracts.

`scripts/tests/test_cargo_admitted_clippy.py` supplies bounded positive/negative
route, identity/config, contention, setup, cancellation and lease controls.
`scripts/tests/test_clippy_driver_contract.py` retains the real installed-driver
spy and no-work/wrong-route controls; no compiler work is claimed by that spy.
`scripts/tests/qualify_admitted_clippy.py` is the reproducible opt-in native proof.
It creates an offline no-dependency library with two tiny normal/test lint units,
then observes deliberate `clippy::useless_vec` failure 101 and clean exit 0 plus two
Cargo compiler-artifact rows. It tests hostile forced Cargo tool/lint/sysroot/
resource settings. The original owner now releases after kernel ECHILD proof and matching original
lease inode/marker. The fixture independently checks that receipt and group
absence; it adds no arbitrary-workload manual release authority.
All source, logs, budget, source/owner hashes and volume receipts are retained.

/workspace has about 30 GiB free and /tmp 8.8 GiB; default 40 GiB admission remains denied.
The explicit fixture budget declares reserve 4 GiB + aggregate peak growth 2 GiB,
conservatively covering this installed-toolchain/no-download/no-build-script
fixture. Every actual destination is checked; no generic reserve change or disk
reservation is claimed. Latest artifacts live under the recorded proof directory
in checklist.md. Hook/resolver qualification needs its own conservative exact
workload budget and independent consumer settlement. No performance gain or
canonical parity/native Windows qualification follows from the fixture. Native
lifecycle microfixtures cover detached generations, cancellation, clone children,
foreign ownership, missing capabilities, wait errors and lost scope, without
Cargo or any cross-process environment scan.
Rollback reverts this finite owner branch; existing artifacts stay intact.
