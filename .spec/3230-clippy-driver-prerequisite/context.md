# Clippy driver prerequisite for #3230

Status: test-local preparation for #11660/#11663; **production admission BLOCKED**.
This packet is not the executor-model decision, a canonical API, or package lint
qualification. It defines the missing Clippy-specific renderer/setup obligations
inside the existing train, with an executable upstream dispatch fixture.

## Current subjects and start gate

Inspected main: `ac383369c40494bd873d9657e8c881b7e8792000`.
Storage prerequisite/test base: `fd8aa27ce2ebf2a1e95b2eae4109be394252b729`
(`recovery/cargo-storage-17477-windows-tests`; production same as `4e0cae40`).
No edits to that branch or storage production source belong to this packet.
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

The planned finite route is the exact installed toolchain's direct `cargo-clippy`
binary with the literal `clippy` token and typed Cargo fields, under the existing
canonical supervisor. Do not launch `cargo clippy`: that re-enters alias/external
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

## Proof ceiling, resources and rollback

`scripts/tests/test_clippy_driver_contract.py` dispatches the real installed driver
to a recording fixture instead of Cargo. It compiles nothing, downloads nothing,
and does not acquire a production lease. Temporary files have one test owner;
test-local exit codes establish dispatch propagation only. The opt-in spy is
POSIX; native Windows remains NOT_PROVEN. Default tests preserve production refusal.

Observed host: /workspace about 30 GiB free, /tmp about 8.8 GiB free. Neither
supports a default 40 GiB reserve build. No scoped Rust workload budget is declared
because no Rust build is admitted here. Default reserve and policies remain
unchanged. No cache/latency saving, lint verdict, terminality or hook qualification
is claimed. Rollback removes these test/spec files; existing artifacts stay intact.
