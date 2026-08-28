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

### C2 — Proposed LSP4IJ Windows wire semantics

The LSP4IJ projection must use an explicit command vector, not a bare command
resolved through `PATH`. This prohibition includes the shell: the `cmd.exe`
image must be resolved before launch through the trusted Windows system
directory (the canonical `%SystemRoot%\System32\cmd.exe`) or an equivalent
trusted OS resolver, then canonicalized and verified as the expected system
image. The resolver must fail closed if `%SystemRoot%` is missing, malformed,
reparse-redirected, or does not yield the expected system `cmd.exe`; it must not
fall back to the current directory or `PATH`. The following is the proposed
wire-level semantics for the future projection; it is not a claim about the
canonical LSP4IJ schema or serialization. Before implementation, the lane must
pin the current LSP4IJ schema authority (release or commit, exact path, and
digest) and establish whether the relevant field is string-valued or an
argument array. A current schema/serialization oracle must then confirm that
the representation preserves this vector and its quoting semantics.

After LSP4IJ substitutes `${BASE_DIR}` with the absolute installation directory,
the proposed effective vector is:

```text
<SYSTEM_ROOT>\System32\cmd.exe
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
<SYSTEM_ROOT>\System32\cmd.exe /d /s /c ""C:\Program Files\perl-lsp\perllsp.cmd" --stdio"
```

The LSP template must preserve the `--stdio` argument and must not replace the
absolute `${BASE_DIR}` reference with `perllsp`, `perllsp.cmd`, or
`perllsp.exe`.

Before constructing the `/c` command string, the implementation must validate
the substituted base directory and selector path as path data, not as an
already-safe command fragment. Validation must reject empty, relative, rooted-
outside, malformed, or otherwise invalid substitutions and must establish that
the selector path is contained by the intended install root. The implementation
must canonicalize the resolved selector/executable path, resolving symlinks,
junctions, and other Windows reparse points, and then re-check containment
against the canonical install root before launch. A path that resolves outside
that root, or whose canonicalization cannot be completed, must fail closed.
These checks apply before quoting or shell encoding and must be identical for
the LSP and DAP selectors.

Because `cmd.exe` expands `%VAR%` during `/c` processing and `!VAR!` under
delayed expansion, the validation and encoding boundary must either reject
selector paths containing percent signs, exclamation marks, carets, or other
shell metacharacters, or prove with the pinned schema/serialization oracle that
the chosen encoding preserves them verbatim. Silent expansion or corruption of
such a path is a fail-closed condition.

If the LSP4IJ field is string-valued, its unescaped value must render as:

```text
<SYSTEM_ROOT>\System32\cmd.exe /d /s /c ""${BASE_DIR}\perllsp.cmd" --stdio"
```

Until the pinned schema and string-vs-argv oracle have confirmed this mapping,
the LSP4IJ projection is `NOT_PROVEN`; these examples are design input, not
permission to edit a template.

### C3 — Proposed LSP4IJ DAP wire semantics

The DAP projection uses the same trusted shell resolution and selector
boundary, with no implicit PATH lookup or independent executable. `cmd.exe` in
the vector below means the canonical system image established by C2, never a
PATH-resolved token:

This is proposed wire-level semantics, not an assertion of the canonical LSP4IJ
schema or serialization. The implementation lane must use the same pinned schema
authority and string-vs-argv oracle required by C2, and must review the LSP and
DAP representations together.

```text
<SYSTEM_ROOT>\System32\cmd.exe
/d
/s
/c
"<BASE_DIR>\perl-dap.cmd"
```

After the LSP4IJ DAP placeholder `<<insert base directory>>` is substituted,
the resulting command-line shape is:

```text
<SYSTEM_ROOT>\System32\cmd.exe /d /s /c ""C:\Program Files\perl-lsp\perl-dap.cmd""
```

The exact LSP4IJ field encoding may be a command string or a structured argument
projection; the implementation must preserve the vector and quoting semantics
above only after the current schema oracle confirms the encoding. The
corresponding LSP and DAP template values must be reviewed together.
If the DAP field is string-valued, its unescaped value must render as:

```text
<SYSTEM_ROOT>\System32\cmd.exe /d /s /c ""<<insert base directory>>\perl-dap.cmd""
```

Until that schema and serialization confirmation exists, the DAP projection is
also `NOT_PROVEN`.

The implementation must not treat rendering the vector as launch proof. Its
focused oracle must execute the substituted command in a controlled promoted
installation and assert successful process completion/handshake for the
expected selector and candidate identity. The oracle must capture stderr with a
bounded size and duration, attribute any diagnostics to the launched process,
and fail on non-zero exit, timeout, unexpected stderr, or identity mismatch.
It must include negative controls for a selector/executable reached through a
reparse point outside the install root and for an invalid substituted path.
The same oracle shape is required for LSP and DAP, with protocol-specific
success criteria recorded separately. A rendered command string, an empty
stderr stream, or a process-started event alone is insufficient evidence.

### C4 — Mirror, patch, and provenance authority

The released upstream fixture and repository-owned desired projection must never
be conflated. A future implementation PR must:

1. keep the pinned upstream repository, release/tag, resolved commit, and source
   blob identities in the existing fixture manifest;
2. apply the corrected template through a mirror or patch mechanism only after
   that mechanism is established and its owner is identified, without silently
   editing the released-evidence namespace;
3. record the source path, upstream identity, patch/delta identity, and resulting
   file digest for every changed LSP and DAP template or mirrored installer
   catalog; and
4. make refresh/check and repeated delta preparation deterministic, failing
   closed when the upstream subject or checksum does not match.

The mirror/patch mechanism is currently `NOT_PROVEN` for this claim. Future
ownership belongs to [#7772](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7772),
which must provide or name the producer that takes a pinned upstream subject and
emits a deterministic delta, and the checker that verifies source identity,
patch applicability, resulting digests, and repeatability. [#7706](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7706)
owns the separate released-template and process-evidence proof surface. The
implementation lane must not infer an existing producer or checker from the
maintenance runbook. The runbook and
`integrations/lsp4ij/upstream/0.20.1/manifest.json` remain starting evidence
only. This spec does not edit generated policy or invent a second checksum
ledger.

### C5 — Promoted-install proof

A future implementation is not complete from template inspection alone. Its
proof must use a real promoted Windows install containing distinguishable
candidate A and candidate B members and must establish all of the following for
both LSP and DAP:

- the template expands its base-directory placeholder to the intended absolute
  install root;
- the substituted base directory and selector path are validated before command
  construction, and canonicalization/reparse resolution proves that the final
  selector and executable remain contained by the intended install root;
- the shell image is resolved from the trusted Windows system location (or an
  equivalent trusted OS resolver), canonicalized, and proven not to come from
  the current directory or `PATH`;
- the launched process reaches the install-root `.cmd` selector and then the
  selected candidate member;
- a stale executable with a different identity in `PATH` is not selected;
- a stale executable or selector left outside `current` is not selected;
- changing `current` changes the selected candidate as specified, without
  changing the template or creating a root `.exe` copy; and
- missing, invalid, or checksum-failing candidate material fails closed.

The launch oracle must assert successful completion or protocol handshake and
record bounded, attributable stderr for each LSP and DAP execution. It must
fail closed on timeout, non-zero exit, unexpected diagnostics, path
canonicalization failure, reparse escape, or candidate-identity mismatch.

The proof must record the promoted candidate identity, the template subject and
digest, the effective command vector, and the observed process/binary identity.
The launch receipt must be a versioned contract: it names a receipt-schema
version and carries the identities above in that version's required fields, so
consumers can detect and reject receipts produced under an older or
incompatible contract.
An LSP4IJ/JetBrains launch receipt is required before making a released-client
support claim. A local imported template can prove the proposed projection, but
cannot prove what an unmodified released LSP4IJ build ships.

### C6 — Atomic promotion and selection rollback invariants

This compatibility change preserves the installer’s existing safety boundary:

1. candidate executable bytes are verified before they become eligible;
2. `current` is replaced atomically, so readers observe the previous complete
   unit or the new complete unit;
3. selectors are prepared before the current selection changes and do not expose
   a mixed pair or a missing selector during promotion;
4. the previous complete candidate remains available for rollback; and
5. a failed checksum, incomplete pair, or failed promotion does not advance
   `current`.

Rollback for a promotion implementation failure is to leave the previous
candidate selected, restore the last known-good repository-owned template
projection, and revert the implementation PR if necessary. Health-driven startup
validation or rollback is deliberately outside this contract: #13289 is the
explicit prerequisite and boundary for that durability behavior. Until #13289
lands and is independently verified, this spec makes no claim that a failed
startup changes or rolls back `current`. This spec alone authorizes no installer
migration, binary deletion, upstream submission, or release action.

## Acceptance

A future implementation PR satisfies this spec only when it:

- uses the immutable-candidate/install-root-selector topology in C1;
- renders both C2 and C3 command vectors with the stated placeholder and quoting
  semantics;
- updates mirror/patch/checksum provenance through the producer/checker owned by
  the future #7772 boundary, once that authority is established;
- supplies promoted-install evidence for both LSP and DAP, including ambient-PATH
  and stale-binary negative controls; and
- preserves C6 atomic promotion, checksum, and selection-rollback behavior; any
  health-driven startup recovery remains a separately proven #13289 concern.

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
Those implementation proofs must include the pre-command substituted-path
validation, canonical/reparse containment negative controls, and bounded
successful-launch/stderr oracle described in C2, C3, and C5.

## Unresolved currentness and authority questions

These questions are intentionally preserved for the implementation lane:

- A live GitHub refresh on 2026-08-28 reports PR #12815 open at head
  `045e34c4c88372cd2d53cb0702acbbf82e6f576d`, based on
  `46a3db8dadbd23493a3ca00ec3053bb64521b819`. The same refresh reports
  `origin/main` through GitHub, and PR #13291’s base, at
  `0fac6d848048113b4d3e3886e874fd32ff8dd8ee`; PR #13291’s current head is
  `1c59479b6945f080b4a539af6d9a6634a2a7a76d` (historical review-time head).
  This is a timestamped refresh-time snapshot, not durable authority: every implementation or proof
  lane must query GitHub again and use the then-current head/base and
  protection state.
- PR #12815 remains open; its proposed `.cmd` topology is not current-main
  behavior until it lands and is revalidated.
- Issue #13289 remains open and may change durability/flush/startup recovery
  details; this spec consumes those details only after the implementation lane
  verifies the landed or current authority.
- The authoritative LSP4IJ schema/encoding for command strings versus argument
  arrays must be confirmed from a pinned current artifact before changing
  templates. The mirror/patch acceptance path is `NOT_PROVEN` pending the
  producer/checker boundary named by #7772; #7706 remains the separate evidence
  subject. Issue states and this snapshot do not substitute for those artifacts.
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
