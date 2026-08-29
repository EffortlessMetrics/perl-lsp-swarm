# Implementation Checklist: #11770 — Emacs semantic train revision governance ledger

## Change order

This is a specification/data-only change. Each step is reviewable without
building or executing any tooling beyond the embedded checker.

### Step 1: Write the fail-closed fixtures first

- **File:** the negative-control suite inside the checker below (DESIGN).
- **Change:** Before the ledger is declared valid, the fourteen required
  revision regressions must exist as in-memory mutations that each make
  validation throw (regression 14 carries both a parsed-value and a raw-byte
  control), the acceptance-bullet mutation classes beyond the numbered list
  (sequence tampering, unknown semantic class, permitted automatic
  execution, kind misclassification, wiring undercoverage, incorporation
  claiming a node, adoption-rule drift) must carry their own controls, plus
  an order-invariance canonicalization control — twenty-three controls in
  total.
- **Verify:** temporarily weakening a law shows a control failing to reject;
  against the real validator all twenty-three reject.

### Step 2: Create the revision ledger

- **File:** `.spec/11770-emacs-train-revisions/revisions.ledger.json` (CREATE)
- **Change:** Encode the `emacs_train_revision.v1` schema — five revision
  kinds, the twenty-three-value shared semantic-class vocabulary of
  #11770/#11375, thirteen invalidation surfaces, six sync actions, fifteen
  ledger laws — and the ten frozen material movements as append-only entries
  with identity preservation, wiring and typed invalidations.
- **Verify:** structural, cross-validation and law checks below;
  `git diff --check`.

### Step 3: Create the context contract

- **File:** `.spec/11770-emacs-train-revisions/context.md` (CREATE)
- **Change:** Record the problem, authority, consumed laws, encoding
  traceability decisions, `AGENTS.md` compatibility, open decisions
  respected (not decided), adoption/rollback/transfer/stop, links.
- **Verify:** heading and term checks below; `git diff --check`.

### Step 4: Create acceptance and negative controls

- **File:** `.spec/11770-emacs-train-revisions/acceptance.md` (CREATE)
- **Change:** All canonical `SPEC_TEMPLATE.md` sections and the fourteen
  revision regressions in fixed order with exact kind/verdict semantics,
  plus the claim boundary and non-goals.
- **Verify:** section and falsifier-order checks below; `git diff --check`.

### Step 5: Create the builder and proof contract (this file)

- **File:** `.spec/11770-emacs-train-revisions/checklist.md` (CREATE)
- **Change:** This change order, the embedded deterministic structural
  checker, the second-run procedure, the `not_proven` boundary, and
  rollback/transfer/stop.
- **Verify:** the embedded checker runs twice from the candidate worktree
  with byte-identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/11770-emacs-train-revisions/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no `.spec/10918-emacs-train-graph/` change (the consumed manifest stays immutable historical evidence), no `crates/` or `xtask/` change, no `docs/` change, no configuration, no generated artifact outside the bundle, no GitHub state, no host execution.

## Deterministic structural proof

The repository has no executable Emacs train validator (the xtask operations named by #11770 remain a separate tooling claim). Do not invent a generated receipt or claim a missing tool passed. From the candidate worktree root, run the following PowerShell 7 checker twice after the four files are complete. The checker asserts:

1. the union of the committed candidate patch
   (`merge-base(origin/main, HEAD)..HEAD`, which stays the candidate's own
   patch even if `origin/main` advances mid-flight because a sibling lane
   fetched), the staged index, the unstaged worktree, and NUL-delimited
   porcelain paths — including untracked files — equals exactly the four
   bundle paths (it fails closed on a malformed status record or a
   rename/copy record without its second path);
2. all four bundle files are hygiene-clean (no BOM, no CR, no tabs, exactly
   one trailing LF) and the ledger contains no live-state tokens anywhere
   (long lowercase hex runs, timestamps, branch/PR path fragments), in both
   raw bytes and parsed values;
3. the ledger parses under a strict schema: exact key sets at every level
   (unknown keys fail closed), fixed-order vocabularies (five kinds,
   twenty-three semantic classes, thirteen invalidation surfaces, six sync
   actions), fifteen ledger laws present, per-kind graph-effect and subject
   shapes, unique ordinal append-only entries, and the frozen ten-movement
   coverage table;
4. every ledger reference resolves against the consumed `emacs_train.v1`
   manifest (node ids, issue references and external authority ids), every
   decompose entry's successor set equals the subject's hard dependency
   children minus declared retained prerequisites (no silent node drop, no
   phantom child), every split child reaches the parent fan-in through its
   manifest successor set, every acceptance cell maps to exactly one
   declared owner or explicit retirement, every unique work item drains to
   a declared owner or explicit retirement, insert wiring matches manifest
   edges with exact classes and covers exactly the subject manifest
   dependents, the incorporation stays a separate train, the
   retarget after-state matches the manifest bytes including the canonical
   adoption rule, metadata-only entries invalidate nothing, material
   entries carry typed invalidations and exact-controller synchronization,
   and every entry forbids automatic execution;
5. all fourteen revision regressions reject through twenty-three fail-closed
   in-memory mutation controls (regression 14 carries a value-level and a
   byte-level control; the acceptance-bullet mutation classes carry four
   more), including the order-invariance control whose rejected subject is
   an order-sensitive canonicalization: the canonical ledger digest of a
   document with shuffled unordered inner collections equals the digest of
   the original; schema-identifier comparison is ordinal (case-sensitive)
   and all culture-sensitive operations run under the invariant culture;
6. the bundle markdown carries its canonical headings/terms and exactly
   fourteen fixed-order rejected falsifier rows;
7. a SHA-256 fingerprint over the four files and the canonical ledger
   digest are printed; two runs must print byte-identical output.

Redirecting output to a temporary file is local proof only; no temporary file
belongs in the PR.

```powershell
$ErrorActionPreference = 'Stop'
# Determinism across locales: every culture-sensitive operation below (sorting,
# string comparison, formatting) must behave identically on any host.
[Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$root = '.spec/11770-emacs-train-revisions'
$manifestRoot = '.spec/10918-emacs-train-graph'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md", "$root/revisions.ledger.json")
foreach ($p in $paths) { if (-not (Test-Path -LiteralPath $p)) { throw "missing bundle file: $p" } }
$manifestPath = "$manifestRoot/train.manifest.json"
if (-not (Test-Path -LiteralPath $manifestPath)) { throw "missing consumed manifest: $manifestPath" }

# --- 1. exact changed-path set (committed + index + worktree + untracked) ---
$statusFile = [IO.Path]::GetTempFileName()
try {
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

# --- 2. deterministic byte hygiene of all four bundle files ---
foreach ($p in $paths) {
  $b = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $p))
  if ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) { throw "UTF-8 BOM in $p" }
  $t = [Text.Encoding]::UTF8.GetString($b)
  if ($t.Contains("`r")) { throw "CR bytes in $p" }
  if ($t.Contains("`t")) { throw "tab bytes in $p" }
  if (-not $t.EndsWith("`n") -or $t.EndsWith("`n`n")) { throw "exactly one trailing LF required in $p" }
}

# --- schema constants ---
$TOP_KEYS = @('schema','schema_version','programme','revision_kind_vocabulary',
  'semantic_class_vocabulary','invalidation_surface_vocabulary','sync_action_vocabulary',
  'ledger_laws','revisions','limitations')
$PROGRAMME_KEYS = @('parent_programme_issue','controller_issue','home_programme','governing_issue',
  'method_authority','reference_contract','consumed_manifest')
$CONSUMED_KEYS = @('bundle','schema','node_count','consumption_rule')
$ENTRY_KEYS = @('entry_id','sequence','revision_kind','semantic_class','subject','reason',
  'ruling_evidence','graph_effect','successors','identity_preservation','invalidations',
  'required_sync','automatic_execution','rollback')
$IDENTITY_KEYS = @('proposition_before','acceptance_cells','unique_work')
$INVALIDATION_KEYS = @('surface','subjects','basis')
$SYNC_KEYS = @('controller','action','detail')
$WIRING_KEYS = @('from','to','class')
$ADOPTION_KEYS = @('node','candidate_pull','confirm_with')
$KINDS = @('decompose','insert','incorporate','retarget','supersede')
$CLASSES = @('metadata_only','nonsemantic_prose','contract_clarification','contract_strengthening',
  'contract_weakening','one_pr_outcome_change','node_split','node_merge','node_add','node_remove',
  'controller_leaf_reclassification','dependency_add','dependency_remove','dependency_reclassify',
  'writer_or_conflict_change','authority_owner_change','subject_or_evidence_stage_change',
  'claim_ceiling_change','spec_or_journey_schema_change','proof_or_falsifier_change',
  'documentation_or_generator_owner_change','supersession_or_transfer_change','external_gate_change')
$SURFACES = @('spec_disposition','exact_tree_context','live_packet','builder_candidate',
  'proof_currentness','review_currentness','journey_cell','fixture','receipt','registry_row',
  'docs_projection','certification','external_gate')
$ACTIONS = @('update_issue_body','close_superseded_issue','record_decomposition_ceiling',
  'record_authority_chain','record_child_train_boundary','record_historical_adoption')
$WORK_KINDS = @('commit','code','test','fixture','spec','docs','review','receipt','packet','candidate')
$RULING_EVIDENCE = @('#11770 body material-change list','#10918 comment stable-graph additions',
  '#10918 comment stable-graph additions 2','#10918 body corrected functional DAG',
  'E00 context.md authority and ownership','E00 context.md platform section',
  'E00 context.md durable dependency ordering','#10918 existing-candidate adoption rule')
$LAW_NAMES = @('manifest_reference','decomposition_wiring','fan_in_reachability','cell_exclusivity',
  'work_drain','insert_wiring','incorporate_boundary','retarget_state_match','metadata_neutrality',
  'material_invalidation','append_only','movement_coverage','sync_explicitness',
  'no_automatic_mutation','canonical_determinism')
$EDGE_CLASSES = @('hard','evidence','optional','external')
# Frozen initial coverage: exactly the ten material movements of #11770, in order.
$EXPECTED_MOVEMENTS = @(
  @('REV-001',1,'insert','#10894'),
  @('REV-002',2,'decompose','SUBJ_FAN'),
  @('REV-003',3,'decompose','ROOT_OBS_FAN'),
  @('REV-004',4,'decompose','ROOT_SEM_FAN'),
  @('REV-005',5,'decompose','E02'),
  @('REV-006',6,'decompose','E04'),
  @('REV-007',7,'decompose','DOG'),
  @('REV-008',8,'insert','JOURNEYS'),
  @('REV-009',9,'incorporate','#9413'),
  @('REV-010',10,'retarget','FIXT')
)
# Kind-specific graph_effect key sets.
$GRAPH_EFFECT_KEYS = @{
  'decompose'   = @('added_nodes','retained_prerequisites','subject_after_role')
  'insert'      = @('added_nodes','wiring')
  'incorporate' = @('added_nodes','separate_train','boundary')
  'retarget'    = @('added_nodes','after_state')
  'supersede'   = @('added_nodes','successor_version')
}

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
function Assert-NonEmpty {
  param($Value, [string]$Where)
  if ([string]::IsNullOrWhiteSpace([string]$Value)) { throw "empty required value at $Where" }
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

function Get-ManifestTables {
  param($Manifest)
  $byId = New-OrdinalTable
  $byIssue = New-OrdinalTable
  $external = New-OrdinalTable
  foreach ($n in @($Manifest.nodes)) {
    $byId[[string]$n.node_id] = $n
    $byIssue[[int]$n.issue] = $n
  }
  foreach ($a in @($Manifest.external_authorities)) { $external[[string]$a.id] = $a }
  return @{ byId = $byId; byIssue = $byIssue; external = $external }
}

# A ledger reference resolves when it is a manifest node id, a manifest issue
# reference ('#NNNN' naming a node issue) or a manifest external authority id.
function Resolve-Ref {
  param([string]$Ref, $Tables, [string]$Where)
  Assert-NonEmpty $Ref $Where
  if ($Ref.StartsWith('#')) {
    if ($Tables.external.Contains($Ref)) { return @{ kind = 'external'; id = $Ref } }
    if ($Ref -cmatch '^#(\d+)$' -and $Tables.byIssue.Contains([int]$Matches[1])) {
      return @{ kind = 'node'; id = [string]$Tables.byIssue[[int]$Matches[1]].node_id }
    }
    throw "unresolvable reference at ${Where}: $Ref"
  }
  if ($Tables.byId.Contains($Ref)) { return @{ kind = 'node'; id = $Ref } }
  throw "unresolvable reference at ${Where}: $Ref"
}

function Invoke-LedgerValidation {
  param($Ledger, $Manifest)
  Assert-KeySet $Ledger $TOP_KEYS 'ledger root'
  if ($Ledger.schema -cne 'emacs_train_revision.v1') { throw 'ledger schema name mismatch' }
  if ($Ledger.schema_version -ne 1) { throw 'ledger schema_version must be 1' }

  # programme block
  Assert-KeySet $Ledger.programme $PROGRAMME_KEYS 'programme'
  if ($Ledger.programme.parent_programme_issue -ne 7979) { throw 'parent programme issue mismatch' }
  if ($Ledger.programme.controller_issue -ne 8706) { throw 'programme controller_issue mismatch' }
  if ($Ledger.programme.home_programme -cne 'emacs-support') { throw 'home programme mismatch' }
  if ($Ledger.programme.governing_issue -ne 11770) { throw 'governing issue mismatch' }
  if ($Ledger.programme.method_authority -cne '#3983') { throw 'method authority mismatch' }
  if ($Ledger.programme.reference_contract -cne '#11375') { throw 'reference contract mismatch' }
  Assert-KeySet $Ledger.programme.consumed_manifest $CONSUMED_KEYS 'programme.consumed_manifest'
  if ($Ledger.programme.consumed_manifest.bundle -cne '.spec/10918-emacs-train-graph') { throw 'consumed manifest bundle mismatch' }
  if ($Ledger.programme.consumed_manifest.schema -cne 'emacs_train.v1') { throw 'consumed manifest schema mismatch' }
  if ($Ledger.programme.consumed_manifest.node_count -ne 55) { throw 'consumed manifest node count mismatch' }
  Assert-NonEmpty $Ledger.programme.consumed_manifest.consumption_rule 'consumed_manifest.consumption_rule'

  # frozen vocabularies: exact values, fixed order
  if (@($Ledger.revision_kind_vocabulary).Count -ne $KINDS.Count) { throw "expected exactly $($KINDS.Count) revision kinds" }
  for ($i = 0; $i -lt $KINDS.Count; $i++) {
    Assert-KeySet $Ledger.revision_kind_vocabulary[$i] @('kind','owns') "revision_kind_vocabulary[$i]"
    if ($Ledger.revision_kind_vocabulary[$i].kind -cne $KINDS[$i]) { throw "revision kind order broken at $($i + 1)" }
    Assert-NonEmpty $Ledger.revision_kind_vocabulary[$i].owns "revision_kind_vocabulary[$i].owns"
  }
  if (@($Ledger.semantic_class_vocabulary).Count -ne $CLASSES.Count) { throw "expected exactly $($CLASSES.Count) semantic classes" }
  for ($i = 0; $i -lt $CLASSES.Count; $i++) {
    Assert-KeySet $Ledger.semantic_class_vocabulary[$i] @('class','owns') "semantic_class_vocabulary[$i]"
    if ($Ledger.semantic_class_vocabulary[$i].class -cne $CLASSES[$i]) { throw "semantic class order broken at $($i + 1)" }
    Assert-NonEmpty $Ledger.semantic_class_vocabulary[$i].owns "semantic_class_vocabulary[$i].owns"
  }
  if (@($Ledger.invalidation_surface_vocabulary).Count -ne $SURFACES.Count) { throw "expected exactly $($SURFACES.Count) invalidation surfaces" }
  for ($i = 0; $i -lt $SURFACES.Count; $i++) {
    Assert-KeySet $Ledger.invalidation_surface_vocabulary[$i] @('surface','owns') "invalidation_surface_vocabulary[$i]"
    if ($Ledger.invalidation_surface_vocabulary[$i].surface -cne $SURFACES[$i]) { throw "invalidation surface order broken at $($i + 1)" }
    Assert-NonEmpty $Ledger.invalidation_surface_vocabulary[$i].owns "invalidation_surface_vocabulary[$i].owns"
  }
  if (@($Ledger.sync_action_vocabulary).Count -ne $ACTIONS.Count) { throw "expected exactly $($ACTIONS.Count) sync actions" }
  for ($i = 0; $i -lt $ACTIONS.Count; $i++) {
    Assert-KeySet $Ledger.sync_action_vocabulary[$i] @('action','owns') "sync_action_vocabulary[$i]"
    if ($Ledger.sync_action_vocabulary[$i].action -cne $ACTIONS[$i]) { throw "sync action order broken at $($i + 1)" }
    Assert-NonEmpty $Ledger.sync_action_vocabulary[$i].owns "sync_action_vocabulary[$i].owns"
  }

  # the fifteen ledger laws, exact names, each a non-empty ruling
  Assert-KeySet $Ledger.ledger_laws $LAW_NAMES 'ledger_laws'
  foreach ($name in $LAW_NAMES) { Assert-NonEmpty $Ledger.ledger_laws.$name "ledger_laws.$name" }

  $tables = Get-ManifestTables $Manifest
  $entries = @($Ledger.revisions)

  # append-only + frozen coverage: exactly the ten movements, in order
  if ($entries.Count -ne $EXPECTED_MOVEMENTS.Count) { throw "expected exactly $($EXPECTED_MOVEMENTS.Count) revision entries, found $($entries.Count)" }
  $seenIds = New-OrdinalTable
  for ($i = 0; $i -lt $entries.Count; $i++) {
    $e = $entries[$i]
    Assert-KeySet $e $ENTRY_KEYS "revisions[$i]"
    Assert-IsString $e.entry_id "revisions[$i].entry_id"
    if ($seenIds.Contains([string]$e.entry_id)) { throw "duplicate entry id: $($e.entry_id)" }
    $seenIds[[string]$e.entry_id] = $true
    $want = $EXPECTED_MOVEMENTS[$i]
    if ([string]$e.entry_id -cne $want[0]) { throw "movement coverage broken at index ${i}: entry_id=$($e.entry_id) expected=$($want[0])" }
    if ([int]$e.sequence -ne [int]$want[1]) { throw "append-only sequence broken at $($e.entry_id): sequence=$($e.sequence) expected=$($want[1])" }
    if ([string]$e.revision_kind -cne $want[2]) { throw "movement coverage broken at $($e.entry_id): kind=$($e.revision_kind) expected=$($want[2])" }
    if ($e.revision_kind -cnotin $KINDS) { throw "unknown revision kind at $($e.entry_id)" }
    if ($e.semantic_class -cnotin $CLASSES) { throw "unknown semantic class at $($e.entry_id)" }
    if ($e.ruling_evidence -cnotin $RULING_EVIDENCE) { throw "unknown ruling evidence at $($e.entry_id)" }
    Assert-NonEmpty $e.reason "revisions[$i].reason"
    Assert-NonEmpty $e.rollback "revisions[$i].rollback"
    if ([string]$e.automatic_execution -cne 'forbidden') { throw "automatic execution must be forbidden at $($e.entry_id)" }

    # subject resolves with its kind-specific shape
    $subjectRef = $null
    if ([string]$e.subject.kind -ceq 'node') {
      Assert-KeySet $e.subject @('kind','node_id','issue') "revisions[$i].subject"
      $subjectRef = [string]$e.subject.node_id
      if ($subjectRef -cne $want[3]) { throw "movement coverage broken at $($e.entry_id): subject=$subjectRef expected=$($want[3])" }
      $r = Resolve-Ref $subjectRef $tables "revisions[$i].subject.node_id"
      if ($r.kind -cne 'node') { throw "subject must resolve to a node at $($e.entry_id)" }
      $mnode = $tables.byId[$subjectRef]
      if ([int]$e.subject.issue -ne [int]$mnode.issue) { throw "subject issue mismatch at $($e.entry_id): ledger=$($e.subject.issue) manifest=$($mnode.issue)" }
    } elseif ([string]$e.subject.kind -ceq 'external_authority') {
      Assert-KeySet $e.subject @('kind','external_id') "revisions[$i].subject"
      $subjectRef = [string]$e.subject.external_id
      if ($subjectRef -cne $want[3]) { throw "movement coverage broken at $($e.entry_id): subject=$subjectRef expected=$($want[3])" }
      $r = Resolve-Ref $subjectRef $tables "revisions[$i].subject.external_id"
      if ($r.kind -cne 'external') { throw "subject must resolve to an external authority at $($e.entry_id)" }
    } else {
      throw "unknown subject kind at $($e.entry_id)"
    }

    # kind-specific graph_effect shape
    Assert-KeySet $e.graph_effect $GRAPH_EFFECT_KEYS[[string]$e.revision_kind] "revisions[$i].graph_effect"
    Assert-IsList $e.graph_effect.added_nodes "revisions[$i].graph_effect.added_nodes"
    $addedNodes = @()
    foreach ($a in @($e.graph_effect.added_nodes)) {
      Assert-IsString $a "revisions[$i].graph_effect.added_nodes[]"
      $ar = Resolve-Ref ([string]$a) $tables "revisions[$i].graph_effect.added_nodes"
      if ($ar.kind -cne 'node') { throw "added node must resolve to a node at $($e.entry_id): $a" }
      $addedNodes += [string]$a
    }

    # successors: unique, resolvable, issue-true
    $successorIds = @()
    $successorSet = New-OrdinalTable
    foreach ($s in @($e.successors)) {
      Assert-KeySet $s @('node_id','issue','owns') "revisions[$i].successors[]"
      if ($successorSet.Contains([string]$s.node_id)) { throw "duplicate successor at $($e.entry_id): $($s.node_id)" }
      $sr = Resolve-Ref ([string]$s.node_id) $tables "revisions[$i].successors.node_id"
      if ($sr.kind -cne 'node') { throw "successor must resolve to a node at $($e.entry_id): $($s.node_id)" }
      if ([int]$s.issue -ne [int]$tables.byId[[string]$s.node_id].issue) { throw "successor issue mismatch at $($e.entry_id): $($s.node_id)" }
      Assert-NonEmpty $s.owns "revisions[$i].successors.owns"
      $successorSet[[string]$s.node_id] = $true
      $successorIds += [string]$s.node_id
    }

    # declared reference set for cells and work drain
    $declared = New-OrdinalTable
    foreach ($d in (@($successorIds) + @($addedNodes))) { $declared[$d] = $true }
    $declared[$subjectRef] = $true

    # wiring (insert): every declared edge must exist in the manifest with exact class
    if ([string]$e.revision_kind -ceq 'insert') {
      $wiring = @($e.graph_effect.wiring)
      if ($wiring.Count -lt 1) { throw "insert entry requires wiring edges at $($e.entry_id)" }
      foreach ($w in $wiring) {
        Assert-KeySet $w $WIRING_KEYS "revisions[$i].graph_effect.wiring[]"
        $fromR = Resolve-Ref ([string]$w.from) $tables "revisions[$i].wiring.from"
        $toR = Resolve-Ref ([string]$w.to) $tables "revisions[$i].wiring.to"
        if ($toR.kind -cne 'node') { throw "wiring target must be a node at $($e.entry_id): $($w.to)" }
        if ([string]$w.class -cnotin $EDGE_CLASSES) { throw "unknown wiring class at $($e.entry_id)" }
        $target = $tables.byId[[string]$w.to]
        $edge = @($target.dependencies | Where-Object { [string]$_.target -ceq [string]$w.from })
        if ($edge.Count -eq 0) { throw "insert wiring edge missing from the manifest at $($e.entry_id): $($w.from) -> $($w.to)" }
        $matched = @($edge | Where-Object { [string]$_.class -ceq [string]$w.class })
        if ($matched.Count -ne 1) { throw "insert wiring class mismatch at $($e.entry_id): $($w.from) -> $($w.to) must be $($w.class)" }
        $declared[[string]$w.from] = $true
        $declared[[string]$w.to] = $true
      }
      if ($addedNodes.Count -lt 1) { throw "insert entry must add at least one node at $($e.entry_id)" }
      if ($r.kind -ceq 'node') {
        $wantDeps = @($tables.byId[$subjectRef].successors | Sort-Object -Unique)
        $gotDeps = @($wiring | ForEach-Object { [string]$_.to } | Sort-Object -Unique)
        if (($wantDeps -join ',') -cne ($gotDeps -join ',')) {
          throw "insert wiring must cover exactly the subject manifest dependents at $($e.entry_id): wiring=[$($gotDeps -join ',')] dependents=[$($wantDeps -join ',')]"
        }
      }
    }

    # decompose: exact split wiring and fan-in reachability
    if ([string]$e.revision_kind -ceq 'decompose') {
      if ($successorIds.Count -lt 1) { throw "decompose entry requires successors at $($e.entry_id)" }
      $mnode = $tables.byId[$subjectRef]
      if ([string]$mnode.train_role -cnotin @('fan_in','dogfood')) { throw "decompose subject must stay a fan-in or dogfood aggregator at $($e.entry_id), found $($mnode.train_role)" }
      if ([string]$e.graph_effect.subject_after_role -cne [string]$mnode.train_role) { throw "declared after-state role mismatches the manifest at $($e.entry_id)" }
      $depTargets = New-OrdinalTable
      $hardChildren = New-OrdinalTable
      foreach ($d in @($mnode.dependencies)) {
        $t = [string]$d.target
        $depTargets[$t] = $true
        if ([string]$d.class -ceq 'hard' -and -not $t.StartsWith('#')) { $hardChildren[$t] = $true }
      }
      $retained = @()
      foreach ($r in @($e.graph_effect.retained_prerequisites)) {
        Assert-IsString $r "revisions[$i].graph_effect.retained_prerequisites[]"
        $rr = Resolve-Ref ([string]$r) $tables "revisions[$i].retained_prerequisites"
        if ($rr.kind -cne 'node') { throw "retained prerequisite must resolve to a node at $($e.entry_id): $r" }
        if (-not $depTargets.Contains([string]$r)) { throw "retained prerequisite is not a dependency of the subject at $($e.entry_id): $r" }
        $retained += [string]$r
      }
      $wantSet = @($hardChildren.Keys | Where-Object { $_ -cnotin $retained } | Sort-Object)
      $gotSet = @($successorIds | Sort-Object -Unique)
      if (($wantSet -join ',') -cne ($gotSet -join ',')) {
        throw "decomposition wiring mismatch at $($e.entry_id): successors=[$($gotSet -join ',')] hard-children-minus-retained=[$($wantSet -join ',')]"
      }
      foreach ($s in $successorIds) {
        if (@($tables.byId[$s].successors | Where-Object { [string]$_ -ceq $subjectRef }).Count -eq 0) {
          throw "fan-in reachability broken at $($e.entry_id): successor $s does not reach $subjectRef"
        }
      }
    }

    # incorporate: a separate train never adds nodes
    if ([string]$e.revision_kind -ceq 'incorporate') {
      if ($e.graph_effect.separate_train -ne $true) { throw "incorporate must declare a separate train at $($e.entry_id)" }
      if ($addedNodes.Count -ne 0) { throw "incorporate must not add nodes at $($e.entry_id)" }
      Assert-NonEmpty $e.graph_effect.boundary "revisions[$i].graph_effect.boundary"
    }

    # retarget: the declared after-state matches the manifest bytes exactly
    if ([string]$e.revision_kind -ceq 'retarget') {
      $mnode = $tables.byId[$subjectRef]
      Assert-KeySet $e.graph_effect.after_state @('role','adoption_rule') "revisions[$i].graph_effect.after_state"
      if ([string]$e.graph_effect.after_state.role -cne [string]$mnode.train_role) {
        throw "retarget after-state role mismatches the manifest at $($e.entry_id): declared=$($e.graph_effect.after_state.role) manifest=$($mnode.train_role)"
      }
      Assert-KeySet $e.graph_effect.after_state.adoption_rule $ADOPTION_KEYS "revisions[$i].graph_effect.after_state.adoption_rule"
      $rule = $e.graph_effect.after_state.adoption_rule
      $mRule = $Manifest.existing_candidate_adoption
      if ([string]$rule.node -cne [string]$mRule.node) { throw "adoption rule node mismatch at $($e.entry_id)" }
      if ([int]$rule.candidate_pull -ne [int]$mRule.candidate_pull) { throw "adoption rule candidate mismatch at $($e.entry_id)" }
      if ([string]$rule.confirm_with -cne [string]$mRule.confirm_with) { throw "adoption rule confirmation mismatch at $($e.entry_id)" }
    }

    # identity preservation: exclusive cells and drained work
    Assert-KeySet $e.identity_preservation $IDENTITY_KEYS "revisions[$i].identity_preservation"
    Assert-NonEmpty $e.identity_preservation.proposition_before "revisions[$i].proposition_before"
    $cells = @($e.identity_preservation.acceptance_cells)
    if ($cells.Count -lt 1) { throw "an entry requires at least one acceptance cell at $($e.entry_id)" }
    $cellNames = New-OrdinalTable
    $ownersSeen = New-OrdinalTable
    foreach ($c in $cells) {
      if ($null -ne $c.owner) {
        Assert-KeySet $c @('cell','owner') "revisions[$i].acceptance_cells[]"
        Assert-NonEmpty $c.cell "revisions[$i].acceptance_cells.cell"
        $null = Resolve-Ref ([string]$c.owner) $tables "revisions[$i].acceptance_cells.owner"
        if (-not $declared.Contains([string]$c.owner)) { throw "cell owner is not declared by the entry at $($e.entry_id): $($c.owner)" }
        $ownersSeen[[string]$c.owner] = $true
      } else {
        Assert-KeySet $c @('cell','retired','retirement_reason') "revisions[$i].acceptance_cells[]"
        if ($c.retired -ne $true) { throw "retirement must be explicit and true at $($e.entry_id)" }
        Assert-NonEmpty $c.retirement_reason "revisions[$i].acceptance_cells.retirement_reason"
      }
      if ($cellNames.Contains([string]$c.cell)) { throw "duplicate acceptance cell at $($e.entry_id): $($c.cell)" }
      $cellNames[[string]$c.cell] = $true
    }
    $work = @($e.identity_preservation.unique_work)
    if ($work.Count -lt 1) { throw "an entry requires at least one unique work item at $($e.entry_id)" }
    foreach ($w in $work) {
      Assert-IsString $w.kind "revisions[$i].unique_work.kind"
      if ($w.kind -cnotin $WORK_KINDS) { throw "unknown work kind at $($e.entry_id): $($w.kind)" }
      Assert-NonEmpty $w.work "revisions[$i].unique_work.work"
      if ($null -ne $w.preserved_to) {
        Assert-KeySet $w @('work','kind','preserved_to') "revisions[$i].unique_work[]"
        $null = Resolve-Ref ([string]$w.preserved_to) $tables "revisions[$i].unique_work.preserved_to"
        if (-not $declared.Contains([string]$w.preserved_to)) { throw "unique work drains to an undeclared owner at $($e.entry_id): $($w.preserved_to)" }
      } else {
        Assert-KeySet $w @('work','kind','retired','retirement_reason') "revisions[$i].unique_work[]"
        if ($w.retired -ne $true) { throw "work retirement must be explicit and true at $($e.entry_id)" }
        Assert-NonEmpty $w.retirement_reason "revisions[$i].unique_work.retirement_reason"
      }
    }
    # nothing is added or split without owned cells
    foreach ($s in (@($successorIds) + @($addedNodes))) {
      if (-not $ownersSeen.Contains($s)) { throw "cell coverage hole at $($e.entry_id): $s owns no acceptance cell" }
    }
    if (-not $ownersSeen.Contains($subjectRef)) { throw "cell coverage hole at $($e.entry_id): the subject owns no acceptance cell" }

    # typed invalidations and exact-controller synchronization
    $invs = @($e.invalidations)
    $material = @($e.semantic_class -cnotin @('metadata_only','nonsemantic_prose'))
    if ($material -and $invs.Count -lt 1) { throw "a material revision requires at least one invalidation at $($e.entry_id)" }
    if (-not $material -and $invs.Count -ne 0) { throw "metadata-only movement must carry zero invalidations at $($e.entry_id)" }
    foreach ($v in $invs) {
      Assert-KeySet $v $INVALIDATION_KEYS "revisions[$i].invalidations[]"
      if ([string]$v.surface -cnotin $SURFACES) { throw "unknown invalidation surface at $($e.entry_id): $($v.surface)" }
      Assert-NonEmpty $v.basis "revisions[$i].invalidations.basis"
      $subjects = @($v.subjects)
      if ($subjects.Count -lt 1) { throw "an invalidation requires at least one subject at $($e.entry_id)" }
      foreach ($s in $subjects) {
        Assert-IsString $s "revisions[$i].invalidations.subjects[]"
        $null = Resolve-Ref ([string]$s) $tables "revisions[$i].invalidations.subjects"
      }
    }
    $syncs = @($e.required_sync)
    if ($material -and $syncs.Count -lt 1) { throw "a material revision requires named synchronization at $($e.entry_id)" }
    foreach ($s in $syncs) {
      Assert-KeySet $s $SYNC_KEYS "revisions[$i].required_sync[]"
      $null = Resolve-Ref ([string]$s.controller) $tables "revisions[$i].required_sync.controller"
      if ([string]$s.action -cnotin $ACTIONS) { throw "unknown sync action at $($e.entry_id): $($s.action)" }
      Assert-NonEmpty $s.detail "revisions[$i].required_sync.detail"
    }
  }

  Assert-IsList $Ledger.limitations 'limitations'
  if (@($Ledger.limitations).Count -lt 1) { throw 'limitations must remain explicit' }
  foreach ($l in @($Ledger.limitations)) { Assert-NonEmpty $l 'limitations[]' }

  Assert-NoLiveStateStrings $Ledger 'root'

  return (Get-CanonicalDigest $Ledger)
}

# --- parse and validate ---
$manifestDoc = ConvertFrom-Json (Get-Content -Raw -LiteralPath $manifestPath)
if ([string]$manifestDoc.schema -cne 'emacs_train.v1') { throw 'consumed manifest schema mismatch' }
if (@($manifestDoc.nodes).Count -ne 55) { throw 'consumed manifest must carry exactly 55 nodes' }
$ledgerText = Get-Content -Raw -LiteralPath $paths[3]
$ledgerDoc = ConvertFrom-Json $ledgerText
$ledgerDigest = Invoke-LedgerValidation $ledgerDoc $manifestDoc

# raw-file live-state scan (mirrors the document scan over exact bytes)
foreach ($line in ($ledgerText -split "`n")) {
  if ($line -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { throw "possible live SHA token in ledger bytes: $($Matches[0])" }
  if ($line -match '\d{4}-\d{2}-\d{2}T') { throw 'possible live timestamp in ledger bytes' }
}

# --- negative controls: every mutation must fail closed ---
function Copy-Doc { param($d) (ConvertTo-Json $d -Depth 100) | ConvertFrom-Json }
$controls = [System.Collections.Generic.List[string]]::new()
function Invoke-NegativeControl {
  param([string]$Name, [scriptblock]$Mutation, [scriptblock]$ManifestMutation)
  $copy = Copy-Doc $ledgerDoc
  $manifestCopy = Copy-Doc $manifestDoc
  & $Mutation $copy
  if ($null -ne $ManifestMutation) { & $ManifestMutation $manifestCopy }
  try {
    Invoke-LedgerValidation $copy $manifestCopy | Out-Null
    throw "negative control FAILED to reject: $Name"
  } catch [System.Management.Automation.RuntimeException] {
    if ($_.Exception.Message -like "negative control FAILED*") { throw }
  }
  $controls.Add($Name)
}
function Find-Entry { param($d, [string]$id) @($d.revisions | Where-Object { $_.entry_id -ceq $id })[0] }

# regression 1: a decompose entry drops one subject class
Invoke-NegativeControl 'R01-cell-dropped' { param($d)
  $ip = Find-Entry $d 'REV-002'
  $ip.identity_preservation.acceptance_cells = @($ip.identity_preservation.acceptance_cells | Where-Object { $_.cell -cne 'Linux-generation subject materialization' })
}
# regression 2: the split lets Eglot evidence satisfy the lsp-mode cell
Invoke-NegativeControl 'R02-cross-family-cell' { param($d)
  $ip = Find-Entry $d 'REV-003'
  foreach ($c in @($ip.identity_preservation.acceptance_cells)) {
    if ($c.cell -ceq 'lsp-mode stock project-root observation') { $c.owner = 'ROOT_E_OBS' }
  }
}
# regression 3: the insertion leaves the adopter without the authority wiring
Invoke-NegativeControl 'R03-insert-wiring-absent' { param($d) } { param($m)
  $n = @($m.nodes | Where-Object { $_.node_id -ceq 'RUNCONF' })[0]
  $n.dependencies = @($n.dependencies | Where-Object { $_.target -cne '#10894' })
}
# regression 4: governance edges drift from their frozen classes
Invoke-NegativeControl 'R04-governance-class-drift' { param($d)
  $e = Find-Entry $d 'REV-008'
  foreach ($w in @($e.graph_effect.wiring)) {
    if ($w.to -ceq 'HOST_E29') { $w.class = 'evidence' }
  }
}
# regression 5: unique candidate work disappears during supersession or transfer
Invoke-NegativeControl 'R05-work-undeclared' { param($d)
  $e = Find-Entry $d 'REV-002'
  foreach ($w in @($e.identity_preservation.unique_work)) {
    if ($w.work -ceq 'generation row builder packets') { $w.preserved_to = 'REG' }
  }
}
# regression 6: a metadata-only edit churns context and packets
Invoke-NegativeControl 'R06-metadata-churn' { param($d)
  (Find-Entry $d 'REV-002').semantic_class = 'metadata_only'
}
# regression 7: completed substrate is recreated as new work
Invoke-NegativeControl 'R07-stage-resurrection' { param($d)
  (Find-Entry $d 'REV-010').graph_effect.after_state.role = 'implementation'
}
# regression 8: a revision silently drops a node from the decomposition
Invoke-NegativeControl 'R08-successor-dropped' { param($d)
  $e = Find-Entry $d 'REV-002'
  $e.successors = @($e.successors | Where-Object { $_.node_id -cne 'SUBJ_L' })
}
# regression 9: a phantom successor invents a split child
Invoke-NegativeControl 'R09-phantom-successor' { param($d)
  $e = Find-Entry $d 'REV-002'
  $e.successors = @($e.successors) + [pscustomobject]@{ node_id = 'OBS'; issue = 11360; owns = 'phantom' }
}
# regression 10: decomposition loses reachability to the fan-in
Invoke-NegativeControl 'R10-fan-in-orphaned' { param($d) } { param($m)
  $n = @($m.nodes | Where-Object { $_.node_id -ceq 'SUBJ_CORE' })[0]
  $n.successors = @($n.successors | Where-Object { $_ -cne 'SUBJ_FAN' })
}
# regression 11: an entry references a node absent from the manifest
Invoke-NegativeControl 'R11-ghost-reference' { param($d)
  (Find-Entry $d 'REV-002').successors[0].node_id = 'GHOST_NODE'
}
# regression 12: synchronization completes without naming affected controllers
Invoke-NegativeControl 'R12-vague-controller' { param($d)
  (Find-Entry $d 'REV-002').required_sync[0].controller = 'related controllers'
}
# regression 13: the ledger mutates history by removing an entry
Invoke-NegativeControl 'R13-entry-removed' { param($d)
  $d.revisions = @($d.revisions | Where-Object { $_.entry_id -cne 'REV-004' })
}
# regression 14: live state enters stable ledger bytes (value level, then byte level)
Invoke-NegativeControl 'R14-live-state-value' { param($d)
  (Find-Entry $d 'REV-001').reason += ' rebased onto head 4f5bcb334 deadbeef 00ff11'
}
$badLine = 'x deadbeefdeadbeef y'
if ($badLine -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { $controls.Add('R14b-live-state-bytes') } else { throw 'R14b raw-byte live-state scan failed to detect a token' }

# acceptance-bullet mutation classes beyond the numbered regressions:
# sequence tampering, unknown semantic class, permitted automatic mutation,
# kind misclassification
Invoke-NegativeControl 'A01-sequence-tampered' { param($d)
  (Find-Entry $d 'REV-006').sequence = 7
}
Invoke-NegativeControl 'A02-unknown-semantic-class' { param($d)
  (Find-Entry $d 'REV-007').semantic_class = 'graph_tweak'
}
Invoke-NegativeControl 'A03-automatic-mutation-permitted' { param($d)
  (Find-Entry $d 'REV-010').automatic_execution = 'permitted'
}
Invoke-NegativeControl 'A04-kind-misclassified' { param($d)
  (Find-Entry $d 'REV-002').revision_kind = 'supersede'
}
# regression 15: an insert under-reports its blast radius by dropping one
# declared governance edge while the manifest dependents stay wired
Invoke-NegativeControl 'R15-wiring-undercoverage' { param($d)
  $e = Find-Entry $d 'REV-008'
  $e.graph_effect.wiring = @($e.graph_effect.wiring | Where-Object { $_.to -cne 'PROD' })
}
# regression 16: an incorporation silently absorbs child-train nodes
Invoke-NegativeControl 'A05-incorporate-claims-node' { param($d)
  (Find-Entry $d 'REV-009').graph_effect.added_nodes = @('SPEC_PUB')
}
# regression 17: the retarget's canonical adoption rule drifts from the manifest
Invoke-NegativeControl 'A06-adoption-rule-drift' { param($d)
  (Find-Entry $d 'REV-010').graph_effect.after_state.adoption_rule.candidate_pull = 9999
}

# order-invariance control: the canonical digest must not move with the order
# of the ledger's unordered inner collections (entry order itself is semantic
# and stays append-only by law).
$orderDigest = $ledgerDigest
$shuffled = Copy-Doc $ledgerDoc
foreach ($e in @($shuffled.revisions)) {
  $e.successors = @($e.successors | Sort-Object -Property node_id -Descending)
  $e.invalidations = @($e.invalidations | Sort-Object -Property surface -Descending)
  $e.required_sync = @($e.required_sync | Sort-Object -Property action -Descending)
  $e.identity_preservation.acceptance_cells = @($e.identity_preservation.acceptance_cells | Sort-Object -Property cell -Descending)
  $e.identity_preservation.unique_work = @($e.identity_preservation.unique_work | Sort-Object -Property work -Descending)
  if ($null -ne $e.graph_effect.wiring) { $e.graph_effect.wiring = @($e.graph_effect.wiring | Sort-Object -Property to -Descending) }
  if ($null -ne $e.graph_effect.added_nodes) { $e.graph_effect.added_nodes = @($e.graph_effect.added_nodes | Sort-Object -Descending) }
  if ($null -ne $e.graph_effect.retained_prerequisites) { $e.graph_effect.retained_prerequisites = @($e.graph_effect.retained_prerequisites | Sort-Object -Descending) }
}
$shuffledDigest = Invoke-LedgerValidation $shuffled $manifestDoc
if ($shuffledDigest -cne $orderDigest) { throw 'order-invariance control failed: canonical digest changed with input order' }
$controls.Add('ORDER-CANONICAL-DIGEST')

# Twenty-three fail-closed mutation controls cover the fourteen revision
# regressions (regression 14 carries both a value-level and a byte-level
# control), the acceptance-bullet mutation classes beyond the numbered list
# (sequence tampering, unknown semantic class, permitted automatic execution,
# kind misclassification, wiring undercoverage, incorporation claiming a node,
# adoption-rule drift), and the order-invariance canonicalization control.
if ($controls.Count -ne 23) { throw "expected 23 negative controls, ran $($controls.Count)" }

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
foreach ($term in @('emacs_train.v1','emacs_train_revision.v1','`.spec/10918-emacs-train-graph/`','#11375',
                    '#11770','append-only','not_proven','#10554','decompose','supersede')) {
  if (-not ($contextText -match [regex]::Escape($term))) { throw "missing context contract term: $term" }
}
foreach ($h in @('## §Behavior','## §Hazards','## §Contracts','## §API-Shape','## §Test-Grid','## §Blast-Radius')) {
  if (-not ($acceptanceText -match "(?m)^$([regex]::Escape($h))\s*$")) { throw "missing acceptance section: $h" }
}
$testGrid = Get-SectionBody $acceptanceText '## §Test-Grid'
$falsifierRows = @($testGrid -split "`r?`n" | Where-Object { $_ -match '^\|\s*\d+\s*\|' })
if ($falsifierRows.Count -ne 14) { throw "expected exactly 14 falsifier rows, found $($falsifierRows.Count)" }
for ($i = 0; $i -lt 14; $i++) {
  if ($falsifierRows[$i] -notmatch ("^\|\s*$($i + 1)\s*\|")) { throw "falsifier order broken at row $($i + 1)" }
  if ($falsifierRows[$i] -notmatch [regex]::Escape('rejected')) { throw "falsifier row $($i + 1) lacks a rejected verdict" }
}
foreach ($term in @('emacs_train_revision.v1','#10918','#11770','#11375','revisions.ledger.json',
                    'twenty-three controls','ten frozen movements')) {
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
Write-Output "SPEC_11770_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_11770_NEGATIVE_CONTROLS=23/23"
Write-Output "SPEC_11770_LEDGER_SHA256=$ledgerDigest"
Write-Output "SPEC_11770_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice. Requirements for a valid proof:

1. both runs print `SPEC_11770_STRUCTURAL_CHECK=PASS`;
2. both runs print `SPEC_11770_NEGATIVE_CONTROLS=23/23`;
3. both runs print the same `SPEC_11770_LEDGER_SHA256` and
   `SPEC_11770_BUNDLE_SHA256` fingerprints;
4. the full captured output of both runs is byte-identical;
5. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
6. `git diff --check` (staged) is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves ledger shape, vocabulary discipline, the
append-only law with frozen ten-movement coverage, ledger-vs-manifest
reference resolution, decomposition wiring and fan-in reachability against
the consumed `emacs_train.v1` manifest, cell exclusivity and work drain,
insert wiring with exact classes and exact dependent coverage, the
incorporation boundary, the retarget after-state match including the
canonical adoption rule, metadata
neutrality, typed invalidation and exact-controller synchronization, the
no-automatic-mutation law, durable-byte hygiene, fail-closed behavior of
all fourteen revision regressions, order-invariant canonicalization, and
byte-level determinism across two runs. It does **not** prove: that the
offline diff, change-check or impact operations named by #11770 exist or
pass (unbuilt tooling, a separate claim); that the ten recorded movements
are the complete history of the Emacs issue graph (the governing issue's
frozen list is the authority); that any affected spec, context, packet,
proof or receipt has actually been re-derived (their owning planes hold
that state); or that any live candidate, check, review or writer is current
(the overlay plane owns live truth). The repository's absent executable
Emacs train validator remains an open tooling gap recorded here rather than
papered over.

## Flags for builder

- Deviation note: the controlling issue names offline xtask operations
  (`cargo xtask integration emacs train diff`, `change check`, `impact`).
  Those are executable repository tooling and are not built in this
  bundle-style claim; the revision contract lands as checked data plus this
  embedded checker, and the absent validator is recorded as `not_proven`.
  A later tooling claim against the same seam must consume
  `emacs_train_revision.v1` and `emacs_train.v1` as data, not clone them.
- The ledger's entry order is semantic (append-only); only the inner
  collections (successors, cells, work, wiring, invalidations,
  synchronization) are unordered for canonicalization.
- Future movements append new entries and extend the checker's frozen
  movement-coverage table in the same change; existing entries are never
  edited. The frozen table is the initial-state proof, not a cap on the
  ledger.
- The `supersede` kind is schema-present but unused by the initial ledger;
  a future manifest version must drain every cell and work item through it.
- If a downstream check can only pass by weakening a law here, stop and
  return to #11770 rather than editing the law locally.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor ledger or manifest version supersedes this one
  only through a `supersede` entry with an exact successor recorded and a
  full cell and work drain; derived artifacts are re-derived, never patched
  valid.
- **Stop:** stop before validator commands, live GitHub observation or
  mutation, automatic issue/body/label/PR updates, packet rendering, host
  execution, dogfood, scheduling, support claims, release or publication.
  If an open decision (OD1, OD2, OD3 or the unbuilt tooling gap) is needed
  as a decision rather than a boundary, stop and route it to its owning
  issue; do not decide it in a ledger entry.
