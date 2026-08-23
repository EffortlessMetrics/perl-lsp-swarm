# Implementation Checklist: #11763 — durable issue-controller architecture bundle

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any tooling.

### Step 1: Create the context contract

- **File:** `.spec/11763-issue-controller-architecture/context.md` (CREATE)
- **Change:** Record the problem, the honest current state (manual issue
  control, `AGENTS.md` routing prose, `spec-builder.js` markdown fan-out only,
  no issue-controller tooling, unstarted programme), programme authority
  (#11681, C01–C06 rail, S00 execution train), the durable laws (roles,
  relationships, discovery vs adjudication, registry vs projection, mutation,
  drift, truth planes, revision, exact-tree/packets, generic entry,
  closeout/proof/dogfood), `AGENTS.md` compatibility, open decisions,
  adoption/rollback/transfer/stop, links.
- **Verify:** structural heading and term checks below; `git diff --check`.

### Step 2: Create the node map

- **File:** `.spec/11763-issue-controller-architecture/plan.md` (CREATE)
- **Change:** Record the programme shape and, for every existing and planned
  node (25 nodes, #11682–#11785 plus S00), the exact one-PR proposition,
  consumed authority and "never" boundary, the per-node execution contract,
  ordering boundaries, the falsifier-first rule, and the T01 handoff.
- **Verify:** structural node-row checks below; `git diff --check`.

### Step 3: Create acceptance and negative controls

- **File:** `.spec/11763-issue-controller-architecture/acceptance.md` (CREATE)
- **Change:** Include all canonical `SPEC_TEMPLATE.md` acceptance sections and
  all sixteen issue falsifiers in fixed order with exact scenario/kind/verdict
  semantics, plus the claim boundary and non-goals.
- **Verify:** structural heading and falsifier-order checks below;
  `git diff --check`.

### Step 4: Create the builder and proof contract

- **File:** `.spec/11763-issue-controller-architecture/checklist.md` (CREATE)
- **Change:** Define the bounded bundle order, the deterministic structural
  checker below, the second-run proof, the `NOT_PROVEN` boundary, and
  rollback/transfer/stop.
- **Verify:** the checker runs twice from the candidate worktree with
  identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/11763-issue-controller-architecture/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no
`.claude/workflows/` change, no `docs/` change, no code, no configuration, no
generated artifact, no GitHub state.

## Deterministic structural proof

The repository has no executable `.spec` graph validator (the same honest gap
the precedent bundle `.spec/10894-editor-host-reliability/` recorded). Do not
invent a generated receipt or claim a missing tool passed. From the candidate
worktree root, run the following PowerShell 7 checker twice after the four
files are complete. The checker asserts:

1. the union of the committed candidate patch (`origin/main..HEAD`), the
   staged index, the unstaged worktree, and NUL-delimited porcelain paths —
   including untracked files — equals exactly the four bundle paths (it fails
   closed on a malformed status record or a rename/copy record without its
   second path);
2. every required canonical heading exists, and load-bearing contract terms
   are present **section-bound** in `context.md` (roles inside the role law,
   the nine non-substitution rows inside the truth-plane law, five numbered
   open decisions inside Open decisions) — a marker elsewhere in the bundle is
   insufficient;
3. `plan.md` carries all 25 node rows (`C01 #11682` … `R05B #11785`) inside
   the node-propositions section;
4. `acceptance.md` has all six canonical `§` sections, and its `§Test-Grid`
   contains exactly sixteen numbered falsifier rows in fixed order `1..16`,
   each with a `rejected` verdict;
5. a SHA-256 fingerprint over the four files is printed; two runs must print
   byte-identical output.

Redirecting output to a temporary file is local proof only; no temporary file
belongs in the PR.

```powershell
$ErrorActionPreference = 'Stop'
$root = '.spec/11763-issue-controller-architecture'
$paths = @("$root/context.md", "$root/plan.md", "$root/acceptance.md", "$root/checklist.md")
foreach ($p in $paths) { if (-not (Test-Path -LiteralPath $p)) { throw "missing bundle file: $p" } }

# --- 1. exact changed-path set (committed + index + worktree + untracked) ---
$statusFile = [IO.Path]::GetTempFileName()
try {
  # This scan is intentionally unscoped: an exact-scope proof must also see
  # tracked and untracked paths outside the four-file bundle.
  & git status --porcelain=v1 -z --untracked-files=all > $statusFile 2>&1
  if ($LASTEXITCODE -ne 0) { throw 'git status porcelain failed' }
  $bytes = [IO.File]::ReadAllBytes($statusFile)
  $raw = [Text.Encoding]::UTF8.GetString($bytes).TrimEnd([char]0x0D, [char]0x0A)
} finally { Remove-Item -LiteralPath $statusFile -Force -ErrorAction SilentlyContinue }
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
$committed = @(& git diff --name-only 'origin/main..HEAD')
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
$contextText = Get-Content -Raw -LiteralPath $paths[0]
$planText = Get-Content -Raw -LiteralPath $paths[1]
$acceptanceText = Get-Content -Raw -LiteralPath $paths[2]

# --- 2. context.md: canonical headings + section-bound laws ---
$contextHeadings = @(
  '## Problem', '## Why this approach', '## Current state \(honest, as of this bundle\)',
  '## Authority and ownership', '## Durable laws',
  '### Primary issue roles and assignability', '### Relationship vocabulary',
  '### Candidate discovery versus reviewed role adjudication',
  '### Stable registry versus generated labels, navigation and directory',
  '### Bounded expected-old-state GitHub metadata mutation',
  '### Read-only metadata drift observation',
  '### Durable truth planes \(load-bearing invariant\)',
  '### Semantic train revision and invalidation',
  '### Exact-tree context and shared actor-packet boundaries',
  '### Generic work-entry adoption and old-heuristic retirement',
  '### Exact-head closeout, composed proof and fresh-agent dogfood',
  '## Compatibility with the repository operating contract \(`AGENTS.md`\)',
  '## Open decisions', '## Adoption, rollback, transfer and stop', '## Links'
)
foreach ($h in $contextHeadings) { if (-not ($contextText -match "(?m)^${h}\s*$")) { throw "missing context heading: $h" } }

$roleSection = Get-SectionBody $contextText '### Primary issue roles and assignability'
foreach ($role in @('`controller`', '`implementation`', '`proof`', '`fan_in`', '`external_gate`')) {
  if (-not ($roleSection -match [regex]::Escape($role))) { throw "missing primary role in role law: $role" }
}
if (-not ($roleSection -match [regex]::Escape('assignable = false'))) { throw 'missing assignability law' }

$planeSection = Get-SectionBody $contextText '### Durable truth planes \(load-bearing invariant\)'
foreach ($row in @('title/body/label signal', 'issue closed', 'registry row', 'label/navigation applied',
                   'current-tree implementation', 'packet generated', 'proof producer exists',
                   'metadata drift clean', 'controller open/closed')) {
  if (-not ($planeSection -match [regex]::Escape($row))) { throw "missing truth-plane non-substitution row: $row" }
}

$openSection = Get-SectionBody $contextText '## Open decisions'
if (-not ($openSection -match '(?m)^1\.' -and $openSection -match '(?m)^5\.')) { throw 'missing five numbered open decisions' }

foreach ($term in @('expected-old-state', 'generated navigation block', 'never rewrites the registry',
                    'not_proven', '#10872', '#10881', '#3983', 'markdown fan-out', 'status:blocked',
                    'assignable = false', 'exactly one home', 'import')) {
  if (-not ($contextText -match [regex]::Escape($term))) { throw "missing context contract term: $term" }
}

# --- 3. plan.md: all 25 node rows inside the propositions section ---
foreach ($h in @('## Programme shape', '## Node propositions', '## Execution contract per node',
                 '## Ordering boundaries', '## Falsifier-first rule', '## Handoff')) {
  if (-not ($planText -match "(?m)^$([regex]::Escape($h))\s*$")) { throw "missing plan heading: $h" }
}
$nodeSection = Get-SectionBody $planText '## Node propositions'
$nodes = [ordered]@{
  'C01' = '#11682'; 'C02' = '#11683'; 'C03' = '#11684'; 'C04' = '#11685'
  'C05' = '#11686'; 'C06' = '#11687'; 'S00' = '#11763'; 'T01' = '#11764'
  'T02' = '#11765'; 'T02R' = '#11767'; 'T03' = '#11769'; 'T04' = '#11771'
  'T05' = '#11772'; 'T06' = '#11773'; 'T02S' = '#11774'; 'T07' = '#11775'
  'T08' = '#11776'; 'T08C' = '#11784'; 'I01' = '#11777'; 'I02' = '#11778'
  'P01' = '#11779'; 'D01' = '#11781'; 'D02' = '#11782'; 'P02' = '#11783'
  'R05B' = '#11785'
}
if ($nodes.Count -ne 25) { throw 'node table must contain exactly 25 nodes' }
foreach ($id in $nodes.Keys) {
  $rowPattern = "\| $([regex]::Escape($id)) \| $($nodes[$id]) \|"
  if (-not ($nodeSection -match $rowPattern)) { throw "missing node proposition row: $id $($nodes[$id])" }
}

# --- 4. acceptance.md: canonical sections + fixed-order falsifier grid ---
foreach ($h in @('## §Behavior', '## §Hazards', '## §Contracts', '## §API-Shape', '## §Test-Grid', '## §Blast-Radius')) {
  if (-not ($acceptanceText -match "(?m)^$([regex]::Escape($h))\s*$")) { throw "missing acceptance section: $h" }
}
$testGrid = Get-SectionBody $acceptanceText '## §Test-Grid'
$falsifierRows = @($testGrid -split "`r?`n" | Where-Object { $_ -match '^\|\s*\d+\s*\|' })
if ($falsifierRows.Count -ne 16) { throw "expected exactly 16 falsifier rows, found $($falsifierRows.Count)" }
for ($i = 0; $i -lt 16; $i++) {
  $row = $falsifierRows[$i]
  $expectedNumber = $i + 1
  if ($row -notmatch ("^\|\s*$expectedNumber\s*\|")) { throw "falsifier order broken at row $($i + 1)" }
  if ($row -notmatch [regex]::Escape('rejected')) { throw "falsifier row $expectedNumber lacks a rejected verdict" }
}
foreach ($term in @('#10872', '#10881', '#3983', '#11682', '#11785', 'issue_role_contract.v1',
                    'issue_controller_registry.v1', 'issue_controller_train.v1')) {
  if (-not ($acceptanceText -match [regex]::Escape($term))) { throw "missing acceptance contract term: $term" }
}

# --- 5. deterministic fingerprint over the four bundle files ---
$sha = [System.Security.Cryptography.SHA256]::Create()
$allBytes = [System.Collections.Generic.List[byte]]::new()
foreach ($p in $paths) {
  $fileBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $p))
  $allBytes.AddRange($fileBytes)
}
$fingerprint = [BitConverter]::ToString($sha.ComputeHash($allBytes.ToArray())) -replace '-', ''
Write-Output "SPEC_11763_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_11763_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice. Requirements for a valid proof:

1. both runs print `SPEC_11763_STRUCTURAL_CHECK=PASS`;
2. both runs print the same `SPEC_11763_BUNDLE_SHA256` fingerprint;
3. the full captured output of both runs is byte-identical;
4. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
5. `git diff --check` (staged) is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves bundle shape, section-bound laws, node-map
completeness, falsifier-grid order/verdicts, exact changed-path scope, and
byte-level determinism across two runs. It does **not** prove: that the
compiled architecture is the *correct* architecture (that is review's job);
that any later tooling works (unbuilt); that roles are correctly adjudicated
(no registry exists); or that a live migration is safe (no migration is
authorized). The repository's absent executable `.spec` graph validator remains
an open tooling gap recorded here rather than papered over.

## Flags for builder

None. This bundle is complete as compiled; later nodes own all implementation
ambiguities through their own issues and T02S specifications.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor bundle supersedes this one by explicit link, with
  T02R (#11767) governing invalidation of derived artifacts.
- **Stop:** stop before stable-train implementation, registry population,
  GitHub metadata changes, current-tree/live observation, packet generation,
  model execution, merge, release or publication. If an "Open decision" in
  `context.md` is needed as a decision rather than a boundary, stop and route
  it to its owning node; do not decide it in a builder PR.
