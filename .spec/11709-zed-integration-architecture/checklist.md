# Implementation Checklist: #11709 — durable Zed integration architecture bundle

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any tooling.

### Step 1: Create the context contract

- **File:** `.spec/11709-zed-integration-architecture/context.md` (CREATE)
- **Change:** Record the problem, the honest past-tense current state (checked
  `.spec` conventions present, Zed validators present and scoped to JSON
  contracts/receipts not `.spec` bundles, closed train candidate as history
  only, upstream fixture still keying the legacy server ID), programme
  authority (#7759), downstream consumers (#10338/#11710/#11711/#10479/
  #9483/#11712–#11714), the durable laws (identity contract, four truth
  planes, stage ladder S01–S15, agentic execution law, stable-vs-mutable
  boundary), `AGENTS.md` compatibility, open decisions surfaced for owning
  nodes, adoption/rollback/transfer/stop, links.
- **Verify:** structural heading and term checks below; `git diff --check`.

### Step 2: Create the decision map

- **File:** `.spec/11709-zed-integration-architecture/plan.md` (CREATE)
- **Change:** Record the decision vocabulary (conflict-key namespace shared
  with #10338), the full decision inventory — every row carrying one
  proposition, its canonical owning authority, bound falsifiers, and an
  explicit `compiled` / `pending(<issue>)` status with reason — the fifteen
  evidence/publication stages S01–S15 with roles and owners, rails and
  cross-cutting boundaries, ordering boundaries, the falsifier-first rule,
  and the #10338 handoff.
- **Verify:** structural decision-row checks below; `git diff --check`.

### Step 3: Create acceptance and negative controls

- **File:** `.spec/11709-zed-integration-architecture/acceptance.md` (CREATE)
- **Change:** Include all canonical `SPEC_TEMPLATE.md` acceptance sections and
  all twelve issue falsifiers (F1–F12) in fixed order with exact
  mutation/kind/verdict/control semantics, plus the claim boundary and
  non-goals.
- **Verify:** structural heading and falsifier-order checks below;
  `git diff --check`.

### Step 4: Create the builder and proof contract

- **File:** `.spec/11709-zed-integration-architecture/checklist.md` (CREATE)
- **Change:** Define the bounded bundle order, the deterministic structural
  checker below, the second-run proof, the `NOT_PROVEN` boundary, and
  rollback/transfer/stop.
- **Verify:** the checker runs twice from the candidate worktree with
  identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/11709-zed-integration-architecture/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no workflow
change, no `docs/` change, no extension/source change, no code, no
configuration, no generated artifact, no GitHub state.

## Deterministic structural proof

The repository has no executable `.spec` graph validator (the honest gap the
precedent bundles recorded; the existing Zed validators scope to staged JSON
contracts and host receipts, not `.spec` bundles). Do not invent a generated
receipt or claim a missing tool passed. From the candidate worktree root, run
the following PowerShell checker twice after the four files are complete. The
checker asserts:

1. the union of the committed candidate patch (`merge-base(origin/main, HEAD)..HEAD`,
   which stays the candidate's own patch even if `origin/main` advances
   mid-flight because a sibling lane fetched), the staged index, the unstaged
   worktree, and NUL-delimited porcelain paths — including untracked files —
   equals exactly the four bundle paths (it fails closed on a malformed status
   record or a rename/copy record without its second path);
2. `context.md` carries the canonical headings, the eight identity rows inside
   the identity law, the eight truth-plane non-substitution rows inside the
   truth-plane law, the stable-versus-mutable forbidden classes inside their
   law, and five numbered open decisions inside Open decisions — a marker
   elsewhere in the bundle is insufficient;
3. `plan.md` carries all eighteen decision rows (`zed.server.identity` …
   `zed.claim.ceiling`) each with a `compiled` or explicit `pending(`
   status, and all fifteen stage rows `S01`…`S15`, inside the inventory
   sections;
4. `acceptance.md` has all six canonical `§` sections, and its `§Test-Grid`
   contains exactly twelve numbered falsifier rows in fixed order `1..12`,
   each with a `rejected` verdict, plus the full owner-issue denominator;
5. no durable file contains a 40-hex digest (mutable-state hygiene);
6. a SHA-256 fingerprint over the four files is printed; two runs must print
   byte-identical output.

Redirecting output to a temporary file outside the repository is local proof
only; no temporary file belongs in the PR.

The checker source below is deliberately pure ASCII: the section sign and
em dash used by canonical headings are built with `[char]` constants, so the
script parses identically under Windows PowerShell 5.1 and PowerShell 7
regardless of how the host decodes the saved `.ps1`.

```powershell
$ErrorActionPreference = 'Stop'
$secSign = [string][char]0x00A7   # section sign used in acceptance headings
$emDash = [string][char]0x2014    # em dash used in canonical titles
# NB: PowerShell variables are case-insensitive, so these deliberately avoid
# any name that later loop counters could clobber.
$root = '.spec/11709-zed-integration-architecture'
$paths = @("$root/context.md", "$root/plan.md", "$root/acceptance.md", "$root/checklist.md")
foreach ($p in $paths) { if (-not (Test-Path -LiteralPath $p)) { throw "missing bundle file: $p" } }

# --- 1. exact changed-path set (committed + index + worktree + untracked) ---
# Capture git output directly: redirected native output would be re-encoded
# by Windows PowerShell (UTF-16LE) and corrupt byte-level parsing.
$statusOutput = @(& git status --porcelain=v1 -z --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw 'git status porcelain failed' }
$raw = (($statusOutput -join "`n")).TrimEnd([char]0x0D, [char]0x0A, [char]0x00)
$records = @($raw -split [char]0 | Where-Object { $_ -ne '' })
$found = [System.Collections.Generic.List[string]]::new()
for ($i = 0; $i -lt $records.Count; $i++) {
  $record = [string]$records[$i]
  if ($record.Length -lt 4 -or $record[2] -ne ' ' -or $record.Substring(0, 2) -notmatch '^[ MADRCU?!]{2}$') { throw 'malformed porcelain record' }
  $found.Add($record.Substring(3))
  if ($record.Substring(0, 2) -match '[RC]') {
    if ($i + 1 -ge $records.Count -or [string]::IsNullOrEmpty($records[$i + 1])) { throw 'rename/copy record has no source path' }
    $found.Add([string]$records[++$i])
  }
}
$mergeBase = @(& git merge-base origin/main HEAD)
if ($LASTEXITCODE -ne 0 -or $mergeBase.Count -ne 1) { throw 'git merge-base failed' }
$committed = @(& git diff --name-only "$($mergeBase[0])..HEAD")
if ($LASTEXITCODE -ne 0) { throw 'git diff committed range failed' }
$staged = @(& git diff --cached --name-only)
if ($LASTEXITCODE -ne 0) { throw 'git diff staged failed' }
$unstaged = @(& git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'git diff unstaged failed' }
$union = @($committed + $staged + $unstaged + $found | Where-Object { $_ -ne '' } | Sort-Object -Unique)
$expected = @($paths | Sort-Object -Unique)
if (($union -join "`n") -cne ($expected -join "`n")) {
  throw "changed-path set mismatch. union=[$($union -join ', ')] expected=[$($expected -join ', ')]"
}

# --- helpers ---
function Get-SectionBody {
  param([string]$Document, [string]$HeadingPattern)
  $match = [regex]::Match($Document, "(?ms)^${HeadingPattern}\s*\r?\n(?<body>.*?)(?=^#{1,3}\s|\z)")
  if (-not $match.Success) { throw "missing contract section: $HeadingPattern" }
  return $match.Groups['body'].Value
}
function Assert-Heading {
  param([string]$Text, [string]$Heading, [string]$Label)
  if (-not ($Text -match "(?m)^$([regex]::Escape($Heading))\s*$")) { throw "missing ${Label}: $Heading" }
}
$contextText = Get-Content -Raw -LiteralPath $paths[0] -Encoding UTF8
$planText = Get-Content -Raw -LiteralPath $paths[1] -Encoding UTF8
$acceptanceText = Get-Content -Raw -LiteralPath $paths[2] -Encoding UTF8

# --- 2. context.md: canonical headings + section-bound laws ---
foreach ($h in @(
  '## Problem', '## Why this approach',
  '## Current state (honest, past tense, at this bundle''s compilation base)',
  '## Authority and ownership', '## Durable laws',
  '### Identity contract (load-bearing invariant)',
  '### Four truth planes (load-bearing invariant)',
  '### Evidence and publication stage ladder',
  '### Agentic execution law',
  '### Stable versus mutable information',
  '## Compatibility with the repository operating contract (`AGENTS.md`)',
  '## Open decisions surfaced for owning nodes',
  '## Adoption, rollback, transfer and stop', '## Links'
)) { Assert-Heading $contextText $h 'context heading' }

$identitySection = Get-SectionBody $contextText '### Identity contract \(load-bearing invariant\)'
foreach ($identityLine in @(
  '^Zed server ID\s*=\s*perl-lsp-rs\s*$',
  '^Zed display name\s*=\s*Perl LSP \(EffortlessMetrics\)\s*$',
  '^launched executable\s*=\s*perllsp\s*$',
  '^product/package\s*=\s*perl-lsp\s*$',
  '^existing upstream\s*=\s*perl-lsp\s*$',
  '^existing default\s*=\s*perlnavigator-server\s*$',
  '^DAP adapter/binary\s*=\s*perl-dap\s*$',
  '^extension ID\s*=\s*perl\s*$'
)) {
  if (-not ($identitySection -match ("(?m)" + $identityLine))) { throw "missing identity row: $identityLine" }
}
if (-not ($identitySection -match [regex]::Escape('perllsp --stdio'))) { throw 'identity law lacks exact launch argv' }

$planeSection = Get-SectionBody $contextText '### Four truth planes \(load-bearing invariant\)'
foreach ($row in @('issue or PR state', 'implementation on tree', 'public asset bytes/process',
                   'exact-source host behavior', 'upstream submission', 'merged subject',
                   'host receipt', 'LSP evidence')) {
  if (-not ($planeSection -match [regex]::Escape($row))) { throw "missing truth-plane non-substitution row: $row" }
}

$mutableSection = Get-SectionBody $contextText '### Stable versus mutable information'
$mutableFlat = $mutableSection -replace '\s+', ' '
foreach ($cls in @('current main SHA', 'open PR number or branch', 'review/check colour',
                   'assigned model or worker', 'wall-clock readiness', 'current candidate uniqueness',
                   'current workflow run', 'mutable release or registry subject')) {
  if (-not ($mutableFlat -match [regex]::Escape($cls))) { throw "missing forbidden mutable class: $cls" }
}

$openSection = Get-SectionBody $contextText '## Open decisions surfaced for owning nodes'
if (-not ($openSection -match '(?m)^1\.' -and $openSection -match '(?m)^5\.')) { throw 'missing five numbered open decisions' }

# --- 3. plan.md: complete decision inventory + stage ladder ---
$planTitle = '# Plan: #11709 ' + $emDash + ' durable Zed integration leaf contracts'
foreach ($h in @($planTitle, '## Decision vocabulary',
                 '## Decision inventory', '### Product authority',
                 '### Managed artifact and mutation authority', '### Evidence and publication stages',
                 '### Rails and cross-cutting boundaries', '## Execution law binding',
                 '## Ordering boundaries', '## Falsifier-first rule', '## Handoff')) {
  Assert-Heading $planText $h 'plan heading'
}
$decisionIds = @(
  'zed.server.identity', 'zed.launch.argv', 'zed.product.package', 'zed.binary.provenance',
  'zed.extension.materialization', 'zed.extension.execution_source', 'zed.settings.defaults.status',
  'zed.fixture.expectations', 'zed.activation.platform',
  'zed.assets.public_contract', 'zed.cache.integrity', 'zed.mutation.safety', 'zed.network.update',
  'zed.managed_route.authority', 'zed.registry.host_authority',
  'zed.dap.sidecar', 'zed.currentness.invalidation', 'zed.claim.ceiling'
)
if ($decisionIds.Count -ne 18) { throw 'decision inventory must contain exactly 18 decisions' }
# The inventory spans four subsections, so take it up to the next level-2
# section instead of the next heading of any level.
$inventoryBody = [regex]::Match($planText, "(?ms)^## Decision inventory\s*\r?\n(?<body>.*?)(?=^## Execution law binding\s*\r?\n)").Groups['body'].Value
if (-not $inventoryBody) { throw 'decision inventory section is empty' }
foreach ($id in $decisionIds) {
  $rowPattern = '(?m)^\| `' + [regex]::Escape($id) + '` \|'
  if (-not ($inventoryBody -match $rowPattern)) { throw "missing decision row: $id" }
}
$pendingRows = @([regex]::Matches($inventoryBody, '(?m)^\| .* \| pending\(')).Count
$compiledRows = @([regex]::Matches($inventoryBody, '\| compiled \|')).Count
if ($compiledRows -ne 16 -or $pendingRows -ne 2) {
  throw "decision status drift: expected 16 compiled and 2 pending, found compiled=$compiledRows pending=$pendingRows"
}
for ($s = 1; $s -le 15; $s++) {
  $stage = 'S{0:D2}' -f $s
  if (-not ($inventoryBody -match "(?m)^\| $stage \|")) { throw "missing stage row: $stage" }
}

# --- 4. acceptance.md: canonical sections + fixed-order falsifier grid ---
foreach ($h in @(('## {0}Behavior' -f $secSign), ('## {0}Hazards' -f $secSign), ('## {0}Contracts' -f $secSign),
                 ('## {0}API-Shape' -f $secSign), ('## {0}Test-Grid' -f $secSign), ('## {0}Blast-Radius' -f $secSign))) {
  Assert-Heading $acceptanceText $h 'acceptance section'
}
$testGrid = Get-SectionBody $acceptanceText ('## {0}Test-Grid' -f $secSign)
$falsifierRows = @($testGrid -split "`r?`n" | Where-Object { $_ -match '^\|\s*\d+\s*\|' })
if ($falsifierRows.Count -ne 12) { throw "expected exactly 12 falsifier rows, found $($falsifierRows.Count)" }
for ($i = 0; $i -lt 12; $i++) {
  $row = $falsifierRows[$i]
  $expectedNumber = $i + 1
  if ($row -notmatch ("^\|\s*$expectedNumber\s*\|")) { throw "falsifier order broken at row $($i + 1)" }
  if ($row -notmatch [regex]::Escape('rejected')) { throw "falsifier row $expectedNumber lacks a rejected verdict" }
}
foreach ($owner in @('#7759', '#10338', '#10842', '#11304', '#10395', '#11041', '#10340', '#10530',
                     '#10392', '#10393', '#11043', '#8647', '#11046', '#10991', '#8661', '#8678',
                     '#10396', '#8753', '#8772', '#11316', '#11308', '#9467', '#7912',
                     '#10168', '#10858', '#10872', '#10881')) {
  if (-not ($acceptanceText -match [regex]::Escape($owner))) { throw "missing owner authority in acceptance: $owner" }
}

# --- 5. mutable-state hygiene across all four durable files ---
$allText = $contextText + "`n" + $planText + "`n" + $acceptanceText + "`n" + (Get-Content -Raw -LiteralPath $paths[3] -Encoding UTF8)
if ($allText -match '[0-9a-f]{40}') { throw 'durable bytes contain a 40-hex digest (mutable state)' }

# --- 6. deterministic fingerprint over the four bundle files ---
$sha = [System.Security.Cryptography.SHA256]::Create()
$allBytes = [System.Collections.Generic.List[byte]]::new()
foreach ($p in $paths) {
  $fileBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $p))
  $allBytes.AddRange($fileBytes)
}
$fingerprint = [BitConverter]::ToString($sha.ComputeHash($allBytes.ToArray())) -replace '-', ''
Write-Output "SPEC_11709_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_11709_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice from the candidate worktree root. Requirements for a
valid proof:

1. both runs print `SPEC_11709_STRUCTURAL_CHECK=PASS`;
2. both runs print the same `SPEC_11709_BUNDLE_SHA256` fingerprint;
3. the full captured output of both runs is byte-identical;
4. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
5. `git diff --check` is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves bundle shape, section-bound laws, decision-row
and stage completeness with explicit statuses, falsifier-grid order/verdicts,
the owner-issue denominator, 40-hex mutable-state hygiene, exact changed-path
scope, and byte-level determinism across two runs. It does **not** prove: that
the compiled architecture is the *correct* architecture (that is review's
job); that any later tooling works (unbuilt); that any Zed behavior, host
receipt, release, support projection, or external submission exists (owned by
later stages); or that #10842's migration or #11041's selection has completed
(their rows stay `pending`). The repository's absent executable `.spec` graph
validator remains an open tooling gap recorded here rather than papered over.

## Flags for builder

None. This bundle is complete as compiled; later nodes own all implementation
ambiguities through their own issues and the #10338 node contracts.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor bundle supersedes this one by explicit link;
  downstream consumers re-derive affected nodes.
- **Stop:** stop before stable-DAG implementation (#10338), current-tree
  observation, readiness/frontier solving, live observation, packet
  generation, product or extension implementation, host execution, packet
  freeze, support promotion, external submission, merge of any train
  candidate, release, publication, or external action. If an open decision in
  `context.md` is needed as a decision rather than a boundary, stop and route
  it to its owning node; do not decide it in a builder PR.
