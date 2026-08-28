# PLSP-SPEC-0036: Windows LSP4IJ entry-point compatibility

Status: proposed
Owner: perl-lsp maintainers
Linked issue: [#13290](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13290)
Related implementation: [#12815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12815)
Related durability follow-up: [#13289](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13289)
Linked maintenance contract: [LSP4IJ Integration Maintenance](../development/LSP4IJ_MAINTENANCE.md)
Linked upstream evidence: [LSP4IJ 0.20.1 fixture](../../integrations/lsp4ij/upstream/0.20.1/README.md)
Status impact: Windows installer selectors, LSP4IJ template compatibility, upstream-mirror provenance, and promoted-install support evidence

## Purpose and current boundary

The Windows standalone installer is moving toward immutable product candidates
with install-root command selectors. The repository-owned LSP4IJ templates must
name the same product surface. This spec defines the compatibility contract for
that future integration change; it does not change the installer or vendored
templates and does not claim that a Windows launch has been completed.

The proposed decision is to invoke the existing install-root `.cmd` selectors
through `cmd.exe`, rather than restore independent install-root `.exe` copies.
The latter would recreate a second mutable product surface outside the candidate
store.

The released LSP4IJ 0.20.1 fixture remains evidence of what was released, not
desired state. Its LSP and DAP templates currently name `perllsp.exe` and
`perl-dap.exe` directly. A corrected repository-owned projection and a future
upstream release are separate evidence subjects.

## Contract

### C1 — One Windows product surface

The promoted product unit has immutable executable members in the candidate
store, for example:

```text
<INSTALL_DIR>/.perl-lsp/candidates/<content-id>/perllsp.exe
<INSTALL_DIR>/.perl-lsp/candidates/<content-id>/perl-dap.exe
```

The install root exposes only pointer-following selectors:

```text
<INSTALL_DIR>/perllsp.cmd
<INSTALL_DIR>/perl-dap.cmd
```

The selectors resolve the install-root `current` selection and dispatch to the
selected candidate member. No implementation covered by this spec may restore
independent `<INSTALL_DIR>/perllsp.exe` or `<INSTALL_DIR>/perl-dap.exe` copies,
hard links, junctions, or mutable mirrors. A source-only unit may omit the DAP
selector only when the installer’s existing product-unit contract says that the
unit is source-only; an LSP+DAP unit must expose both selectors.

### C2 — Canonical LSP4IJ Windows invocation

The LSP4IJ projection must use an explicit command vector, not a bare command
resolved through `PATH`. After LSP4IJ substitutes `${BASE_DIR}` with the
absolute installation directory, the canonical vector is:

```text
cmd.exe
/d
/s
/c
"<BASE_DIR>\perllsp.cmd" --stdio
```

The final `/c` argument is one command string. Its inner quotes protect the
selector path, including spaces; the outer quotes required by the Windows
command-line encoding must be preserved when the vector is rendered as one
process command line. The resulting command-line shape is:

```text
cmd.exe /d /s /c ""C:\Program Files\perl-lsp\perllsp.cmd" --stdio"
```

The LSP template must preserve the `--stdio` argument and must not replace the
absolute `${BASE_DIR}` reference with `perllsp`, `perllsp.cmd`, or
`perllsp.exe`.

If the LSP4IJ field is string-valued, its unescaped value must render as:

```text
cmd.exe /d /s /c ""${BASE_DIR}\perllsp.cmd" --stdio"
```

### C3 — Canonical DAP Windows invocation

The DAP projection uses the same selector and shell boundary, with no implicit
PATH lookup or independent executable:

```text
cmd.exe
/d
/s
/c
"<BASE_DIR>\perl-dap.cmd"
```

After the LSP4IJ DAP placeholder `<<insert base directory>>` is substituted,
the resulting command-line shape is:

```text
cmd.exe /d /s /c ""C:\Program Files\perl-lsp\perl-dap.cmd""
```

The exact LSP4IJ field encoding may be a command string or a structured argument
projection; the implementation must preserve the vector and quoting semantics
above. The corresponding LSP and DAP template values must be reviewed together.
If the DAP field is string-valued, its unescaped value must render as:

```text
cmd.exe /d /s /c ""<<insert base directory>>\perl-dap.cmd""
```

### C4 — Mirror, patch, and provenance authority

The released upstream fixture and repository-owned desired projection must never
be conflated. A future implementation PR must:

1. keep the pinned upstream repository, release/tag, resolved commit, and source
   blob identities in the existing fixture manifest;
2. apply the corrected template through the repository’s established mirror or
   patch mechanism, without silently editing the released-evidence namespace;
3. record the source path, upstream identity, patch/delta identity, and resulting
   file digest for every changed LSP and DAP template or mirrored installer
   catalog; and
4. make refresh/check and repeated delta preparation deterministic, failing
   closed when the upstream subject or checksum does not match.

The existing LSP4IJ maintenance runbook and `integrations/lsp4ij/upstream/0.20.1/manifest.json`
remain the starting authorities. This spec does not edit generated policy or
invent a second checksum ledger.

### C5 — Promoted-install proof

A future implementation is not complete from template inspection alone. Its
proof must use a real promoted Windows install containing distinguishable
candidate A and candidate B members and must establish all of the following for
both LSP and DAP:

- the template expands its base-directory placeholder to the intended absolute
  install root;
- the launched process reaches the install-root `.cmd` selector and then the
  selected candidate member;
- a stale executable with a different identity in `PATH` is not selected;
- a stale executable or selector left outside `current` is not selected;
- changing `current` changes the selected candidate as specified, without
  changing the template or creating a root `.exe` copy; and
- missing, invalid, or checksum-failing candidate material fails closed.

The proof must record the promoted candidate identity, the template subject and
digest, the effective command vector, and the observed process/binary identity.
An LSP4IJ/JetBrains launch receipt is required before making a released-client
support claim. A local imported template can prove the proposed projection, but
cannot prove what an unmodified released LSP4IJ build ships.

### C6 — Atomic promotion and rollback invariants

This compatibility change preserves the installer’s existing safety boundary:

1. candidate executable bytes are verified before they become eligible;
2. `current` is replaced atomically, so readers observe the previous complete
   unit or the new complete unit;
3. selectors are prepared before the current selection changes and do not expose
   a mixed pair or a missing selector during promotion;
4. the previous complete candidate remains available for rollback; and
5. a failed checksum, incomplete pair, failed promotion, or failed startup does
   not advance `current`.

Rollback for an implementation failure is to leave the previous candidate
   selected, restore the last known-good repository-owned template projection,
   and revert the implementation PR if necessary. This spec alone authorizes no
   installer migration, binary deletion, upstream submission, or release action.

## Acceptance

A future implementation PR satisfies this spec only when it:

- uses the immutable-candidate/install-root-selector topology in C1;
- renders both C2 and C3 command vectors with the stated placeholder and quoting
  semantics;
- updates mirror/patch/checksum provenance through the existing LSP4IJ authority;
- supplies promoted-install evidence for both LSP and DAP, including ambient-PATH
  and stale-binary negative controls; and
- preserves C6 atomic promotion, checksum, and rollback behavior.

This docs-only PR satisfies only the narrower claim that these obligations and
boundaries are recorded. It does not satisfy the implementation acceptance above.

## Proof Commands

For this docs-only change, the cheapest applicable proof is:

```powershell
cargo xtask ci-hygiene check-doc-paths docs/specs
git diff --check
```

No Cargo build, installer harness, vendored-template edit, or Windows launch
probe is part of this spec PR. Implementation PRs must add the existing focused
PowerShell promotion/checksum checks and the existing LSP4IJ template/installer
contract checks, followed by a real promoted-install LSP and DAP launch receipt.

## Unresolved currentness and authority questions

These questions are intentionally preserved for the implementation lane:

- At preparation time, PR #12815 reports live base `46a3db8…` while this checkout
  reports `origin/main` at `7fbcc04…`; the implementation lane must rederive the
  live base and relevant protection before editing or proving anything.
- PR #12815 is still open at head `045e34c…`; its proposed `.cmd` topology is
  not current-main behavior until it lands and is revalidated.
- Issue #13289 is open and may change durability/flush/startup recovery details;
  this spec consumes those details only after the implementation lane verifies
  the landed or current authority.
- The authoritative LSP4IJ schema/encoding for command strings versus argument
  arrays, and the upstream mirror/patch acceptance path, must be confirmed from
  the current #7706/#7772-related artifacts before changing templates.
- The pinned 0.20.1 fixture proves released file content only. It does not prove
  actual IntelliJ behavior, managed installation, or a released corrected
  template.

An unresolved answer is `NOT_PROVEN`, not permission to infer compatibility.

## Non-goals

- PATH persistence or PATH environment setup;
- managed VS Code caches or their garbage collection;
- the durability implementation in #13289;
- broad LSP4IJ or JetBrains product support;
- installer code, vendored upstream templates, generated policy, or release
  publishing from this spec alone; and
- claiming a Windows promotion, LSP launch, DAP launch, upstream merge, or
  released-client support receipt.

## Claim Boundaries

This proposed spec establishes a reviewable design and proof boundary for the
Windows LSP4IJ entry-point compatibility gap. It does not establish that the
selectors exist on `main`, that either template has been changed, that a
promoted installation launches, that ambient PATH is excluded in practice, or
that upstream or a released LSP4IJ client accepts the projection. Those claims
remain `NOT_PROVEN` until the named implementation and host evidence exists.
