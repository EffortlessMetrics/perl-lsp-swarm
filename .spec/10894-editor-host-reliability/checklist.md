# Implementation Checklist: #11766 — shared editor-host reliability and adoption contract

## Change order

This is a documentation/specification-only change. Each step is reviewable without
building or executing a host/editor process.

### Step 1: Create the context contract

- **File:** `.spec/10894-editor-host-reliability/context.md`
- **Change:** Record the problem, #10894 versus consumer ownership, platform-neutral
  identity/freshness/deadline/process/cleanup/artifact/four-plane laws, adoption,
  rollback/transfer/stop boundaries, links, and claim limits.
- **Verify:** `Select-String` checks each required authority and law; `git diff --check`.

### Step 2: Create acceptance and negative controls

- **File:** `.spec/10894-editor-host-reliability/acceptance.md`
- **Change:** Include all canonical `SPEC_TEMPLATE.md` sections, all twelve issue
  falsifiers, representative consumers, adoption dispositions, and explicit
  non-goals.
- **Verify:** Structural heading and falsifier-count checks below; `git diff --check`.

### Step 3: Create the builder and proof contract

- **File:** `.spec/10894-editor-host-reliability/checklist.md`
- **Change:** Define the bounded implementation order, deterministic structural
  checking, second-run proof, rollback/transfer/stop conditions, and handoff.
- **Verify:** Read-only checker runs twice with identical output and no tree diff.

## Deterministic structural proof

The repository has no executable `.spec` graph validator. Do not invent a generated
receipt or claim a missing tool passed. From the candidate worktree, run the following
PowerShell check twice after the files are complete. The command checks the exact
three files, required canonical headings, required terms, and twelve numbered
falsifiers in fixed order. Redirecting the output to a temporary file is local proof
only; no temporary file belongs in the PR.

```powershell
$root = '.spec/10894-editor-host-reliability'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  'HostRunSubject', 'FreshReceiptTarget', 'parent-owned deadline',
  'process domain', 'complete process ledger', 'independent cleanup',
  'bounded artifact', 'product, instrument, reporting, and cleanup',
  'not_proven', '#7777', '#10527', 'Emacs', 'LSP4IJ', 'Coc', 'Lite XL',
  'Vim', 'rollback', 'transfer', 'legacy', 'Stop', 'deterministic'
)
$headings = @('§Behavior', '§Hazards', '§Contracts', '§API-Shape', '§Test-Grid', '§Blast-Radius')
$falsifiers = 1..12 | ForEach-Object { "| $_ |" }
$text = $paths | ForEach-Object { Get-Content -Raw $_ }
if ($text.Count -ne 3) { throw 'expected exactly three spec files' }
foreach ($term in $required) { if (-not ($text -match [regex]::Escape($term))) { throw "missing term: $term" } }
foreach ($heading in $headings) { if (-not ($text -match [regex]::Escape($heading))) { throw "missing heading: $heading" } }
foreach ($marker in $falsifiers) { if (-not ($text -match [regex]::Escape($marker))) { throw "missing falsifier: $marker" } }
$base = git merge-base HEAD origin/main
$changed = @(
  git diff --name-only $base HEAD -- .spec/10894-editor-host-reliability
  git diff --name-only HEAD -- .spec/10894-editor-host-reliability
  git diff --cached --name-only HEAD -- .spec/10894-editor-host-reliability
) | Sort-Object -Unique
if ($changed.Count -ne 3) { throw 'unexpected changed paths' }
'SPEC_10894_STRUCTURAL_CHECK=PASS'
```

Run it twice, capture stdout separately, and compare the two outputs byte-for-byte:

```powershell
$tmp = Join-Path $env:TEMP 'spec-10894-check'
Remove-Item -LiteralPath "$tmp.1","$tmp.2" -Force -ErrorAction SilentlyContinue
<# save the exact command output above to $tmp.1 #>
<# repeat the exact command without edits and save output to $tmp.2 #>
$h1 = (Get-FileHash -Algorithm SHA256 -LiteralPath "$tmp.1").Hash
$h2 = (Get-FileHash -Algorithm SHA256 -LiteralPath "$tmp.2").Hash
if ($h1 -ne $h2) { throw 'second run is not deterministic' }
git diff --check
if (git status --short -- .spec/10894-editor-host-reliability | Select-String '^...\.spec/10894-editor-host-reliability/(?!context|acceptance|checklist)') { throw 'unexpected spec artifact' }
```

The comments above intentionally require the operator to capture the exact
read-only command rather than hiding an invented script. A future repository-owned
checker may replace this proof only through a separate tooling claim.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are changed.
- [ ] #10894 shared authority and consumer ownership are explicit and non-overlapping.
- [ ] Identity/freshness, parent deadline, process-domain/ledger, independent
      cleanup, artifact integrity, and four-plane laws are platform-neutral.
- [ ] Missing capability/instrumentation is `not_proven`; no status-0/client-event
      cleanup inference is allowed.
- [ ] All twelve issue falsifiers are present as rejectable designs.
- [ ] #7777/#10527 remain generic receipt authority; no copied receipt framework exists.
- [ ] Emacs/Eglot/lsp-mode, LSP4IJ, Coc, Lite XL, and Vim/DAP can reference one
      contract without local generic-policy copies.
- [ ] Rollback, transfer, compatibility, legacy adoption, and stop conditions exist.
- [ ] Deterministic structural proof passes twice and the second run is byte-clean.
- [ ] No host execution, editor behavior, CI route, support promotion, or external
      mutation is claimed or changed.

## Scope boundary

Files in scope:

- `.spec/10894-editor-host-reliability/context.md`
- `.spec/10894-editor-host-reliability/acceptance.md`
- `.spec/10894-editor-host-reliability/checklist.md`

Files and surfaces out of scope: all host/editor/client code, `crates/`, generic
receipt implementations, workflows, CI routing, policy/support/release claims,
generated status, dependency manifests, and external processes.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check output,
two-run hash comparison, and `git diff --check` result. Independent review must
challenge claim-versus-spec coverage, proof discrimination, ownership and consumer
reachability, and rollback/legacy adoption. A clean review does not prove host
execution or OS cleanup; those belong to the separate #10894 implementation and
consumer conformance lanes. Any missing tooling remains `NOT_PROVEN` and is a
follow-up issue, not a reason to widen this PR.
