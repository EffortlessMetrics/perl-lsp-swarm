# Exact-source executable handoff checkpoint

This bounded slice follows consumer checkpoint
`e3e38cc757c79b2825ecd8a7f0509028ce93ced3`. It is unactivated and is not a
whole-workload or runtime completion claim.

## Observable change

The existing finite `perllsp-build` row retains `build -p perllsp --locked`
and adds `--message-format=json`. The preparation adapter accepts exactly one
compiler-artifact for the workspace perllsp manifest, binary target and source
file, with the actual reported executable under the admitted native debug root,
plus a successful terminal build message. A current Cargo fingerprint no-op
(`fresh: true`) is valid; a pre-existing file without that current build is not.
Cargo's JSON contract is documented in the
[Cargo Book](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages).

The receipt supplements successful current compilation with artifact identity
and digest. It binds the existing nested plan's source/tools/configuration,
native platform/debug profile, both roots, original live lease/marker, and the
validator/interpreter identities. The outer owner retains its existing
descendant lifetime. Before and after an executable version eligibility probe,
the validator checks ownership and artifact identity. This probe is not a
behavioral test population or an executable provenance substitute.

Two active LSP resolver families enter one shared admitted-mode seam before
explicit paths or developer candidates. Any present admission/handoff selector
requires a complete valid handoff; absent, stale, mismatched, mutated or
unspawnable handoffs refuse without Cargo, PATH or opposite-profile fallback.
The interpreter uses isolated mode. Developer mode retains the existing
freshness builds.

Topology: the unactivated adapter performs one admitted product build followed
by the existing routed compile or runtime row. Admitted resolver calls perform
validation and version probes, with zero additional compiler invocations.
Ordinary developer resolver topology is unchanged. Hash/probe overhead and
actual compilation savings are not measured here.

## Cheap evidence

- Handoff composition controls: 18 passed. The fixture mocks source/config/tool
  observations but uses actual owner membership, snapshots, original identities,
  canonical roots, rendering, receipt/digest checks and executable launches.
- Resolver controls: two Python controls passed, including six tests compiled
  from the actual shared std-only Rust seam. Structural controls check both
  resolver entries precede developer candidates; full owning modules are not
  compiled by this proof.
- Existing owner/storage/budget/nested controls: 131 run, 130 passed, one opt-in
  product build skipped. Installed Clippy driver/setup controls: ten passed.
  Existing parser/DAP consumer controls: ten passed. These are overlapping
  controls, not an additive executed-runtime denominator.
- Four assertion-rejected semantic mutants: omit receipt binding, accept a
  missing/unsuccessful terminal build, ignore failed version eligibility, and
  omit post-probe artifact/ownership revalidation. No instrumentation errors.
- Independent read-only review cleared the hardened scoped slice and reran
  18 handoff controls and two resolver controls, including the six Rust tests.
- Affected Rust formatting and diff hygiene pass. New Python controls are wired
  into the existing self-test workflow; no hosted run was requested.

## Selected source inventory and proof ceiling

Fresh `cargo metadata --locked --offline --no-deps --format-version 1` reports
the same nine declared packages and 995 integration target declarations:
139 DAP, eight incremental, 16 perltidy, 290 LSP, 108 LSP core, 181 parser,
one parser bench, 16 perllsp, and 236 xtask. This is not a fresh ci-scope
measurement against the genuine planner base, nor executed-test evidence.

`cargo test --tests` also selects eligible lib/bin harnesses. The manifests
declare 43 test-enabled lib/bin harnesses: two DAP, one incremental, one
perltidy, one LSP, one LSP core, two parser, one parser bench, two perllsp,
and 32 xtask. Required features, package/target eligibility, actual selected
counts, execution and concurrency remain separate. No root integration helper
target was added; the shared seam resides below `tests/common`.

Parser common/test_utils fallbacks are dormant in the inspected import graph.
Documentation compilation requires nondefault `doc-coverage`. Active parser
diagnostic occupancy and xtask temporary-crate compiler fixtures remain
explicitly unqualified and unadmitted, including their inherited intermediate
roots and intended-failure semantics. Dynamic CLI/shell closure inventory is
not declared complete.

Gate policy and native runtime adapter are unchanged. Parent composition must
route the current planner's exact package selection through the admitted
handoff; the finite routed catalog currently names nine fixed packages. It
must preserve existing current nonzero behavioral-result checks. This adapter's
build is debug-only and refuses release consumers. No release, cross-target,
or profile change is substituted. Native nested ownership is still Linux-only;
Windows suffix/mismatch controls are not native Windows execution proof.

Still NOT_PROVEN: genuine product compiler-artifact capture, owning LSP harness
compilation, native Windows execution, complete compiler-fixture admission,
fresh planner scope, conservative aggregate disk/RAM/concurrency/cache
qualification, actual runtime completion and full gate coverage. The existing
40 GiB reserve remains unchanged; no profile/storage bypass, PR update, gate
activation, Ready, merge, public comment or expensive CI occurred in this slice.
