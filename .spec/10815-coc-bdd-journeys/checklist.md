# Implementation Checklist: #10815 — checked Coc user journeys and evidence boundaries

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any editor/host process.

### Step 1: Create the journey/evidence-boundary context contract

- **File:** `.spec/10815-coc-bdd-journeys/context.md`
- **Change:** record the problem, ledger-format evolution record, consumed
  substrate (#8956 pin authority consumed by reference; registered coc_nvim
  tier), host rail split (including the native-Neovim-LSP exclusion),
  stable host-qualified scenario-ID namespace, journey inventory (42 baseline
  rows across two rails), claim profiles and laws, per-rail evidence chains
  and tag mapping, outcome vocabulary, security boundary, authority split,
  stable-vs-mutable rule, alternatives rejected, prior art, links, scope.
- **Verify:** checker below enforces authority identities, profile names,
  chain owners, and vocabulary terms; `git diff --check`.

### Step 2: Create the normative behavior ledger and falsifiers

- **File:** `.spec/10815-coc-bdd-journeys/acceptance.md`
- **Change:** include all required `SPEC_TEMPLATE.md` sections; §Behavior
  carries both host-rail scenario ledgers (stable IDs, user-visible wording,
  profile/evidence-tag membership, per-row owner chains), the extension-boundary
  table, profile membership/laws, and the allowed non-pass outcome vocabulary;
  §Test-Grid carries all twenty-two normative falsifiers in fixed order.
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
after the files are complete. It reads the exact three packet files and enforces: required
required headings; required contract terms (authority identities, registry
vocabulary, profile names, evidence-chain owners, schema/schema-path terms,
outcome ladder terms); the exact forty-two host-qualified scenario IDs bound
to their §Behavior ledger rows in fixed family order (Vim rail, then Neovim
rail); and all twenty-two falsifiers with exact scenario/kind/verdict text in
fixed order. Exact-string comparisons are deliberately case-sensitive
(`-cmatch`, `-cne`, `-CaseSensitive`). Its changed-path assertion unions the
candidate patch with unstaged/staged/untracked paths fail-closed and requires
that union to equal this two-file repair set (`acceptance.md` and `checklist.md`).
This is not a provenance checker:
it does not resolve Markdown links or inspect source ownership, schema emitters,
validators, adapters, or generated projections. Those claims remain explicitly
bounded in `acceptance.md` and require source/link review or a future executable
owner proof.

```powershell
function Get-SpecStatusPaths {
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = 'git'
  $psi.Arguments = 'status --porcelain=v1 -z --untracked-files=all'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $process = New-Object Diagnostics.Process
  $process.StartInfo = $psi
  if (-not $process.Start()) { throw 'git status process failed to start' }
  $stream = New-Object IO.MemoryStream
  try {
    $process.StandardOutput.BaseStream.CopyTo($stream)
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "git status porcelain failed: $stderr" }
    $bytes = $stream.ToArray()
    while ($bytes.Length -ge 1 -and ($bytes[$bytes.Length - 1] -eq 0x0A -or $bytes[$bytes.Length - 1] -eq 0x0D)) {
      if ($bytes.Length -gt 1) { $bytes = $bytes[0..($bytes.Length - 2)] } else { $bytes = [byte[]]::new(0) }
    }
    $raw = [Text.Encoding]::UTF8.GetString($bytes)
  } finally {
    $stream.Dispose()
    $process.Dispose()
  }
  $records = @($raw -split [char]0 | Where-Object { $_ -ne '' })
  $found = [System.Collections.Generic.List[string]]::new()
  for ($i = 0; $i -lt $records.Count; $i++) {
    $record = [string]$records[$i]
    if ($record.Length -lt 4 -or $record[2] -ne ' ' -or $record.Substring(0,2) -notmatch '^[ MADRCU?!]{2}$') { throw 'malformed porcelain record' }
    $found.Add($record.Substring(3).Replace('\', '/'))
    if ($record.Substring(0,2) -match '[RC]') {
      if ($i + 1 -ge $records.Count -or [string]::IsNullOrEmpty($records[$i + 1])) { throw 'rename/copy record has no source path' }
      $found.Add(([string]$records[++$i]).Replace('\', '/'))
    }
  }
  return @($found)
}

function Invoke-Spec10815Check {
$root = '.spec/10815-coc-bdd-journeys'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  '#8949', '#10658', '#8956', '#10674', '#8962', '#8978', '#8967', '#10717',
  '#8992', '#7122', '#11102', '#11107', '#11125', '#11127',
  '#10685', '#10704', '#10678', '#11112', '#10680', '#10527', '#7777',
  '#10858', '#10894', '#3983', '#4998', '#6736',
  '#7762', '#7743', '#7938', '#8092', '#10019', '#6739',
  '#11302', '#11303', '#11307', '#11309', '#11314', '#11317',
  'perllsp --stdio', 'coc_nvim', 'coc_language_server', 'configuration_documented',
  'requires_actual_client_receipt', 'synthetic_profile',
  'docs/EDITORS/COC_NEOVIM_SETUP.md', 'policy/lsp-client-support.toml',
  'docs/reference/SPEC_TEMPLATE.md', '.ci/schemas/editor-client-compat.v1.schema.json',
  'editor_client_compat.v1', 'not_proven_unsupported',
  'native Neovim LSP', 'client_not_exposed', 'instrument_failed',
  'reporting_failed', 'cleanup_failed'
)
$headings = @('§Behavior', '§Hazards', '§Contracts', '§API-Shape', '§Test-Grid', '§Blast-Radius', '§Coverage-Map')
$text = @($paths | ForEach-Object { [IO.File]::ReadAllText($_, [Text.Encoding]::UTF8) })
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
# The per-rail fan-in owners must appear inside acceptance owner chains, not
# merely anywhere in the bundle.
foreach ($term in @('#10678 operation', '#8967 actual_client_core fan-in', '#10717 actual_client_core fan-in')) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($term))) { throw "missing acceptance chain term: $term" }
}
foreach ($term in @('security-sensitive configuration', 'A stronger profile never erases', 'native Neovim LSP')) {
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

# Twenty-two falsifiers: fixed order (F1-F6 family brief, F7-F22 issue list),
# exact semantics, non-empty verdicts.
$grid = [regex]::Match($acceptanceText, '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 22) { throw "expected exactly twenty-two falsifier rows, found $($rows.Count)" }
$rowIds = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($rowIds | Sort-Object -Unique).Count -ne $rowIds.Count) { throw 'falsifier IDs are not unique' }
if (($rowIds -join ',') -cne ((1..22) -join ',')) { throw 'falsifier IDs are not in fixed order' }
$expectedRows = @(
  @{ id = 1; scenario = 'A core row silently widens into a first_class_coc_host prerequisite'; kind = 'negative'; verdict = 'reject; the core stays bounded and specialized cells join only as consumes_if_available inputs (#10858)' }
  @{ id = 2; scenario = 'Manual textDocument/formatting success is offered as proof of save-triggered behavior'; kind = 'negative'; verdict = 'reject; save-triggered propositions belong to #11102/#8092 and manual formatting never satisfies them' }
  @{ id = 3; scenario = 'A host capability asymmetry passes without an explicit unsupported/not_proven disposition'; kind = 'negative'; verdict = 'reject; asymmetry terminates explicitly inside the owning leaf and is never borrowed from the other rail' }
  @{ id = 4; scenario = 'A Vim + coc.nvim row is satisfied by a Neovim + coc.nvim observation, or the reverse'; kind = 'negative'; verdict = 'reject; host identity is load-bearing and rows never cross rails' }
  @{ id = 5; scenario = 'A scenario ID drifts from the published form or drops its host qualification'; kind = 'negative'; verdict = 'reject; published IDs remain exactly coc.vim.bdd.<family>.<nn> or coc.neovim.bdd.<family>.<nn>' }
  @{ id = 6; scenario = 'A registration event, launch log line, or settings echo stands in for the user-visible result'; kind = 'negative'; verdict = 'reject; the observable editor-side semantic result is the proposition (attach/nav/edit rows)' }
  @{ id = 7; scenario = 'An ambient or wrong perllsp, coc.nvim, Node, or editor subject satisfies an attachment row'; kind = 'negative'; verdict = 'reject; only the exact governed subject/service launch (#8956) satisfies attach rows' }
  @{ id = 8; scenario = 'Native filetype detection is manufactured or asserted without being observed before any override'; kind = 'negative'; verdict = 'reject; observed-before-override is the proposition (attach.01)' }
  @{ id = 9; scenario = 'The outer CWD or a same-named sibling root returns the same symbol spelling and passes as root-correct'; kind = 'negative'; verdict = 'reject; root-sensitive answers require the governed root contract (#8956)' }
  @{ id = 10; scenario = 'Any non-empty diagnostic list is accepted while the expected diagnostic is absent'; kind = 'negative'; verdict = 'reject; the expected diagnostic itself must appear (attach.06)' }
  @{ id = 11; scenario = 'Raw completion succeeds plus an independent snippet insertion, bypassing Coc application'; kind = 'negative'; verdict = 'reject; consumption through coc.nvim is the proposition (nav.01–nav.02)' }
  @{ id = 12; scenario = 'A completion item with only the same label but different kind/text-edit is accepted as applied'; kind = 'negative'; verdict = 'reject; the intended server item identity is the proposition (nav.01)' }
  @{ id = 13; scenario = 'Hover/definition/references return plausible-but-wrong entities, including wrong-project symbols or decoy sites'; kind = 'negative'; verdict = 'reject; entity/site identity is the proposition (nav.03–nav.05)' }
  @{ id = 14; scenario = 'Code action, rename, or format request succeeds while buffer/file state does not reach the exact resulting state'; kind = 'negative'; verdict = 'reject; applied exact state is the proposition (edit.01–edit.03)' }
  @{ id = 15; scenario = 'Configuration presence or log lines are accepted without root-specific semantic effect'; kind = 'negative'; verdict = 'reject; independent semantic effect within the governed root is the proposition (edit.04)' }
  @{ id = 16; scenario = 'An absolute/traversal client include path is used to make a fixture pass as ordinary behavior'; kind = 'negative'; verdict = 'reject; unsafe paths stay governed/rejected per #4998 (edit.05)' }
  @{ id = 17; scenario = 'A Unicode operation targets bytes or lands on the wrong side of an astral character'; kind = 'negative'; verdict = 'reject; character-aligned intended-target resolution (lifecycle.01)' }
  @{ id = 18; scenario = 'Zero captured didChange traffic is interpreted as a synchronization claim'; kind = 'negative'; verdict = 'reject; observed wire edit shape is the proposition (lifecycle.03)' }
  @{ id = 19; scenario = 'A stale generation/result survives an accepted edit and is served as current'; kind = 'negative'; verdict = 'reject; accepted-generation currentness (lifecycle.02)' }
  @{ id = 20; scenario = 'Built-in native Neovim LSP or another service supplies a Neovim + coc.nvim cell'; kind = 'negative'; verdict = 'reject; native Neovim LSP is a distinct subject and never satisfies coc.neovim.bdd rows' }
  @{ id = 21; scenario = 'A host/client shutdown event substitutes for OS process evidence of cleanup'; kind = 'negative'; verdict = 'reject; shutdown leaves no bound child process, observed independently (lifecycle.04)' }
  @{ id = 22; scenario = 'A stale receipt, timeout, missing instrument, or unknown cleanup becomes pass'; kind = 'negative'; verdict = 'reject; instrument/cleanup failure stays an explicit terminal disposition, never silent pass' }
)
for ($i = 0; $i -lt $expectedRows.Count; $i++) {
  $row = $rows[$i]
  $expectedRow = $expectedRows[$i]
  foreach ($field in @('scenario', 'kind', 'verdict')) {
    $actual = $row.Groups[$field].Value.Trim()
    if ($actual -cne $expectedRow[$field]) { throw "falsifier $($expectedRow.id) has unexpected $field" }
  }
}

# Bind the proof to PR #12965's verified base. A moving origin/main may contain
# unrelated work from concurrent lanes; the PR base is the compare authority
# for this bounded repair.
$candidateBaseRef = '1c274cfcc6f5538f00e9b0d725c7be799c5bcd21'
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
)
if ($changed.Count -ne $expected.Count -or (Compare-Object -CaseSensitive $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_10815_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice with fingerprinted inputs, using
this wrapper around the exact checker body above:

```powershell
function Get-SpecSha256([string]$path) {
  $sha = [Security.Cryptography.SHA256]::Create()
  try {
    return ([BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes($path)))).Replace('-', '')
  } finally {
    $sha.Dispose()
  }
}
function Get-SpecFingerprints {
  $expected = @(
    '.spec/10815-coc-bdd-journeys/acceptance.md'
    '.spec/10815-coc-bdd-journeys/checklist.md'
    '.spec/10815-coc-bdd-journeys/context.md'
  )
  return @($expected | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "missing spec file: $_" }
    "$_=$(Get-SpecSha256 $_)"
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
  [IO.File]::WriteAllLines($tmp1, @(Invoke-Spec10815Check), [Text.UTF8Encoding]::new($false))
  $tree2 = @(Get-SpecStatusPaths) -join "`n"
  $fpBetween = @(Get-SpecFingerprints) -join "`n"
  [IO.File]::WriteAllLines($tmp2, @(Invoke-Spec10815Check), [Text.UTF8Encoding]::new($false))
  $tree3 = @(Get-SpecStatusPaths) -join "`n"
  $fpAfter = @(Get-SpecFingerprints) -join "`n"
  if ($tree1 -cne $tree2 -or $tree2 -cne $tree3 -or $fpBefore -cne $fpBetween -or $fpBetween -cne $fpAfter) { throw 'checker changed the spec tree or file contents' }
  foreach ($captured in @($tmp1, $tmp2)) {
    if (-not (Test-Path -LiteralPath $captured -PathType Leaf)) { throw "checker output capture failed: $captured" }
  }
  $h1 = Get-SpecSha256 $tmp1
  $h2 = Get-SpecSha256 $tmp2
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
      rail and Neovim rail stay independently addressable through their own
      #8967/#10717 fan-in owners.
- [ ] No subject digest is recorded; #8956 is consumed by reference as the
      open pin/root authority.
- [ ] Specialized journeys stay outside the core: extension boundary table,
      `consumes_if_available` relation only, no `opt.` IDs minted here.
- [ ] All twenty-two falsifiers present in fixed order (family brief F1–F6,
      issue false-green enumeration F7–F22) with exact verdict semantics:
      profile conflation, save identity collapse, terminal dispositions,
      cross-host relabeling, ID stability, log-line theater, wrong subject,
      manufactured activation, wrong root, non-empty diagnostics, bypassed
      application, same-label item, wrong entity, request-without-state,
      configuration theater, unsafe include path, astral misalignment,
      unobserved wire shape, stale generation, native Neovim substitution,
      shutdown-event substitution, and stale-instrument pass.
- [ ] Allowed non-pass outcomes enumerated against existing vocabularies;
      no Coc-only scalar verdict minted.
- [ ] Security boundary keeps absolute/traversal include paths out of
      positive behavior (#4998).
- [ ] No fixture bytes, host execution, receipt, support-tier change, docs
      prose beyond this packet, CI edit, or upstream action.
- [ ] Deterministic structural proof passes twice; second run byte-clean.

## Callers and consumers

- #11102 becomes spawn-ready against these baseline IDs; new families mint
  under the namespace law through its own revision.
- #10674 binds fixture/expectation cells to these scenario IDs (+#11107
  freshness); #10678/#11112 driver operations bind against named scenarios.
- #10685/#10704 bind raw host-leaf observations to IDs per rail.
- #10680 producers project `editor_client_compat.v1` cells citing these IDs.
- #8967/#10717 compose the per-rail promotable receipts; #8962/#8978 converge
  per-host evidence programs; #11125/#11127 emit host-qualified cells;
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
  `context.md`. This PR intentionally claims only the two-run no-diff
  structural proof below; it does not claim generated BDD/status projections or
  full #10815 acceptance. Keep that issue obligation open until current
  authority defines and executes an equivalent projection path.

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
stay independently addressable including the native Neovim LSP exclusion, and
whether any row smuggles implementation trivia into specification. A clean
review proves no Coc behavior; executable truth belongs to the downstream
leaves, and every scenario remains `not_proven` as behavior until its
exact-host chain passes.
