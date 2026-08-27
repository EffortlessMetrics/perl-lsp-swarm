# Implementation Checklist: #10815 — canonical Coc user journeys and evidence boundaries

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any editor/host process.

### Step 1: Create the journey/evidence-boundary context contract

- **File:** `.spec/10815-coc-bdd-journeys/context.md`
- **Change:** record the problem, ledger-format evolution record, consumed
  substrate (#8956 pin authority consumed by reference; registered coc_nvim
  tier), host rail split, stable host-qualified scenario-ID namespace,
  journey inventory (42 baseline rows across two rails), claim profiles and
  laws, per-rail evidence chains and tag mapping, security boundary,
  authority split, stable-vs-mutable rule, alternatives rejected, prior art,
  links, scope.
- **Verify:** checker below enforces authority identities, profile names,
  chain owners, and vocabulary terms; `git diff --check`.

### Step 2: Create the normative behavior ledger and falsifiers

- **File:** `.spec/10815-coc-bdd-journeys/acceptance.md`
- **Change:** include all canonical `SPEC_TEMPLATE.md` sections; §Behavior
  carries both host-rail scenario ledgers (stable IDs, user-visible wording,
  profile/evidence-tag membership, per-row owner chains), the extension-boundary
  table, and profile membership/laws; §Test-Grid carries all thirteen
  falsifiers in fixed order.
- **Depends on:** Step 1.
- **Verify:** structural heading, scenario-ID-set, profile-vocabulary, and
  falsifier-table checks below; `git diff --check`.

### Step 3: Create the builder/proof contract (this file)

- **File:** `.spec/10815-coc-bdd-journeys/checklist.md`
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
canonical headings; required contract terms (authority identities, registry
vocabulary, profile names, evidence-chain owners, schema/schema-path terms);
the exact forty-two host-qualified scenario IDs bound to their §Behavior
ledger rows in fixed family order (Vim rail, then Neovim rail); and all
thirteen falsifiers with exact scenario/kind/verdict text in fixed order.
Exact-string comparisons are deliberately case-sensitive (`-cmatch`, `-cne`,
`-CaseSensitive`). Its changed-path assertion unions the candidate patch with
unstaged/staged/untracked paths fail-closed and requires that union to equal
the exact three-file set.

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

function Invoke-Spec10815Check {
$root = '.spec/10815-coc-bdd-journeys'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  '#8949', '#10658', '#8956', '#10674', '#8962', '#8978', '#8992', '#7122',
  '#11102', '#11107', '#11125', '#11127', '#10685', '#10704', '#10678', '#11112',
  '#10680', '#10527', '#7777', '#10858', '#10894', '#3983', '#4998',
  '#7762', '#7743', '#7938', '#8092', '#10019', '#6739',
  '#11302', '#11303', '#11307', '#11309', '#11314', '#11317',
  'perllsp --stdio', 'coc_nvim', 'coc_language_server', 'configuration_documented',
  'requires_actual_client_receipt', 'synthetic_profile',
  'docs/EDITORS/COC_NEOVIM_SETUP.md', 'policy/lsp-client-support.toml',
  'docs/reference/SPEC_TEMPLATE.md', '.ci/schemas/editor-client-compat.v1.schema.json',
  'editor_client_compat.v1', 'not_proven_unsupported', 'vim_coc', 'neovim_coc'
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
foreach ($term in @('coc_configuration_documented', 'coc_actual_client_core', 'first_class_coc_host', 'coc_programme_closeout', 'consumes_if_available')) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing acceptance profile term: $term" }
}
foreach ($term in @('security-sensitive configuration', 'A stronger profile never erases')) {
  if (-not ($contextText -cmatch [regex]::Escape($term)) -and -not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing boundary term: $term" }
}

# Forty-two host-qualified scenario IDs bound to ledger rows, fixed family
# order (Vim rail then Neovim rail), unique.
$ids = [regex]::Matches($acceptanceText, '(?m)^\|\s*`(?<id>coc\.(?:vim|neovim)\.bdd\.(?:attach|nav|edit|lifecycle)\.\d{2})`\s*\|') |
  ForEach-Object { $_.Groups['id'].Value }
$expectedIds = @(
  'coc.vim.bdd.attach.01','coc.vim.bdd.attach.02','coc.vim.bdd.attach.03','coc.vim.bdd.attach.04','coc.vim.bdd.attach.05','coc.vim.bdd.attach.06','coc.vim.bdd.attach.07',
  'coc.vim.bdd.nav.01','coc.vim.bdd.nav.02','coc.vim.bdd.nav.03','coc.vim.bdd.nav.04','coc.vim.bdd.nav.05',
  'coc.vim.bdd.edit.01','coc.vim.bdd.edit.02','coc.vim.bdd.edit.03','coc.vim.bdd.edit.04','coc.vim.bdd.edit.05',
  'coc.vim.bdd.lifecycle.01','coc.vim.bdd.lifecycle.02','coc.vim.bdd.lifecycle.03','coc.vim.bdd.lifecycle.04',
  'coc.neovim.bdd.attach.01','coc.neovim.bdd.attach.02','coc.neovim.bdd.attach.03','coc.neovim.bdd.attach.04','coc.neovim.bdd.attach.05','coc.neovim.bdd.attach.06','coc.neovim.bdd.attach.07',
  'coc.neovim.bdd.nav.01','coc.neovim.bdd.nav.02','coc.neovim.bdd.nav.03','coc.neovim.bdd.nav.04','coc.neovim.bdd.nav.05',
  'coc.neovim.bdd.edit.01','coc.neovim.bdd.edit.02','coc.neovim.bdd.edit.03','coc.neovim.bdd.edit.04','coc.neovim.bdd.edit.05',
  'coc.neovim.bdd.lifecycle.01','coc.neovim.bdd.lifecycle.02','coc.neovim.bdd.lifecycle.03','coc.neovim.bdd.lifecycle.04'
)
if ($ids.Count -ne 42) { throw "expected exactly forty-two scenario ledger rows, found $($ids.Count)" }
if (($ids | Sort-Object -Unique).Count -ne 42) { throw 'scenario IDs are not unique' }
if (($ids -join ',') -cne ($expectedIds -join ',')) { throw "scenario ledger rows do not match the stable ID set in fixed order: found $($ids -join ',')" }

# Thirteen falsifiers: fixed order, exact semantics, non-empty verdicts.
$grid = [regex]::Match($acceptanceText, '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 13) { throw "expected exactly thirteen falsifier rows, found $($rows.Count)" }
$rowIds = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($rowIds | Sort-Object -Unique).Count -ne $rowIds.Count) { throw 'falsifier IDs are not unique' }
if (($rowIds -join ',') -cne ((1..13) -join ',')) { throw 'falsifier IDs are not in fixed order' }
$expectedRows = @(
  @{ id = 1; scenario = 'A core row silently widens into a first_class_coc_host prerequisite'; kind = 'negative'; verdict = 'reject; the core stays bounded and specialized cells join only as consumes_if_available inputs (#10858)' }
  @{ id = 2; scenario = 'Manual textDocument/formatting success is offered as proof of save-triggered behavior'; kind = 'negative'; verdict = 'reject; save-triggered propositions belong to #11102/#8092 and manual formatting never satisfies them' }
  @{ id = 3; scenario = 'A host capability asymmetry passes without an explicit unsupported/not_proven disposition'; kind = 'negative'; verdict = 'reject; asymmetry terminates explicitly inside the owning leaf' }
  @{ id = 4; scenario = 'A Vim + coc.nvim row is satisfied by a Neovim + coc.nvim observation, or the reverse'; kind = 'negative'; verdict = 'reject; host identity is load-bearing and rows never cross hosts' }
  @{ id = 5; scenario = 'A scenario ID drifts from the published form or drops its host qualification'; kind = 'negative'; verdict = 'reject; published IDs must remain exactly of the form coc.<host>.bdd.<family>.<nn>' }
  @{ id = 6; scenario = 'A registration event or server log line is presented as the user-visible result'; kind = 'negative'; verdict = 'reject; the observable editor-side result is the proposition' }
  @{ id = 7; scenario = 'Wrong sibling/outer root returns the same symbol spelling and passes as root-correct'; kind = 'negative'; verdict = 'reject; root-sensitive answers require the governed root contract (#8956)' }
  @{ id = 8; scenario = 'A substitute coc.nvim build/copy or service mutation satisfies attachment'; kind = 'negative'; verdict = 'reject; only the exact governed subject/service launch (#8956) satisfies attach rows' }
  @{ id = 9; scenario = 'Completion/action response exists but Coc did not apply it, or literal snippet placeholders survive'; kind = 'negative'; verdict = 'reject; client application through coc.nvim is the proposition (nav rows)' }
  @{ id = 10; scenario = 'Rename applies fewer or more occurrences/files than intended'; kind = 'negative'; verdict = 'reject; complete-intended-edit-only is the proposition (edit rows)' }
  @{ id = 11; scenario = 'Formatting diverges from canonical output or a second pass changes bytes again'; kind = 'negative'; verdict = 'reject; canonical idempotent result is the proposition (edit rows)' }
  @{ id = 12; scenario = 'A post-edit answer reflects pre-edit state, or wire edit shape is inferred instead of observed'; kind = 'negative'; verdict = 'reject; accepted-generation currentness and observed wire shape are propositions (lifecycle rows)' }
  @{ id = 13; scenario = 'A non-BMP operation lands on an adjacent range'; kind = 'negative'; verdict = 'reject; intended-target resolution is the proposition (lifecycle rows)' }
)
for ($i = 0; $i -lt $expectedRows.Count; $i++) {
  $row = $rows[$i]
  $expectedRow = $expectedRows[$i]
  foreach ($field in @('scenario', 'kind', 'verdict')) {
    $actual = $row.Groups[$field].Value.Trim()
    if ($actual -cne $expectedRow[$field]) { throw "falsifier $($expectedRow.id) has unexpected $field" }
  }
}

# Bind the proof to the explicit candidate range. Concurrent lanes move
# origin/main freely, so the base anchors at the merge-base with HEAD: with a
# conflict-free candidate on a fast-forwarding main that is exactly the
# branch-point commit, independent of when other lanes land.
$candidateBaseRef = 'origin/main'
$candidateHeadRef = 'HEAD'
$candidateBase = (& git merge-base $candidateBaseRef $candidateHeadRef 2>&1 | Select-Object -Last 1)
$candidateHead = (& git rev-parse --verify "$candidateHeadRef^{commit}" 2>&1).Trim()
if ($LASTEXITCODE -ne 0 -or -not $candidateBase -or -not $candidateHead) { throw 'candidate base/HEAD refs are not resolvable' }
& git merge-base --is-ancestor $candidateBase $candidateHeadRef 2>&1 | Out-Null
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
  '.spec/10815-coc-bdd-journeys/acceptance.md'
  '.spec/10815-coc-bdd-journeys/checklist.md'
  '.spec/10815-coc-bdd-journeys/context.md'
)
if ($changed.Count -ne $expected.Count -or (Compare-Object -CaseSensitive $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_10815_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice with fingerprinted inputs, using
this wrapper around the exact checker body above:

```powershell
function Get-SpecFingerprints {
  $expected = @(
    '.spec/10815-coc-bdd-journeys/acceptance.md'
    '.spec/10815-coc-bdd-journeys/checklist.md'
    '.spec/10815-coc-bdd-journeys/context.md'
  )
  return @($expected | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "missing spec file: $_" }
    "$_=$((Get-FileHash -Algorithm SHA256 -LiteralPath $_ -ErrorAction Stop).Hash)"
  })
}
$ErrorActionPreference = 'Stop'
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("spec-10815-check-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$tmp1 = Join-Path $tmpDir 'run1.out'
$tmp2 = Join-Path $tmpDir 'run2.out'
try {
  $tree1 = @(Get-SpecStatusPaths) -join "`n"
  $fpBefore = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec10815Check | Set-Content -LiteralPath $tmp1 -Encoding utf8NoBOM -ErrorAction Stop
  $tree2 = @(Get-SpecStatusPaths) -join "`n"
  $fpBetween = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec10815Check | Set-Content -LiteralPath $tmp2 -Encoding utf8NoBOM -ErrorAction Stop
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
'SPEC_10815_SECOND_RUN=PASS'
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'working tree diff --check failed' }
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'staged diff --check failed' }
$expected = @(
  '.spec/10815-coc-bdd-journeys/acceptance.md'
  '.spec/10815-coc-bdd-journeys/checklist.md'
  '.spec/10815-coc-bdd-journeys/context.md'
)
if ((Get-SpecStatusPaths | Where-Object { $_ -cnotin $expected })) { throw 'unexpected spec artifact' }
```

The `Invoke-Spec10815Check` function is the exact command body above, not a
copied output; each invocation rereads the files and revalidates every table.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are changed.
- [ ] All 42 baseline scenarios carry host-qualified stable IDs, user-visible
      wording, profile/evidence tags, and named downstream owner chains; Vim
      rail and Neovim rail stay independently addressable.
- [ ] No subject digest is recorded; #8956 is consumed by reference as the
      open pin/root authority.
- [ ] Specialized journeys stay outside the core: extension boundary table,
      `consumes_if_available` relation only, no `opt.` IDs minted here.
- [ ] All thirteen falsifiers present, fixed order, exact verdict semantics,
      including profile conflation, save identity collapse, terminal
      dispositions, cross-host relabeling, ID stability, and log-line theater.
- [ ] Security boundary keeps absolute/traversal include paths out of
      positive behavior (#4998).
- [ ] No fixture bytes, host execution, receipt, support-tier change, docs
      prose beyond this packet, CI edit, or upstream action.
- [ ] Deterministic structural proof passes twice; second run byte-clean.

## Callers and consumers

- #11102 becomes spawn-ready against these baseline IDs; new families mint
  under the namespace law through its own revision.
- #10674 binds fixture/oracle cells to these scenario IDs (+#11107 freshness).
- #10678/#11112 driver operations bind against named scenarios downstream.
- #10685/#10704 bind raw observations to IDs; #8962/#8978 converge per-host
  evidence; #11125/#11127 emit host-qualified cells.
- #10680 producers project `editor_client_compat.v1` cells citing these IDs;
  #8992/#7122 support projection cites IDs downstream.

## Flags for builder

- Scenario IDs are immutable once published downstream; changes route through
  #10815 revision, never silent reuse.
- Behavior wording stays user-visible; implementation trivia belongs to
  #10674 and host leaves.
- If a later leaf can pass only by widening a proposition here, stop and
  return to #10815 instead of editing boundaries locally.
- Deviation note: the controlling issue sketched Gherkin feature files plus
  generated status commands; neither exists on current main, so the journeys
  project into the shipped `.spec` ledger per the evolution record in
  `context.md`.

## Scope boundary

Files IN scope:

- `.spec/10815-coc-bdd-journeys/context.md`
- `.spec/10815-coc-bdd-journeys/acceptance.md`
- `.spec/10815-coc-bdd-journeys/checklist.md`

Files OUT of scope: fixtures, host harnesses/runners, provisioning, server/
client behavior, receipts, support registry, docs prose, CI workflows,
external upstream surfaces, any new BDD runner infrastructure, and the
specialized #11102 journeys.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check
output, two-run hash comparison, and `git diff --check` result. Independent
review must challenge whether every behavioral statement traces to a named
authority above (including open #8956 as pure reference), whether evidence
boundaries name real owning issues without duplication, whether host rails
stay independently addressable, and whether any row smuggles implementation
trivia into specification. A clean review proves no Coc behavior; executable
truth belongs to the downstream leaves, and every scenario remains
`not_proven` as behavior until its exact-host chain passes.
