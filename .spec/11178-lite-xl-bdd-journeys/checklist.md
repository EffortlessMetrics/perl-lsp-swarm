# Implementation Checklist: #11178 — bounded Lite XL client journeys and evidence boundaries

## Change order

This is a documentation/specification-only change (plus one machine-generated
inventory projection). No step builds or executes any client/host process.

### Step 1: Create the journey/evidence-boundary context contract

- **File:** `.spec/11178-lite-xl-bdd-journeys/context.md`
- **Change:** record the problem, ledger-format evolution record, consumed
  substrate anchors, stable scenario-ID namespace, journey inventory (73
  baseline + 8 optional), claim profiles and laws, evidence chains and tag
  mapping, security boundary, authority split, stable-vs-mutable rule,
  alternatives rejected, prior art, links, scope.
- **Verify:** checker below enforces profile names, chain owners, vocabulary
  terms, and amendment authorities; `git diff --check`.

### Step 2: Create the normative behavior ledger and falsifiers

- **File:** `.spec/11178-lite-xl-bdd-journeys/acceptance.md`
- **Change:** include all canonical `SPEC_TEMPLATE.md` sections; §Behavior
  carries the baseline scenario ledger (stable IDs, user-visible wording,
  profile/evidence-tag membership, per-row owner chains), the optional-input
  table, and profile membership/laws; §Test-Grid carries all twenty
  controlling-issue false-green examples in fixed order.
- **Depends on:** Step 1.
- **Verify:** structural heading, scenario-ID-set, profile-vocabulary, and
  falsifier-table checks below; `git diff --check`.

### Step 3: Create the builder/proof contract (this file)

- **File:** `.spec/11178-lite-xl-bdd-journeys/checklist.md`
- **Change:** bounded change order, deterministic structural checking,
  second-run proof, acceptance gates, handoff.
- **Depends on:** Steps 1–2.
- **Verify:** read-only checker runs twice with byte-identical output and no
  tree diff.

### Step 4: Inventory projection boundary

- **File:** `docs/policy/NON_RUST_INVENTORY.md` (not changed by this bundle)
- **Rule:** the projection stays owned by the sanctioned writer
  (`cargo xtask non-rust inventory --write`) executed outside this packet;
  listing this directory lands through whichever leaf next regenerates it.
  A newly added packet file is allowlist-classified by `.spec/**`, so the
  gate stays green with the projection temporarily behind (warning-level),
  never hand-edited.

## Deterministic structural proof

The repository has no executable `.spec` graph validator and no Gherkin/
feature-status generator on current main (recorded as the ledger evolution in
`context.md`). Do not invent a generated receipt or claim a missing tool
passed. From the candidate worktree, run the following PowerShell 7 check twice
after the files are complete. It enforces: the exact three packet files; required canonical headings; required
contract terms (profile names, evidence-chain owners, vocabulary tokens,
amendment authorities, substrate paths); the exact eighty-one scenario IDs
bound to their §Behavior ledger rows in fixed family order; and all twenty
falsifier rows with exact scenario/kind/verdict text in fixed order.
Exact-string comparisons are deliberately case-sensitive (`-cmatch`, `-cne`,
`-CaseSensitive`). Its changed-path assertion unions the candidate patch with
unstaged/staged/untracked paths fail-closed and requires that union to equal
the exact four-file set.

```powershell
function Get-SpecStatusPaths {
  $statusFile = [IO.Path]::GetTempFileName()
  try {
    & git status --porcelain=v1 -z --untracked-files=all > $statusFile 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'git status porcelain failed' }
    $bytes = [IO.File]::ReadAllBytes($statusFile)
    while ($bytes.Length -ge 1 -and ($bytes[$bytes.Length - 1] -eq 0x0A -or $bytes[$bytes.Length - 1] -eq 0x0D)) {
      $bytes = $bytes[0..($bytes.Length - 2)]
    }
    $raw = [Text.Encoding]::UTF8.GetString($bytes)
  } finally {
    Remove-Item -LiteralPath $statusFile -Force -ErrorAction SilentlyContinue
  }
  $records = @($raw -split [char]0 | Where-Object { $_ -ne '' })
  $found = [System.Collections.Generic.List[string]]::new()
  for ($i = 0; $i -lt $records.Count; $i++) {
    $record = [string]$records[$i]
    if ($record.Length -lt 4 -or $record[2] -ne ' ' -or $record.Substring(0,2) -notmatch '^[ MADRCU?!]{2}$') { throw 'malformed porcelain record' }
    $found.Add($record.Substring(3))
    if ($record.Substring(0,2) -match '[RC]') {
      if ($i + 1 -ge $records.Count -or [string]::IsNullOrEmpty($records[$i + 1])) { throw 'rename/copy record has no source path' }
      $found.Add([string]$records[++$i])
    }
  }
  return @($found)
}

function Invoke-Spec11178Check {
$root = '.spec/11178-lite-xl-bdd-journeys'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  'clients/lite-xl', 'journey_session_test.lua', 'harness.lua',
  'lite_xl_fixture_expectations.v1', '.lite_lsp.lua', 'perllsp',
  'lite_xl_protocol_baseline', 'lite_xl_exact_source_core',
  'lite_xl_workspace_fresh', 'lite_xl_first_class_public',
  'lite_xl_quality_breadth', 'consumes_if_available',
  '#8950', '#9008', '#10651', '#11176', '#10858', '#10338', '#3983',
  '#11181', '#11103', '#8960', '#10673',
  '#10676', '#10679', '#10681', '#10684', '#10691', '#10693',
  '#7122', '#9016', '#10733', '#9010', '#10739', '#9012', '#10767',
  '#11170', '#11172', '#10653', '#2298', '#7713',
  '#11186', '#11188', '#11189', '#11194', '#11197', '#11198',
  'configuration_documented', 'protocol_profile_proven',
  'client_simulation_proven', 'composed_exact_source',
  'exact_source_actual_host', 'managed_exact_source',
  'accepted_unreleased', 'released_upstream', 'public_artifact_actual_host',
  'not_proven', 'instrument_failed', 'cleanup_failed'
)
$headings = @('§Behavior', '§Hazards', '§Contracts', '§API-Shape', '§Test-Grid', '§Blast-Radius', '§Coverage-Map')
$text = @($paths | ForEach-Object { Get-Content -Raw $_ })
if ($text.Count -ne 3) { throw 'expected exactly three spec files' }
$contextText = $text[0]
$acceptanceText = $text[1]
$contractText = @($contextText, $acceptanceText)
foreach ($term in $required) {
  if (-not ($contractText -cmatch [regex]::Escape($term))) { throw "missing contract term: $term" }
}
foreach ($heading in $headings) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($heading))) { throw "missing acceptance heading: $heading" }
}
# Profile membership is the acceptance ledger's own obligation; presence in
# another bundle file cannot satisfy it.
foreach ($term in @('lite_xl_protocol_baseline', 'lite_xl_exact_source_core', 'lite_xl_workspace_fresh', 'lite_xl_first_class_public', 'lite_xl_quality_breadth')) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing acceptance profile term: $term" }
}
foreach ($term in @('security-sensitive', 'A stronger profile never erases')) {
  if (-not ($contextText -cmatch [regex]::Escape($term)) -and -not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing boundary term: $term" }
}

# Eighty-one scenario IDs bound to ledger rows, fixed family order, unique.
$ids = [regex]::Matches($acceptanceText, '(?m)^\|\s*`(?<id>lite_xl\.bdd\.(?:activate|protocol|read|edit|lifecycle|wire|support|opt)\.\d{2})`\s*\|') |
  ForEach-Object { $_.Groups['id'].Value }
$families = @{
  activate = 9; protocol = 13; read = 11; edit = 12
  lifecycle = 11; wire = 8; support = 9; opt = 8
}
$expectedIds = [System.Collections.Generic.List[string]]::new()
foreach ($family in @('activate','protocol','read','edit','lifecycle','wire','support','opt')) {
  for ($n = 1; $n -le $families[$family]; $n++) {
    $expectedIds.Add(('lite_xl.bdd.{0}.{1:d2}' -f $family, $n))
  }
}
if ($ids.Count -ne 81) { throw "expected exactly eighty-one scenario ledger rows, found $($ids.Count)" }
if (($ids | Sort-Object -Unique).Count -ne 81) { throw 'scenario IDs are not unique' }
if (($ids -join ',') -cne ($expectedIds -join ',')) { throw "scenario ledger rows do not match the stable ID set in fixed order: found $($ids -join ',')" }

# Twenty falsifiers: fixed issue order, exact semantics, non-empty verdicts.
$grid = [regex]::Match($acceptanceText, '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 20) { throw "expected exactly twenty falsifier rows, found $($rows.Count)" }
$rowIds = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($rowIds | Sort-Object -Unique).Count -ne $rowIds.Count) { throw 'falsifier IDs are not unique' }
if (($rowIds -join ',') -cne ((1..20) -join ',')) { throw 'falsifier IDs are not in fixed order' }
$expectedRows = @(
  @{ id = 1; scenario = 'Another Perl server or an ambient perllsp binary satisfies the row'; kind = 'negative'; verdict = 'reject; the exact selected perllsp provider identity is the proposition (activate.02/activate.03)' }
  @{ id = 2; scenario = 'Syntax activation is counted without actual server/document attach'; kind = 'negative'; verdict = 'reject; activation requires actual attach (activate.01)' }
  @{ id = 3; scenario = 'Project Lua sentinel executes while setup is called trusted'; kind = 'negative'; verdict = 'reject; project-controlled config stays inert and untrusted (activate.04/activate.05)' }
  @{ id = 4; scenario = 'False becomes null, empty list becomes object, or response item order changes'; kind = 'negative'; verdict = 'reject; JSON shape, cardinality, and order are exact (activate.06/protocol.01)' }
  @{ id = 5; scenario = 'A request is transmitted twice or a required response/message is silently dropped'; kind = 'negative'; verdict = 'reject; single-send request IDs and exactly-one-response laws hold (protocol.02/protocol.03)' }
  @{ id = 6; scenario = 'A stale diagnostic empty list clears newer diagnostics'; kind = 'negative'; verdict = 'reject; only current provider/version publications replace or clear (read.03/read.04/read.05)' }
  @{ id = 7; scenario = 'The same cursor after an edit admits a stale hover/completion/format result'; kind = 'negative'; verdict = 'reject; post-edit answers belong to the current generation (read.08/edit.11)' }
  @{ id = 8; scenario = 'A non-empty hover/navigation/symbol result refers to the wrong target or root'; kind = 'negative'; verdict = 'reject; answered identity must match the intended subject (read.07/read.11)' }
  @{ id = 9; scenario = 'Preview mutation changes the navigation target'; kind = 'negative'; verdict = 'reject; navigation targets stay authoritative, previews observational (read.09)' }
  @{ id = 10; scenario = 'A completion/format/rename edit is returned or logged but not applied'; kind = 'negative'; verdict = 'reject; actual validated application is the proposition (edit.01/edit.07/edit.09)' }
  @{ id = 11; scenario = 'Final bytes are right but the caret jumps or an unrelated sentinel changes'; kind = 'negative'; verdict = 'reject; caret intent survives edits and unrelated subjects are untouched (edit.08)' }
  @{ id = 12; scenario = 'A watcher test secretly restarts or reopens the file'; kind = 'negative'; verdict = 'reject; external-change freshness must not be manufactured by restarts (lifecycle.06/lifecycle.07)' }
  @{ id = 13; scenario = 'Old and new server processes overlap or old callbacks publish after replacement'; kind = 'negative'; verdict = 'reject; generation-owned processes and callbacks never overlap publishes (lifecycle.08/lifecycle.10)' }
  @{ id = 14; scenario = 'The >50 KiB scenario silently uses a smaller file'; kind = 'negative'; verdict = 'reject; large-wire decode exactness requires the full admitted size (wire.07)' }
  @{ id = 15; scenario = 'The protocol trace is correct but the actual Lite XL UI/buffer result is wrong'; kind = 'negative'; verdict = 'reject; trace evidence never substitutes for applied editor results (edit.01/read.01)' }
  @{ id = 16; scenario = 'A staged patch/composed profile is labeled accepted/released/public'; kind = 'negative'; verdict = 'reject; stage labels stay monotone-distinct with their own direct evidence (support.05/support.02)' }
  @{ id = 17; scenario = 'One Linux row satisfies Windows/macOS'; kind = 'negative'; verdict = 'reject; platforms never substitute (support.03/opt.04)' }
  @{ id = 18; scenario = 'A server capability bit creates Lite XL feature support'; kind = 'negative'; verdict = 'reject; advertisement is not consumption (support.06)' }
  @{ id = 19; scenario = 'Raw/source-bearing logs enter canonical evidence'; kind = 'negative'; verdict = 'reject; canonical evidence stays bounded and redacted (protocol.12)' }
  @{ id = 20; scenario = 'A DAP or advanced result satisfies a core LSP profile row'; kind = 'negative'; verdict = 'reject; advanced/DAP rails never fill LSP core scenarios (support.07)' }
)
for ($i = 0; $i -lt $expectedRows.Count; $i++) {
  $row = $rows[$i]
  $expectedRow = $expectedRows[$i]
  foreach ($field in @('scenario', 'kind', 'verdict')) {
    $actual = $row.Groups[$field].Value.Trim()
    if ($actual -cne $expectedRow[$field]) { throw "falsifier $($expectedRow.id) has unexpected $field" }
  }
}

# Bind the proof to the explicit candidate range. The base is resolved ONCE
# as the merge-base of origin/main and HEAD (or supplied by the two-run
# wrapper via $script:CandidatePinnedBase), so unrelated main movement with a
# conflict-free candidate never invalidates or re-triggers this proof.
if (-not $script:CandidatePinnedBase) {
  # Full-drain capture: an early-stopping upstream consumer can leave
  # $LASTEXITCODE unset, so never stop a native pipeline mid-stream.
  $script:CandidatePinnedBase = ([string](& git merge-base 'origin/main' 'HEAD')).Trim()
  if ($LASTEXITCODE -ne 0 -or -not $script:CandidatePinnedBase) { throw 'cannot resolve candidate base via git merge-base' }
}
$candidateBase = [string]$script:CandidatePinnedBase
$candidateHead = (& git rev-parse --verify 'HEAD^{commit}' 2>&1).Trim()
if ($LASTEXITCODE -ne 0 -or -not $candidateHead) { throw 'candidate HEAD ref is not resolvable' }
& git merge-base --is-ancestor $candidateBase $candidateHead 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "candidate range is not $candidateBase..$candidateHead" }
$candidateRange = "$candidateBase..$candidateHead"
git diff --check $candidateRange
if ($LASTEXITCODE -ne 0) { throw "candidate diff --check failed for $candidateRange" }

# Each path scan fails closed before a later command can mask it.
$rangePaths = @(git diff --name-only $candidateRange)
if ($LASTEXITCODE -ne 0) { throw "git diff --name-only failed for $candidateRange" }
$worktreePaths = @(git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'git diff --name-only failed for the worktree' }
$cachedPaths = @(git diff --cached --name-only HEAD)
if ($LASTEXITCODE -ne 0) { throw 'git diff --cached --name-only failed' }
$changed = @($rangePaths + $worktreePaths + $cachedPaths + (Get-SpecStatusPaths)) | Sort-Object -Unique -CaseSensitive
$expected = @(
  '.spec/11178-lite-xl-bdd-journeys/acceptance.md'
  '.spec/11178-lite-xl-bdd-journeys/checklist.md'
  '.spec/11178-lite-xl-bdd-journeys/context.md'
)
if ($changed.Count -ne $expected.Count -or (Compare-Object -CaseSensitive $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_11178_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice with fingerprinted inputs, using
this wrapper around the exact checker body above:

```powershell
function Get-SpecFingerprints {
  $expected = @(
    '.spec/11178-lite-xl-bdd-journeys/acceptance.md'
    '.spec/11178-lite-xl-bdd-journeys/checklist.md'
    '.spec/11178-lite-xl-bdd-journeys/context.md'
  )
  return @($expected | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "missing spec file: $_" }
    "$_=$((Get-FileHash -Algorithm SHA256 -LiteralPath $_ -ErrorAction Stop).Hash)"
  })
}
$ErrorActionPreference = 'Stop'
# Full-drain capture: an early-stopping upstream consumer can leave
# $LASTEXITCODE unset, so never stop a native pipeline mid-stream.
$script:CandidatePinnedBase = ([string](& git merge-base 'origin/main' 'HEAD')).Trim()
if ($LASTEXITCODE -ne 0 -or -not $script:CandidatePinnedBase) { throw 'wrapper cannot resolve candidate base via git merge-base' }
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("spec-11178-check-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$tmp1 = Join-Path $tmpDir 'run1.out'
$tmp2 = Join-Path $tmpDir 'run2.out'
try {
  $tree1 = @(Get-SpecStatusPaths) -join "`n"
  $fpBefore = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11178Check | Set-Content -LiteralPath $tmp1 -Encoding utf8NoBOM -ErrorAction Stop
  $tree2 = @(Get-SpecStatusPaths) -join "`n"
  $fpBetween = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11178Check | Set-Content -LiteralPath $tmp2 -Encoding utf8NoBOM -ErrorAction Stop
  $tree3 = @(Get-SpecStatusPaths) -join "`n"
  $fpAfter = @(Get-SpecFingerprints) -join "`n"
  if ($tree1 -cne $tree2 -or $tree2 -cne $tree3 -or $fpBefore -cne $fpBetween -or $fpBetween -cne $fpAfter) { throw 'checker changed the spec tree or file contents' }
  foreach ($captured in @($tmp1, $tmp2)) {
    if (-not (Test-Path -LiteralPath $captured -PathType Leaf)) { throw "checker output capture failed: $captured" }
  }
  $h1 = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp1 -ErrorAction Stop).Hash
  $h2 = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp2 -ErrorAction Stop).Hash
  if ($h1 -cne $h2) { throw 'second run is not deterministic' }
} finally {
  Remove-Item -LiteralPath $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
"h1=$h1"
"h2=$h2"
'SPEC_11178_SECOND_RUN=PASS'
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'working tree diff --check failed' }
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'staged diff --check failed' }
$expected = @(
  '.spec/11178-lite-xl-bdd-journeys/acceptance.md'
  '.spec/11178-lite-xl-bdd-journeys/checklist.md'
  '.spec/11178-lite-xl-bdd-journeys/context.md'
)
# Trailing-whitespace coverage independent of staging state, so newly created
# packet files cannot escape the whitespace proof before being staged.
foreach ($path in $expected) {
  $resolved = (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
  $lines = [IO.File]::ReadAllLines($resolved)
  for ($li = 0; $li -lt $lines.Count; $li++) {
    if ($lines[$li] -cmatch '[ 	]+$') { throw "trailing whitespace at ${path}:$($li + 1)" }
  }
}
if ((Get-SpecStatusPaths | Where-Object { $_ -cnotin $expected })) { throw 'unexpected spec artifact' }
```

The `Invoke-Spec11178Check` function is the exact command body above, not a
copied output; each invocation rereads the files and revalidates every table.

## Acceptance gates

- [ ] Exactly the three packet files are changed; nothing else.
- [ ] All 73 baseline scenarios carry stable IDs, user-visible wording,
      profile/evidence tags, and named downstream owner chains.
- [ ] Optional/stronger rows stay `consumes_if_available`; core stays bounded.
- [ ] All twenty false-green controls present, fixed order, exact verdict
      semantics.
- [ ] Security boundary keeps project-executable config, unsafe launch,
      traversal/out-of-root paths, ambient/wrong providers, and raw logs out of
      positive behavior (#10653 class).
- [ ] Downstream substrate (#11181 manifest schema, #11103 suites, #8960,
      host lanes) consumed by reference; no second pin.
- [ ] No fixture bytes, Lua/Rust/shell behavior change, host execution,
      receipt, support-tier change, docs prose beyond this packet plus the
      generated projection, CI edit, or upstream action.
- [ ] Inventory projection untouched here; owned by the sanctioned writer
      outside this bundle (regenerating twice produces no diff).
- [ ] Deterministic structural proof passes twice; second run byte-clean.

## Callers and consumers

- #11181 binds fixture/expectation cells to these scenario IDs (currently
  blocked exactly on them).
- #11103 suites and #8960 cells cite IDs in provenance where applicable.
- Host lanes #10676/#10679/#10681/#10684/#10691/#10693 via the #10673 adapter
  observe against named scenarios.
- #11170/#11172 producers and #7122/#9016 projections cite IDs downstream.
- #10733/#9010/#10739/#9012/#10767 own the optional/distribution rails.

## Flags for builder

- Scenario IDs are immutable once published downstream; changes route through
  #11178 revision, never silent reuse.
- Behavior wording stays user-visible; implementation trivia belongs to
  #11181, the Lua suites, and the host lanes.
- If a later leaf can pass only by widening a proposition here, stop and
  return to #11178 instead of editing boundaries locally.
- Deviation note: the controlling issue sketched Gherkin feature files plus
  generated status commands (`cargo xtask bdd` / `ac-status` / `docs-check`);
  none exists on current main, so the journeys project into the shipped
  `.spec` ledger per the evolution record in `context.md`.

## Scope boundary

Files IN scope:

- `.spec/11178-lite-xl-bdd-journeys/context.md`
- `.spec/11178-lite-xl-bdd-journeys/acceptance.md`
- `.spec/11178-lite-xl-bdd-journeys/checklist.md`
- `docs/policy/NON_RUST_INVENTORY.md` (regenerated projection only)

Files OUT of scope: `docs/policy/NON_RUST_INVENTORY.md` (generated elsewhere
by its sanctioned writer), fixtures, client/host harness code, provisioning,
server/client behavior, receipts, support registry values, docs prose, CI
workflows, external upstream surfaces, and any new BDD runner infrastructure.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check
output, two-run hash comparison, and `git diff --check` result. Independent review must challenge whether every
behavioral statement traces to merged mechanics or a named authority, whether
evidence boundaries name real owning issues without duplication, whether the
profile membership honors the controlling issue's initial IDs, and whether any
row smuggles implementation trivia into specification. A clean review proves no
Lite XL behavior; executable truth belongs to the downstream leaves, and every
scenario remains `not_proven` as behavior until its own chain passes.
