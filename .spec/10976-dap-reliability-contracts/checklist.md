# Implementation Checklist: #10976 — checked compilation bundle and stable index

## Change order

Specification/data-only change. Reviewable without building Rust tooling; the
only executable proof is the embedded checker below.

### Step 1: Reconcile sources against current main

- Read #10976, #9415, #10558, and the owning issues joined by title catalog;
  re-run the owner scan (`.spec/`, `schemas/`, `docs/specs`).
- Verify: every enumerated number resolves or is recorded in
  `scope_corrections`.

### Step 2: Fail-closed controls first

- Write the falsifier mutations T01–T15 (`acceptance.md` §Test-Grid, fixed
  order) before trusting any positive result.
- Verify: each mutation throws against the real manifest.

### Step 3: Create the stable index

- File: `.spec/10976-dap-reliability-contracts/reliability.manifest.json`
  (CREATE). Seven family views, compiled invariants, 107 contract nodes,
  scope corrections, limitations. No live state anywhere.
- Verify: structural and law checks below; `git diff --check`.

### Step 4: Create context/acceptance views

- Files: `context.md`, `acceptance.md` (CREATE). Derived current facts only,
  canonical section set, fixed-order falsifier grid.
- Verify: heading and term checks below.

### Step 5: Deterministic proof

- Run the embedded checker twice from the candidate worktree root; both runs
  must print byte-identical output including file digests and the canonical
  semantic digest.
- Regenerate `docs/policy/NON_RUST_INVENTORY.md` through the sanctioned command
  only (`cargo xtask non-rust inventory --write`) so new bundle files are
  classified; never hand-edit the snapshot beyond that regen.

## Scope boundary

IN scope: exactly `.spec/10976-dap-reliability-contracts/{context.md,
acceptance.md,checklist.md,reliability.manifest.json}` plus the sanctioned
regeneration of `docs/policy/NON_RUST_INVENTORY.md`.
OUT of scope: code, schemas/, xtask changes, .github/, docs/ edits other than
the inventory snapshot, product behavior, CI routing, GitHub state.

## Embedded deterministic structural checker

From the candidate worktree root extract the script below (between the
powershell fences) into a file outside the repository and run
`pwsh -NoProfile -File <extracted>.ps1`. Culture-sensitive operations must not
affect results; all comparisons are ordinal. The checker asserts:

1. union of committed patch paths, staged, unstaged, and untracked NUL-porcelain
   paths equals exactly the four bundle files plus (optionally)
   `docs/policy/NON_RUST_INVENTORY.md`;
2. hygiene: no BOM, CR, or tabs, exactly one trailing LF in all four files;
3. manifest live-state law: no long hex runs, ISO dates, ref/pull/actions path
   fragments in raw bytes or parsed string values; strictly ascending storage
   of `contract_nodes` by issue;
4. strict structure: exact key sets at every level; closed role/disposition/
   consumer vocabularies; unique stable semantic IDs of form `DAPREL-<issue>`;
   invariant ids belong to their view's naming space; invariant references
   resolve in any compiled view; every invariant covered by at least one node;
   hard dependencies are distinct integers not self-referencing; disposition
   basis non-empty; semantic authority non-empty unless NOT_PROVEN;
   controller/fan_in nodes keep non-empty authority summaries;
5. markdown contracts: required headings/terms present; acceptance Test-Grid
   rows T01..T14 present in fixed order;
6. T01–T14 fail-closed mutation controls all reject (T12 is two-sided:
   rotation preserves the canonical digest while unsorted storage still
   rejects; T13 is population drift; T14 is enforced bidirectionally inside
   the changed-scope check above);
7. prints SHA-256 per file plus the canonical semantic digest; two runs must be
   byte-identical.

```powershell
$ErrorActionPreference = 'Stop'
[Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$root = '.spec/10976-dap-reliability-contracts'
$files = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md", "$root/reliability.manifest.json")
foreach ($p in $files) { if (-not (Test-Path -LiteralPath $p)) { throw "missing bundle file: $p" } }
$bytes = @{}
foreach ($p in $files) { $bytes[$p] = [IO.File]::ReadAllBytes((Join-Path (Get-Location) $p)) }

function Assert-Hygiene([byte[]]$b, [string]$path) {
  if ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) { throw "BOM found: $path" }
  $text = [Text.Encoding]::UTF8.GetString($b)
  if ($text.Contains("`r")) { throw "CR found: $path" }
  if ($text.Contains("`t")) { throw "tab found: $path" }
  if (-not $text.EndsWith("`n") -or $text.EndsWith("`n`n")) { throw "trailing-newline law violated: $path" }
}
foreach ($p in $files) { Assert-Hygiene $bytes[$p] $p }

# --- 1. exact changed-path set: committed patch + index + worktree + untracked ---
$base = git merge-base origin/main HEAD
if ($LASTEXITCODE -ne 0) { throw 'merge-base with origin/main failed' }
$paths = New-Object System.Collections.Generic.List[string]
# (a) committed candidate paths, independent of worktree state
$nameStatus = @(& git diff --name-status $base HEAD)
if ($LASTEXITCODE -ne 0) { throw 'committed diff failed' }
foreach ($line in $nameStatus) {
  if ($line -match '^([RCA])([0-9]*)\t(.+)\t(.+)$') {
    $paths.Add($Matches[4])
  } elseif ($line -match '^([ADM])\t(.+)$') {
    if ($Matches[1] -ne 'D') { $paths.Add($Matches[2]) }
  } elseif ($line.Trim().Length -gt 0) { throw "malformed name-status line: '$line'" }
}
# (b) staged, unstaged, and untracked NUL-porcelain paths
$statusLine = & git status --porcelain=v1 -z --untracked-files=all
if ($LASTEXITCODE -ne 0) { throw 'git status failed' }
$raw = ($statusLine -join '')
$i = 0
while ($i -lt $raw.Length) {
  if ($raw[$i] -eq [char]0) { $i++; continue }
  $rec = $raw.IndexOf([char]0, $i)
  if ($rec -lt 0) { $rec = $raw.Length }
  $entry = $raw.Substring($i, $rec - $i)
  if ($entry.Length -lt 4) { throw "malformed porcelain status entry: '$entry'" }
  $xy = $entry.Substring(0, 2)
  if ($xy[0] -eq [char]'R' -or $xy[0] -eq [char]'C') {
    $rest0 = $entry.Substring(3)
    $tab = $rest0.IndexOf("`t")
    if ($tab -lt 0) { throw 'rename/copy record missing second path' }
    $paths.Add($rest0.Substring($tab + 1))
  } else {
    $paths.Add($entry.Substring(3))
  }
  $i = $rec
}
$allowed = $files + @('docs/policy/NON_RUST_INVENTORY.md')
$unexpected = @($paths | Where-Object { $_ -notin $allowed })
if ($unexpected.Count -gt 0) { throw "unexpected changed/untracked paths: $($unexpected -join ', ')" }
# T14 direction: every required bundle path must appear in the changed set
foreach ($req in $files) {
  if (-not (@($paths | Where-Object { $_ -eq $req }).Count)) { throw "required bundle path absent from committed/status set: $req" }
}

$manifest = Get-Content -LiteralPath "$root/reliability.manifest.json" -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable

# --- 2. live-state law ---
$livePatterns = @('[a-fA-F0-9]{16,}', '\d{4}-\d{2}-\d{2}', 'refs/', 'pull/', 'actions/', 'runs/')
$manifestRaw = [Text.Encoding]::UTF8.GetString($bytes["$root/reliability.manifest.json"])
foreach ($pat in $livePatterns) {
  if ([regex]::IsMatch($manifestRaw, $pat)) { throw "live-state token in raw manifest bytes: $pat" }
}
function Walk-Strings($node, [scriptblock]$fn) {
  if ($null -eq $node) { return }
  if ($node -is [string]) { & $fn $node; return }
  if ($node -is [System.Collections.IDictionary]) { foreach ($v in $node.Values) { Walk-Strings $v $fn }; return }
  if ($node -is [System.Collections.IEnumerable]) { foreach ($v in $node) { Walk-Strings $v $fn }; return }
}
function Invoke-Live-State-Law($M) {
  Walk-Strings $M { param($s) foreach ($pat in $livePatterns) { if ([regex]::IsMatch($s, $pat)) { throw "live-state: $pat in '$($s.Substring(0, [Math]::Min(60, $s.Length)))'" } } }
}
Invoke-Live-State-Law $manifest

# --- 3. strict structure ---
$roles = @('controller','implementation_leaf','evidence_leaf','integration_leaf','cutover_leaf','external_action','fan_in')
$disps = @('SPEC_COMPILED','SPEC_UPDATED','EXISTING_CONTRACT_SUFFICIENT','ISSUE_PLAN_SUFFICIENT','NO_SPEC_DELTA','RETURN_TO_ISSUE','NOT_PROVEN')
$consumers = @('10558','10982','7278','4346','6056')
$rootKeys = @('schema','schema_version','programme','role_vocabulary','disposition_vocabulary','consumer_vocabulary','index_law','family_views','contract_nodes','scope_corrections','limitations')
$viewOrder = @('FAM-LIFECYCLE','FAM-SOURCE','FAM-BREAKPOINT','FAM-INSPECTION','FAM-CAPABILITY','FAM-TRANSPORT','FAM-EVIDENCE')
$suffixOf = @{ 'FAM-LIFECYCLE'='LC'; 'FAM-SOURCE'='SR'; 'FAM-BREAKPOINT'='BP'; 'FAM-INSPECTION'='IN'; 'FAM-CAPABILITY'='CP'; 'FAM-TRANSPORT'='TR'; 'FAM-EVIDENCE'='EV' }
$nodeKeys = @('stable_semantic_id','family_view','issue','train_slot','role','semantic_authority','disposition','disposition_basis','hard_dependency_issues','covered_invariants','consumers')
# Authoritative enumerated decision population (issue-body expansion, reviewed on #10976).
# Semantic revision of this pin must move together with the manifest.
$expectedIssues = @(1742,4346,4786,4973,6056,6680,6684,6688,6694,6949,6952,6991,7187,7206,7276,7278,7310,7337,7338,7339,7340,7341,7342,7343,7344,7345,7346,7347,7348,7363,7364,7366,7486,7565,7566,7567,7568,8045,8172,8354,8368,8564,8581,8591,8602,8615,8624,8635,8656,8668,8687,8691,8703,8707,8974,8981,9021,9035,9042,9045,9046,9048,9050,9051,9054,9057,9059,9064,9065,9069,9074,9081,9522,9527,9528,9529,9530,9531,9532,9533,9534,9535,9536,9537,9538,9568,9570,9578,9581,9765,10524,10563,10564,10565,10566,10567,10736,10745,10752,10759,10765,10774,10782,10789,10797,10891,10926)
function Invoke-Population-Law($M) {
  $have = @($M['contract_nodes'] | ForEach-Object { $_['issue'] })
  if ($have.Count -ne $expectedIssues.Count) { throw "T13 population drift: have $($have.Count) want $($expectedIssues.Count)" }
  $want = @($expectedIssues | Sort-Object)
  for ($k = 0; $k -lt $want.Count; $k++) {
    if ([int]$have[$k] -ne [int]$want[$k]) { throw "T13 population mismatch at rank $k : have $($have[$k]) want $($want[$k])" }
  }
}
$viewKeys = @('view_id','title','compiled_invariants','first_falsifier','compilation_boundary_note','consumers')

function Assert-ExactKeys($obj, [string[]]$keys, [string]$where) {
  $have = @($obj.Keys) | Sort-Object -Property { [string]$_ }
  $want = $keys | Sort-Object -Property { [string]$_ }
  if (($have -join '|') -ne ($want -join '|')) { throw "key-set mismatch at ${where}: have [$($have -join ',')] want [$($want -join ',')]" }
}

function Invoke-Structural-Laws($M) {
  Assert-ExactKeys $M $rootKeys 'root'
  if ($M['schema'] -ne 'dap_reliability_contracts.v1') { throw 'wrong schema id' }
  if ($M['schema_version'] -ne 1) { throw 'wrong schema version' }
  Assert-ExactKeys $M['programme'] @('home_programme','programme_controller_issue','compilation_issue','compilation_bundle','stable_train_consumer_issue','static_validator_issue','method_authority_issue','linked_spec_graph_issue','scope_statement') 'programme'
  if (($M['role_vocabulary'] -join '|') -ne ($roles -join '|')) { throw 'role vocabulary drift' }
  if (($M['disposition_vocabulary'] -join '|') -ne ($disps -join '|')) { throw 'disposition vocabulary drift' }
  if (($M['consumer_vocabulary'] -join '|') -ne ($consumers -join '|')) { throw 'consumer vocabulary drift' }
  if ($M['index_law'].Count -ne 6) { throw 'index_law cardinality drift' }

  $viewsIdx = @{}; $invsIdx = @{}; $orderIdx = 0
  foreach ($v in $M['family_views']) {
    Assert-ExactKeys $v $viewKeys "view $($v['view_id'])"
    $vid = $v['view_id']
    if ($orderIdx -ne $viewOrder.IndexOf($vid)) { throw "family_views out of declared order near $vid" }
    $orderIdx++
    $viewsIdx[$vid] = $true
    if (-not $v['title']) { throw "omission: title $vid" }
    if (-not $v['first_falsifier']) { throw "omission: first_falsifier $vid" }
    if ($null -eq $v['compilation_boundary_note']) { throw "omission: compilation_boundary_note $vid" }
    foreach ($inv in $v['compiled_invariants']) {
      Assert-ExactKeys $inv @('invariant_id','statement') "invariant in $vid"
      $iid = $inv['invariant_id']
      if ($invsIdx.ContainsKey($iid)) { throw "second authority: duplicate invariant $iid" }
      if ($iid -notmatch ('^INV-' + $suffixOf[$vid] + '-\d\d$')) { throw "invariant $iid outside naming space of $vid" }
      $invsIdx[$iid] = $vid
    }
  }
  if ($M['family_views'].Count -ne $viewOrder.Count) { throw 'family view cardinality drift' }

  $seenIds = @{}; $covered = @{}; $prevIssue = -1
  foreach ($nd in $M['contract_nodes']) {
    Assert-ExactKeys $nd $nodeKeys "node $($nd['issue'])"
    if ($nd['stable_semantic_id'] -ne ('DAPREL-' + $nd['issue'])) { throw "semantic id law violated at $($nd['issue'])" }
    if ($seenIds.ContainsKey($nd['stable_semantic_id'])) { throw "duplicate identity $($nd['stable_semantic_id'])" }
    $seenIds[$nd['stable_semantic_id']] = 1
    if (-not ($nd['issue'] -is [int64] -or $nd['issue'] -is [int32]) -or $nd['issue'] -le $prevIssue) { throw "ordering/type violation at node around issue $($nd['issue'])" }
    $prevIssue = $nd['issue']
    if (-not $viewsIdx.ContainsKey($nd['family_view'])) { throw "unknown family view at $($nd['issue'])" }
    if ($nd['role'] -notin $roles) { throw "bad role at $($nd['issue'])" }
    if ($nd['disposition'] -notin $disps) { throw "bad disposition at $($nd['issue'])" }
    foreach ($c in $nd['consumers']) { if ([string]$c -notin $consumers) { throw "out-of-vocabulary consumer at $($nd['issue'])" } }
    if (-not $nd['disposition_basis']) { throw "empty disposition_basis at $($nd['issue'])" }
    if ($nd['disposition'] -ne 'NOT_PROVEN' -and -not $nd['semantic_authority']) { throw "empty semantic_authority at $($nd['issue'])" }
    if ($nd['role'] -in @('controller','fan_in') -and -not $nd['semantic_authority']) { throw "authority-plane node $($nd['issue']) lost its summary" }
    if ($nd['disposition'] -eq 'SPEC_COMPILED' -and @($nd['covered_invariants']).Count -eq 0) { throw "ownerless SPEC_COMPILED at $($nd['issue'])" }
    $depList = @($nd['hard_dependency_issues'])
    if ($depList.Count -ne (@($depList | Sort-Object -Unique)).Count) { throw "duplicate dependency at $($nd['issue'])" }
    foreach ($dep in $depList) {
      if (-not ($dep -is [int64] -or $dep -is [int32])) { throw "non-integer dependency at $($nd['issue'])" }
      if ($dep -eq $nd['issue']) { throw "self dependency at $($nd['issue'])" }
      if ($dep -notin $expectedIssues) { throw "unresolved hard dependency $dep at $($nd['issue'])" }
    }
    foreach ($iid in @($nd['covered_invariants'])) {
      if (-not $invsIdx.ContainsKey($iid)) { throw "unresolved invariant $iid at $($nd['issue'])" }
      $covered[$iid] = 1
    }
  }
  foreach ($k in $invsIdx.Keys) { if (-not $covered.ContainsKey($k)) { throw "orphan authority invariant $k" } }

  $sci = 0
  foreach ($scItem in $M['scope_corrections']) {
    Assert-ExactKeys $scItem @('referenced_numbers','finding','action_required_on') "scope_correction $sci"
    if ($scItem['action_required_on'] -ne 10976) { throw 'scope correction owner drifted' }
    foreach ($rn in $scItem['referenced_numbers']) { if (-not ($rn -is [int64] -or $rn -is [int32])) { throw 'non-integer cited number' } }
    $sci++
  }
  $li = 0
  foreach ($lim in $M['limitations']) { if ($lim -isnot [string] -or -not $lim) { throw "limitation $li not a non-empty string" }; $li++ }
  Invoke-Live-State-Law $M
}
Invoke-Structural-Laws $manifest

function Invoke-Role-Law($M) {
  foreach ($nd in $M['contract_nodes']) {
    if ($nd['issue'] -eq 8591 -and $nd['role'] -notin @('controller','fan_in')) { throw 'role-law: controller demoted to builder leaf' }
  }
}
function Invoke-Invariant-Uniqueness($M) {
  $s = @{}
  foreach ($v in $M['family_views']) {
    foreach ($inv in $v['compiled_invariants']) {
      if ($s.ContainsKey($inv['invariant_id'])) { throw 'second authority: duplicated invariant id across views' }
      $s[$inv['invariant_id']] = 1
    }
  }
}
function Invoke-Consumer-Law($M) {
  foreach ($nd in $M['contract_nodes']) { foreach ($c in $nd['consumers']) { if ([string]$c -notin $consumers) { throw 'out-of-vocabulary consumer' } } }
}
function Invoke-Command-Spelling-Law($M) {
  Walk-Strings $M { param($s) if ($s -match 'cargo xtask|(^|\s)(just)\s|\.github/workflows') { throw 'command spelling used as durable proof obligation' } }
}
function Invoke-View-Omission-Law($M) { foreach ($v in $M['family_views']) { if (-not $v['first_falsifier']) { throw 'omission: empty first_falsifier' } } }
function Invoke-Orphan-Law($M) {
  $all = @{}; $cov2 = @{}
  foreach ($v in $M['family_views']) { foreach ($inv in $v['compiled_invariants']) { $all[$inv['invariant_id']] = 1 } }
  foreach ($nd in $M['contract_nodes']) { foreach ($iid in @($nd['covered_invariants'])) { $cov2[$iid] = 1 } }
  foreach ($k in $all.Keys) { if (-not $cov2.ContainsKey($k)) { throw "orphan authority invariant $k" } }
}
function Invoke-Root-Key-Law($M) {
  foreach ($k in $M.Keys) { if ([string]$k -notin $rootKeys) { throw "root: unknown key added ($k)" } }
  if (@($M.Keys).Count -ne $rootKeys.Count) { throw 'root key cardinality changed' }
}
Invoke-Population-Law $manifest
Invoke-Role-Law $manifest
Invoke-Invariant-Uniqueness $manifest
Invoke-Consumer-Law $manifest
Invoke-Command-Spelling-Law $manifest
Invoke-View-Omission-Law $manifest
Invoke-Orphan-Law $manifest
Invoke-Root-Key-Law $manifest

# --- 4. markdown contracts ---
$ctx = [Text.Encoding]::UTF8.GetString($bytes["$root/context.md"])
$acc = [Text.Encoding]::UTF8.GetString($bytes["$root/acceptance.md"])
foreach ($h in @('# Context:', '## One-PR result', '## Derivation method', '## Compiled summary', '## Rollback / transfer / stop')) {
  if (-not $ctx.Contains($h)) { throw "context.md missing heading/term: $h" }
}
foreach ($h in @('## §Behavior', '## §Hazards', '## §Contracts', '## §API-Shape', '## §Test-Grid', '## Claim boundary', '## Non-goals')) {
  if (-not $acc.Contains($h)) { throw "acceptance.md missing heading: $h" }
}
$rowIdx = 1
foreach ($m in [regex]::Matches($acc, '\| T(\d\d) \|')) {
  if ([int]$m.Groups[1].Value -ne $rowIdx) { throw "test-grid rows out of fixed order near T$rowIdx" }
  $rowIdx++
}
if ($rowIdx -ne 15) { throw "expected 14 test-grid rows, saw $($rowIdx - 1)" }

# --- 5. canonical digest + determinism ---
function Canonicalize($node) {
  if ($null -eq $node) { return '~' }
  if ($node -is [bool]) { return $(if ($node) { '#1' } else { '#0' }) }
  if ($node -is [int64] -or $node -is [int32]) { return "#$node" }
  if ($node -is [string]) { return ('s' + $node.Length + ':' + $node) }
  if ($node -is [System.Collections.IDictionary]) {
    $parts = foreach ($k in (@($node.Keys) | Sort-Object -Property { [string]$_ })) { (Canonicalize ([string]$k)) + '=' + (Canonicalize $node[$k]) }
    return '{' + (@($parts) -join ';') + '}'
  }
  if ($node -is [System.Collections.IEnumerable]) {
    $parts = foreach ($x in $node) { Canonicalize $x }
    return '[' + (@($parts) -join ';') + ']'
  }
  throw "unsupported canonical type: $($node.GetType().FullName)"
}
function Semantic-Digest($doc) {
  $nodesMap = @{}
  foreach ($nd in $doc['contract_nodes']) { $nodesMap[$nd['stable_semantic_id']] = $nd }
  $proj = @{
    schema = $doc['schema']; schema_version = $doc['schema_version']; programme = $doc['programme']
    role_vocabulary = $doc['role_vocabulary']; disposition_vocabulary = $doc['disposition_vocabulary']
    consumer_vocabulary = $doc['consumer_vocabulary']; index_law = $doc['index_law']
    family_views = $doc['family_views']; contract_nodes_map = $nodesMap
    scope_corrections = $doc['scope_corrections']; limitations = $doc['limitations']
  }
  $sha = [Security.Cryptography.SHA256]::Create()
  $canonical = Canonicalize $proj
  $hashBytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($canonical))
  return ([BitConverter]::ToString($hashBytes)) -replace '-', ''
}
$d1 = Semantic-Digest $manifest

# --- 6. fail-closed falsifier mutations T01..T15 ---
function Deep-Copy($node) { return ConvertFrom-Json -InputObject (ConvertTo-Json -InputObject $node -Depth 100) -AsHashtable }
function Expect-Reject([string]$name, [scriptblock]$mutate) {
  $m = Deep-Copy $manifest
  & $mutate $m
  try { Invoke-Structural-Laws $m } catch { return }
  try { Invoke-Role-Law $m } catch { return }
  try { Invoke-Invariant-Uniqueness $m } catch { return }
  try { Invoke-Consumer-Law $m } catch { return }
  try { Invoke-Command-Spelling-Law $m } catch { return }
  try { Invoke-View-Omission-Law $m } catch { return }
  try { Invoke-Orphan-Law $m } catch { return }
  try { Invoke-Root-Key-Law $m } catch { return }
  try { Invoke-Population-Law $m } catch { return }
  throw "negative control did not reject: $name"
}
Expect-Reject 'T01' { param($m) $m['contract_nodes'][0].Remove('disposition_basis') }
Expect-Reject 'T02' { param($m) $m['next_ready_slot'] = 'x' }
Expect-Reject 'T03' { param($m) $m['contract_nodes'][1]['stable_semantic_id'] = $m['contract_nodes'][0]['stable_semantic_id'] }
Expect-Reject 'T04' { param($m) foreach ($nd in $m['contract_nodes']) { if ($nd['issue'] -eq 8591) { $nd['role'] = 'evidence_leaf'; break } } }
Expect-Reject 'T05' { param($m) $clone = Deep-Copy $m['family_views'][0]['compiled_invariants'][0]; $arr2 = @($m['family_views'][2]['compiled_invariants']); $arr2 += $clone; $m['family_views'][2]['compiled_invariants'] = $arr2 }
Expect-Reject 'T06' { param($m) $keep = @(); foreach ($nd in $m['contract_nodes']) { if (@($nd['covered_invariants']) -contains 'INV-EV-03') { continue }; $keep += $nd }; $m['contract_nodes'] = $keep }
Expect-Reject 'T07' { param($m) $m['family_views'][2]['first_falsifier'] = '' }
Expect-Reject 'T08' { param($m) $m['contract_nodes'][10]['covered_invariants'] = @() }
Expect-Reject 'T09' { param($m) $m['contract_nodes'][3]['semantic_authority'] = 'authority abcdef0123456789abcd token' }
Expect-Reject 'T10' { param($m) $m['contract_nodes'][5]['consumers'] = @('99999') }
Expect-Reject 'T11' { param($m) $m['contract_nodes'][7]['disposition_basis'] = 'proof obligation is cargo xtask check tidy' }
Expect-Reject 'T13' { param($m) $keep = @($m['contract_nodes']); $m['contract_nodes'] = @($keep[0..($keep.Count - 2)]) }
Expect-Reject 'T15' { param($m) $m['contract_nodes'][0]['hard_dependency_issues'] = @([int]999999) }
# T12a rotation preserves semantic digest
$rotNodes = @()
$rotNodes += $manifest['contract_nodes'][$manifest['contract_nodes'].Count - 1]
foreach ($nd in $manifest['contract_nodes'][0..($manifest['contract_nodes'].Count - 2)]) { $rotNodes += $nd }
$mRot = Deep-Copy $manifest
$mRot['contract_nodes'] = $rotNodes
if ((Semantic-Digest $mRot) -ne $d1) { throw 'T12 rotation unexpectedly moved the semantic digest' }
# T12b shuffled stored bytes must still reject structural storage-order law
$mShuf = Deep-Copy $manifest
$arrS = @(); $arrS += $mShuf['contract_nodes'][1]; $arrS += $mShuf['contract_nodes'][0]
for ($z = 2; $z -lt @($mShuf['contract_nodes']).Count; $z++) { $arrS += $mShuf['contract_nodes'][$z] }
$mShuf['contract_nodes'] = $arrS
try { Invoke-Structural-Laws $mShuf; throw 'T12 unsorted storage was accepted' } catch { if ("$_" -notmatch 'ordering/type violation') { throw } }

# --- 7. receipts ---
$sha256 = [Security.Cryptography.SHA256]::Create()
foreach ($p in $files) {
  $h = ([BitConverter]::ToString($sha256.ComputeHash($bytes[$p]))) -replace '-', ''
  "FILE  $h  $p"
}
"CANON $d1"
"OK    dap_reliability_contracts.v1 laws hold; T01-T15 all rejected"
```

## Adjacent-defect transfer boundary

Defects discovered in the underlying decisions belong to their owning issues,
not here. Defects in bundle form (key drift, ordering, live-state leakage) are
fixed by semantic revision of this bundle only.

## STOP / RETURN_TO_ISSUE / NOT_PROVEN conditions

- Checker cannot reach byte-identical output twice: stop, NOT_PROVEN.
- Any enumerated decision cannot be placed honestly into a family: RETURN_TO_ISSUE
  row, never a guess (already exercised by the four range-anomaly rows).
- Maintainer chooses a different stable-index location/schema: transfer whole,
  do not fork per-node dispositions silently.
