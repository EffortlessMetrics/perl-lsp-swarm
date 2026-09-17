# LSP4IJ host journey — declared-host launch and real-session receipt contract

This directory is the IntelliJ/LSP4IJ first real-host evidence tier, following the
Sublime Text tier precedent (`clients/sublime/` receipt schemas + validators +
real-host lanes) adapted to a declared IDE host. Controlling issue: #12826.
Exact actual-host composer it feeds: #7719 (heavier Starter/Driver substrate
stays owned by #8539/#8644).

## Honest boundary: what exists today

| Artifact | Status |
| --- | --- |
| `lsp4ij-launch-spec.v1.schema.json` + `validate_lsp4ij_launch_spec.py` | checked in, tested |
| `lsp4ij-host-receipt.v1.schema.json` + `validate_lsp4ij_host_receipt.py` | checked in, tested |
| discriminating validator tests (`tests/`) | checked in, green locally |
| repro fixture project (`host-fixture/`) | checked in |
| hosted execution of the recipe below | **UNOBSERVED — no hosted session has ever run.** No workflow is wired; there is no verified in-repo provisioning precedent for JetBrains tooling (the upstream LSP4IJ integration workflow is disabled), so per the tier rule this stays **declared-ready-pending-CI** instead of inventing unverifiable CI. |

No checked-in file in this directory is a session receipt. Receipts are only ever
produced by an actual hosted run and published as run artifacts.

## Laws

1. **Declared vs observed.** A launch spec is configuration accepted before a
   run (`stage: "declared_host"`). A receipt is evidence produced after one
   observed session (`stage: "exact_source_local"`). The two never mix.
2. **Synthetic is forbidden for production closure.** Every observation in a
   receipt carries `origin: "live_wire_capture"` plus the SHA-256 digest of the
   captured wire bytes. The validator rejects any other origin unconditionally;
   unit-test calibration objects constructed to exercise validators offline are
   test-scoped forever and must never be written out as receipts.
3. **Maintained line.** Observed or declared LSP4IJ subjects below 0.20.0 are
   rejected (docs/EDITORS/INTELLIJ_IDEA_SETUP.md).
4. **stdio only.** `perllsp --stdio`, exactly two command tokens.
5. **Bound to its precondition.** The launch-spec digest is defined as SHA-256
   over `json.dumps(spec, sort_keys=True, separators=(",", ":"))` UTF-8 bytes.
   Receipt validation requires the matching spec file as a second input,
   recomputes that digest, and rejects any subject drift between declaration
   and observation (source SHA, IDE, plugin, binary path/command/digest). A
   receipt alone never closes a claim.
6. **Hermetic.** The four sandbox roots are pairwise distinct, run-owned, and
   replace config/system/plugins/log state; ambient user profiles, ambient
   plugin directories, and ambient settings are bypassed entirely.

## Run recipe (host enablement pending)

Performed by the future host driver on one lane machine/runner with IntelliJ CE
installed at `$PERLLSP_INTELLIJ_CE_HOME`; every step is checkable offline once
the artifacts exist.

1. **Declare.** Copy `declared-host.launch-spec.example.json`; substitute the
   placeholders (`PERLLSP_INTELLIJ_CE_HOME`, `CARGO_TARGET_DIR`,
   `PERLLSP_LSP4IJ_SANDBOX`), set `source_sha` to the current candidate commit,
   record the exact IDE build number, and fill `server_binary.sha256`.
2. **Validate the spec.**
   `python integrations/lsp4ij/host-journey/validate_lsp4ij_launch_spec.py SPEC.json`
3. **Sandbox-launch the IDE** with redirected state roots
   (Windows launcher names shown; mac/linux use their scripts):

   ```text
   idea64.exe ^
     -Didea.config.path=<sandbox.config_root> ^
     -Didea.system.path=<sandbox.system_root> ^
     -Didea.plugins.path=<sandbox.plugins_root> ^
     -Didea.log.path=<sandbox.log_root> ^
     <workspace root containing host-fixture>
   ```

4. **Provision the pinned LSP4IJ subject into `<sandbox.plugins_root>`** from
   the release archive matching the spec's `pinned_commit`
   (0.20.1 = `1f62a3f8d8718db00b3db9189772f3a9172e4fb3` for the vendored
   reference). Never reuse an existing profile's plugin directory.
5. **Import the corrected template** from `integrations/lsp4ij/perl-lsp/`
   (repository-owned stage, not the released built-in template) and point the
   server executable at the exact built binary from step 1.
6. **Capture.** Record per observation: origin `live_wire_capture`, the SHA-256
   over each captured message body (initialize request/response, diagnostics
   settle evidence, one captured response per provider tap), pid ledger entries
   for every spawned `perllsp`, orderly shutdown confirmation.
7. **Assemble + validate the receipt against its spec**, then **publish both
   the spec and the receipt as run artifacts** and attach the validator verdict
   to the controlling issue:

   ```bash
   python integrations/lsp4ij/host-journey/validate_lsp4ij_host_receipt.py RECEIPT.json LAUNCH_SPEC.json
   ```

## What a first green run closes

One admissible receipt proves exactly: the declared subject launched hermetically,
reached initialize with live capability presence, settled diagnostics on the
fixture project, observed the core provider surface (completion/hover/diagnostic)
across file families including `.pl`, and shut down `perllsp` cleanly under a
process ledger. It does **not** prove other JetBrains products, other LSP4IJ
versions, managed-install routes, DAP behavior (#7877/#10431 own those), or any
support-registry claim (registry adapters own promotion).
