# Change Log

All notable changes to the Perl Language Server extension will be documented in this file.

## [Unreleased]

### Changed

- **Packaged activation failures are provably cleaned up and recoverable on
  the installed path.** The published-smoke harness can now fail one
  deterministic pre-commit activation boundary of the INSTALLED extension —
  a seam armed only when the private smoke-harness extension is present and a
  namespaced test variable names a real activation phase — and a new packaged
  journey proves in the real extension host that the failed attempt rolls
  every mandatory registration back (command-registry truth), keeps exactly
  the approved support surfaces, leaves no candidate server process behind,
  cannot start one from real document demand, and consumes no crash-recovery
  budget; an explicit retry against the same installed profile then activates
  the same bundled candidate once — no duplicate servers on new demand —
  answers a representative provider request, and shuts down cleanly, all
  recorded in one candidate-bound `vscode_activation_recovery.v1` receipt.
  (#7856)
- **Production activation failures are now deterministically injectable at
  every resource boundary.** The activation owner exposes a test-only
  phase-boundary failure injector, so a test can fail the real `activate()`
  composition immediately after a named production resource registers —
  through the same rollback path any mid-activation exception takes — without
  monkey-patching module globals. The injected matrix proves reverse-order
  rollback of every crossed boundary, retained support surfaces, clean retry
  from baseline without duplicate resources, stale-callback barriers, bounded
  cleanup-failure receipts, mid-rollback event tolerance, and fallback-path
  deactivation on the partial state. (#7855)
- **Extension activation now runs inside one transactional owner with a
  deterministic rollback stack.** Every activation-created resource (status
  and health surfaces, command groups, workspace/configuration listeners,
  document providers, debugger registrations, the language-client lifecycle,
  and the server-demand coordinator) registers with a single activation
  attempt that carries an explicit phase and resource class. A failed
  activation rolls back in reverse registration order — retaining only the
  support surfaces needed to report the failure — and clears module-level
  projections from the same authority; a successful activation commits before
  `perl-lsp.activated` is published, and ordinary deactivation reuses the
  same cleanup primitives. (#7854)
- **Garbage-collect managed server generations only when proven stale.**
  Installs now mint an immutable candidate manifest and a versioned
  current-selection record, each extension-host session persists a live
  reference to the candidate it actually launched (released on clean
  shutdown), and stale-generation cleanup consumes the landed retention
  policy: only candidates classified `stale_unreferenced` are deleted. The
  current selection, the previous-known-good fallback, live-referenced,
  unknown, and partial generations are always preserved, and unreadable or
  incomplete evidence blocks destructive cleanup instead of widening it. The
  legacy `current` pointer stays in place, so rollback to an older extension
  build keeps working. (#10083)
- **Moved the extension toolchain authority to Node 26.x and npm 11.18.0; CI
  pins Node 26.5.0.** The shipped-extension compatibility boundary remains
  `engines.vscode`. (#4121)
- **Migrated the extension toolchain to TypeScript 7, type-aware Oxlint,
  Oxfmt, and Rolldown.** TypeScript remains the type-check authority;
  Rolldown now produces the CommonJS extension bundle. (#3645, #3690,
  #3721, #3736, #3755)
- **Reduced the packaged VSIX from 458 files / 1.25 MB to 33 files / 291
  KB** while preserving extension activation, command registration, native
  LSP and DAP startup, binary auto-download and extraction, source maps,
  integration tests, and published-extension smoke coverage. (#3755)
- **Removed the legacy `ts-jest`, ESLint, and `@typescript-eslint`
  compiler-API dependencies.** No TypeScript or JavaScript toolchain
  binaries are shipped inside the extension. (#3645, #3690)
- **Made extension development and packaging reproducible:** npm/Node
  authority, all TypeScript authority configurations, non-growing strictness
  and lint baselines, exact-source VSIX/current-server smoke, repeated startup
  receipts, and VSIX inventory/size checks now have explicit gates and current
  documentation.

### Added

- **Folder-less debug launches are refused early with followable advice, and
  `perl-lsp.debug.trustedRoots` supplies folder-less launch authority.**
  Debugging a lone Perl file with no workspace folder open used to spawn
  `perl-dap` with no startup authority and fail with a launch refusal naming
  `--trusted-root` / `--allow-unbounded` CLI flags the extension UI does not
  expose. Launch resolution and the `perl-lsp.debugTest` command now refuse
  before the adapter starts, with one actionable warning, unless the launch
  carries its own authority. The new machine-scoped `perl-lsp.debug.trustedRoots`
  setting is forwarded to the adapter as `--trusted-root`, one per entry, so
  the refusal's advice is followable from editor settings and folder-less
  launches can be authorized deliberately; workspace settings can never set
  it. (#16554)
- **First-run include-path discovery**: on activation the extension scans
  common Perl module directories (`src`, `local`, `vendor`, `lib`, `t/lib`,
  `blib/lib`, `modules`) and offers a one-time suggestion to add any directory
  that holds `.pm` files but isn't in `perl-lsp.includePaths` — so projects
  without a `lib/` layout no longer fail silently. The suggestion is cached per
  unique project structure and never re-prompts after dismissal. (#1633)
- **AI completion discoverability**: a new "AI-Powered Completions (Optional)"
  walkthrough step plus a capability-gated one-time prompt. When the running
  server advertises `inlineCompletionProvider` and the feature is off, the
  extension offers to enable `perl-lsp.aiCompletion.enabled`. The setting
  description now states it is off by default and server-gated. (#1634)
- **Demo project**: a new `Perl: Open Demo Project` command opens a bundled
  demo project (`lib/Utils.pm`, `lib/Database.pm`) so first-time users can try
  completion, hover, and go-to-definition without their own project ready. The
  Get Started walkthrough's "Open a Perl Project" step now links to it. (#1635)

### Fixed

- **A coexistence conflict caused by one global setting is no longer repeated
  for every workspace root.** The advisory evaluates one host-wide pass plus
  one pass per workspace folder, and each folder pass re-read the same global
  and workspace settings. Because a finding's suppression identity includes
  its scope and folder name, a single condition — for example the native
  critic running alongside an installed Perl Navigator, which is the default
  configuration — was reported once host-wide and again under every root:
  twice in a single-root window and N+1 times across N roots. "Disable for
  this exact conflict" then dismissed only one of those identities, so the
  same advisory returned on the next window and could not be cleared in one
  action. A conflict already established host-wide is now reported once, at
  user scope; a folder still reports a conflict it genuinely establishes
  itself, such as a real folder override or a folder-local `.perltidyrc`.
  (#16000)

- **Extension activation no longer blocks on language-server startup.** UI and
  commands now register and activation returns immediately while the language
  client's startup tail completes in the background, instead of blocking
  activation (and every command that depends on it) behind a slow server
  start on large workspaces. (#3162)

### Security

- **Workspace Trust is now required for binary download and server spawn.**
  The extension manifest now declares `untrustedWorkspaces.supported: false`,
  and the activation path defers language server startup, auto-download,
  background update checks, and reinstall commands until workspace trust is
  granted. This prevents untrusted workspace content from triggering binary
  downloads or spawning server processes. (#4631)

### Language intelligence

#### Fixed

- A custom perl-lsp.serverPath that points to a directory or a non-executable file is now rejected at startup with a message naming the actual problem, instead of being spawned and failing with a misleading permission error. Paths starting with ~ are expanded to the home directory instead of silently falling back to a different bundled or downloaded binary. ([#18119](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18119))

### Editor behavior

#### Added

- Added a versioned accessibility inventory and semantic guards for extension-authored VS Code control surfaces. ([#7882](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7882))
- The POD preview webview now ships a main landmark, a theme-variable focus indicator, and enforced semantic/theme/reflow accessibility contracts, and the installed VS Code smoke writes a candidate-bound accessibility receipt (keyboard routes, labels, and honest not-proven screen-reader rows) for the exact packaged VSIX (#7807, ([#17044](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17044))

#### Changed

- Workspace status now separates the product name from runtime server state and reports the observed server version only from a live initialize probe, preserved verbatim and single-line instead of asserting an executable identity the snapshot never established. ([#6869](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6869))
- **Breaking:** New .pm and .t files are no longer auto-populated. Use Perl: Create Module or Perl: Create Test to choose a new file and create it with starter content. The old perl-lsp.autoPopulateNewFiles setting is retired; existing true/false values have no effect. ([#16515](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16515))

#### Fixed

- Status bar now shows workspace file and error counts in the running state; HealthWidget.setFileCount/setErrorCount are wired to client-side diagnostics and workspace file-scan telemetry instead of being dead code. ([#4712](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4712))
- A mid-session server crash is no longer silent: the extension now shows an error toast with a diagnosis hint, attempts up to 3 automatic restarts, and prompts the user to restart manually or run a Health Check once the retry budget is exhausted. ([#4732](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4732))
- The format-on-save error cooldown was reduced from 30s to 5s so a formatting error doesn't silently block saves for half a minute, and the indexing status bar now shows file count. ([#5184](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5184))
- Show Output, Reinstall, Restart, and Status Menu commands are now available even without a Perl file open, all 35 commands are now grouped under a single "Perl" category in the command palette instead of "Perl LSP", and format-on-save now logs a debug message explaining why formatting was skipped while the server is still indexing. ([#5188](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5188))
- The extension no longer accumulates duplicate @INC/AST output channels on repeated use, the status bar now renders indexing percentage when the server reports it, and the file count shown in the status bar now refreshes when workspace folders change instead of being computed once and never updated. ([#5190](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5190))
- The deprecated perl-lsp.perlcritic.theme setting's deprecation message now points to perl-lsp.critic.profile instead of the nonexistent perl-lsp.critic.theme. ([#5291](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5291))
- The Test Explorer's TAP result parser now honors '# TODO' and '# SKIP' directives, reporting them as skipped instead of counting an expected-to-fail TODO test as a failure or a SKIP test as a pass. ([#5301](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5301))
- The Test Explorer's prove-based test runner now resolves the prove executable from the directory of the perl found on PATH (so perlbrew/plenv users whose shims are on PATH get the matching prove) and falls back to shell:true on Windows, instead of failing on Windows where prove is a .bat shim. Note: this resolution uses the PATH perl and does not consult the perl.perlPath setting. ([#5305](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5305))
- A failed or incomplete startup health check no longer reports that Perl is missing. The extension now names a root cause only when an explicit check result establishes it, and otherwise says the cause could not be determined. ([#10149](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10149))
- Per-folder include paths now reach the language server. The server asks for its configuration once per workspace folder, but those requests were resolved against the unrelated `perl` settings namespace instead of the extension's `perl-lsp` one, so every folder-scoped answer came back empty and only a single workspace-wide push was ever applied. Include paths set on one folder of a multi-root workspace are now answered for that folder alone — note this means a folder-scoped `perl-lsp.includePaths` value that previously had no effect will now take precedence over a checked-in project config for that folder. Critic settings remain session-wide on the server and are unchanged. Accepting the discovered-include-paths suggestion, and setting Critic severity from the command palette, now write to the folder you acted on rather than publishing the value to every folder in the workspace. ([#14447](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14447))
- `perl-lsp.autoPopulateNewFiles` now honours the per-folder scope it is contributed with. The setting is declared `resource`, so the settings UI offers it per workspace folder, but the file-creation listener read it once without a resource before looking at the created files — a read with no resource cannot see a folder value, so in a multi-root workspace the global or workspace-wide value decided the behaviour for every folder. The gate is now resolved against each created file's own URI: a folder that sets the value to `false` no longer has new `.pm` and `.t` files auto-populated even when the workspace-wide value is `true`, and a folder that sets it to `true` gets population even when the workspace-wide value is `false`. A workspace opened as a single folder has no per-folder layer and behaves exactly as before, as do files created outside any workspace folder and workspaces that never set the value. A `.code-workspace` listing exactly one folder is multi-root for this purpose: a value set on that folder now takes effect where it previously did not. ([#14547](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14547))
- Language-client shutdown now retains the spawned server process witness so Run Health Check can distinguish a live failed process from a safely exited one before retrying startup. ([#15162](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15162))
- Run Health Check now reports the current language-server startup and recovery state, revalidates setup across one bounded lifecycle replacement, and labels explicit-path checks as setup diagnostics. ([#15146](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15146))
- Run Health Check now keeps binary presence separate from runtime startup, reports cleanup-blocked failures with reload guidance, prevents stale startup diagnoses from overwriting a recovered server generation, and keeps transport-write failures on the public request path without orphaned rejections. ([#15158](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15158))
- Health Check now fails closed when the language-server process witness is unavailable or malformed, preventing replacement startup from assuming cleanup completed. ([#15159](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15159))
- The Windows Perl Test Explorer path runs through Perl's documented stdin test-list interface, so test files with spaces and shell metacharacters execute without the batch-shim globbing failure. ([#15163](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15163))
- Keep each workspace folder's Perl include paths separate during startup and live setting changes, while retaining include paths for standalone files. ([#16955](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16955))
- Reset the automatic crash recovery budget when Restart Now is accepted after changing server settings, matching the Restart Server command. ([#17966](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17966))

#### Security

- AI-completion settings are now machine-scoped in the extension, the 'Enable' suggestion writes the user (global) setting instead of a workspace setting, and changes to machine-scoped AI-completion settings are re-sent to the server, so a workspace can no longer switch AI completion on for its users. ([#5022](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5022))
- The extension now forwards external include roots to the server only from the user/machine-scoped externalIncludePaths setting (inspect().globalValue), never from workspace- or folder-scoped values, so a checked-in .vscode/settings.json can no longer widen the server's include path on behalf of the user. ([#5021](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5021))
- The legacy tooling settings perl-lsp.perltidyConfig, perl-lsp.critic.engine, perl-lsp.perlcritic.profile and perl-lsp.perlcritic.theme are now machine-scoped, so a checked-in .vscode/settings.json can no longer set them and a cloned repository cannot re-arm the legacy Perl::Critic/perltidy subprocess path on your machine; set them in your user or machine settings instead. ([#5023](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5023))

### Performance and reliability

#### Added

- Added a bounded VS Code client measurement recorder that keeps missing instrumentation distinct from zero cost. ([#7884](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7884))
- Added a generation-owned crash recovery arbiter that deduplicates watchdog and process-exit failure episodes. ([#7888](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7888))

#### Changed

- Added a transactional activation resource owner with deterministic rollback and idempotent deactivation. ([#7918](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7918))
- The Perl language server now starts on demand instead of on every activation. Gherkin-only, walkthrough-only, and debug-configuration sessions no longer spawn or download a language server they never use, and the status bar reports a truthful "not started" state instead of an indefinite spinner. Opening a Perl file later in the session starts exactly one server without a window reload. ([#10221](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10221))
- The VS Code language-client lifecycle retains a completed cleanup episode's exact process subject and permits an explicit restart only after that subject is observed terminal; failed client cleanup, unknown observation, and timed-out startup remain blocked and request reload remediation (#14486). ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15168))

#### Fixed

- Perl test discovery caps the initial .t file collection at 500 files, removing one source of unbounded work in large workspaces, and binary-download redirects are bounded to a small count instead of potentially following a redirect loop indefinitely. ([#5141](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5141))
- The extension now runs a liveness watchdog that pings the language server every 30 seconds and automatically restarts it when a request goes unanswered for 10 seconds, so a hung server recovers on its own instead of staying unresponsive until a manual restart. ([#5152](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5152))
- Restored watchdog recovery of a hung language server; a shutdown-timeout stop rejection from an already-stopped client no longer blocks the replacement as incomplete cleanup. ([#14155](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14155))
- Restarting the language server or rebuilding Test Explorer integration no longer retains disposed test adapters in the extension host subscription array. ([#18327](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18327))
- Ignore malformed disabled-feature settings during language-client startup, filter non-string feature entries, and apply feature-disable aliases only for explicit false settings. ([#18322](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18322))

#### Performance

- Added an evidence-backed VS Code client performance policy layer that classifies metrics by evidence quality and policy class, keeps missing instrumentation explicitly not-proven instead of zero, refuses noisy or unattributable evidence as blocking gates, and validates the committed vscode_client_perf_policy.v1 baseline record against those rules. ([#11909](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11909))

### Packaging and distribution

#### Added

- Added versioned managed-cache candidate and mutation-lease identities for cross-extension-host coordination. ([#7880](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7880))
- Added the internal managed-startup decision policy that separates local binary readiness from update-metadata availability. No user-visible startup behavior changes until a later slice consumes it. ([#7914](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7914))
- Added the checked, fail-closed workspace-host target decision used to distinguish exact release targets from unsupported architecture, Android/Termux, unresolved libc, and topology-drift states. Production downloader wiring follows in the closing ([#10001](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10001))
- Added the topology-derived VSIX target projection and closed candidate payload manifest, keeping GNU/musl, Windows ARM64, required server/DAP members, and binaryless universal packages distinct before package construction. ([#10003](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10003))

#### Changed

- Added immutable managed candidate selection, host binding, and fail-safe garbage-collection policy for shared caches. ([#11780](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11780))
- Binary identity status now offers working follow-up actions — managed-binary repair runs the governed reinstall flow, refresh re-reads identity, and inspect shows the resolved server path — and identity requests carry a version-independent packaged authority token plus the installed managed candidate and target when known. ([#17040](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17040))

#### Fixed

- Checking for a newer perl-lsp release now times out after 30 seconds instead of being able to hang indefinitely and block the binary download/update flow. ([#5018](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5018))
- The VS Code Marketplace listing's keywords were trimmed from 12 (exceeding the 5-tag limit, so the last 7 were silently dropped) to the 5 highest-intent terms; the README's command table and links were corrected to match actual command-palette labels; and the bug-report issue template now points at the real 'Perl: Show Output Channel' command instead of one that doesn't exist. ([#5158](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5158))
- Ordinary VSIX packaging now validates the exact archive it produced, so package inventory checks use shipped archive names and reject missing or corrupt artifacts. ([#15123](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15123))
- The manual Check for Binary Updates command now always performs a real check and reports the outcome, including a You-are-up-to-date confirmation, instead of being silently suppressed for up to a day by a recent background check; pinned tag-channel users are told why checks are disabled. ([#16573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16573))
- Cancelling a binary download now shows a short confirmation instead of a failure dialog; checksum failures offer a working Retry Download button; and a missing SHA256SUMS manifest is reported as missing release metadata rather than a corrupted download. ([#16573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16573))
- HTTP 403 download failures stop blaming the GitHub rate limit unconditionally and now name GitHub-or-network/proxy with a proxy hint, and network-failure guidance widens to DNS, unreachable-network, connection-reset, and TLS certificate errors with a concrete retry command. ([#16573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16573))
- Don't ask again on the update prompt now suppresses only the prompt: interval update checks and perl-lsp.autoUpdate keep working, instead of the old behavior that permanently rewrote updateCheckInterval to 0. ([#16573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16573))
- perl-lsp.versionTag pinning failures are now attributable. A tag that GitHub has no release record for reports "No release found for the configured tag "<tag>"; check the perl-lsp.versionTag setting." instead of "No releases found"; channel="tag" with an empty versionTag names the empty setting and its remedy instead of the "Invalid managed release policy" wrapper; and a versionTag set while channel is still latest emits a one-time warning that it is ignored unless channel="tag". ([#16533](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16533))
- Accepting a perllsp update — through the availability prompt's Update button or perl-lsp.autoUpdate — now confirms the staged install with "perllsp x.y.z downloaded. Reload the window to switch to it." and a Reload Window button, instead of finishing silently while the running server stays on the old binary. The update install's progress is labelled "Updating Perl Language Server" so it no longer looks identical to a first-run download. ([#16531](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16531))
- When perl-lsp.serverPath names a missing path, the extension shows a once-per-session warning that names the actual fallback source (downloaded, bundled, or PATH), or says when no fallback is available. The warning links to the serverPath setting. ([#16535](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16535))
- Managed binary downloads now honor the `http.proxy` setting and the standard proxy environment variables (including CONNECT tunnels for HTTPS release hosts), follow relative redirects instead of failing the archive download, keep proxy credentials out of every error and log, classify proxy/TLS failures as actionable network problems, and route the proxy hop itself around the extension host's patched HTTP modules so a proxy that resolves itself cannot loop the plain-HTTP path. ([#17041](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17041))

#### Security

- Pinned the brace-expansion npm dependency to patched versions (1.1.16 / 2.1.3 / 5.0.8) to remediate a known runtime security alert in the VS Code extension's dependency tree. ([#5263](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5263))
- **Breaking:** Managed binary archive downloads are bounded independently of timeout, and extraction inspects the archive then installs only the unique perllsp executable and optional perl-dap. Oversized, traversing, linked, or ambiguous archives are rejected and leave no partial candidate. ([#12808](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12808))
- Updated the development-time fast-uri override used through Ajv to the patched 3.1.6 release, removing the four known host-normalization advisories from the extension dependency lock. ([#15120](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15120))

### Compatibility

#### Added

- Added a versioned public-beta VS Code settings migration registry with deterministic security and scope invariants. ([#7878](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7878))

#### Changed

- Added runtime interpretation for compatible, action-required, and inert public-beta VS Code setting migrations without rewriting user state, naming why a legacy setting could not be interpreted so a registry defect is not reported as an unknown user setting. ([#7896](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7896))
- The public-beta settings migration registry now treats a row's historical era as a comparable window rather than opaque text: bounds accept a strict release or a whole minor series (0.17.x), two rows claiming any release in common for the same setting and scope are rejected as a registry defect, and several legitimate disjoint eras for one setting are resolved by the era covering the registry's own source public release instead of failing closed as ambiguous. A differing value shape no longer licenses two eras a reader cannot separate. No shipped setting changes behavior: today's single-row registry is validated and interpreted exactly as before (#15503). ([#15504](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15504))

#### Security

- Disabled the generic configured-command MCP passthrough so stale public-beta settings cannot register arbitrary local MCP processes. ([#7911](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7911))

### Developer experience

#### Added

- Added a typed structurally redacted support-packet model for public-beta issue reproduction without log scraping. ([#7879](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7879))

#### Changed

- npm test now runs Jest quietly by default; verbose per-test output is opt-in via `npm run test:ci` or the JEST_VERBOSE env var. ([#3904](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3904))
- Report Issue now copies a typed redacted public-beta support packet while keeping unavailable live state explicitly not proven. ([#7904](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7904))
- Report Issue now separates showing, copying, and filing — copying the support packet no longer opens the browser, the packet can be inspected in an editor tab first, and clipboard or packet failures stay bounded without blocking the issue form. ([#13815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13815))

#### Fixed

- A failing VSIX packaging step now says what failed instead of exiting 1 with no output. `packageVsix()` no longer returns a bare `false` that the caller could only turn into an exit code; each failing stage (vsce packaging, prebuilt payload member, prebuilt inventory SHA, and the inventory ratchet) throws a reason naming the stage and the artifact, so the existing CLI `catch` reports it on stderr. The inventory ratchet also writes its violations to stderr, naming the offending file with its baseline and actual sizes, and the missing-file case names the file. What the ratchet rejects is unchanged. ([#16570](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16570))

## [0.17.0] - 2026-06-28

### Added

- **DAP debug sessions use the same Perl interpreter as LSP analysis.** When a
  `launch.json` configuration omits an explicit `perl`, the debugger now
  resolves it from the same discovery path as the language server.
- **`perl.runScript` command** for running the current script via the LSP
  binary's execute-command provider.
- **`__CLASS__` token support** (Perl 5.40) in lexer, parser, and version
  compatibility lint.

### Changed

- **Native critic engine** is the default, replacing the external `perlcritic`
  binary for diagnostics. The legacy engine is still available via
  `[critic].engine = "legacy"` in `.perl-lsp.toml`.
- **Folding ranges** computed off-lock to reduce editor blocking.

### Fixed

- Document highlight now covers occurrences in comments, POD, and
  non-interpolated strings.
- Unclosed-block diagnostics anchor at the opening brace, not end-of-input.
- POD command parsing no longer misidentifies `=cutlery` or `=headache`.

Full release notes: [v0.17.0](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/releases/v0.17.0.md)

## [0.16.0] - 2026-06-06

### Changed

- **Workspace indexing** rewritten with generation-guarded freshness.
- **JSON-RPC type safety** hardening across the dispatch layer.

Full release notes: [v0.16.0](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/releases/v0.16.0.md)

## [0.15.2] - 2026-05-26

### Fixed

- Crates.io packaging hotfix for the `cargo install` path.

## [0.15.1] - 2026-05-26

### Fixed

- LSP4IJ inline completion and lean editor-mode hardening.
- Neovim interactive-latency improvements (generation-aware stale-read guard).

## [0.15.0] - 2026-05-22

### Changed

- **JSON-RPC type safety** — breaking change in the public protocol module
  (raw `Value` params replaced with typed structs).
- Fixed the LSP4IJ file-watcher registration crash.

## [0.14.0] - 2026-05-12

### Added

- **`perl.featureProfile` setting** (`auto`, `ga-lock`, `prod`, `all`) for
  gating experimental features by stability tier.

### Fixed

- `=over/=item` blocks before the first `=head` are no longer silently dropped.

## [0.13.0–0.13.4] - 2026-04-30 to 2026-05-07

### Added

- **Inline completion** (AI-powered, gated behind `ai-provider` feature).
- **`perl.debugTests` and `perl.debugTestFile`** commands for DAP test
  debugging.
- **Neovim** interactive-latency improvements.

### Fixed

- VS Code managed binary install reliability.
- `$foo->bar` in interpolated strings no longer classified as MethodCall.

## [0.12.4] - 2026-04-12

### Added

- **DAP debugger launch scorecard**: the debug adapter now tracks cold-launch
  success rate and P50/P95 latency across a suite of fixture programs; results
  surface in the new `status/dap.md` page. (#4237)
- **Inherited and role method navigation**: `Go to Definition`, hover, and
  workspace completion now traverse Moo/Moose `with 'Role'` and
  `extends`/`use parent` chains. AUTOLOAD-backed method calls also resolve.
  (#4077, #4091)
- **Phase-scoped pragma diagnostics** (`PL502`, `PL503`): flags `use strict` /
  `use warnings` placed inside phase blocks (`BEGIN`, `END`, etc.) where they
  have block scope rather than file scope, with quick-fixes that move them to
  file scope. (#4131)
- **`workspace/configuration` live reload**: server re-fetches per-folder
  configuration from the client on `workspace/didChangeConfiguration`, merging
  returned overlays over `.perl-lsp.toml` without a restart. (#4093)
- **`workspace/willDeleteFiles` warnings**: deleting a file that is referenced
  by other files in the workspace now surfaces a `Warning` diagnostic before
  the delete completes. (#4056)
- **Run Test at Cursor**: new command palette entry runs the nearest test
  subroutine or subtest under the cursor without navigating to a test lens.
  (#4025)
- **`yath` test runner preference**: the VS Code test runner uses `yath` when
  present on PATH, falling back to `prove` and `perl`. (#4031)

### Fixed

- **Rename operations are faster on large files**: the internal scope traversal
  was O(n×d) per rename; replaced with a single-pass BFS so renames on deeply
  nested files no longer stall. (#4240)
- **Windows: Run Tests / Run File no longer fail** with extended-length path
  errors — the `\\?\` prefix injected by `Path::canonicalize` is stripped
  before spawning `perl`, `prove`, or `yath`. (#4089)
- **`use if` / `use unless` conditional pragmas** no longer produce false
  missing-strict/warnings diagnostics. (#4050)
- **Eval- and sub-scoped pragmas** no longer suppress file-level PL100/PL101
  missing strict/warnings diagnostics. (#4052)
- **Non-`file://` workspace roots** (`vscode-remote://`, virtual schemes) are
  now tolerated — the server keeps them as LSP strings and skips non-filesystem
  folders during indexing without crashing. (#4059)

### Changed

- **Pre-push hook speed**: the hook now runs the fast Tier A (`pr-fast`) check
  instead of the full `ci-gate`, making pre-push validation ~3× faster for
  routine pushes. (#4088, #4110)

## [0.12.3] - 2026-04-09

The 0.12.3 release line. Aligned with the workspace `v0.12.3` cut and consolidates
the user-facing UX hardening work from April 2026.

### Fixed

- **Actionable error messages**: server startup errors now surface specific,
  actionable messages instead of a generic "corrupted" fallback
  (PR #3308, PR #3291).
- **Perl interpreter detection**: distinct error shown when the Perl interpreter
  is not found in PATH, with a clear install prompt (PR #3312).
- **Binary download errors**: download failures include the underlying OS error
  and a retry hint instead of a silent failure (PR #3306).
- **Settings schema polish**: extension settings entries cleaned up for the
  VS Code Settings UI — descriptions, defaults, and enum labels improved
  (PR #3269).
- **Enterprise/offline deployment**: documentation added for air-gapped binary
  distribution and internal mirror configuration (PR #3310).

## [0.12.2] - 2026-04-04

The 0.12.2 release line. Aligned with the workspace `v0.12.2` cut on 2026-04-04
and consolidates the v0.12.2 through v0.12.8 milestones into a single release.

### Added

- **AI inline completion**: opt-in OpenAI-compatible streaming provider with
  session management, version-aware cancellation, and a deterministic fallback
  when AI is disabled. Wired into the LSP server's `inlineCompletion` request
  via the new `experimental.perlInlineCompletionStream` extension.
- **Heredoc language injection**: SQL keyword and JSON key detection in
  heredocs with multi-heredoc-per-line support.
- **Type inference in hover**: type hints rendered alongside symbol info.
- **Dead code highlighting**: `DiagnosticTag::Unnecessary` for unreachable code.
- **Refactoring code actions**: extract variable, extract subroutine, inline
  subroutine, scoped rename.
- **POD preview panel**: `Perl: Preview POD` command opens a side panel with
  formatted POD output.
- **AST explorer debug panel**: `perl/showAst` custom LSP handler for
  inspecting parser output during diagnostics.

### Performance

- Incremental parsing pipeline (token caching + checkpoint recovery).
- Large-workspace HashMap optimization (faster startup).
- CPAN-scale benchmarks: 10K files indexed in 672ms.

### Fixed

- Inline completion duplicate backend types removed.
- Streaming completion controller gating on AI config flags.

### Changed

- **Version Bump**: 0.12.2 release aligned with the workspace and shipped
  `perllsp` asset line.

## [0.12.1] - 2026-03-30

### Fixed

- **Release Surface Recovery**: Restored the top-level source snapshot and release-facing docs after the launch regressions that slipped into the first `0.12.0` tag.
- **Hook Hygiene**: Hardened hook-test fixture isolation and worktree hook installation so placeholder identities do not leak into normal local commit flows.

### Changed

- **Version Bump**: 0.12.1 fix-forward release aligned with the workspace and shipped `perllsp` asset line.

## [0.12.0] - 2026-03-19

### Changed

- **Display Name**: Updated to "Perl Language Server (perl-lsp)" for clearer marketplace identification
- **Description**: Rewritten for marketplace SEO: highlights key features, speed, and zero-dependency install
- **Version Bump**: 0.12.0 public alpha release
- **Preview Flag**: Marked as preview for public alpha period
- **Keywords**: Added `debugger`, `refactoring`, `code-completion`, `diagnostics` for marketplace discovery
- **Categories**: Added "Testing" to reflect Test Explorer integration

### Added

- **Open VSX Publishing**: Extension now publishes to Open VSX Registry alongside VS Marketplace, enabling first-class support for VSCodium and other open-source VS Code derivatives. Added `ovsx` publish/check scripts and `@types/vscode` dev dependency fix for compatible builds.
- **Comprehensive README**: Rewritten for marketplace listing with full configuration reference table, keyboard shortcuts, troubleshooting guide, and command reference

## [0.11.0] - 2026-03-11

### Fixed

- **Corrupted Extension Icon**: Added binary rules for image files in `.gitattributes` to prevent line-ending normalization from corrupting PNGs; regenerated `icon.png` with correct PNG signature header.
- **TextMate Grammar Registration**: Registered `syntaxes/perl.tmLanguage.json` in `contributes.grammars` so VS Code actually loads the bundled grammar.
- **Stub Refactoring Commands**: Removed `Extract Subroutine`, `Extract Variable`, and `Inline Variable` from editor context and command palette menus (still available from the full command list with "in development" messaging).
- **VSCE Packaging**: Removed `--no-dependencies` flag so runtime dependencies (`vscode-languageclient`, `adm-zip`, `tar`) are properly bundled in the `.vsix`.
- **pnpm Compatibility**: Made `vsce package`/`publish` resilient in pnpm environments.

### Changed

- **Parser Bug Fixes** (6 fixes in the underlying `perl-lsp` server):
  - Handle postfix modifiers after dereference expressions (`$obj->method if $cond`)
  - Re-lex slash as regex delimiter after `split` keyword
  - Accept fat arrow as argument separator in function calls
  - Handle subscripts on package-qualified variables (`$Foo::bar[0]`)
  - Support `&{expr}` code dereference syntax
  - Expand builtin list and support forward declarations
- **Lexer Bug Fixes** (2 fixes):
  - Skip POD blocks in whitespace/comment handler
  - Make POD scanning byte-safe for UTF-8 source files
- **Regex Engine Fixes** (2 fixes):
  - Eliminate false positive nested quantifier detection
  - Clean up false positive regex detection
- **LSP Feature Polish**: Maintained 100% user-visible feature coverage and protocol compliance.
- **Documentation**: Comprehensive release readiness updates including README, roadmap, and launch article series.

### Added

- Marketplace readiness workflow via `npm run verify:marketplace` for compile + bundle + package validation.
- VS Marketplace badges and installation link in extension README.
- Publishing guide refreshed with launch checklist and pre-release recommendation.
- SRP microcrate extraction campaign: 30+ new single-responsibility microcrates extracted.
- Comprehensive unit test campaign: 80+ new test modules across all crate tiers.

## [0.10.0] - 2026-02-28

### Changed

- **Version Sync**: Extension version aligned with workspace v0.10.0.
- **LSP Coverage**: Maintained 100% user-visible feature coverage (53/53).
- **Protocol Compliance**: Maintained 100% protocol compliance (97/97).

## [0.9.0] - 2026-01-18

### Added

- 🔧 **Advanced Refactoring Support**
  - Extract method refactoring with parameter detection
  - Inline variable/expression refactoring
  - Move code refactoring for relocating code blocks
  - Transactional safety with rollback infrastructure
- 🎯 **Semantic Definition Integration**
  - Precise go-to-definition using semantic analysis instead of text search
  - Multi-symbol support: scalars, arrays, hashes, subroutines, packages
  - Lexical scoping with proper handling of nested scopes and shadowed variables
- 🔒 **Security Hardening**
  - Complete path traversal protection for execute commands
  - Command injection hardening in executeCommand
- ⚡ **Performance Optimizations**
  - O(1) symbol lookups (from linear time)
  - Stack-based scope analysis for improved performance
  - Reduced string allocations in parser
- 🎨 **Product Icons**: Added icons to extension commands
- 📋 **Context Menu**: Run Tests exposed in editor context menu

### Changed

- Cross-file Package->method resolution improved
- Better error logging for incremental document changes
- Configuration setting descriptions improved

## [0.8.0] - 2025-09-01

### Added

- **Cross-File Navigation**: Workspace indexing with dual storage pattern for qualified and bare names
- **Import Optimization**: Detect and organize imports, remove unused imports
- **Incremental Parsing V2**: Advanced edit tracking with node reuse for faster re-parsing
- **File Path Completion**: Enterprise-grade file completion with security safeguards

### Changed

- Optimized workspace indexing for large codebases
- Enhanced comment documentation extraction for hover

## [0.7.0] - 2025-08-24

### Added

- **LSP 3.17 Features**: Inlay hints, document links, selection ranges, on-type formatting
- **Code Actions**: Robust refactoring and quick fixes
- **Type Hierarchy**: View inheritance relationships
- **Rename Support**: Symbol renaming with validation

## [0.6.0] - 2025-01-29

### Added

- 🔍 **Call Hierarchy Support**
  - View incoming calls (functions that call the selected function)
  - View outgoing calls (functions called by the selected function)
  - Navigate complex call chains with ease
  - Right-click any function and select "Show Call Hierarchy"
- 💡 **Inlay Hints**
  - Parameter name hints for function calls
  - Type hints for variable declarations
  - Smart filtering to reduce visual clutter
  - Fully configurable via settings
- 🧪 **Test Explorer Integration**
  - Automatic discovery of test files (.t) and test functions
  - Visual test hierarchy in Testing panel
  - Run individual tests or entire test files
  - Real-time test results with pass/fail indicators
  - TAP (Test Anything Protocol) support
- 🐛 **Debug Adapter Protocol Support**
  - Full step-through debugging for Perl scripts
  - Breakpoints with conditional support
  - Variable inspection and watch expressions
  - Call stack navigation
  - Test debugging integration
  - Debug configurations for scripts and tests
- ⚡ **Performance Optimizations**
  - AST caching for faster parsing (100 files, 5-min TTL)
  - Symbol index for instant workspace searches
  - 10x faster symbol lookup in large projects

### Enhanced

- Added "Testing" category to extension capabilities
- Improved activation events for test files
- Better TypeScript types and error handling

### Fixed

- Improved handling of anonymous subroutines in navigation features
- Better error recovery for malformed syntax
- Fixed race conditions in document synchronization

## [0.5.0] - 2025-01-01

### Added

- Initial release of Perl Language Server for Visual Studio Code
- Full Language Server Protocol support with 8 core features:
  - Real-time syntax diagnostics
  - Code completion with context awareness
  - Go to definition
  - Find all references
  - Document symbols (outline)
  - Signature help
  - Hover information
  - Code actions (quick fixes)
- Code formatting with Perl::Tidy integration
  - Format document (Shift+Alt+F)
  - Format selection
  - Automatic .perltidyrc discovery
- Enhanced syntax highlighting
- Commands:
  - Restart Language Server
  - Show Language Server Output
- Bundled perl-lsp binary for easy installation
- Support for modern Perl features (try/catch, signatures, class/method)
