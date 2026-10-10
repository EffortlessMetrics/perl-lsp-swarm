# Bounded nested admission prerequisite

Consumer authority: issue #17479 comment 6095456552, exact consumer source
556772bf2f78654bc53aad256a0c3361559863e1 atop e633ac77. Owner base:
#17507 at 6ac32625c60edcaba7ac2d656857ff7b1f8877ae. This candidate changes
only the existing Python owner, relevant controls and this interface guidance;
it does not import the consumer's older owner or change existing PR heads.

The request contains schema_version=1 and unique named rows (at most the finite
catalog). No raw argv, shell text, path/profile/lint override or complete-workload
claim is accepted. The parent CLI takes --nested-plan PATH before its existing
admission options. Existing budget scope additionally binds the complete plan
hash: declared rows, request identity, exact committed Git source/lockfile,
renderer, installed toolchain/runtime and config identities, compiler/profile,
network and build-tool environment. Source must be clean. This is Git-subject
binding, not hermetic proof of ignored/build-script/external inputs.

The eight parser/DAP propositions retain exact default targets/profiles/lints.
Other finite rows cover perllsp/xtask builds, parser docs and graph reads,
the exact nine-package routed compile/runtime forms, inline warmup and two
navigation targets. Graph reads omit unsupported target-dir flags. Docs keep
RUSTFLAGS=-W missing_docs and force an equivalent encoded pair so an encoded
parent/config value cannot mask that exception. Strict rows refuse unsupported
flags (including cap-lints/suppression); only finite debuginfo compiler choices
and explicitly frozen profile overrides are supported. Tool/loader/target
selector overrides and changed network/build-tool environment refuse.

The parent retains its existing lease, capacity checks and Linux owner. It runs
installed version probes under that owner before the root operation. Children
use nested_command(row, env) or --nested-row ROW. Validation invokes read-only
Git commands; rendering allocates no resources and starts no product. The child
CLI execs the exact direct installed tool, preserving the owned descendant PID.
No child reacquires/releases capacity or creates another subreaper/watchdog.

The live descriptor adds original marker identity and kernel owner process fact.
An exclusive 0600 snapshot outside the empty lease binds descriptor and plan;
it is retained as ownership evidence. Child validation requires matching native
snapshot identity, directed live ancestry (no host scan or PID-age inference),
canonical resource_plan results, both original lease/marker inodes, forced
paths/jobs/temp, and current source/renderer/tool/config/environment identities.
This is not an OS sandbox for arbitrary same-user repository code. Raw launches
outside the migrated interface are not magically intercepted or admitted.

# Deliberate remaining boundaries

Hosted activation and broad consumer execution remain blocked. The current
parser helper/DAP/gate commands still need migration to this interface.
Formatter alias, full-tier Clippy variants, ci-subject/scope commands and unknown
xtask/fixture leaves have no generic passthrough. Runtime server-run/build
fallbacks need the separately owned current-source binary handoff and fail-closed
consumer seams. Parser legacy helper reachability is NOT_PROVEN; ignored tests
stay ignored. Conditional docs must not be skipped to green the suite.

The contract's nine packages/995 integration targets remain the denominator,
not executed tests or a fresh planner result. Fixed row membership does not
certify inventory completeness or enforce aggregate concurrency/call counts.
Existing disk/job controls and default 40 GiB reserve are unchanged; no growth,
RAM, cache or complete-lifetime qualification is fabricated. Consumer migration
must establish the actual complete inventory, overlapping lifetimes, current
scope and measured conservative capacity basis before activating hosted work.
Linux ownership controls do not fill native Windows/macOS cells.
