# Copied xtask and sibling installer reproduction

Observed 2026-10-15 UTC on base `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`.
This is a test-only collaboration slice for the desktop installer owner
`01a13da1-660e-7308-92b7-ddb4da76e0c6`. No production installer edits belong here.
The separate `test/installer-selection-contract` branch owns planner-selection
fixtures; this branch does not change that candidate.

## Instrument and boundary

`scripts/tests/test_installer_root_fixtures.py` copies these complete production
modules into a task-owned disposable crate, without rewriting their bodies:

| Source | SHA-256 |
| --- | --- |
| `xtask/src/utils.rs` | `1a38b9defdff70fcff6967cd820590431edcd95b2908d6a29ede1d139fbb1c14` |
| `xtask/src/tasks/ci_hygiene.rs` | `56b86002a2c128e1542ebdd632eeaeefc81015e51653d6945d494f72ee28e125` |
| `crates/perl-ci-hygiene/src/git_hooks.rs` | `948a28595769b5483900ddcc343ae39ac3f49569cde57cf62cd8a7c43a1c736f` |

The existing hygiene library is a path dependency. The standalone CLI's actual
`find_repo_root` body is extracted unchanged. A thin driver substitutes nested
Cargo compilation and standalone dispatch, calling the actual installer. Both
the Cargo fallback and fresh local-binary branches execute. A canonical sandbox
guard refuses targets outside the task-created repositories. Installer commands
were never run on shared real worktrees.

This is production-module proof, not execution of the full copied xtask CLI or
the shipped standalone CLI parser. Explicit runtime selection calls the actual
`select_worktree_root` function; it does not establish public flag parsing.
Linux execution does not establish native Windows/macOS qualification.

## Observed acceptance results

Nine tests executed: **four intended failures, five passing controls, zero
instrument errors**. These are deliberately red acceptance fixtures for the
installer owner's repair, not a passing repair candidate.

| Case | Result on base |
| --- | --- |
| Foreign caller without a project refuses before writes | FAIL: returns zero through compiled root |
| Copied forwarder preserves compiled checkout and sibling | FAIL: compiled `.git/config` and both trees' pre-commit/pre-push changed |
| Explicit component root preserves sibling hook bytes/private configuration | FAIL: sibling pre-commit and pre-push overwritten |
| Explicit component root preserves sibling effective configuration | FAIL: sibling `core.hooksPath` changes from unset to `.githooks` |
| Explicit component root installs both hooks and preserves unrelated repository | PASS |
| Explicit runtime root aligns forwarder and child | PASS |
| Explicit runtime root refuses inherited `GIT_DIR` with clear diagnostic and no writes | PASS |
| Fresh local-binary dispatch preserves unrelated repository | PASS |
| Invalid explicit component root refuses with Git diagnostic and no writes | PASS |

The fresh branch's success alongside fallback failure exposes dispatch-dependent
behavior. Both possible selected roots receive fresh binaries in that control,
so fixing default root selection cannot silently turn it into fallback proof.

Snapshots compare raw bytes, existence and hook-directory inventories, including
CRLF/NUL/non-UTF-8 sentinels, managed and common/private hook directories, local
configs and private `config.worktree` absence. Sibling private Git metadata is
selected by its actual Git directory, not its worktree path. A separate Git read
checks effective sibling hook configuration. Success requires both embedded hook
authorities with the writer's additional LF, POSIX executable bits, and active
`.githooks` configuration. Unrelated post-commit hooks remain byte-identical.

A controlled no-op installer returning zero was substituted at the invocation
seam: all three installation-success controls failed with assertions and zero
instrument errors. This rejects vacuous success. Independent review requested
and obtained snapshot, effective-config, payload and dispatch hardening; those
findings were repaired and affected proof rerun.

## Reproduction and capacity

Use installed pinned Rust 1.95.0 and Python 3.10+; provide an explicit host sizing
basis. This run used Python 3.12.14, Git 2.52.0, one offline two-job build, a
16 GiB reserve and 4 GiB total-growth allowance for the small hygiene/module graph.
Those numbers are exploratory task policy, not a qualified production envelope.
Admission explicitly reports `basis_verified: false`.

```bash
TMPDIR=/workspace/scratch \
INSTALLER_ROOT_FIXTURE_BUILD=1 \
INSTALLER_ROOT_FIXTURE_RESERVE_GIB=16 \
INSTALLER_ROOT_FIXTURE_GROWTH_GIB=4 \
INSTALLER_ROOT_FIXTURE_BASIS='Task-owned exploratory offline hygiene-module fixture; one two-job build without product graph; retain 16 GiB and allow 4 GiB total growth; not a qualified production envelope.' \
CARGO_HOME=/workspace/shared/cargo \
RUSTUP_HOME=/workspace/shared/rustup \
PATH=/workspace/shared/cargo/bin:$PATH \
python3 -B scripts/tests/test_installer_root_fixtures.py
```

The test invokes unchanged `scripts/cargo_admitted.py` for read-only preflight
and scope-bound execution. Initial `/tmp` admission refused before compilation;
the workspace volume admitted this exact bounded request. An initial harness
build missed `toml`; no behavioral result was claimed for it. The corrected build
completed; kernel ECHILD settlement confirmed release of its owned Cargo lease.
An enhanced payload oracle initially omitted the production writer's appended
LF; that harness defect was corrected before the final results above.

Final test source SHA-256:
`9b3401a92633abf6fa05276212002a5f0cb06019e87378406cf9ab388b42b089`.
Immutable module executable SHA-256:
`11ace8955e2c252567598a54f3ddbb98bf7d615b8a5cd0c8b21ab6ced3e6bc25`.
Final test-only oracle changes were replayed against that same executable,
checking copied module byte equality first; no production module changed.

Runtime evidence retained for the coordinating executor:

- `/workspace/scratch/installer-root-qualified.log`, SHA-256 `a2ed09f4d9ce0674a23748a6a463e646886c44e9d71c08b8cfbf1efd3c1eec30`
- `/workspace/scratch/installer-root-noop.log`, SHA-256 `7172c8f06fa6f7f270a1e1ade48478630fbf91680283d29a41cfc784900d19f0`
- `/workspace/scratch/replay_installer_fixture.py` and `noop_installer_control.py`
- `/workspace/scratch/installer-root-proof-3nql1iwi/`: immutable executable,
  copied modules, generated Cargo.lock, budget, build logs and private resources;
  retained with reason: reproduction/evidence, approximately 411 MiB.

These runtime paths are executor-local; the committed test and this evidence
summary are the portable deliverables. No uploaded raw-log artifact is claimed.
Default opt-out discovers nine skipped tests and performs no build/installation.

## Provisioning and handoff

Environment provisioning completed separately from execution. Git fetch refreshed
main; connected GitHub reads refreshed open candidates. Shell `gh` reads returned
Forbidden. No protections, credentials, workflow activation, release or deployment
were changed. Repository writer preflight passed six checks but reported missing
pre-push hooks. The user's prohibition on invoking the installer in shared real
worktrees was preserved. Normal commit/push cannot be represented as hook-qualified
when hooks are absent; Ready/merge qualification remains blocked.

Keep this a collaboration draft. The desktop owner must integrate the disjoint
fixtures with the repair, demonstrate green affected acceptance against the exact
repair, and qualify full CLI/platform/publication behavior through normal gates.
No integration or deployment is requested by this evidence candidate.
