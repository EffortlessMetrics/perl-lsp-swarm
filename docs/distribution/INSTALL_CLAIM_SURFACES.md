# Install Claim Surface Inventory

An indexed inventory of every active material install claim surface in the
tracked documentation and workflow surfaces, with per-row drift status. It is
the reviewable denominator required by
[#11575](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11575)
before any claim-strength validator (#10342) or route catalog consumer
(#11548) may treat coverage as complete.

- **Audited against:** commit `9a3169228b4595` on `origin/main` (2026-08-26).
  Line numbers cite that audit revision; they are locations, not anchors —
  re-check the cited line when a surface drifts.
- **Drift anchor:** workspace version `0.17.0`
  ([`Cargo.toml`](../../Cargo.toml)); verified release receipt `v0.17.0`
  (2026-06-28) per the [Distribution Matrix](../project/DISTRIBUTION_MATRIX.md).
  Rows are judged against those receipts, not against intent.
- **This inventory does not:** judge semantic wording strength, rewrite any
  surface, decide route preference, or mutate public state. Those belong to
  #10342/#10336 and their consumers. Literal-pin hazards are recorded here as
  findings; no version was rewritten in this PR.

## Materiality boundary

A row exists when tracked prose or structured text asserts or materially
implies: an install command or action, a channel or artifact identity,
platform/target support, product-unit membership (`perllsp`, `perl-dap`,
extension), verification behavior, install/upgrade/repair availability, or a
next action after failure (#11575 boundary). Pure contributor tooling installs
and source-build developer recipes are annotated in
[boundary notes](#boundary-notes), not rows.

## Surface index

| ID | Surface | Role | Claim class | Registry cross-ref |
| --- | --- | --- | --- | --- |
| S01 | [README.md](../../README.md) | root landing prose | commands + boundaries | — |
| S02 | docs/how-to/INSTALLATION.md | canonical install guide | full command matrix | — |
| S03 | docs/how-to/GITHUB_ACTIONS.md | CI consumer guide | action usage prose | — |
| S04 | `.github/actions/setup-perl-lsp/action.yml` | reusable composite action | inputs + usage comment | [`action.setup-perl-lsp`](../../policy/install-surface-registry.toml) |
| S05 | `.github/actions/README.md` | actions catalog prose | inputs + platform claims | — |
| S06 | `docs/examples/github-actions/setup-perl-lsp-consumer.yml` | maintained example | executable usage | — |
| S07 | docs/how-to/UPGRADING.md | upgrade guide | reinstall/upgrade claims | — |
| S08 | docs/how-to/TROUBLESHOOTING.md | support guide | diagnostic route advice | — |
| S09 | docs/how-to/EDITOR_SETUP.md | editor integration guide | deferral + currentness claims | — |
| S10 | docs/tutorials/GETTING_STARTED.md | first-run tutorial | condensed install claims | — |
| S11 | `vscode-extension/package.json` | extension manifest metadata | identity fields | [`package.vscode.manifest`](../../policy/install-surface-registry.toml) |
| S12 | vscode-extension/README.md | tracked marketplace copy | marketplace-facing claims | — |

## Claim rows

Drift status vocabulary:

- `current` — consistent with the v0.17.0 receipts and sibling surfaces at audit.
- `pending` — explicitly gated on future work; gate and issue are named.
- `stale_example` — example value is behind the current release line.
- `mutable_pin` — literal-pin hazard (@master/@latest/unpinned git). Finding listed; not rewritten.
- `cross_surface_drift` — contradicts another current surface on the same fact.
- `volatile_number` — numeric public-state value embedded in tracked source.

### S01 — README.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C101 | README.md:20-22 | Marketplace badge "656 installs" in source copy | `volatile_number` | Duplicate of S12 badge; see FND-8 |
| C102 | README.md:68-74 | VS Code: `code --install-extension EffortlessMetrics.perl-lsp-rs`; extension downloads matching binary | `current` | Matches S02/S12 |
| C103 | README.md:76-90 | macOS/Linux: manual archive until immutable installer ref+digest exist; identity-bound curl shape with `INSTALLER_REF`/`INSTALLER_SHA256` | `pending` | Gate: release packet publication (#4348 chain); matches S02 L54-82 |
| C104 | README.md:92-96 | Homebrew from owned tap `brew install effortlessmetrics/tap/perllsp`, not core | `current` | Tap version independently versioned per same lines |
| C105 | README.md:98-108 | Windows x86_64 manual zip; PowerShell installer **not usable** until fix promoted to publication repo (#4348) | `pending` | See FND-9 for split issue references |
| C106 | README.md:110-128 | Verification semantics: `--doctor` diagnostic only, not CI gate; `--health` liveness-only printing `ok <version>`; channels independently versioned vs v0.17.0 receipt | `current` | Matches S02 L282-299, S09 L11-13 |
| C107 | README.md:130-134 | Generic clients launch `perllsp --stdio`; Zed registration absent → unsupported | `current` | Product-unit support boundary claim |
| C108 | README.md:136-138 | Native formatting/critic need no `perltidy`/`perlcritic`; opt-in external tools | `current` | Non-install dependency boundary |

### S02 — docs/how-to/INSTALLATION.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C201 | INSTALLATION.md:13-15 | Verified `v0.17.0` archives are public beta; other channels not proven by receipt | `current` | Channel-independence frame used across surfaces |
| C202 | INSTALLATION.md:19-25 | Fastest-path enumeration: VS Code extension, manual archive, other-editor download, local `cargo install --path crates/perllsp` | `current` | Local path also claimed by S10 C310 |
| C203 | INSTALLATION.md:27-28 | Do not install crates.io package `perl-lsp`; it is another project; supported Cargo package is `perllsp` | `current` | Name-collision boundary; restated L117-118, L259 |
| C204 | INSTALLATION.md:39-66 | Root `install.sh` is bootstrap wrapper; canonical logic in `scripts/install.sh`; piped wrapper non-authoritative without `PERL_LSP_INSTALLER_REF` (40-char SHA) + `PERL_LSP_INSTALLER_SHA256` | `current` | Fail-closed identity bootstrap contract |
| C205 | INSTALLATION.md:68-94 | Remote bootstrap command shape plus env-var options incl. `VERSION=v0.18.0` example | `stale_example` | `v0.18.0` example vs shipped line v0.17.0 (FND-2) |
| C206 | INSTALLATION.md:96-97 | Release-archive platforms: Linux x86_64/aarch64 (gnu/musl), macOS x86_64/aarch64 | `current` | Matches prebuilt table C213 |
| C207 | INSTALLATION.md:99-114 | POSIX installer requires downloadable `SHA256SUMS`, exactly one normalized matching row; fails closed; artifact-integrity ≠ publisher provenance; PowerShell retains fail-open boundary; open boundaries under #6097 | `current` | Basis for FND-7 contrast |
| C208 | INSTALLATION.md:116-122 | `BUILD_FROM_SOURCE=1` installs **perllsp only**, not perl-dap; debugger needs archive or `cargo build -p perl-dap --release` | `current` | Product-unit dimension join risk for #11549 |
| C209 | INSTALLATION.md:124-167 | Windows: manual archive only working path; published installer broken via asset-name mismatch `perl-lsp-*` vs `perllsp-*` (404, #5461); repo copy fixed but unsynced (#4348); installs `%USERPROFILE%\.local\bin` | `pending` | Live v0.17.0 probe quoted L140-143; see FND-9 |
| C210 | INSTALLATION.md:158-167 | Windows script installs `perllsp.exe` only (#5036); only x86_64-msvc built; ARM64 Win10 cannot emulate x64 → build from source (#5007) | `current` | Ground truth contradicted by S05 FND-4 |
| C211 | INSTALLATION.md:169-179 | Pin/dir variants of installer "404 until then"; example includes `irm .../master/install.ps1` URL | `mutable_pin` | Intentional do-not-use exhibit; still a literal master URL; see FND-10 |
| C212 | INSTALLATION.md:189-213 | Scoop/Chocolatey/winget manifest sources exist but are **not proven-current** paths; verify via `scoop search` / `choco search` / `winget search` | `current` | Honest unsupported-channel annotation |
| C213 | INSTALLATION.md:215-240 | Homebrew tap route + two-step form; completions not installed by default, added via `--completion {bash,zsh,fish}` | `current` | Platform breadth L232 matches C206 |
| C214 | INSTALLATION.md:242-260 | Source build: `cargo build --release --bin perllsp -p perllsp`; registry route `cargo install perllsp` | `current` | crate name collision note repeated L259 |
| C215 | INSTALLATION.md:261-280 | Prebuilt v0.17.0 archives; gnu-vs-musl selection rule; suffix table for all seven targets | `current` | Canonical platform matrix |
| C216 | INSTALLATION.md:282-299 | After install: `--stdio`, `--doctor`, `--health ok <version>`; doctor exit status not a CI gate | `current` | Mirrors C106 |

### S03 — docs/how-to/GITHUB_ACTIONS.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C301 | GITHUB_ACTIONS.md:6-13 | Reusable action usage `- uses: .github/actions/setup-perl-lsp@master` with `version: '0.12.3'` | `mutable_pin` + `stale_example` | The miner-flagged pin; see FND-1, FND-2 |
| C302 | GITHUB_ACTIONS.md:15-23 | Action resolves tag or pinned version, downloads per-OS archive, verifies against published `SHA256SUMS`, optional source-build fallback, adds to `PATH` | `current` | Behavior verified against S04 resolve/download steps |
| C303 | GITHUB_ACTIONS.md:38-42 | Explicit `version:` for reproducible install; `version: latest` endorsed for newest binary | `mutable_pin` | latest = floating currentness; FND-3 |

### S04 — .github/actions/setup-perl-lsp/action.yml

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C401 | action.yml:1-5 | Header usage comment shows `@master` ref and `version: '0.12.3'` | `mutable_pin` + `stale_example` | FND-1, FND-2 |
| C402 | action.yml:12-16 | Input `version`, default `latest` (resolves GitHub `/releases/latest`) | `mutable_pin` | Default floats across releases; FND-3 |
| C403 | action.yml:17-24 | Inputs `cache` (default true) and `install-dir` (default runner temp) | `current` | |
| C404 | action.yml:25-28 | Input `build-from-source` (default false) | `current` | |
| C405 | action.yml:70-105 | Platform/target resolution incl. Windows ARM64 → `aarch64-pc-windows-msvc` release target | `cross_surface_drift` | No such published asset exists; contradicts C210/S05 claims; FND-4 |
| C406 | action.yml:183-190 | Release mode downloads exact asset plus `SHA256SUMS` and verifies before extraction | `current` | Matches C302/C207 strength |

### S05 — .github/actions/README.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C501 | actions/README.md:30-35 | "Supported release binaries auto-resolve" for Linux/macOS/Windows each listing `x86_64`, `aarch64` | `cross_surface_drift` | Windows aarch64 has no released binary (C210); FND-4 |
| C502 | actions/README.md:36-41 | Usage block with `@master` ref and `version: '0.12.3'` | `mutable_pin` + `stale_example` | FND-1, FND-2 |
| C503 | actions/README.md:43-45 | Inputs table: `version` default `latest` | `mutable_pin` | FND-3 |

### S06 — docs/examples/github-actions/setup-perl-lsp-consumer.yml

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C601 | setup-perl-lsp-consumer.yml:37-41 | Executable consumer step pins `setup-perl-lsp@master` with `version: latest` | `mutable_pin` | Both mutable dimensions combined; FND-1, FND-3 |

### S07 — docs/how-to/UPGRADING.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C701 | UPGRADING.md:18 | Cargo-installed users: `cargo install --locked perllsp`; pin `--version 0.17.0` only after verifying receipt | `current` | Version-consistent pin guidance |
| C702 | UPGRADING.md:35-49 | Reinstall server/adapter: `cargo install --locked perllsp` + `cargo install --locked perl-dap`; local checkouts via `--path … --force` | `current` | Binary pair matches product-unit model |
| C703 | UPGRADING.md:31 | Channels independent: GitHub Release, crates.io, marketplace, Homebrew are separate receipts | `current` | Same frame as C201 |

### S08 — docs/how-to/TROUBLESHOOTING.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C801 | TROUBLESHOOTING.md:3-16 | If basic probes fail, fix binary installation and `PATH` first before deeper debugging | `current` | Route-recommendation claim (diagnostic-surface class) |

### S09 — docs/how-to/EDITOR_SETUP.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C901 | EDITOR_SETUP.md:3-9 | Page defers installation to INSTALLATION.md; VS Code-compatible editors get managed download; generic clients need exact binary on PATH | `current` | Deferral surface, no install commands |
| C902 | EDITOR_SETUP.md:11-13 | v0.17.0 assets are beta; marketplace/package-manager versions pending or unproven; verify `--version`/`--health` | `current` | Channel currentness claim |

### S10 — docs/tutorials/GETTING_STARTED.md

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C1001 | GETTING_STARTED.md:14-15 | Other editors: put `perllsp` on PATH; run `--health` first | `current` | |
| C1002 | GETTING_STARTED.md:24 | Product unit: single native binary, "No Perl runtime is required" | `current` | Parser-embedded posture claim |
| C1003 | GETTING_STARTED.md:38-41 | `code --install-extension EffortlessMetrics.perl-lsp-rs`; extension downloads matching binary | `current` | Condensed duplicate of C102 |
| C1004 | GETTING_STARTED.md:47-53 | Identity-bound wrapper shape identical to S01/S02 | `current` | Third restatement of bootstrap (annotation-dedup candidate) |
| C1005 | GETTING_STARTED.md:58-61 | Canonical installer "verifies against the release SHA256SUMS when that file is available" | `cross_surface_drift` | Installer now requires the manifest and fails closed (C207); conditional phrasing describes the old fail-open mode; FND-7 |
| C1006 | GETTING_STARTED.md:68-79 | Windows zip route; other editors: "Download the latest archive" from Releases | `mutable_pin` | "latest" currentness phrasing without pin; FND-3 |
| C1007 | GETTING_STARTED.md:86 | `cargo install --path crates/perllsp` local-testing route | `current` | |
| C1008 | GETTING_STARTED.md:93-103 | Post-install probes `--version`/`--health`/`--info`/`--check` | `current` | |

### S11 — vscode-extension/package.json

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C1101 | package.json:2-5 | Extension identity `perl-lsp-rs`, display "Perl Language Server (perl-lsp)", description claims native Rust LSP+DAP, version `0.17.0` | `current` | Identity triple (`perl-lsp-rs`/`perllsp`/foreign `perl-lsp`) is the collision field consumed by #11549 subject binding |
| C1102 | package.json:35 | Virtual-workspace limitation note | `current` | Scope-of-support metadata claim |

### S12 — vscode-extension/README.md (tracked marketplace copy)

| ID | Location | Claim | Drift status | Notes |
| --- | --- | --- | --- | --- |
| C1201 | README.md:7-10 | Badge "656 installs" | `volatile_number` | FND-8 |
| C1202 | README.md:118-137 | Install from VS Marketplace or Open VSX; `code`/`codium`/PearAI variants of `--install-extension` | `current` | Marketplace-facing command set |
| C1203 | README.md:137,190-192 | Managed binary auto-download on first activation; settings `autoDownload`, `serverPath`, `channel` default `"latest"` | `mutable_pin` | Channel default floats; FND-3 |
| C1204 | README.md:145-148 | No native ARM64 Windows build; x64 fallback; links build-from-source | `current` | Reinforces C210; contradicted upstream by C501 |
| C1205 | README.md:157,208 | INTERNAL_DEPLOYMENT guidance linked via `blob/master` URLs | `mutable_pin` | File exists in-tree (`vscode-extension/INTERNAL_DEPLOYMENT.md`); FND-5 |
| C1206 | README.md:163-176 | Manual Installation: brew tap; identity-bound curl bootstrap identical to S01/S02/S10 | `current` | Fourth restatement of bootstrap |
| C1207 | README.md:179 | `cargo install --git https://github.com/EffortlessMetrics/perl-lsp --package perllsp` | `mutable_pin` | Git route without tag/rev pin; FND-6 |
| C1208 | README.md:383 | Open VSX listed as alternative marketplace | `current` | Matches Distribution Matrix channel row |

## Findings

Pin hazards and cross-surface contradictions, ordered by blast radius. Each is
recorded, not repaired: disposition belongs to the owning family issues.

- **FND-1 — `@master` literal action pins (consumer-facing).**
  `docs/how-to/GITHUB_ACTIONS.md:9`,
  `docs/examples/github-actions/setup-perl-lsp-consumer.yml:38`,
  `.github/actions/README.md:37`, and header comment
  `.github/actions/setup-perl-lsp/action.yml:3` all teach consumers to bind the
  reusable action to the mutable `master` ref of the publication repository.
  The repo's own supply-chain ledger already tracks this class internally for
  third-party actions (`MAY2026-MEDIUM-001`); these four are first-party
  consumer-teaching instances.
- **FND-2 — stale example versions behind the release line.** Example pins
  `version: '0.12.3'` (`GITHUB_ACTIONS.md:11`,
  `.github/actions/README.md:39`, `action.yml:4`) and `VERSION=v0.12.0`
  (`scripts/install.sh:7`), `VERSION=v0.18.0`
  (`INSTALLATION.md:91`) span three releases of examples while the audited
  line is `v0.17.0`. Individually cosmetic; collectively they normalize
  versionless copying.
- **FND-3 — `latest` mutable defaults endorsed in prose.** Action default
  (`action.yml:14-16`), endorsement (`GITHUB_ACTIONS.md:40-42`), example use
  (`setup-perl-lsp-consumer.yml:40`), tutorial phrasing
  (`GETTING_STARTED.md:79`), extension channel default
  (`vscode-extension/README.md:192`). Each makes currentness a moving target
  that the v0.17.0 receipt does not cover (contradicting C201's own warning).
- **FND-4 — Windows ARM64 contradiction.** `.github/actions/README.md:30-35`
  claims Windows `aarch64` binaries auto-resolve and `action.yml:105`
  maps ARM64 runners to the `aarch64-pc-windows-msvc` release asset, while
  `INSTALLATION.md:162-167` and `vscode-extension/README.md:145` state only
  `x86_64-pc-windows-msvc` is built and there is no native ARM64 Windows
  binary. A windows-arm64 release-mode run requests a nonexistent asset.
- **FND-5 — in-tree file advertised through mutable `blob/master` URL.**
  `vscode-extension/README.md:157` and `:208` link
  `INTERNAL_DEPLOYMENT.md` via the publication repo at `master`, although the
  file is tracked here; relative links would carry the same content without a
  mutable ref.
- **FND-6 — unpinned git-source install.** `vscode-extension/README.md:179`
  offers `cargo install --git …` with no `--tag`/`--rev`; drifts freely with
  default branch movement.
- **FND-7 — checksum-semantics drift between siblings.**
  `GETTING_STARTED.md:59-61` describes the POSIX installer as verifying
  "`when that file is available`" (fail-open era);
  `INSTALLATION.md:99-104` states the canonical installer now requires
  `SHA256SUMS` and fails closed. Both describe the same
  `scripts/install.sh`.
- **FND-8 — volatile numeric counts in tracked source.** "656 installs"
  badges at `README.md:21` and `vscode-extension/README.md:9` embed live
  public metrics inside audited source copy.
- **FND-9 — PowerShell-breakage annotation spread across four sites with
  split issue references.** The broken-Windows-installer claim appears at
  `README.md:103-108` (cites #4348), `INSTALLATION.md:23` (cites #5461),
  `INSTALLATION.md:129-147` (cites both), and the `install.ps1` header
  (cites both). Any promotion of the fix must update four prose sites.
- **FND-10 — intentional do-not-use exhibits contain literal mutable URLs.**
  `INSTALLATION.md:174` keeps an `.../master/install.ps1` irm one-liner as an
  explicit 404 exhibit. Deliberate, but any literal-pin linter (#10342
  cutover) will flag it; record as expected-noise allowlist input.

## Boundary notes

Adjacent surfaces inspected and deliberately **not rowed**:

- Contributor tooling installs (`cargo install cargo-llvm-cov`,
  `cargo-semver-checks`, `flamegraph`, `tokio-console`, `cargo-udeps`,
  `cargo-outdated`, `cargo-audit`, `cargo-deny`) across
  `docs/how-to/{COVERAGE,SEMVER_WORKFLOW,LARGE_WORKSPACE_GUIDE,DEAD_CODE_DETECTION,DEPENDENCY_MANAGEMENT}.md`
  — developer toolchain acquisition, not perl-lsp distribution claims.
- Contributor source-build recipes (`DEBUGGING.md:33,175`
  `cargo install --path crates/perl-dap`; `AI_BUILD_GUIDE.md` `cargo build`)
  — development flows feeding DEBUGGING/AI audiences rather than user
  distribution, though DEBUGGING's perl-dap row is the nearest boundary case.
- Editor setup guides under `docs/EDITORS/` defer installation to S02 and
  assert configuration, not acquisition; they inherit S02 drift wholesale.
- Machine-readable producers, validators, and workflows
  (`policy/install-surface-registry.toml`, `install.sh`, `install.ps1`,
  `scripts/install.sh`, `distribution/*` manifests, release workflows) are
  owned by the existing registry surfaces; this inventory covers their
  *prose projections* only.

## Family handoff notes

Structured findings downstream lanes consume directly:

For **#11548** (catalog assembly): every current/pending row above is a prose
claim source that today has no generated fragment backing (no #10339 fragments
exist for any row). The denominator gap between this inventory and
`policy/install-surface-registry.toml` is total on the prose side — the
registry holds zero README/tutorial/guide surfaces — so catalog inputs must be
joined through this document until fragment generation lands. Duplicated
bootstrap claims (C103/C204/C1004/C1206) and verification claims
(C106/C216/C1008/C801) need one canonical fragment each plus bounded curated
annotations at the other three sites.

For **#11549** (classifier/preference): three conjunctive dimensions are
currently asserted inconsistently across surfaces and must not be collapsed —
(a) Windows ARM64 support: unresolved between C501+C405 versus C210+C1204;
(b) SHA256SUMS enforcement mode: fail-open residue C1005 versus fail-closed
C207; (c) product-unit membership under `BUILD_FROM_SOURCE=1` (server-only,
C208) versus archives (server+adapter pair, C209/C212). Subject-identity
binding must also respect the four-name collision field documented at C203 and
C1101 (`perl-lsp` foreign crate, `perllsp` server, `perl-dap` adapter,
`perl-lsp-rs`/`EffortlessMetrics.perl-lsp` extension IDs).
