# Implementation Checklist: #11764 — canonical stable issue_controller_train.v1 topology graph

## Change order

This is a specification/data-only change. Each step is reviewable without
building or executing any tooling beyond the embedded checker.

### Step 1: Write the fail-closed fixtures first

- **File:** the negative-control suite inside the checker below (DESIGN).
- **Change:** Before the manifest is declared valid, the fifteen falsifier
  mutations (malformed node, duplicate authority/conflict identity, live-state
  bytes, class collapse, order sensitivity, missing revision owner) must exist
  as in-memory mutations that each make validation throw.
- **Verify:** temporarily running the control suite against a deliberately
  weakened validator shows a control failing to reject; against the real
  validator all fifteen reject.

### Step 2: Create the stable manifest

- **File:** `.spec/11764-controller-train-graph/train.manifest.json` (CREATE)
- **Change:** Encode the complete 26-node graph (programme controller CTRL
  #11681, S00 #11763, C01–C06 #11682–#11687, R05B #11785, T-series, I-series,
  P/D-series) with typed, provenance-traced edges, conflict keys,
  dispositions, controls, exits, rollback quartets, successors and identity
  fields.
- **Verify:** structural and law checks below; `git diff --check`.

### Step 3: Create the context contract

- **File:** `.spec/11764-controller-train-graph/context.md` (CREATE)
- **Change:** Record the problem, authority, consumed laws, encoding
  traceability decisions, `AGENTS.md` compatibility, open decisions respected
  (not decided), adoption/rollback/transfer/stop, links.
- **Verify:** heading and term checks below; `git diff --check`.

### Step 4: Create acceptance and negative controls

- **File:** `.spec/11764-controller-train-graph/acceptance.md` (CREATE)
- **Change:** All canonical `SPEC_TEMPLATE.md` sections and the fifteen issue
  falsifiers in fixed order with exact kind/verdict semantics, plus the claim
  boundary and non-goals.
- **Verify:** section and falsifier-order checks below; `git diff --check`.

### Step 5: Create the builder and proof contract (this file)

- **File:** `.spec/11764-controller-train-graph/checklist.md` (CREATE)
- **Change:** This change order, the embedded deterministic structural
  checker, the second-run proof, the `NOT_PROVEN` boundary, and
  rollback/transfer/stop.
- **Verify:** the embedded checker runs twice from the candidate worktree
  with byte-identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/11764-controller-train-graph/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no
`.claude/workflows/` change, no `docs/` change, no code, no configuration, no
generated artifact outside the bundle, no GitHub state.

## Deterministic structural proof

The repository has no executable issue-controller validator (T02 #11765 owns
the independent one). Do not invent a generated receipt or claim a missing
tool passed. From the candidate worktree root, run the following PowerShell 7
checker twice after the four files are complete. The checker asserts:

1. the union of the committed candidate patch
   (`merge-base(origin/main, HEAD)..HEAD`, which stays the candidate's own
   patch even if `origin/main` advances mid-flight because a sibling lane
   fetched), the staged index, the unstaged worktree, and NUL-delimited
   porcelain paths — including untracked files — equals exactly the four
   bundle paths (it fails closed on a malformed status record or a
   rename/copy record without its second path);
2. the manifest bytes are hygiene-clean (no BOM, no CR, no tabs, exactly one
   trailing LF) and contain no live-state tokens anywhere (long lowercase hex
   runs, timestamps, branch/PR path fragments), in both raw bytes and parsed
   values;
3. the manifest parses under a strict schema: exact key sets at every level
   (unknown keys fail closed), exact expected node/issue pairs for all 26
   nodes, unique node IDs, issues, aliases, conflict keys and authority-after
   propositions, title fingerprints recomputed, dependency classes from the
   four-value vocabulary, every dependency target resolvable, successor sets
   exactly the derived reverse-edge set, no cycles over hard/evidence edges,
   controller non-buildability, R05B's explicit external authorization
   dependency, proof/fan-in repair bounds, I01's old-heuristic exit, and all
   fifteen graph-law edges present with exactly their declared classes;
4. all fifteen falsifiers of `acceptance.md` §Test-Grid reject through
   sixteen fail-closed in-memory mutation controls (falsifier 5 carries two:
   class reclassification and a duplicate edge under a conflicting class),
   including the order-invariance control whose rejected subject is an
   order-sensitive canonicalization: the canonical semantic digest of a
   shuffled document equals the digest of the original;
   schema-identifier comparison is ordinal (case-sensitive) and all
   culture-sensitive operations run under the invariant culture;
5. the bundle markdown carries its canonical headings/terms and exactly
   fifteen fixed-order rejected falsifier rows;
6. a SHA-256 fingerprint over the four files and the semantic digest are
   printed; two runs must print byte-identical output.

Redirecting output to a temporary file is local proof only; no temporary file
belongs in the PR.

```powershell
$ErrorActionPreference = 'Stop'
# Determinism across locales: every culture-sensitive operation below (sorting,
# string comparison, formatting) must behave identically on any host.
[Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$root = '.spec/11764-controller-train-graph'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md", "$root/train.manifest.json")
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

# --- 2. deterministic byte hygiene of the manifest ---
$manifestPath = $paths[3]
$manifestBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $manifestPath))
if ($manifestBytes.Length -ge 3 -and $manifestBytes[0] -eq 0xEF -and $manifestBytes[1] -eq 0xBB -and $manifestBytes[2] -eq 0xBF) { throw 'manifest has UTF-8 BOM' }
$manifestText = [Text.Encoding]::UTF8.GetString($manifestBytes)
if ($manifestText.Contains("`r")) { throw 'manifest contains CR bytes' }
if ($manifestText.Contains("`t")) { throw 'manifest contains tab bytes' }
if (-not $manifestText.EndsWith("`n") -or $manifestText.EndsWith("`n`n")) { throw 'manifest must end with exactly one LF' }

# --- schema constants ---
$TOP_KEYS = @('schema','schema_version','programme','authority_planes','train_role_vocabulary',
  'evidence_semantics','external_authorities','open_decisions_routed_elsewhere','nodes',
  'supersessions','revision_governance','limitations')
$NODE_KEYS = @('node_id','issue','title','title_fingerprint','aliases','train_role','lane','chain',
  'one_pr_outcome','authority_before','authority_after','buildable','dependencies','claim_ceiling',
  'writer','consumed_authorities','allowed_components','forbidden_adjacent_owners','spec',
  'first_falsifier','controls','proof','review_forward','obligations','exits','rollback',
  'successors','identity_fields','limitations')
$CONTROL_KEYS = @('positive','opposite','stale','wrong_subject','fault','mutation')
$OBLIGATION_KEYS = @('schema','generated','docs','changelog','receipt')
$EXIT_KEYS = @('old_path','compatibility','supersession','transfer')
$ROLLBACK_KEYS = @('rollback','return_to_issue','not_proven','stop')
$DEP_KEYS = @('target','class','provenance')
$CLASSES = @('hard','evidence','optional','external')
$DISPOSITIONS = @('SPEC_COMPILED','EXISTING_CONTRACT_SUFFICIENT','ISSUE_PLAN_SUFFICIENT',
  'CONTROLLER_NO_CODING_SPEC','FAN_IN_OR_CERTIFICATION_SPEC','EXTERNAL_OR_MANUAL_NO_CODING_SPEC',
  'RETURN_TO_ISSUE','NOT_PROVEN')
$PLANES = @('stable train contract','semantic train revision','current-tree implementation state',
  'offline readiness/frontier','exact-tree context','live collaboration/candidate state',
  'exact-head proof/review closeout','live GitHub metadata state','behavior/proof/support/external truth')
$ROLES = @('controller','specification','stable_contract','validator','current_tree_probe',
  'offline_frontier','context_projection','live_observer','packet_adapter','implementation',
  'proof','fan_in','integration','external_gate','dogfood')
$EXPECTED_NODES = [ordered]@{
  'C01'=11682; 'C02'=11683; 'C03'=11684; 'C04'=11685; 'C05'=11686; 'C06'=11687
  'CTRL'=11681; 'D01'=11781; 'D02'=11782; 'I01'=11777; 'I02'=11778
  'P01'=11779; 'P02'=11783; 'R05B'=11785; 'S00'=11763
  'T01'=11764; 'T02'=11765; 'T02R'=11767; 'T02S'=11774; 'T03'=11769
  'T04'=11771; 'T05'=11772; 'T06'=11773; 'T07'=11775; 'T08'=11776; 'T08C'=11784
}
$OPEN_DECISION_OWNERS = @(@('OD1','C02',11683),@('OD2','R05B',11785),@('OD3','C03',11684),@('OD4','I01',11777),@('OD5','T08C',11784))
# Graph-law edges (from #11764 initial graph laws and the #11681 dependency graph).
# Each entry must exist as a dependency edge with exactly this class.
$LAW_EDGES = @(
  @('S00','T01','hard'), @('T01','T02','hard'), @('T02','T02R','hard'),
  @('T02R','T03','hard'), @('T02R','T02S','hard'), @('T02S','T04','evidence'),
  @('T03','T04','hard'), @('T04','T05','hard'), @('T04','T06','hard'),
  @('T04','T08C','hard'), @('T05','T07','hard'), @('T06','T07','hard'),
  @('T07','T08','hard'), @('T08C','T08','hard'),
  @('C01','C02','hard'), @('C01','C03','hard'), @('C02','C04','hard'),
  @('C03','C04','hard'), @('C04','C05','hard'), @('T02R','C05','hard'),
  @('C05','C06','hard'), @('C05','R05B','hard'),
  @('C04','I01','hard'), @('T07','I01','hard'), @('T08','I01','hard'), @('I01','I02','hard'),
  @('C01','P01','hard'), @('C02','P01','hard'), @('C03','P01','hard'), @('C04','P01','hard'),
  @('C05','P01','hard'), @('C06','P01','hard'), @('I01','P01','hard'), @('I02','P01','hard'),
  @('T08','P01','hard'),
  @('P01','D01','hard'), @('D01','D02','hard'), @('D02','P02','hard'),
  @('C01','P02','hard'), @('C02','P02','hard'), @('C03','P02','hard'), @('C04','P02','hard'),
  @('C05','P02','hard'), @('C06','P02','hard'), @('I01','P02','hard'), @('I02','P02','hard'),
  @('P01','P02','hard'), @('D01','P02','hard'), @('T08','P02','hard')
)

function Assert-KeySet {
  param($Object, [string[]]$Expected, [string]$Where)
  $actual = @($Object.PSObject.Properties.Name)
  $missing = @($Expected | Where-Object { $_ -cnotin $actual })
  $extra = @($actual | Where-Object { $_ -cnotin $Expected })
  if ($missing.Count -or $extra.Count) {
    throw "key set mismatch at ${Where}: missing=[$($missing -join ',')] extra=[$($extra -join ',')]"
  }
}
function New-OrdinalTable { [System.Collections.Hashtable]::new([StringComparer]::Ordinal) }
function Assert-IsString { param($Value, [string]$Where) if (-not ($Value -is [string])) { throw "expected a JSON string at ${Where}" } }
function Assert-IsList { param($Value, [string]$Where) if (-not ($Value -is [System.Collections.IList])) { throw "expected a JSON array at ${Where}" } }
function Assert-IsObject { param($Value, [string]$Where) if (-not ($Value -is [psobject]) -or $Value -is [System.Collections.IList]) { throw "expected a JSON object at ${Where}" } }
function Assert-NonEmpty {
  param($Value, [string]$Where)
  if ([string]::IsNullOrWhiteSpace([string]$Value)) { throw "empty required value at $Where" }
}

function Get-NodeSha256Fingerprint {
  param([string]$Text)
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try {
    $hash = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text))
    return ([BitConverter]::ToString($hash) -replace '-', '').Substring(0, 16)
  } finally { $sha.Dispose() }
}

# Recursive live-state scan over every string value in the document.
function Assert-NoLiveStateStrings {
  param($Value, [string]$Where)
  if ($null -eq $Value) { return }
  if ($Value -is [string]) {
    if ($Value -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { throw "possible live SHA/state token at ${Where}: $($Matches[0])" }
    if ($Value -match '\d{4}-\d{2}-\d{2}T') { throw "possible live timestamp at ${Where}" }
    foreach ($tok in @('origin/', 'refs/heads/', 'pull/', 'PR #', 'merge-base', 'worktrees/')) {
      if ($Value.Contains($tok)) { throw "possible live-state token '${tok}' at ${Where}" }
    }
    return
  }
  if ($Value -is [System.Collections.IList]) {
    for ($i = 0; $i -lt $Value.Count; $i++) { Assert-NoLiveStateStrings $Value[$i] "${Where}[${i}]" }
    return
  }
  if ($Value -is [psobject]) {
    foreach ($prop in $Value.PSObject.Properties) {
      Assert-NoLiveStateStrings $prop.Value "${Where}.$($prop.Name)"
    }
  }
}

# Canonical semantic digest: pure function of content, independent of map/array input order.
function Get-CanonicalDigest {
  param($Value)
  $sb = [System.Text.StringBuilder]::new()
  function Walk($v, [System.Text.StringBuilder]$b) {
    if ($null -eq $v) { [void]$b.Append('n;'); return }
    if ($v -is [bool]) { [void]$b.Append("b:$v;"); return }
    if ($v -is [long] -or $v -is [int] -or $v -is [double]) { [void]$b.Append("i:$v;"); return }
    if ($v -is [string]) { [void]$b.Append('s:'); [void]$b.Append(($v -replace '\\', '\\\\' -replace ';', '\\;')); [void]$b.Append(';'); return }
    if ($v -is [System.Collections.IList]) {
      [void]$b.Append('[')
      $parts = @()
      for ($i = 0; $i -lt $v.Count; $i++) {
        $inner = [System.Text.StringBuilder]::new()
        Walk $v[$i] $inner
        $parts += $inner.ToString()
      }
      foreach ($p in ($parts | Sort-Object)) { [void]$b.Append($p) }
      [void]$b.Append(']')
      return
    }
    if ($v -is [psobject]) {
      [void]$b.Append('{')
      foreach ($name in (@($v.PSObject.Properties.Name) | Sort-Object)) {
        [void]$b.Append($name); [void]$b.Append('=')
        Walk $v.$name $b
      }
      [void]$b.Append('}')
      return
    }
    throw "unwalkable value type: $($v.GetType().FullName)"
  }
  Walk $Value $sb
  $sha = [System.Security.Cryptography.SHA256]::Create()
  try {
    $hash = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($sb.ToString()))
    return ([BitConverter]::ToString($hash) -replace '-', '')
  } finally { $sha.Dispose() }
}

function Invoke-TrainValidation {
  param($doc)
  Assert-KeySet $doc $TOP_KEYS 'manifest root'

  if ($doc.schema -cne 'issue_controller_train.v1') { throw 'schema name mismatch' }
  if ($doc.schema_version -cne 1) { throw 'schema_version must be 1' }

  # programme block
  Assert-KeySet $doc.programme @('controller_issue','home_programme','durable_architecture_issue','durable_architecture_bundle','method_authority') 'programme'
  if ($doc.programme.controller_issue -cne 11681) { throw 'programme controller_issue mismatch' }
  if ($doc.programme.home_programme -cne 'issue-controllers') { throw 'home programme mismatch' }
  if ($doc.programme.durable_architecture_issue -cne 11763) { throw 'durable architecture issue mismatch' }

  # authority planes: exactly nine, fixed order
  if (@($doc.authority_planes).Count -ne 9) { throw 'expected exactly 9 authority planes' }
  for ($i = 0; $i -lt 9; $i++) {
    Assert-KeySet $doc.authority_planes[$i] @('plane','owns','never_substitutes') "authority_planes[$i]"
    if ($doc.authority_planes[$i].plane -cne $PLANES[$i]) { throw "authority plane order broken at $($i + 1)" }
    Assert-NonEmpty $doc.authority_planes[$i].owns "authority_planes[$i].owns"
    Assert-NonEmpty $doc.authority_planes[$i].never_substitutes "authority_planes[$i].never_substitutes"
  }

  # train role vocabulary: exactly fifteen, fixed order, no issue-role vocabulary fusion
  if (@($doc.train_role_vocabulary).Count -ne 15) { throw 'expected exactly 15 train roles' }
  for ($i = 0; $i -lt 15; $i++) {
    Assert-KeySet $doc.train_role_vocabulary[$i] @('role','owns') "train_role_vocabulary[$i]"
    if ($doc.train_role_vocabulary[$i].role -cne $ROLES[$i]) { throw "train role order broken at $($i + 1)" }
    Assert-NonEmpty $doc.train_role_vocabulary[$i].owns "train_role_vocabulary[$i].owns"
  }

  # evidence semantics
  Assert-KeySet $doc.evidence_semantics @('not_proven_law','optional_visibility') 'evidence_semantics'
  Assert-NonEmpty $doc.evidence_semantics.not_proven_law 'evidence_semantics.not_proven_law'
  Assert-NonEmpty $doc.evidence_semantics.optional_visibility 'evidence_semantics.optional_visibility'

  # external authorities
  $authIds = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($a in @($doc.external_authorities)) {
    Assert-KeySet $a @('id','subject') 'external_authorities[]'
    if (-not $a.id.StartsWith('#')) { throw "external authority id must start with '#': $($a.id)" }
    if (-not $authIds.Add($a.id)) { throw "duplicate external authority: $($a.id)" }
    Assert-IsString $a.subject 'external_authorities[].subject'
    Assert-NonEmpty $a.subject 'external_authorities[].subject'
  }
  foreach ($needed in @('#10858','#10872','#10881','#10554','#11114','#3983','#3949','#4177','#3982','#3957','#EXPLICIT-AUTHORIZATION')) {
    if (-not $authIds.Contains($needed)) { throw "required external authority missing: $needed" }
  }

  # open decisions routed elsewhere: exactly five, exact owners, not decided here
  if (@($doc.open_decisions_routed_elsewhere).Count -ne 5) { throw 'expected exactly 5 open decisions' }
  for ($i = 0; $i -lt 5; $i++) {
    $od = $doc.open_decisions_routed_elsewhere[$i]
    Assert-KeySet $od @('id','subject','owning_node','owning_issue') "open_decisions[$i]"
    if ($od.id -cne $OPEN_DECISION_OWNERS[$i][0]) { throw "open decision id order broken at $($i + 1)" }
    if ($od.owning_node -cne $OPEN_DECISION_OWNERS[$i][1]) { throw "open decision $($od.id) owning-node mismatch" }
    if ($od.owning_issue -cne $OPEN_DECISION_OWNERS[$i][2]) { throw "open decision $($od.id) owner-issue mismatch" }
    Assert-IsString $od.subject "open_decisions[$i].subject"
    Assert-NonEmpty $od.subject "open_decisions[$i].subject"
    Assert-NonEmpty $od.owning_node "open_decisions[$i].owning_node"
  }

  # revision governance
  Assert-KeySet $doc.revision_governance @('owner_node','owner_issue','invalidates','never') 'revision_governance'
  if ($doc.revision_governance.owner_node -cne 'T02R' -or $doc.revision_governance.owner_issue -cne 11767) {
    throw 'revision governance must be owned by T02R #11767'
  }
  Assert-NonEmpty $doc.revision_governance.invalidates 'revision_governance.invalidates'
  Assert-NonEmpty $doc.revision_governance.never 'revision_governance.never'

  # supersession registry shape is checked here; registry resolution against
  # the node maps happens after the maps are built below.

  # nodes: exact expected set, uniqueness, per-node contract completeness
  $EXPECTED_ISSUES = [System.Collections.Generic.HashSet[long]]::new()
  foreach ($v in $EXPECTED_NODES.Values) { [void]$EXPECTED_ISSUES.Add([long]$v) }
  $EXPECTED_IDS = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($k in $EXPECTED_NODES.Keys) { [void]$EXPECTED_IDS.Add($k) }
  $nodes = @($doc.nodes)
  if ($nodes.Count -ne 26) { throw "expected exactly 26 nodes, found $($nodes.Count)" }
  $byId = New-OrdinalTable
  $seenIssues = New-OrdinalTable
  $seenAliases = New-OrdinalTable
  $seenKeys = New-OrdinalTable
  $seenAuthorityAfter = New-OrdinalTable
  foreach ($n in $nodes) {
    Assert-KeySet $n $NODE_KEYS "node $($n.node_id)"
    $id = [string]$n.node_id
    if ($byId.ContainsKey($id)) { throw "duplicate node_id: $id" }
    $byId[$id] = $n
    if (-not $EXPECTED_IDS.Contains($id)) { throw "unexpected node_id: $id" }
    if ($EXPECTED_NODES[$id] -cne $n.issue) { throw "node/issue mismatch: $id -> $($n.issue)" }
    if ($seenIssues.ContainsKey([string]$n.issue)) { throw "duplicate issue assignment: $($n.issue)" }
    $seenIssues[[string]$n.issue] = $id
    foreach ($alias in @($n.aliases)) {
      if ($seenAliases.ContainsKey([string]$alias)) { throw "duplicate alias: $alias" }
      $seenAliases[[string]$alias] = $id
    }
    if ($seenKeys.ContainsKey([string]$n.writer.conflict_key)) { throw "duplicate conflict key: $($n.writer.conflict_key)" }
    $seenKeys[[string]$n.writer.conflict_key] = $id
    if ($seenAuthorityAfter.ContainsKey([string]$n.authority_after)) { throw "duplicate authority-after proposition between $($seenAuthorityAfter[[string]$n.authority_after]) and $id" }
    $seenAuthorityAfter[[string]$n.authority_after] = $id
  }
  foreach ($id in $EXPECTED_IDS) {
    if (-not $byId.ContainsKey($id)) { throw "missing expected node: $id" }
  }

  # supersession registry resolved against the node/issue maps
  $seenSuperseded = New-OrdinalTable
  foreach ($s in @($doc.supersessions)) {
    if (-not $byId.ContainsKey([string]$s.superseded_node)) { throw "supersession names unknown node: $($s.superseded_node)" }
    if ($seenSuperseded.ContainsKey([string]$s.superseded_node)) { throw "duplicate supersession for node: $($s.superseded_node)" }
    $seenSuperseded[[string]$s.superseded_node] = $true
    if (-not $EXPECTED_ISSUES.Contains([long]$s.successor_issue)) { throw "supersession names unknown successor issue: $($s.successor_issue)" }
    if ([long]$s.successor_issue -eq [long]$byId[[string]$s.superseded_node].issue) { throw "successor issue must differ from the superseded node's own issue" }
  }

  foreach ($n in $nodes) {
    $id = [string]$n.node_id
    # JSON shape: exact types before content checks (scalars must not pose as
    # arrays/objects and vice versa)
    foreach ($sf in @('node_id','title','title_fingerprint','train_role','lane','one_pr_outcome',
                      'authority_before','authority_after','claim_ceiling','first_falsifier')) {
      Assert-IsString $n.$sf "node $id $sf"
    }
    Assert-IsList $n.aliases "node $id aliases"
    Assert-IsList $n.dependencies "node $id dependencies"
    Assert-IsList $n.consumed_authorities "node $id consumed_authorities"
    Assert-IsList $n.allowed_components "node $id allowed_components"
    Assert-IsList $n.forbidden_adjacent_owners "node $id forbidden_adjacent_owners"
    Assert-IsList $n.successors "node $id successors"
    Assert-IsList $n.identity_fields "node $id identity_fields"
    Assert-IsList $n.limitations "node $id limitations"
    foreach ($of in @('chain','writer','spec','controls','proof','review_forward','obligations','exits','rollback')) {
      Assert-IsObject $n.$of "node $id $of"
    }
    if (-not ($n.buildable -is [bool])) { throw "node $id buildable must be a JSON boolean" }
    if (-not ($n.issue -is [long] -or $n.issue -is [int])) { throw "node $id issue must be a JSON number" }
    Assert-NonEmpty $n.title "node $id title"
    $expectedFp = Get-NodeSha256Fingerprint ([string]$n.title)
    if ([string]$n.title_fingerprint -cne $expectedFp) { throw "title fingerprint mismatch at $id" }
    if ($n.train_role -cnotin $ROLES) { throw "unknown train role at ${id}: $($n.train_role)" }
    Assert-KeySet $n.chain @('home','controller') "node $id chain"
    if ($n.chain.controller -cne 'CTRL') { throw "node $id chain controller mismatch" }
    Assert-KeySet $n.writer @('conflict_key','parallel_group','stack_relation') "node $id writer"
    Assert-NonEmpty $n.writer.conflict_key "node $id conflict_key"
    Assert-KeySet $n.spec @('disposition','owner','stale_policy') "node $id spec"
    if ($n.spec.disposition -cnotin $DISPOSITIONS) { throw "unknown disposition at ${id}: $($n.spec.disposition)" }
    if ($n.spec.owner -cne $id) { throw "node $id spec owner must be itself" }
    Assert-NonEmpty $n.spec.stale_policy "node $id stale_policy"
    Assert-KeySet $n.controls $CONTROL_KEYS "node $id controls"
    foreach ($k in $CONTROL_KEYS) { Assert-NonEmpty $n.controls.$k "node $id controls.$k" }
    Assert-KeySet $n.proof @('focused','routed') "node $id proof"
    Assert-NonEmpty $n.proof.focused "node $id proof.focused"
    Assert-KeySet $n.review_forward @('questions','lenses') "node $id review_forward"
    if (@($n.review_forward.questions).Count -lt 1) { throw "node $id lacks review questions" }
    if (@($n.review_forward.lenses).Count -lt 1) { throw "node $id lacks review lenses" }
    Assert-KeySet $n.obligations $OBLIGATION_KEYS "node $id obligations"
    foreach ($k in $OBLIGATION_KEYS) { Assert-NonEmpty $n.obligations.$k "node $id obligations.$k" }
    Assert-KeySet $n.exits $EXIT_KEYS "node $id exits"
    Assert-KeySet $n.rollback $ROLLBACK_KEYS "node $id rollback"
    foreach ($k in $ROLLBACK_KEYS) { Assert-NonEmpty $n.rollback.$k "node $id rollback.$k" }
    Assert-NonEmpty $n.one_pr_outcome "node $id one_pr_outcome"
    Assert-NonEmpty $n.authority_before "node $id authority_before"
    Assert-NonEmpty $n.authority_after "node $id authority_after"
    Assert-NonEmpty $n.claim_ceiling "node $id claim_ceiling"
    Assert-NonEmpty $n.first_falsifier "node $id first_falsifier"
    if (@($n.identity_fields).Count -lt 1) { throw "node $id lacks identity_fields" }
    if (@($n.allowed_components).Count -lt 1) { throw "node $id lacks allowed_components" }
    if (@($n.forbidden_adjacent_owners).Count -lt 1) { throw "node $id lacks forbidden_adjacent_owners" }
    foreach ($a in @($n.consumed_authorities)) {
      Assert-IsString $a "node $id consumed authority entry"
      if (-not $authIds.Contains($a)) { throw "node $id consumes unknown authority: $a" }
    }
    # dependencies: exactly one edge per target (two edges to one target with
    # different classes is an ambiguous identity, not a richer contract)
    $depTargets = New-OrdinalTable
    foreach ($d in @($n.dependencies)) {
      Assert-IsObject $d "node $id dependency"
      Assert-KeySet $d $DEP_KEYS "node $id dependency"
      foreach ($df in @('target','class','provenance')) { Assert-IsString $d.$df "node $id dependency $df" }
      Assert-NonEmpty $d.provenance "node $id dependency provenance"
      if ($d.provenance -cnotmatch '^#\d+ body (references|fan-in consumers)$' -and
          $d.provenance -cnotin @('S00 plan.md node row','S00 plan.md ordering boundaries','S00 plan.md programme shape','#11681 dependency graph')) {
        throw "unrecognized provenance at ${id} -> $($d.target): $($d.provenance)"
      }
      if ($d.class -cnotin $CLASSES) { throw "unknown dependency class at ${id}: $($d.class)" }
      $t = [string]$d.target
      if ($depTargets.ContainsKey($t)) { throw "node $id carries more than one dependency edge to target ${t}: conflicting identities" }
      $depTargets[$t] = $true
      if ($t.StartsWith('#')) {
        if (-not $authIds.Contains($t)) { throw "node $id depends on unknown authority: $t" }
      } else {
        if (-not $byId.ContainsKey($t)) { throw "node $id depends on unknown node: $t" }
        if ($t -eq $id) { throw "node $id self-dependency" }
      }
    }
  }

  # controller/fan-in/external-gate law: they never enter ordinary builder
  # frontiers; every other node is one ordinary reviewable one-PR proposition
  foreach ($n in $nodes) {
    $role = [string]$n.train_role
    if ($role -cin @('controller','fan_in','external_gate')) {
      if ($n.buildable) { throw "${role} node $($n.node_id) must not be buildable" }
    } else {
      if (-not $n.buildable) { throw "${role} node $($n.node_id) must be buildable as one one-PR proposition" }
    }
  }
  if (-not ($byId['CTRL'].train_role -ceq 'controller')) { throw 'CTRL must carry train role controller' }

  # external gate law: R05B requires the explicit external authorization dependency
  $r05bAuth = @($byId['R05B'].dependencies | Where-Object { $_.target -ceq '#EXPLICIT-AUTHORIZATION' -and $_.class -ceq 'external' })
  if ($r05bAuth.Count -ne 1) { throw 'R05B must carry exactly one external #EXPLICIT-AUTHORIZATION dependency' }

  # proof/fan-in never repair product work
  foreach ($n in $nodes) {
    if ($n.train_role -cin @('proof','fan_in')) {
      if ([string]$n.claim_ceiling -notmatch 'repair') { throw "proof/fan_in node $($n.node_id) must bound product repair in claim ceiling" }
    }
  }

  # generic entry cutover must keep its old-heuristic exit
  if ([string]$byId['I01'].exits.old_path -notmatch 'heuristic') { throw 'I01 must keep an explicit old-heuristic retirement exit' }

  # successors must be exactly the derived reverse edge set
  $derived = New-OrdinalTable
  foreach ($id in $EXPECTED_NODES.Keys) { $derived[$id] = [System.Collections.Generic.List[string]]::new() }
  foreach ($n in $nodes) {
    foreach ($d in @($n.dependencies)) {
      if (-not ([string]$d.target).StartsWith('#')) { $derived[[string]$d.target].Add([string]$n.node_id) }
    }
  }
  foreach ($n in $nodes) {
    $id = [string]$n.node_id
    $actual = @($n.successors | Sort-Object -Unique)
    $want = @($derived[$id] | Sort-Object -Unique)
    if (($actual -join ',') -cne ($want -join ',')) { throw "successor set mismatch at ${id}: actual=[$($actual -join ',')] derived=[$($want -join ',')]" }
  }

  # graph-law edges exist with exactly the declared class
  foreach ($le in $LAW_EDGES) {
    $from = $le[0]; $to = $le[1]; $cls = $le[2]
    $edge = @($byId[$to].dependencies | Where-Object { $_.target -ceq $from })
    if ($edge.Count -eq 0) { throw "graph-law edge missing: ${from} -> ${to}" }
    $matched = @($edge | Where-Object { $_.class -ceq $cls })
    if ($matched.Count -ne 1) { throw "graph-law edge class mismatch: ${from} -> ${to} must be ${cls}" }
  }

  # acyclicity over hard/evidence node edges
  $colour = @{}
  foreach ($id in $EXPECTED_NODES.Keys) { $colour[$id] = 0 }
  function Visit([string]$id) {
    if ($colour[$id] -eq 1) { throw "dependency cycle detected at $id" }
    if ($colour[$id] -eq 2) { return }
    $colour[$id] = 1
    foreach ($d in @($byId[$id].dependencies)) {
      $t = [string]$d.target
      if (-not $t.StartsWith('#') -and $d.class -cin @('hard','evidence')) { Visit $t }
    }
    $colour[$id] = 2
  }
  foreach ($id in $EXPECTED_NODES.Keys) { Visit $id }

  # no live state anywhere in the document
  Assert-NoLiveStateStrings $doc 'root'

  return (Get-CanonicalDigest $doc)
}

# --- parse and validate ---
$doc = ConvertFrom-Json (Get-Content -Raw -LiteralPath $manifestPath)
$semanticDigest = Invoke-TrainValidation $doc

# raw-file live-state scan (mirrors the document scan over exact bytes)
foreach ($line in ($manifestText -split "`n")) {
  if ($line -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { throw "possible live SHA token in manifest bytes: $($Matches[0])" }
  if ($line -match '\d{4}-\d{2}-\d{2}T') { throw 'possible live timestamp in manifest bytes' }
}

# --- negative controls: every mutation must fail closed ---
function Copy-Doc { param($d) (ConvertTo-Json $d -Depth 100) | ConvertFrom-Json }
$controls = [System.Collections.Generic.List[string]]::new()
function Invoke-NegativeControl {
  param([string]$Name, [scriptblock]$Mutation)
  $copy = Copy-Doc $doc
  & $Mutation $copy
  try {
    Invoke-TrainValidation $copy | Out-Null
    throw "negative control FAILED to reject: $Name"
  } catch [System.Management.Automation.RuntimeException] {
    if ($_.Exception.Message -like "negative control FAILED*") { throw }
  }
  $controls.Add($Name)
}
function Set-Prop { param($o, [string]$n, $v) $o.PSObject.Properties.Remove($n); $o | Add-Member -MemberType NoteProperty -Name $n -Value $v }
function Find-Node { param($d, [string]$id) @($d.nodes | Where-Object { $_.node_id -ceq $id })[0] }
function Find-Dep { param($n, [string]$t) @($n.dependencies | Where-Object { $_.target -ceq $t })[0] }

# falsifier 1: controller emitted as ordinary implementation
Invoke-NegativeControl 'F01-controller-buildable' { param($d) (Find-Node $d 'CTRL').buildable = $true }
# falsifier 2: issue role and train node role collapse
Invoke-NegativeControl 'F02-issue-role-collapse' { param($d) Set-Prop (Find-Node $d 'T02') 'issue_role' 'controller' }
# falsifier 3: two active nodes own the same authority-after proposition
Invoke-NegativeControl 'F03-duplicate-authority-after' { param($d) (Find-Node $d 'T03').authority_after = (Find-Node $d 'T02').authority_after }
# falsifier 4: two incompatible writer/conflict identities
Invoke-NegativeControl 'F04-duplicate-conflict-key' { param($d) (Find-Node $d 'D01').writer.conflict_key = (Find-Node $d 'T02').writer.conflict_key }
# falsifier 5: hard/evidence/optional/external dependencies collapse
Invoke-NegativeControl 'F05-law-edge-reclassified' { param($d) (Find-Dep (Find-Node $d 'C02') 'C01').class = 'optional' }
Invoke-NegativeControl 'F05b-duplicate-edge' { param($d) (Find-Node $d 'C02').dependencies += [pscustomobject]@{ target = 'C01'; class = 'optional'; provenance = '#11683 body references' } }
# falsifier 6: current SHA/PR/check/model/writer state enters stable bytes
Invoke-NegativeControl 'F06-live-state-bytes' { param($d) (Find-Node $d 'T03').limitations += ' rebased onto head 4f5bcb334 deadbeef 00ff11' }
# falsifier 7: label/navigation application treated as product readiness
Invoke-NegativeControl 'F07-labels-as-readiness' { param($d) Set-Prop (Find-Node $d 'C03') 'labels_applied' $true }
# falsifier 8: proof/fan-in can repair missing product work
Invoke-NegativeControl 'F08-proof-repair-authority' { param($d) (Find-Node $d 'P01').claim_ceiling = 'proves the composed denominator' }
# falsifier 9: generic entry cutover has no old-heuristic exit
Invoke-NegativeControl 'F09-cutover-without-exit' { param($d) (Find-Node $d 'I01').exits.old_path = 'none' }
# falsifier 10: node lacks first falsifier, review question, rollback or stop boundary
Invoke-NegativeControl 'F10-missing-falsifier' { param($d) (Find-Node $d 'T05').first_falsifier = ' ' }
# falsifier 11: superseded/transferred node loses unique work or exact successor
Invoke-NegativeControl 'F11-supersession-without-successor' { param($d) $d.supersessions = @([pscustomobject]@{ superseded_node = 'T08'; reason = 'replaced' }) }
# falsifier 12: optional/unavailable/instrument-failed rows disappear
Invoke-NegativeControl 'F12-evidence-semantics-removed' { param($d) $d.PSObject.Properties.Remove('evidence_semantics') }
# falsifier 13: path order becomes semantic dependency order
Invoke-NegativeControl 'F13-source-paths-injected' { param($d) Set-Prop (Find-Node $d 'C01') 'source_paths' @('crates/perl-lsp-rs/src/a.rs') }
# falsifier 14: canonical serialization changes with map/input order
$orderDigest = $semanticDigest
$shuffled = Copy-Doc $doc
$shuffled.nodes = @($shuffled.nodes | Sort-Object -Property issue -Descending)
foreach ($n in @($shuffled.nodes)) {
  $n.dependencies = @($n.dependencies | Sort-Object -Property class -Descending)
  $n.successors = @($n.successors | Sort-Object -Descending)
  $n.identity_fields = @($n.identity_fields | Sort-Object -Descending)
}
$shuffledDigest = Invoke-TrainValidation $shuffled
if ($shuffledDigest -cne $orderDigest) { throw 'F14 canonical digest changed with input order' }
$controls.Add('F14-order-canonical-digest')
# falsifier 15: a future semantic graph change has no revision/invalidation owner
Invoke-NegativeControl 'F15-revision-owner-removed' { param($d) $d.nodes = @($d.nodes | Where-Object { $_.node_id -cne 'T02R' }) }

# Sixteen fail-closed mutation controls cover the fifteen falsifiers; falsifier 5
# (dependency-class collapse) carries two controls: reclassifying a frozen
# graph-law edge, and duplicating one edge under a conflicting class.
if ($controls.Count -ne 16) { throw "expected 16 negative controls, ran $($controls.Count)" }

# --- bundle markdown structure ---
function Get-SectionBody {
  param([string]$Document, [string]$HeadingPattern)
  $match = [regex]::Match($Document, "(?ms)^${HeadingPattern}\s*\r?\n(?<body>.*?)(?=^#{1,3}\s|\z)")
  if (-not $match.Success) { throw "missing contract section: $HeadingPattern" }
  return $match.Groups['body'].Value
}
$contextText = Get-Content -Raw -LiteralPath $paths[0]
$acceptanceText = Get-Content -Raw -LiteralPath $paths[1]
foreach ($h in @('## Problem','## Why this approach','## Current state \(honest, as of this bundle\)',
                 '## Authority and ownership','## Durable laws consumed','## Encoding decisions and traceability',
                 '## Compatibility with the repository operating contract \(`AGENTS.md`\)',
                 '## Open decisions respected, not decided','## Adoption, rollback, transfer and stop','## Links')) {
  if (-not ($contextText -match "(?m)^${h}\s*$")) { throw "missing context heading: $h" }
}
foreach ($term in @('issue_controller_train.v1','`.spec/11763-issue-controller-architecture/`','#11681',
                    'authority planes','train roles','not_proven','OD1','OD5','#EXPLICIT-AUTHORIZATION',
                    'no readiness command','no scheduler')) {
  if (-not ($contextText -match [regex]::Escape($term))) { throw "missing context contract term: $term" }
}
foreach ($h in @('## §Behavior','## §Hazards','## §Contracts','## §API-Shape','## §Test-Grid','## §Blast-Radius')) {
  if (-not ($acceptanceText -match "(?m)^$([regex]::Escape($h))\s*$")) { throw "missing acceptance section: $h" }
}
$testGrid = Get-SectionBody $acceptanceText '## §Test-Grid'
$falsifierRows = @($testGrid -split "`r?`n" | Where-Object { $_ -match '^\|\s*\d+\s*\|' })
if ($falsifierRows.Count -ne 15) { throw "expected exactly 15 falsifier rows, found $($falsifierRows.Count)" }
for ($i = 0; $i -lt 15; $i++) {
  if ($falsifierRows[$i] -notmatch ("^\|\s*$($i + 1)\s*\|")) { throw "falsifier order broken at row $($i + 1)" }
  if ($falsifierRows[$i] -notmatch [regex]::Escape('rejected')) { throw "falsifier row $($i + 1) lacks a rejected verdict" }
}
foreach ($term in @('issue_controller_train.v1','#11763','#11681','#11682','#11785','train.manifest.json')) {
  if (-not ($acceptanceText -match [regex]::Escape($term))) { throw "missing acceptance contract term: $term" }
}

# --- deterministic fingerprints over the four bundle files ---
$sha2 = [System.Security.Cryptography.SHA256]::Create()
$allBytes = [System.Collections.Generic.List[byte]]::new()
foreach ($p in $paths) {
  $fileBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $p))
  $allBytes.AddRange($fileBytes)
}
$fingerprint = [BitConverter]::ToString($sha2.ComputeHash($allBytes.ToArray())) -replace '-', ''
$sha2.Dispose()
Write-Output "SPEC_11764_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_11764_NEGATIVE_CONTROLS=16/16"
Write-Output "SPEC_11764_SEMANTIC_SHA256=$semanticDigest"
Write-Output "SPEC_11764_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice. Requirements for a valid proof:

1. both runs print `SPEC_11764_STRUCTURAL_CHECK=PASS`;
2. both runs print `SPEC_11764_NEGATIVE_CONTROLS=16/16`;
3. both runs print the same `SPEC_11764_SEMANTIC_SHA256` and
   `SPEC_11764_BUNDLE_SHA256` fingerprints;
4. the full captured output of both runs is byte-identical;
5. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
6. `git diff --check` (staged) is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves manifest shape, node-set completeness, edge
typing and graph-law freezing, uniqueness laws, controller/gate laws,
durable-byte hygiene, fail-closed behavior of all fifteen falsifiers,
order-invariant canonicalization, and byte-level determinism across two runs.
It does **not** prove: that the topology is the semantically correct reading
of every leaf body (that is this PR's review job, and T02R's after); that any
later tooling works (T02's validator is unbuilt); that the graph stays
current as issues evolve (T02R owns invalidation); or that the manifest is
accepted by an independent validator (none exists yet). The repository's
absent executable issue-controller validator remains an open tooling gap
recorded here rather than papered over.

## Flags for builder

None. The manifest is complete as compiled; T02, T02R and T02S own all later
per-node elaboration through their own issues.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor manifest version supersedes this one only through
  a T02R-classified revision with an exact successor recorded; derived
  artifacts are re-derived, never patched valid.
- **Stop:** stop before validator commands, current-tree probes, frontier,
  source-context resolution, live observation, packet rendering, GitHub
  metadata work, exact-head checkers, dogfood, scheduling, support claims,
  release or publication. If an open decision OD1–OD5 is needed as a decision
  rather than a boundary, stop and route it to its owning node; do not decide
  it in a builder PR.
