# Parser occupancy finite native proof

Source `7e6decfa44f62eb4b5d6d5c079e0920fa8318d2f` passed the actual native
Clippy measurement and all fourteen original owning integration tests:
**14 passed, 0 failed, 0 ignored, 0 filtered**. Libtest took 1.06 seconds;
the gate took 368607 ms and the entire original-owner admission took
370.372024 seconds. Independent source/control and native-evidence review found
no bounded blocker.

The exact preparation obligation remains `clippy -p perl-parser --all-targets
--features incremental --locked --no-deps --message-format=json --
--cap-lints=allow --force-warn clippy::collapsible_if`, with default features,
workspace cwd, native Rust 1.95 and both worktree-private output roots. Native
Clippy returned 0 with a successful JSON terminal and **zero actual lint hits**.
Its raw 507508-byte stdout and 2295509-byte stderr are preserved. Runtime reads
that current subject-frozen result through the original diagnostic oracle;
actual failed preparation blocks owning runtime. Ordinary developer execution
retains its direct Clippy command.

Fresh locked/offline/no-deps metadata identifies 248 declared parser targets.
Default plus incremental enables **247 distinct target identities**; the CLI
binary requires the unselected cli feature. Actual Clippy artifacts reconcile
all 247 with no missing or unexpected identity: two library variants, 181 test,
59 example and six bench artifacts. Every parser artifact has the expected
feature set: anyhow, default, incremental, lsp-compat, lsp-types, perl-line-index,
tracing, workspace, workspace_refactor. The two library artifacts represent
test/non-test profile variants. This is compilation/diagnostic coverage, not
execution of the other 180 integration targets, examples or benches.

Freshness denominators: **542 Clippy artifacts / 63 fresh**, and **295 owning
artifacts / 258 fresh**. Nonfresh artifacts are retained in the evidence and
are not equated with compiler-process counts. The owning integration artifact
has the exact parser manifest/source, test kind/profile and default feature
set. Its 7250088-byte read-only copy was captured under the original live owner,
with digest `081e0a855f6f87f237b9bbe3dfac62ef4173edf338cc32334b6fae69cb5b9cb0`.

Before/after frontend topology is **3 → 3**: one root xtask bootstrap, one real
Clippy preparation and one owning Cargo test. The original integration already
batched fourteen tests. No invocation reduction is claimed. Native process
observations identify three Cargo operations (run, check delegated by the
cargo-clippy wrapper, test) and one cargo-clippy wrapper. Samples observed 144
rustc and 98 clippy-driver processes; these are **lower bounds**, not complete
compiler counts. One read-only metadata Cargo call and four admitted version
probes (cargo, rustc, cargo-clippy, clippy-driver) are accounted separately and
excluded from package-work counts. Only one native admission was attempted.

Peak sampled aggregate memory was **12005183488 bytes (11.180698 GiB)**.
Minimum free volume space was **22347927552 bytes (20.813129 GiB)**; observed
additional volume growth was **580898816 bytes (0.541004 GiB)**. The new scoped
12 GiB additional-growth forecast plus 8 GiB reserve was explicitly unverified;
these finite warm measurements do not establish cold capacity, process RSS or
quota sizing. Jobs remained 1, incremental compilation off, profiles and global
storage policy unchanged. Clippy's changed all-target/incremental/cap/force
inputs produced permitted new ring metadata variants; the prior linked ring
rlib subject remained identical. Existing artifacts were retained.

The original native owner stayed live through snapshot and read-only harness
capture. Final closure is positive **kernel ECHILD (__WALL)**, with no pending
bound PIDs, errors or cancellation, and matching original lease/marker release.
Cgroup memory-event counters remained unchanged at zero. Caller package context
was cleared before native children; compiler tool/output identities are frozen
in the captured original-owner plan.

Controls: the scoped suite ran **212 tests, 211 passed, one opt-in skip**.
Independent review reran all eleven occupancy controls. A portable run executed
all fourteen original Rust tests using a stand-in Cargo command; that run is
separate from the native proof above. Ten mutants failed actual assertions with
no instrumentation errors. They cover frozen measurement, actual argv/status,
required incremental target, blocked runtime after failed preparation, exact
harness binding, reserved instrument status and original scanner/result
semantics. Original native owning tests also ran the three result fixtures.
A real native injected lint hit or failed-Clippy counterfactual was not run;
portable transport fixtures and mutants must not be described as that proof.

Exact owning names:

- cfg_attr_allow_fixture_is_detected
- cfg_attr_without_collapsible_if_does_not_occupy
- clippy_all_targets_has_no_collapsible_if_hits
- comment_and_string_literals_do_not_occupy
- crate_level_allow_fixture_is_detected
- expect_attr_fixture_is_detected
- item_allow_fixture_is_detected
- lib_rs_crate_allow_list_does_not_name_collapsible_if
- nested_cfg_attr_allow_fixture_is_detected
- occupancy_requires_allow_attr_not_scanner_literals
- perl_parser_sources_do_not_allow_collapsible_if
- successful_clippy_without_hits_is_clean
- unsuccessful_clippy_with_hits_still_reports_occupancy
- unsuccessful_clippy_without_hits_is_instrument_failure

Durable raw evidence is [occupancy-evidence.json](occupancy-evidence.json),
8218431 bytes, SHA-256
`f0fb3d74194ce93fd9734904720474d3864211317e3d73c51d6bcc5666813490`.
The source-only patch against `02d55fe7e0b089d2165c6d411b2a1433d444e8a8`
is `/tmp/17479-occupancy-qualification/occupancy-source.patch`, 48145 bytes,
SHA-256 `0df9a0099e42465aff52d74426e70092f4a34a899d0ca2724b73b7a962ff4c38`.
The packet includes actual raw streams, gate/runtime log, captured immutable
plan and artifact receipt, native process contexts, samples, resource admission,
metadata-derived inventory, ring subjects, cheap-control logs and validators.

Remaining closure is the finite inventory in
[remaining-closure.md](remaining-closure.md): qualify and compose the DAP/parser
preparation under current ownership, join the five-mode owner, project bound
selectors into canonical routed consumers, classify genuine dynamic fallbacks,
then establish useful nine-package no-run/runtime proof. Windows and hook owners
remain separate. This manual leaf policy is unactivated; canonical/full runtime,
cold capacity and broader platform qualification remain **NOT_PROVEN**. No
hosted CI trial was launched; expensive final CI follows a green, merge-ready
candidate.
