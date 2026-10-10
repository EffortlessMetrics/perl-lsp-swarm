# Scoped native qualification result

The manual, unactivated three-row experiment passed on clean source
`b2f4ea124fa0c5aa3e436d164f634a23b2fe861e`, Linux x86_64, installed Rust
1.95.0, ordinary debug profiles and one build job. The existing owner admitted
the exact request using an independently reviewed warm estimate of 18 GiB
additional growth and an 8 GiB reserve. Retained bootstrap cache was already
deducted from current free space. The default 40 GiB policy still denied, and
neither global policy nor profiles changed. The scoped estimate remained
`basis_verified: false`; this observation qualifies only this warm experiment.

The exact command is in [qualification.md](qualification.md); its frozen argv,
resource roots, artifact, evidence digests and observations are recorded in
[qualification-evidence.json](qualification-evidence.json). `SSL_CERT_DIR` was
explicitly declared at initial admission to match the observed native trust
directory. Ambient insertion, change and deletion controls still refuse.

## Observed product and behavior

Cargo reported the real workspace executable under the owner's private
`target/debug/perllsp`, size 141,146,776 bytes, SHA-256
`cda511066a24cc628fbe86396666c56d54c6570a937d6cc36d8c2c37bbe276ce`.
Its immutable evidence copy and exact owner snapshot were captured while the
original lease and marker were live and matched their initial identities.

Both separate, serial Cargo leaves reached current one-test success:

| Consumer | Test | Passed | Failed / ignored | Filtered | Runtime |
| --- | --- | --- | --- | --- | --- |
| support resolver / product health | `health_prints_ok` | 1 | 0 / 0 | 32 | 1.04s |
| common resolver / real stdio initialize, version, shutdown, exit | `lsp_server_version_matches_crate_version` | 1 | 0 / 0 | 40 | 1.19s |

The executed behavioral denominator is **two tests**. The generic xtask receipt
records `tests_total=1` and `tests_passed=1`: its parser returns the first nonzero
harness summary rather than aggregating this adapter's two summaries. That
receipt is preserved unchanged. The preparation adapter requires both exact
named successes, and the raw log preserves each separate summary. This result
does not establish 995 integration targets or 43 additional lib/bin harnesses.
The receipt also reports Rust `unknown`; the installed native pin and tool
identities are instead bound in the actual owner snapshot.

## Invocation topology and resources

Before the handoff, each resolver family could launch its own freshness build
inside process tests. In admitted mode, one explicit product preparation build
now supplies both resolvers, which validate the captured artifact and live
owner without invoking Cargo. Developer-mode fallback remains unchanged.

The observed narrow topology contains four Cargo invocations: cached xtask
bootstrap, explicit perllsp build, health harness/test, then version harness/test.
Bootstrap finished in 0.24s, product preparation in 6m54s, the first test-profile
compilation in 7m16s, and the second in 5.01s. Total elapsed was 868.29s. These are
observations of one warm run, not a controlled speedup comparison; harness
compilation remains substantial.

Two-second sampling observed 1,469,988,864 bytes (1.369 GiB) of additional volume
growth, minimum free space 27,359,232,000 bytes (25.481 GiB), and aggregate cgroup
memory peak 6,103,719,936 bytes (5.685 GiB). Volume growth includes the concurrent
immutable evidence copy. Memory is aggregate usage, including cache and other
services, not compiler RSS. No memory-event counters increased, cancellation
occurred, or observer error was recorded. These sampled measurements are not
hard resource quotas or cold-state capacity qualification.

The actual owner reported positive kernel `ECHILD (__WALL)` settlement,
`tree_settled=true`, no errors or pending bound children, and
`lease_released=true`. The original lease was absent after settlement. Observer
exit alone was not used as closure proof. All private compilation cache was
retained.

## Remaining boundary

This proves genuine product capture and both selected Linux resolver paths,
including the native Cargo-test loader composition actually traversed by these
two leaves. It does not prove arbitrary test/build-script loader variants,
Windows, fresh dynamic planner coverage, remaining parser/xtask compiler
fixtures and intentional diagnostics, the canonical nine-package workload,
cold aggregate sizing, or hosted activation. No PR, public comment, Ready,
merge, or hosted run was performed for this experiment.
