# Implementation Checklist: #11371 — canonical Vim + vim-lsp user journeys and evidence boundaries

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any Vim/host process.

### Step 1: Create the journey/evidence-boundary context contract

- **File:** `.spec/11371-vim-bdd-journeys/context.md`
- **Change:** record the problem, ledger-format evolution record, consumed
  #12050 substrate, stable scenario-ID namespace, journey inventory (23
  baseline + 7 optional), claim profiles and laws, evidence chain and tag
  mapping, security boundary, authority split, stable-vs-mutable rule,
  alternatives rejected, prior art, links, scope.
- **Verify:** checker below enforces substrate identities, profile names,
  chain owners, and vocabulary terms; `git diff --check`.

### Step 2: Create the normative behavior ledger and falsifiers

- **File:** `.spec/11371-vim-bdd-journeys/acceptance.md`
- **Change:** include all canonical `SPEC_TEMPLATE.md` sections; §Behavior
  carries the baseline scenario ledger (stable IDs, user-visible wording,
  profile/evidence-tag membership, per-row owner chains), the optional-input
  table, and profile membership/laws; §Test-Grid carries all thirteen issue
  falsifiers in fixed order.
- **Depends on:** Step 1.
- **Verify:** structural heading, scenario-ID-set, profile-vocabulary, and
  falsifier-table checks below; `git diff --check`.

### Step 3: Create the builder/proof contract (this file)

- **File:** `.spec/11371-vim-bdd-journeys/checklist.md`
- **Change:** bounded change order, deterministic structural checking,
  second-run proof, acceptance gates, handoff.
- **Depends on:** Steps 1–2.
- **Verify:** read-only checker runs twice with byte-identical output and no
  tree diff.

## Deterministic structural proof

The repository has no executable `.spec` graph validator and no Gherkin/
feature-status generator on current main (recorded as the ledger evolution in
`context.md`). Do not invent a generated receipt or claim a missing tool
passed. From the candidate worktree, run the following PowerShell check twice
after the files are complete. It enforces: the exact three files; required
canonical headings; required contract terms (substrate identities, profile
names, evidence-chain owners, tag vocabularies, security shape); the exact
thirty scenario IDs bound to their §Behavior ledger rows in fixed family
order; and all thirteen falsifiers with exact scenario/kind/verdict text in
fixed order. Exact-string comparisons are deliberately case-sensitive
(`-cmatch`, `-cne`, `-CaseSensitive`). Its changed-path assertion unions the
candidate patch with unstaged/staged/untracked paths fail-closed and requires
that union to equal the exact three-file set.

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

function Invoke-Spec11371Check {
$root = '.spec/11371-vim-bdd-journeys'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  'e10d186452743beb7b43d2b3427020832f930c2b',
  'dd24cb8e10096c82766143c9fd058105637d72dc',
  'vim-vim-lsp-subject.v1.json', 'vim-vim-lsp-configuration.v1.json',
  'vim-vim-lsp-public-surface.v1.json',
  'perllsp --stdio', 'perl.workspace.includePaths = lib',
  'vim_configuration_documented', 'vim_actual_client_core',
  'vim_first_class_exact_source', 'vim_public_artifact',
  'vim_programme_closeout', 'consumes_if_available',
  '#10938', '#10944', '#10946', '#10951', '#10955', '#10958',
  'editor_client_compat.v1', '#10962', '#10974', '#7122', '#10978',
  '#10960', '#10966', '#10970', '#7712', '#7771', '#7717', '#7702',
  '#11369', '#12050', '#7691', '#7760', '#7762', '#6736', '#4998',
  '#10527', '#7777', '#10858', '#10894', '#8734', '#3983',
  'configuration_documented', 'actual_client', 'not_proven_unsupported',
  'vim_lsp_plugin', 'unknown_not_proven', 'not_proven'
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
foreach ($term in @('vim_configuration_documented', 'vim_actual_client_core', 'vim_first_class_exact_source', 'vim_public_artifact', 'vim_programme_closeout')) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing acceptance profile term: $term" }
}
foreach ($term in @('security-sensitive configuration', 'A stronger profile never erases')) {
  if (-not ($contextText -cmatch [regex]::Escape($term)) -and -not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing boundary term: $term" }
}

# Thirty scenario IDs bound to ledger rows, fixed family order, unique.
$ids = [regex]::Matches($acceptanceText, '(?m)^\|\s*`(?<id>vim\.bdd\.(?:attach|nav|edit|lifecycle|opt)\.\d{2})`\s*\|') |
  ForEach-Object { $_.Groups['id'].Value }
$expectedIds = @(
  'vim.bdd.attach.01','vim.bdd.attach.02','vim.bdd.attach.03','vim.bdd.attach.04','vim.bdd.attach.05','vim.bdd.attach.06','vim.bdd.attach.07',
  'vim.bdd.nav.01','vim.bdd.nav.02','vim.bdd.nav.03','vim.bdd.nav.04','vim.bdd.nav.05','vim.bdd.nav.06',
  'vim.bdd.edit.01','vim.bdd.edit.02','vim.bdd.edit.03','vim.bdd.edit.04','vim.bdd.edit.05',
  'vim.bdd.lifecycle.01','vim.bdd.lifecycle.02','vim.bdd.lifecycle.03','vim.bdd.lifecycle.04','vim.bdd.lifecycle.05',
  'vim.bdd.opt.01','vim.bdd.opt.02','vim.bdd.opt.03','vim.bdd.opt.04','vim.bdd.opt.05','vim.bdd.opt.06','vim.bdd.opt.07'
)
if ($ids.Count -ne 30) { throw "expected exactly thirty scenario ledger rows, found $($ids.Count)" }
if (($ids | Sort-Object -Unique).Count -ne 30) { throw 'scenario IDs are not unique' }
if (($ids -join ',') -cne ($expectedIds -join ',')) { throw "scenario ledger rows do not match the stable ID set in fixed order: found $($ids -join ',')" }

# Thirteen falsifiers: fixed order, exact semantics, non-empty verdicts.
$grid = [regex]::Match($acceptanceText, '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 13) { throw "expected exactly thirteen falsifier rows, found $($rows.Count)" }
$rowIds = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($rowIds | Sort-Object -Unique).Count -ne $rowIds.Count) { throw 'falsifier IDs are not unique' }
if (($rowIds -join ',') -cne ((1..13) -join ',')) { throw 'falsifier IDs are not in fixed order' }
$expectedRows = @(
  @{ id = 1; scenario = 'Wrong sibling/outer root returns the same symbol spelling and passes as root-correct'; kind = 'negative'; verdict = 'reject; root-sensitive answers require the governed root (#7762)' }
  @{ id = 2; scenario = 'Unrelated diagnostic exists while the expected one is absent, presented as pass'; kind = 'negative'; verdict = 'reject; the expected diagnostic itself must appear (attach.06)' }
  @{ id = 3; scenario = 'Completion response exists but vim-lsp did not apply/consume it'; kind = 'negative'; verdict = 'reject; consumption through the client is the proposition (nav.01–02)' }
  @{ id = 4; scenario = 'Literal snippet placeholders survive in the no-snippet buffer'; kind = 'negative'; verdict = 'reject; final plain text must be correct (nav.03)' }
  @{ id = 5; scenario = 'Hover/navigation result is non-empty but semantically wrong'; kind = 'negative'; verdict = 'reject; identity of the answered entity is the proposition (nav.04–06)' }
  @{ id = 6; scenario = 'Rename changes only some occurrences or touches the decoy root'; kind = 'negative'; verdict = 'reject; exactly-the-intended-occurrences/files (edit.01)' }
  @{ id = 7; scenario = 'Format request returns but actual buffer state is wrong or unchanged'; kind = 'negative'; verdict = 'reject; canonical buffer result is the proposition (edit.02)' }
  @{ id = 8; scenario = 'Configuration object exists but has no independent effect'; kind = 'negative'; verdict = 'reject; independent semantic change required (edit.04)' }
  @{ id = 9; scenario = 'Stale pre-edit result appears after an accepted edit'; kind = 'negative'; verdict = 'reject; accepted-generation currentness (lifecycle.03)' }
  @{ id = 10; scenario = 'Non-BMP request lands on an adjacent range'; kind = 'negative'; verdict = 'reject; intended-target resolution (lifecycle.01)' }
  @{ id = 11; scenario = 'Server capability or synthetic peer used instead of actual client traffic'; kind = 'negative'; verdict = 'reject; only actual vim-lsp traffic satisfies actual-host rows' }
  @{ id = 12; scenario = 'Client exit event occurs while perllsp survives'; kind = 'negative'; verdict = 'reject; shutdown leaves no bound process (lifecycle.05)' }
  @{ id = 13; scenario = 'Another client, build, platform, or evidence stage substituted for the pinned subject/stage'; kind = 'negative'; verdict = 'reject; subject and stage non-substitution (F1–F12 substrate)' }
)
for ($i = 0; $i -lt $expectedRows.Count; $i++) {
  $row = $rows[$i]
  $expectedRow = $expectedRows[$i]
  foreach ($field in @('scenario', 'kind', 'verdict')) {
    $actual = $row.Groups[$field].Value.Trim()
    if ($actual -cne $expectedRow[$field]) { throw "falsifier $($expectedRow.id) has unexpected $field" }
  }
}

# Bind the proof to the explicit candidate range.
$candidateBaseRef = 'origin/main'
$candidateHeadRef = 'HEAD'
$candidateBase = (& git rev-parse --verify "$candidateBaseRef^{commit}" 2>&1).Trim()
$candidateHead = (& git rev-parse --verify "$candidateHeadRef^{commit}" 2>&1).Trim()
if ($LASTEXITCODE -ne 0 -or -not $candidateBase -or -not $candidateHead) { throw 'candidate base/HEAD refs are not resolvable' }
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
  '.spec/11371-vim-bdd-journeys/acceptance.md'
  '.spec/11371-vim-bdd-journeys/checklist.md'
  '.spec/11371-vim-bdd-journeys/context.md'
)
if ($changed.Count -ne $expected.Count -or (Compare-Object -CaseSensitive $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_11371_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice with fingerprinted inputs, using
this wrapper around the exact checker body above:

```powershell
function Get-SpecFingerprints {
  $expected = @(
    '.spec/11371-vim-bdd-journeys/acceptance.md'
    '.spec/11371-vim-bdd-journeys/checklist.md'
    '.spec/11371-vim-bdd-journeys/context.md'
  )
  return @($expected | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "missing spec file: $_" }
    "$_=$((Get-FileHash -Algorithm SHA256 -LiteralPath $_ -ErrorAction Stop).Hash)"
  })
}
$ErrorActionPreference = 'Stop'
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("spec-11371-check-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$tmp1 = Join-Path $tmpDir 'run1.out'
$tmp2 = Join-Path $tmpDir 'run2.out'
try {
  $tree1 = @(Get-SpecStatusPaths) -join "`n"
  $fpBefore = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11371Check | Set-Content -LiteralPath $tmp1 -Encoding utf8NoBOM -ErrorAction Stop
  $tree2 = @(Get-SpecStatusPaths) -join "`n"
  $fpBetween = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11371Check | Set-Content -LiteralPath $tmp2 -Encoding utf8NoBOM -ErrorAction Stop
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
'SPEC_11371_SECOND_RUN=PASS'
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'working tree diff --check failed' }
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'staged diff --check failed' }
$expected = @(
  '.spec/11371-vim-bdd-journeys/acceptance.md'
  '.spec/11371-vim-bdd-journeys/checklist.md'
  '.spec/11371-vim-bdd-journeys/context.md'
)
if ((Get-SpecStatusPaths | Where-Object { $_ -cnotin $expected })) { throw 'unexpected spec artifact' }
```

The `Invoke-Spec11371Check` function is the exact command body above, not a
copied output; each invocation rereads the files and revalidates every table.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are changed.
- [ ] All 23 baseline scenarios carry stable IDs, user-visible wording,
      profile/evidence tags, and named downstream owner chains.
- [ ] Optional/stronger rows stay `consumes_if_available`; core stays bounded.
- [ ] All thirteen falsifiers present, fixed order, exact verdict semantics.
- [ ] Security boundary keeps absolute/traversal include paths out of
      positive behavior (#4998).
- [ ] Subject/config/public-surface substrate consumed by reference from the
      merged #12050 artifacts; no second pin.
- [ ] No fixture bytes, host execution, receipt, support-tier change, docs
      prose beyond this packet, CI edit, or upstream action.
- [ ] Deterministic structural proof passes twice; second run byte-clean.

## Callers and consumers

- #10938 binds fixture/oracle cells to these scenario IDs.
- #10944/#10946/#10951/#10955/#10958 bind raw observations to IDs.
- Generic `editor_client_compat.v1` producers, #10962 fan-in, #10974/#7122
  support projection, and #10978 prose cite IDs downstream.
- #10960/#10966/#10970/#7712/#7771/#7717/#7702 own the optional rails.

## Flags for builder

- Scenario IDs are immutable once published downstream; changes route through
  #11371 revision, never silent reuse.
- Behavior wording stays user-visible; implementation trivia belongs to
  #10938 and host leaves.
- If a later leaf can pass only by widening a proposition here, stop and
  return to #11371 instead of editing boundaries locally.
- Deviation note: the controlling issue sketched Gherkin feature files plus
  generated status commands; neither exists on current main, so the journeys
  project into the shipped `.spec` ledger per the evolution record in
  `context.md`.

## Scope boundary

Files IN scope:

- `.spec/11371-vim-bdd-journeys/context.md`
- `.spec/11371-vim-bdd-journeys/acceptance.md`
- `.spec/11371-vim-bdd-journeys/checklist.md`

Files OUT of scope: fixtures, host harnesses/runners, provisioning, server/
client behavior, receipts, support registry, docs prose, CI workflows,
external upstream surfaces, and any new BDD runner infrastructure.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check
output, two-run hash comparison, and `git diff --check` result. Independent
review must challenge whether every behavioral statement traces to the #12050
contract or a named authority, whether evidence boundaries name real owning
issues without duplication, and whether any row smuggles implementation trivia
into specification. A clean review proves no Vim behavior; executable truth
belongs to the downstream leaves, and every scenario remains `not_proven` as
behavior until its exact-host chain passes.
