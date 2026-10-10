# Native Windows qualification handoff

Helper source: `42ea165fec26d75644d687090876eaa1c6dae4bc` (base `1f973039d812fe080abddc44e461112d7b95a6fa`). Selection remains separate: `ce99f24f7df1f8561ecd28cda57b055094d2920a`, exactly19child paths; receipt `8456eae1dd55f45597fc3e6fcbfe2bf423206701/.spec/17485-current-main/receipt.json`.

Use a fresh independent normal clone with only its owned fixture worktrees. **Installer fans out over every worktree in its common repository.** Check `git worktree list --porcelain` before installation; never invoke it against a shared/old pool. Paths with spaces belong in a separate owned fixture. Clone with `core.autocrlf=false` for byte proof, or record conversion and normalize CRLF exactly as production does. Do not change shared/global Git configuration.

## Admission and build

Use native Windows CPython3.11+, native Git for Windows and installed native Rust1.95, with Git Bash on PATH; WSL/Linux/mocked Windows do not qualify. Record actual executable paths/version/host triple; set `RUSTUP_AUTO_INSTALL=0`, `CARGO_NET_OFFLINE=true`, jobs1or2. All DEVPLANE/CARGO_HOME/temp roots must be native absolute Windows paths, short and private. Do not inherit Linux roots or override admitted target/build paths. No toolchain installation as part of a probe.

Run from the candidate root in PowerShell, after selecting actual installed Python:

```powershell
$env:RUSTUP_AUTO_INSTALL='0'
$env:CARGO_NET_OFFLINE='true'
$env:CARGO_BUILD_JOBS='2'
python scripts/cargo_admitted.py --preflight test --locked --offline -p perl-ci-hygiene --lib --bin perl-ci-hygiene --test change_set_cli --test package_resolver_cli --message-format json-render-diagnostics
```

Preflight is read-only, not reservation or launch. Default40GiB must remain unchanged. If it refuses, stop or qualify a new exact Windows `--budget-file` from its emitted scope with positive reserve/growth and defensible whole-workload basis, checking every actual destination. Do not reuse Linux budgets, change MIN_FREE_GB, bypass guards, or treat estimates as peak measurements.

Under the parent's separately established native Windows ownership/admission route, execute the **same exact request** (remove `--preflight`; optionally insert the qualified `--budget-file` before `test`). Current built-in Windows admission is allowed but legacy: leader0/101 can release without proving descendants ended. Original native owner must prove launched descendants/independent consumers are finished before reuse. Linux ClippyTree/ECHILD is not a Windows instrument; no invented owner command or status-only proof. If no native owner can establish this, retain outputs and report the execution boundary. Do not invoke staged admitted Clippy on Windows: current source explicitly refuses it.

The test request builds only hygiene and its test dependencies. Cargo JSON supplies the normal CLI artifact: target name `perl-ci-hygiene`, `profile.test=false`, executable present. Copy it while the owner retains output control, preserve SHA256/size/path and host triple; never guess target/debug or use PATH hygiene. Actual tests must execute named cases, not silently filter them all out. Counts can differ by platform; Unix-only execute-bit/newline-path tests are N/A, not Windows passes.

Prefer one narrow build on selection `ce99f24f` when qualifying both layers: run the above seam corpus plus selection owner's native corpus against the same exact binary. Record ce99 as executable source and compare unchanged helper owning blobs against42ea; do not relabel it as a42ea binary. No Windows xtask/product build is needed merely to repeat the Linux39-call compatibility proof. If an additional native xtask adapter build is required by parent, admit it separately before launch.

## Required native commands and contract cases

Use Git Bash for the real wrappers, from an unrelated CWD and a source path containing spaces. Resolve the scripts by absolute source path. Each `--budget-file` must match its exact emitted `run --locked -p perl-ci-hygiene -- COMMAND ...` request; different argv/CWD/host/config scopes cannot share a budget.

```bash
bash "$SOURCE/scripts/install-githooks.sh" --budget-file "$INSTALL_BUDGET"
bash "$SOURCE/scripts/check-githooks.sh" --budget-file "$CHECK_BUDGET"
bash "$SOURCE/scripts/githooks-bootstrap.sh" change-set --budget-file "$CHANGE_BUDGET" --base auto --head HEAD --format paths --root "$FIXTURE"
bash "$SOURCE/scripts/githooks-bootstrap.sh" resolve-package-name --budget-file "$PACKAGE_BUDGET" 'crates/perl-ci-hygiene'
```

Omit the budget option only when the unchanged default policy admits the exact request. Actual wrappers must select their source root, clear Git selectors only for their child, and remain product-free by Cargo artifact trace or Windows normal/build dependency analysis. Caller Git environment must remain unchanged.

The real bootstrap package example resolves exactly `perl-ci-hygiene\n` in SOURCE, checked against independent native Cargo metadata. The directory/name-mismatch case below runs the immutable CLI directly from its separate fixture CWD.

Direct immutable CLI commands use argv arrays / PowerShell `& $Binary`, not raw unquoted backslash command text:

- `check-githooks`: missing1 with NOT_PROVEN; current0 with `current: pre-commit`/`current: pre-push`; stale1 with `stale:`; unreadable hook (a directory at hook path is deterministic)1 with NOT_PROVEN. All checks preserve bytes, attributes/ACLs and timestamps. CRLF/trailing whitespace equivalence is accepted by production normalization. **Windows regular-file validity substitutes for Unix executable-bit checks**; chmod/removing Unix mode must not be asserted to fail on Windows.
- `install-githooks`:0, `.githooks` per-worktree, relative repo-local core.hooksPath; common hook directory unchanged. Generated pre-push equals normalized maintained authority text; installer adds the established extra LF. Exercise two owned sibling worktrees, then observe actual installed hook invocation via Git Bash. Snapshot exact bytes/hashes and destinations.
- `change-set --base BASE|auto --head HEAD --format paths|json --root FIXTURE`:0; sorted forward-slash paths with LF; pretty JSON `{base_sha,head_sha,changed_paths}` and one final LF. Use real Git fixtures for docs/mixed paths, rename and deletion, `origin/main` present/`origin/master` absent, root with spaces/native drive spelling, and a staged-tree case through the shared library corpus. Old shell self-diff control must produce empty result while current resolver reports changes. Hostile GIT_DIR/GIT_WORK_TREE/GIT_COMMON_DIR must not redirect explicit-root work. Lightweight implicit root uses CWD; legacy xtask's compiled root is a distinct preserved contract, already proved on Linux.
- Invalid base or format:1, empty stdout, useful diagnostic naming invalid input; no fallback/no panic. Capture bytes rather than PowerShell text-rendering newline conversions.
- `resolve-package-name 'crates/directory-name'` and native backslash spelling:0, exactly `actual-package-name\n` for directory/name mismatch. Unknown directory or removed Cargo.toml:1, empty stdout, no guessed directory fallback.

Git Bash launcher matrix commands:

```bash
python scripts/tests/test-pre-push-launchers.py
bash scripts/tests/test-install-githooks-wrapper.sh
bash scripts/tests/test-worktree-add-hooks.sh
bash scripts/tests/test-hookspath-isolation.sh
```

Tool-fixture preflight is mandatory for these cross-platform instruments: native Python must find Git/Bash/coreutils and create the required fixture tool links (Windows privilege/Developer Mode may be relevant). Missing symlink privilege/PATH is instrument NOT_PROVEN, not a passing skipped case or production defect. If adaptation is needed, preserve a fixture-only patch replacing tool-link construction with source-bound Git Bash shims; do not mutate candidate hook logic or relax assertions.

Both generated and maintained hook authorities must pass: no Nix+flake/just route1 +NOT PROVEN, no gate-passed text/no build retry; Nix+flake precedence, exact `nix develop -c just pr-fast`; no flake falls to just; Nix child37 and just child38 propagate exactly with no retry; docs/nonprotected deletion0 without code gates; protected deletion1; new mixed/unknown base1; single-crate metadata identity used; metadata failure1 without fallback. Stubs prove launcher/exit propagation, **not actual full pr-fast execution**. Actual full launcher and selected nested portfolio need separate supported admission.

## Artifact identity / return

Receipt must bind source/predecessor/base SHAs and trees, dirty/EOL state, exact source file hashes, native OS/architecture, installed toolchain/Git/Python/Bash identities, Cargo JSON-selected artifact hashes, actual argv/CWD/selected root, raw stdout/stderr and statuses, fixture refs/expected independent Git outputs, generated/installed hook hashes, pre/post attributes/times, budget scope/capacity observations, resource/lease identity and original owner settlement. Publish archive+receipt with stable path/content hashes only after parent guidance. State count/skip denominator, instrument failures, native ownership limits, measured wall/net costs and sampling limits.

**Reusable published native Windows binary: none verified.** SHA-specific workflow queries for42ea and prior1b03 returned no runs; existing preserved e675… hygiene/d6ab… xtask binaries are native Linux and cannot qualify Windows. Unrelated Windows launcher/test binaries are not this helper subject. Do not adopt old unproven resources.
