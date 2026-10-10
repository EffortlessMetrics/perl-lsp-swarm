# Parser diagnostic occupancy qualification

The current consumer `crates/perl-parser/tests/collapsible_if_occupancy.rs`
contains fourteen tests: ten scanner/source tests, three status/result controls
and one real Clippy consumer. The source scanner detects actual allow/expect
attributes, including nested cfg_attr, rather than scanner literals/comments.
The compiler oracle preserves failure with lint hits as occupancy, failure
without hits as instrument error, and successful silence as clean.

Its exact measured command is `clippy -p perl-parser --all-targets --features
incremental --locked --no-deps --message-format=json -- --cap-lints=allow
--force-warn clippy::collapsible_if`. Preserve default features plus incremental,
the cap/force pair, workspace cwd and both worktree-private output roots.
Existing canonical parser preparation does not supply this command; selected
`unit_routed_full` reaches this test through `--tests`.

This manual, unactivated policy measures native Clippy before the owning Cargo
test under the existing original owner. Actual nonzero preparation blocks
runtime. The raw stdout/stderr/status remain available to the original oracle
through a subject-frozen reader, with current source, tool, configuration,
command, cwd, original process, lease/marker and immutable-plan validation.
Reserved adapter status 75 and partial selectors become instrument error;
admitted execution never falls back to developer Cargo. Ordinary local execution
retains its original direct Clippy operation. No file-presence substitute or
synthetic empty success is introduced.

All fourteen exact owning tests run in one integration harness. Cargo's actual
parser test-kind artifact must be copied read-only under its original live
owner. A success summary without current genuine measurement cannot capture
owning evidence. Both raw streams have a finite 16 MiB ceiling for the full
all-target trace; only its raw receipt reader has the corresponding finite JSON
escape ceiling. Ordinary 64 KiB plans/budgets remain unchanged.

Invocation topology remains one root bootstrap, one Clippy front end and one
owning Cargo test: three compiler-producing front-end calls. The original
one-target runtime already batched fourteen tests; no invocation reduction is
claimed against it. The change separates diagnostic compilation from runtime
and reuses precisely one current measurement. Tool-version and read-only
metadata probes are separately accounted; sampled processes are lower bounds.

Source inventory declares 248 parser targets: incremental/default enables 247
(one library, 181 integrations, 59 examples, six benches), while the CLI binary
requires the unselected cli feature. Native evidence must reconcile actual
Clippy artifact target identities against that eligible set; library test/non-test
variants are distinct artifacts. Default-feature owning-harness freshness does
not establish incremental/all-target Clippy freshness. Declare possible new
compiler variants, including ring when inputs differ; do not inherit the earlier
ring-rebuild prohibition.

Native proof uses fresh inherited capacity admission, jobs 1, incremental off,
retained roots and native Rust 1.95. Its new scoped additional-growth forecast is
unverified and must cover the entire declared Clippy and owning build closure.
Monitor aggregate memory, filesystem free bytes and original-owner liveness;
cancel through the original pidfd and require ECHILD settlement plus matching
lease/marker release. Preserve outputs, profiles and storage policy.

Portable Rust runs with a stand-in Cargo command validate original scanners and
failure controls only. They are not native diagnostic, Cargo coverage, capacity
or full-runtime qualification. Independently review native results, resource
arithmetic and artifact/command denominators after execution.

Remaining convergence is recorded in `remaining-closure.md`. This slice does not
activate canonical policy, run hosted CI, qualify Windows or establish cold
capacity. Expensive final CI follows a green, merge-ready candidate.
