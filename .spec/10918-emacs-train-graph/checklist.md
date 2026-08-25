# Implementation Checklist: #10918 — canonical stable emacs_train.v1 topology graph

## Change order

This is a specification/data-only change. Each step is reviewable without
building or executing any tooling beyond the embedded checker.

### Step 1: Write the fail-closed fixtures first

- **File:** the negative-control suite inside the checker below (DESIGN).
- **Change:** Before the manifest is declared valid, the fourteen required
  graph regressions of #10918 must exist as in-memory mutations that each
  make validation throw (regression 12 carries both a parsed-value and a
  raw-byte control), the acceptance-bullet mutation classes beyond the
  numbered list (stage inflation, duplicate owner, hard cycle, controller
  selection) must carry their own controls, plus an order-invariance
  canonicalization control — twenty controls in total.
- **Verify:** temporarily weakening the validator shows a control failing to
  reject; against the real validator all sixteen reject.

### Step 2: Create the stable manifest

- **File:** `.spec/10918-emacs-train-graph/train.manifest.json` (CREATE)
- **Change:** Encode the complete 55-node graph (programme, E00, E01, E01R,
  the shared reliability adoption spec, historical foundations, the
  subject/adapter/profile/host/root/public/projection lanes, the journeys
  catalog, the routing policy, the E02/E04/E06 planes and the dogfood chain)
  with typed, provenance-traced edges, conflict keys, dispositions,
  controls, exits, rollback quartets, successors and identity fields.
- **Verify:** structural and law checks below; `git diff --check`.

### Step 3: Create the context contract

- **File:** `.spec/10918-emacs-train-graph/context.md` (CREATE)
- **Change:** Record the problem, authority, consumed laws, encoding
  traceability decisions, `AGENTS.md` compatibility, open decisions
  respected (not decided), adoption/rollback/transfer/stop, links.
- **Verify:** heading and term checks below; `git diff --check`.

### Step 4: Create acceptance and negative controls

- **File:** `.spec/10918-emacs-train-graph/acceptance.md` (CREATE)
- **Change:** All canonical `SPEC_TEMPLATE.md` sections and the fourteen
  #10918 graph regressions in fixed order with exact kind/verdict
  semantics, plus the claim boundary and non-goals.
- **Verify:** section and falsifier-order checks below; `git diff --check`.

### Step 5: Create the builder and proof contract (this file)

- **File:** `.spec/10918-emacs-train-graph/checklist.md` (CREATE)
- **Change:** This change order, the embedded deterministic structural
  checker, the second-run procedure, the `not_proven` boundary, and
  rollback/transfer/stop.
- **Verify:** the embedded checker runs twice from the candidate worktree
  with byte-identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/10918-emacs-train-graph/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no `crates/` or
`xtask/` change, no `docs/` change, no configuration, no generated artifact
outside the bundle, no GitHub state, no host execution.

## Deterministic structural proof

The repository has no executable Emacs train validator (the xtask operations
named by #10918 remain a separate tooling claim). Do not invent a generated
receipt or claim a missing tool passed. From the candidate worktree root, run
the following PowerShell 7 checker twice after the four files are complete.
The checker asserts:

1. the union of the committed candidate patch
   (`merge-base(origin/main, HEAD)..HEAD`, which stays the candidate's own
   patch even if `origin/main` advances mid-flight because a sibling lane
   fetched), the staged index, the unstaged worktree, and NUL-delimited
   porcelain paths — including untracked files — equals exactly the four
   bundle paths (it fails closed on a malformed status record or a
   rename/copy record without its second path);
2. the manifest bytes are hygiene-clean (no BOM, no CR, no tabs, exactly one
   trailing LF) and contain no live-state tokens anywhere (long lowercase
   hex runs, timestamps, branch/PR path fragments), in both raw bytes and
   parsed values;
3. the manifest parses under a strict schema: exact key sets at every level
   (unknown keys fail closed), exact expected node/issue pairs for all 55
   nodes, unique node IDs, issues, aliases, conflict keys and authority-after
   propositions, title fingerprints recomputed, dependency classes from the
   four-value vocabulary, provenance strings from the closed #10918/E00
   vocabulary, every dependency target resolvable, exactly one edge per
   target, successor sets exactly the derived reverse-edge set, no cycles
   over hard/evidence edges, controller/fan-in non-buildability, chain and
   writer-class laws, claim-ceiling length bounds, spec-authority reference
   to #11717, the canonical candidate-adoption block, strict fan-in and
   substrate ceilings, the evidence-policy role law, optional-class law for
   child-train references, external-authorization gate law, semantic path
   neutrality, and all 121 graph-law edges present with exactly their
   declared classes while all 24 forbidden edges stay absent;
4. all fourteen #10918 graph regressions reject through twenty fail-closed
   in-memory mutation controls (regression 12 carries a value-level and a
   byte-level control; the acceptance-bullet mutation classes carry four
   more), including the order-invariance control whose
   rejected subject is an order-sensitive canonicalization: the canonical
   semantic digest of a shuffled document equals the digest of the original;
   schema-identifier comparison is ordinal (case-sensitive) and all
   culture-sensitive operations run under the invariant culture;
5. the bundle markdown carries its canonical headings/terms and exactly
   fourteen fixed-order rejected falsifier rows;
6. a SHA-256 fingerprint over the four files and the semantic digest are
   printed; two runs must print byte-identical output.

Redirecting output to a temporary file is local proof only; no temporary file
belongs in the PR.

```powershell
$ErrorActionPreference = 'Stop'
# Determinism across locales: every culture-sensitive operation below (sorting,
# string comparison, formatting) must behave identically on any host.
[Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$root = '.spec/10918-emacs-train-graph'
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
  'evidence_semantics','external_authorities','open_decisions_routed_elsewhere',
  'existing_candidate_adoption','nodes','supersessions','revision_governance','limitations')
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
$SPEC_KEYS = @('disposition','owner','stale_policy','spec_authority')
$CLASSES = @('hard','evidence','optional','external')
$GROUPS = @('none','A','B','C','D')
$DISPOSITIONS = @('SPEC_COMPILED','EXISTING_CONTRACT_SUFFICIENT','ISSUE_PLAN_SUFFICIENT',
  'CONTROLLER_NO_CODING_SPEC','FAN_IN_OR_CERTIFICATION_SPEC',
  'EXTERNAL_OR_MANUAL_NO_CODING_SPEC','RETURN_TO_ISSUE','NOT_PROVEN')
$PROVENANCE = @('#10918 body corrected functional DAG','#10918 body canonical seed graph',
  '#10918 body observation/receipt separation','#10918 body CI/proof-routing policy',
  '#10918 comment stable-graph additions','#10918 comment stable-graph additions 2',
  'E00 context.md authority and ownership','E00 context.md durable dependency ordering',
  'E00 acceptance.md per-leaf contracts','E00 context.md links (spec method)',
  'E00 context.md platform section','#10918 body architecture planes',
  '#10918 body train position (begin after #11716)','#10918 body durable architecture prerequisite',
  '#7979/#8706 programme header')
$PLANES = @('durable Emacs semantic architecture and evidence boundaries',
  'stable reviewed implementation topology','checked per-node spec dispositions',
  'exact current-tree implementation state','exact-tree source and instruction navigation',
  'live branches, worktrees, PRs, checks, reviews and current writers',
  'disposable shared-contract builder, reviewer and reconciliation packets',
  'behavior evidence and support truth')
$ROLES = @('controller','specification','stable_contract','semantic_revision','historical',
  'implementation','fan_in','packet_adapter','dogfood','evidence_policy','external_gate')
$EXPECTED_NODES = [ordered]@{
  'ADP_E'=8776; 'ADP_L'=8795; 'CERT'=8865; 'COHORT'=11760; 'CTRL'=8706; 'CTXENG'=11756
  'CTX_PUB'=11758; 'CTX_SUB'=11757; 'DOCS'=8862; 'DOG'=10936; 'E00'=11716; 'E01'=10918
  'E01R'=11770; 'E02'=11717; 'E04'=11718; 'E06'=11719; 'FIXT'=11366; 'H7777'=7777
  'H7778'=7778; 'HOST_E29'=8822; 'HOST_E30'=8823; 'HOST_E_REL'=8824; 'HOST_E_SRC'=8825
  'HOST_L_REL'=8828; 'HOST_L_SRC'=8830; 'JOURNEYS'=11768; 'LINUX'=8842; 'OBS'=11360
  'POLICY'=9375; 'PROD'=11361; 'PROF_E'=8819; 'PROF_L'=8821; 'PROG'=7979; 'PUB_E_B'=8846
  'PUB_E_R'=8849; 'PUB_L_R'=8853; 'REG'=8858; 'RELY'=11766; 'ROOT_E_OBS'=11747
  'ROOT_E_SEM'=11749; 'ROOT_L_OBS'=11748; 'ROOT_L_SEM'=11750; 'ROOT_OBS_FAN'=8834
  'ROOT_SEM_FAN'=8838; 'ROUTE'=11759; 'RUNCONF'=8734; 'SPECENG'=11751; 'SPEC_CTRL'=11755
  'SPEC_HOST'=11753; 'SPEC_PUB'=11754; 'SPEC_SUB'=11752; 'SUBJ_CORE'=11744; 'SUBJ_E'=11745
  'SUBJ_FAN'=8755; 'SUBJ_L'=11746
}
$OPEN_DECISION_OWNERS = @(@('OD1','#10554'),@('OD2','#10930'),@('OD3','#11744'),@('OD4','#9310'),@('OD5','#9375'))
$OPTIONAL_AUTHORITIES = @('#9310','#9374','#9413','#7774','#7775','#7776')
# Graph-law edges (from the #10918 corrected functional DAG, its two stable-graph
# comments, and the E00 bundle). Each entry must exist as a dependency edge with
# exactly this class.
$LAW_EDGES = @(
  @('PROG','CTRL','hard'), @('CTRL','E00','hard'), @('E00','E01','hard'), @('E01','E01R','hard'),
  @('RELY','RUNCONF','evidence'), @('#10894','RUNCONF','evidence'),
  @('H7777','RUNCONF','hard'), @('H7778','RUNCONF','hard'),
  @('H7777','FIXT','hard'), @('H7778','FIXT','hard'),
  @('E01','SUBJ_CORE','hard'), @('SUBJ_CORE','SUBJ_E','hard'), @('SUBJ_CORE','SUBJ_L','hard'),
  @('RUNCONF','SUBJ_FAN','hard'), @('SUBJ_CORE','SUBJ_FAN','hard'),
  @('SUBJ_E','SUBJ_FAN','hard'), @('SUBJ_L','SUBJ_FAN','hard'),
  @('SUBJ_FAN','OBS','hard'), @('OBS','PROD','hard'), @('H7777','PROD','evidence'),
  @('E01','JOURNEYS','hard'),
  @('JOURNEYS','OBS','evidence'), @('JOURNEYS','PROD','evidence'),
  @('JOURNEYS','ADP_E','evidence'), @('JOURNEYS','ADP_L','evidence'),
  @('SUBJ_FAN','ADP_E','hard'), @('OBS','ADP_E','hard'),
  @('SUBJ_FAN','ADP_L','hard'), @('OBS','ADP_L','hard'),
  @('ADP_E','PROF_E','hard'), @('ADP_L','PROF_L','hard'),
  @('PROF_E','HOST_E29','hard'), @('PROD','HOST_E29','hard'), @('JOURNEYS','HOST_E29','hard'),
  @('PROF_E','HOST_E30','hard'), @('PROD','HOST_E30','hard'), @('JOURNEYS','HOST_E30','hard'),
  @('PROF_E','HOST_E_REL','hard'), @('PROD','HOST_E_REL','hard'), @('JOURNEYS','HOST_E_REL','hard'),
  @('PROF_E','HOST_E_SRC','hard'), @('PROD','HOST_E_SRC','hard'), @('JOURNEYS','HOST_E_SRC','hard'),
  @('PROF_L','HOST_L_REL','hard'), @('PROD','HOST_L_REL','hard'), @('JOURNEYS','HOST_L_REL','hard'),
  @('PROF_L','HOST_L_SRC','hard'), @('PROD','HOST_L_SRC','hard'), @('JOURNEYS','HOST_L_SRC','hard'),
  @('FIXT','ROOT_E_OBS','hard'), @('SUBJ_FAN','ROOT_E_OBS','hard'),
  @('ADP_E','ROOT_E_OBS','hard'), @('OBS','ROOT_E_OBS','hard'),
  @('FIXT','ROOT_L_OBS','hard'), @('SUBJ_FAN','ROOT_L_OBS','hard'),
  @('ADP_L','ROOT_L_OBS','hard'), @('OBS','ROOT_L_OBS','hard'),
  @('ROOT_E_OBS','ROOT_E_SEM','hard'), @('PROD','ROOT_E_SEM','hard'), @('JOURNEYS','ROOT_E_SEM','hard'),
  @('ROOT_L_OBS','ROOT_L_SEM','hard'), @('PROD','ROOT_L_SEM','hard'), @('JOURNEYS','ROOT_L_SEM','hard'),
  @('ROOT_E_OBS','ROOT_OBS_FAN','hard'), @('ROOT_L_OBS','ROOT_OBS_FAN','hard'),
  @('ROOT_E_SEM','ROOT_SEM_FAN','hard'), @('ROOT_L_SEM','ROOT_SEM_FAN','hard'),
  @('RUNCONF','LINUX','hard'), @('SUBJ_FAN','LINUX','hard'),
  @('#5903','LINUX','hard'), @('#6990','LINUX','hard'),
  @('HOST_E29','PUB_E_B','hard'), @('HOST_E30','PUB_E_B','hard'),
  @('ROOT_SEM_FAN','PUB_E_B','hard'), @('LINUX','PUB_E_B','hard'),
  @('PROD','PUB_E_B','hard'), @('JOURNEYS','PUB_E_B','evidence'),
  @('HOST_E_REL','PUB_E_R','hard'), @('ROOT_SEM_FAN','PUB_E_R','hard'),
  @('LINUX','PUB_E_R','hard'), @('PROD','PUB_E_R','hard'), @('JOURNEYS','PUB_E_R','evidence'),
  @('HOST_L_REL','PUB_L_R','hard'), @('ROOT_SEM_FAN','PUB_L_R','hard'),
  @('LINUX','PUB_L_R','hard'), @('PROD','PUB_L_R','hard'), @('JOURNEYS','PUB_L_R','evidence'),
  @('PUB_E_B','REG','hard'), @('PUB_E_R','REG','hard'), @('PUB_L_R','REG','hard'),
  @('REG','DOCS','hard'), @('DOCS','CERT','hard'), @('#9310','CERT','optional'),
  @('E01','SPECENG','hard'), @('SPECENG','SPEC_SUB','hard'), @('SPECENG','SPEC_HOST','hard'),
  @('SPECENG','SPEC_PUB','hard'), @('SPECENG','SPEC_CTRL','hard'),
  @('SPECENG','E02','hard'), @('SPEC_SUB','E02','hard'), @('SPEC_HOST','E02','hard'),
  @('SPEC_PUB','E02','hard'), @('SPEC_CTRL','E02','hard'),
  @('E01','CTXENG','hard'), @('CTXENG','CTX_SUB','hard'), @('CTXENG','CTX_PUB','hard'),
  @('CTXENG','E04','hard'), @('CTX_SUB','E04','hard'), @('CTX_PUB','E04','hard'),
  @('E01R','E04','evidence'),
  @('E02','E06','hard'), @('E04','E06','hard'), @('E01R','E06','evidence'),
  @('#10872','E06','evidence'), @('#10881','E06','evidence'),
  @('E06','ROUTE','hard'), @('#11114','ROUTE','evidence'),
  @('ROUTE','COHORT','hard'), @('ROUTE','DOG','hard'), @('COHORT','DOG','hard'),
  @('#7956','POLICY','evidence')
)
# Forbidden edges (the #10918 graph regressions 2, 5 and 6): no dependency of the
# second node on the first may exist under any class.
$FORBIDDEN_EDGES = @(
  @('FIXT','RUNCONF'), @('FIXT','SUBJ_FAN'),
  @('ROOT_E_SEM','ROOT_OBS_FAN'), @('ROOT_L_SEM','ROOT_OBS_FAN'),
  @('ROOT_SEM_FAN','ROOT_OBS_FAN'), @('PROF_E','ROOT_OBS_FAN'), @('PROF_L','ROOT_OBS_FAN'),
  @('HOST_E29','ROOT_OBS_FAN'), @('HOST_E30','ROOT_OBS_FAN'),
  @('HOST_E_REL','ROOT_OBS_FAN'), @('HOST_E_SRC','ROOT_OBS_FAN'),
  @('HOST_L_REL','ROOT_OBS_FAN'), @('HOST_L_SRC','ROOT_OBS_FAN'),
  @('HOST_E29','LINUX'), @('HOST_E30','LINUX'), @('HOST_E_REL','LINUX'),
  @('HOST_E_SRC','LINUX'), @('HOST_L_REL','LINUX'), @('HOST_L_SRC','LINUX'),
  @('PROF_E','LINUX'), @('PROF_L','LINUX'), @('ROOT_E_SEM','LINUX'),
  @('ROOT_L_SEM','LINUX'), @('ROOT_SEM_FAN','LINUX')
)
if ($LAW_EDGES.Count -ne 121) { throw "law-edge table declares $($LAW_EDGES.Count) edges, expected 121" }
if ($FORBIDDEN_EDGES.Count -ne 24) { throw "forbidden-edge table declares $($FORBIDDEN_EDGES.Count) edges, expected 24" }

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

  if ($doc.schema -cne 'emacs_train.v1') { throw 'schema name mismatch' }
  if ($doc.schema_version -cne 1) { throw 'schema_version must be 1' }

  # programme block
  Assert-KeySet $doc.programme @('parent_programme_issue','controller_issue','home_programme',
    'durable_architecture_issue','durable_architecture_bundle','method_authority') 'programme'
  if ($doc.programme.parent_programme_issue -cne 7979) { throw 'parent programme issue mismatch' }
  if ($doc.programme.controller_issue -cne 8706) { throw 'programme controller_issue mismatch' }
  if ($doc.programme.home_programme -cne 'emacs-support') { throw 'home programme mismatch' }
  if ($doc.programme.durable_architecture_issue -cne 11716) { throw 'durable architecture issue mismatch' }

  # authority planes: exactly eight, fixed order
  if (@($doc.authority_planes).Count -ne 8) { throw 'expected exactly 8 authority planes' }
  for ($i = 0; $i -lt 8; $i++) {
    Assert-KeySet $doc.authority_planes[$i] @('plane','owns','never_substitutes') "authority_planes[$i]"
    if ($doc.authority_planes[$i].plane -cne $PLANES[$i]) { throw "authority plane order broken at $($i + 1)" }
    Assert-NonEmpty $doc.authority_planes[$i].owns "authority_planes[$i].owns"
    Assert-NonEmpty $doc.authority_planes[$i].never_substitutes "authority_planes[$i].never_substitutes"
  }

  # train role vocabulary: exactly eleven, fixed order
  if (@($doc.train_role_vocabulary).Count -ne 11) { throw 'expected exactly 11 train roles' }
  for ($i = 0; $i -lt 11; $i++) {
    Assert-KeySet $doc.train_role_vocabulary[$i] @('role','owns') "train_role_vocabulary[$i]"
    if ($doc.train_role_vocabulary[$i].role -cne $ROLES[$i]) { throw "train role order broken at $($i + 1)" }
    Assert-NonEmpty $doc.train_role_vocabulary[$i].owns "train_role_vocabulary[$i].owns"
  }

  # evidence semantics
  Assert-KeySet $doc.evidence_semantics @('not_proven_law','optional_visibility','metadata_only_rule') 'evidence_semantics'
  foreach ($k in @('not_proven_law','optional_visibility','metadata_only_rule')) {
    Assert-NonEmpty $doc.evidence_semantics.$k "evidence_semantics.$k"
  }

  # external authorities
  $authIds = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($a in @($doc.external_authorities)) {
    Assert-KeySet $a @('id','subject') 'external_authorities[]'
    if (-not $a.id.StartsWith('#')) { throw "external authority id must start with '#': $($a.id)" }
    if (-not $authIds.Add($a.id)) { throw "duplicate external authority: $($a.id)" }
    Assert-IsString $a.subject 'external_authorities[].subject'
    Assert-NonEmpty $a.subject 'external_authorities[].subject'
  }
  foreach ($needed in @('#3983','#3949','#4177','#3982','#3957','#3390','#3693','#6275','#5903','#6990',
                        '#7122','#7956','#9310','#9374','#9413','#10527','#10554','#10858','#10872',
                        '#10881','#10894','#10923','#10930','#11114','#EXPLICIT-AUTHORIZATION')) {
    if (-not $authIds.Contains($needed)) { throw "required external authority missing: $needed" }
  }

  # open decisions routed elsewhere: exactly five, exact owners, not decided here
  if (@($doc.open_decisions_routed_elsewhere).Count -ne 5) { throw 'expected exactly 5 open decisions' }
  for ($i = 0; $i -lt 5; $i++) {
    $od = $doc.open_decisions_routed_elsewhere[$i]
    Assert-KeySet $od @('id','subject','owner') "open_decisions[$i]"
    if ($od.id -cne $OPEN_DECISION_OWNERS[$i][0]) { throw "open decision id order broken at $($i + 1)" }
    if ($od.owner -cne $OPEN_DECISION_OWNERS[$i][1]) { throw "open decision $($od.id) owner mismatch" }
    Assert-IsString $od.subject "open_decisions[$i].subject"
    Assert-NonEmpty $od.subject "open_decisions[$i].subject"
  }

  # canonical existing-candidate adoption rule (regression 1)
  Assert-KeySet $doc.existing_candidate_adoption @('node','candidate_pull','confirm_with','rule') 'existing_candidate_adoption'
  if ($doc.existing_candidate_adoption.node -cne 'FIXT') { throw 'candidate adoption must name node FIXT (#11366)' }
  if ($doc.existing_candidate_adoption.candidate_pull -cne 8026) { throw 'candidate adoption must name pull 8026' }
  if ($doc.existing_candidate_adoption.confirm_with -cne '#10930') { throw 'candidate adoption must be confirmed by #10930' }
  Assert-NonEmpty $doc.existing_candidate_adoption.rule 'existing_candidate_adoption.rule'

  # revision governance
  Assert-KeySet $doc.revision_governance @('owner_node','owner_issue','invalidates','never','metadata_only') 'revision_governance'
  if ($doc.revision_governance.owner_node -cne 'E01R' -or $doc.revision_governance.owner_issue -cne 11770) {
    throw 'revision governance must be owned by E01R #11770'
  }
  Assert-NonEmpty $doc.revision_governance.invalidates 'revision_governance.invalidates'
  Assert-NonEmpty $doc.revision_governance.never 'revision_governance.never'
  Assert-NonEmpty $doc.revision_governance.metadata_only 'revision_governance.metadata_only'

  # nodes: exact expected set, uniqueness, per-node contract completeness
  $EXPECTED_ISSUES = [System.Collections.Generic.HashSet[long]]::new()
  foreach ($v in $EXPECTED_NODES.Values) { [void]$EXPECTED_ISSUES.Add([long]$v) }
  $EXPECTED_IDS = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($k in $EXPECTED_NODES.Keys) { [void]$EXPECTED_IDS.Add($k) }
  $nodes = @($doc.nodes)
  if ($nodes.Count -ne 55) { throw "expected exactly 55 nodes, found $($nodes.Count)" }
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
    if ($seenSuperseded.ContainsKey($s.superseded_node)) { throw "duplicate supersession for node: $($s.superseded_node)" }
    $seenSuperseded[[string]$s.superseded_node] = $true
    if (-not $EXPECTED_ISSUES.Contains([long]$s.successor_issue)) { throw "supersession names unknown successor issue: $($s.successor_issue)" }
    if ([long]$s.successor_issue -eq [long]$byId[[string]$s.superseded_node].issue) { throw 'successor issue must differ from the superseded node own issue' }
  }

  foreach ($n in $nodes) {
    $id = [string]$n.node_id
    # JSON shape: exact types before content checks
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
    # chain law: the parent programme is its own controller; every other node hangs from CTRL
    Assert-KeySet $n.chain @('home','controller') "node $id chain"
    $wantController = 'PROG'
    if ($id -cne 'PROG') { $wantController = 'CTRL' }
    if ($n.chain.controller -cne $wantController) { throw "node $id chain controller mismatch" }
    if ($n.chain.home -cne 'emacs-support') { throw "node $id chain home mismatch" }
    # writer law: capacity classes from the #10918 model are identities, not permissions
    Assert-KeySet $n.writer @('conflict_key','parallel_group','stack_relation') "node $id writer"
    Assert-NonEmpty $n.writer.conflict_key "node $id conflict_key"
    if ($n.writer.parallel_group -cnotin $GROUPS) { throw "unknown writer capacity class at ${id}: $($n.writer.parallel_group)" }
    # spec law: detailed leaf specs are referenced to #11717, never duplicated (regression 13)
    Assert-KeySet $n.spec $SPEC_KEYS "node $id spec"
    if ($n.spec.disposition -cnotin $DISPOSITIONS) { throw "unknown disposition at ${id}: $($n.spec.disposition)" }
    if ($n.spec.owner -cne $id) { throw "node $id spec owner must be itself" }
    Assert-NonEmpty $n.spec.stale_policy "node $id stale_policy"
    if ($n.spec.spec_authority -cne '#11717') { throw "node $id spec authority must reference #11717" }
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
    if (([string]$n.one_pr_outcome).Length -gt 240) { throw "node $id one_pr_outcome duplicates leaf-spec prose (over 240 chars)" }
    Assert-NonEmpty $n.authority_before "node $id authority_before"
    Assert-NonEmpty $n.authority_after "node $id authority_after"
    Assert-NonEmpty $n.claim_ceiling "node $id claim_ceiling"
    Assert-NonEmpty $n.first_falsifier "node $id first_falsifier"
    if (@($n.identity_fields).Count -lt 1) { throw "node $id lacks identity_fields" }
    if (@($n.allowed_components).Count -lt 1) { throw "node $id lacks allowed_components" }
    if (@($n.forbidden_adjacent_owners).Count -lt 1) { throw "node $id lacks forbidden_adjacent_owners" }
    # path neutrality (regression 14): semantic names only, never source paths
    foreach ($s in @($n.identity_fields) + @($n.allowed_components)) {
      Assert-IsString $s "node $id identity/component entry"
      if ($s.Contains('/') -or $s -cmatch '\.(rs|el|toml|json)$') { throw "source path treated as semantic identity at ${id}: $s" }
    }
    foreach ($a in @($n.consumed_authorities)) {
      Assert-IsString $a "node $id consumed authority entry"
      if (-not $authIds.Contains($a)) { throw "node $id consumes unknown authority: $a" }
    }
    # dependencies: exactly one edge per target; typed classes; closed provenance
    $depTargets = New-OrdinalTable
    foreach ($d in @($n.dependencies)) {
      Assert-IsObject $d "node $id dependency"
      Assert-KeySet $d $DEP_KEYS "node $id dependency"
      foreach ($df in @('target','class','provenance')) { Assert-IsString $d.$df "node $id dependency $df" }
      Assert-NonEmpty $d.provenance "node $id dependency provenance"
      if ($d.provenance -cnotin $PROVENANCE) {
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
      # external authorization law (regression 11): gate-only, external-class
      if ($t -ceq '#EXPLICIT-AUTHORIZATION') {
        if ($d.class -cne 'external') { throw "node $id carries #EXPLICIT-AUTHORIZATION as $($d.class); it is external-class only" }
        if ($n.train_role -cne 'external_gate') { throw "node $id carries #EXPLICIT-AUTHORIZATION without the external_gate role" }
      }
      # optional-breadth law (regression 9): child-train references stay optional
      if ($OPTIONAL_AUTHORITIES -ccontains $t -and $d.class -cne 'optional') {
        throw "optional breadth authority ${t} appears as $($d.class) at node $id; it must stay optional"
      }
    }
  }

  # controller/fan_in/external_gate law: they never enter ordinary builder frontiers
  foreach ($n in $nodes) {
    $role = [string]$n.train_role
    if ($role -cin @('controller','fan_in','external_gate')) {
      if ($n.buildable) { throw "${role} node $($n.node_id) must not be buildable" }
    } else {
      if (-not $n.buildable) { throw "${role} node $($n.node_id) must be buildable as one one-PR proposition" }
    }
  }
  if (-not ($byId['CTRL'].train_role -ceq 'controller')) { throw 'CTRL must carry train role controller' }
  if (-not ($byId['PROG'].train_role -ceq 'controller')) { throw 'PROG must carry train role controller' }

  # strict fan-in law (regression 8): complete hard denominators only
  foreach ($fid in @('REG','DOCS','CERT')) {
    $fnode = $byId[$fid]
    foreach ($d in @($fnode.dependencies)) {
      if (-not ([string]$d.target).StartsWith('#') -and $d.class -cne 'hard') {
        throw "strict fan-in node $fid carries a $($d.class) node edge; its denominator must be all-hard"
      }
    }
  }
  foreach ($fid in @('REG','CERT')) {
    if ([string]$byId[$fid].claim_ceiling -notmatch 'denominator') { throw "strict fan-in node $fid must bound its complete denominator in its claim ceiling" }
  }
  # substrate promotion bound (regression 7)
  if ([string]$byId['LINUX'].claim_ceiling -notmatch 'substrate') { throw 'LINUX must bound its claim ceiling to install and fresh-process substrate' }
  # routing-policy authority bound (regression 10)
  if ([string]$byId['POLICY'].train_role -cne 'evidence_policy') { throw 'POLICY (#9375) must keep the evidence_policy role' }
  if ([string]$byId['POLICY'].claim_ceiling -notmatch 'receipt or support authority') { throw 'POLICY must explicitly renounce receipt and support authority' }

  # stage-ceiling law (#10918 acceptance: stage-inflation mutations must fail):
  # subjects earn no support claim, profiles prove shapes only, host passes are
  # never public artifacts, and public replays earn no registry completion.
  $STAGE_CEILING_LAWS = @(
    @{ nodes = @('SUBJ_CORE','SUBJ_E','SUBJ_L'); pattern = 'support claim' },
    @{ nodes = @('PROF_E','PROF_L'); pattern = 'negotiation and result shapes only' },
    @{ nodes = @('HOST_E29','HOST_E30','HOST_E_REL','HOST_E_SRC','HOST_L_REL','HOST_L_SRC'); pattern = 'never a public artifact' },
    @{ nodes = @('PUB_E_B','PUB_E_R','PUB_L_R'); pattern = 'registry completion' }
  )
  foreach ($law in $STAGE_CEILING_LAWS) {
    foreach ($sid in $law.nodes) {
      if ([string]$byId[$sid].claim_ceiling -notmatch $law.pattern) { throw "stage ceiling inflation at ${sid}: the claim ceiling must bound ($($law.pattern))" }
    }
  }

  # successors must be exactly the derived reverse edge set (bidirectional traceability)
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
  # forbidden edges stay absent (regressions 2, 5, 6)
  foreach ($fe in $FORBIDDEN_EDGES) {
    $from = $fe[0]; $to = $fe[1]
    if (@($byId[$to].dependencies | Where-Object { $_.target -ceq $from }).Count -gt 0) {
      throw "forbidden dependency present: ${from} -> ${to}"
    }
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

# regression 1: the existing candidate is encoded as a stable node
Invoke-NegativeControl 'R01-candidate-as-node' { param($d) $d.nodes += [pscustomobject]@{ node_id = 'PR8026'; issue = 8026; title = 'x'; title_fingerprint = '00'; aliases = @(); train_role = 'implementation'; lane = 'foundation'; chain = [pscustomobject]@{ home = 'emacs-support'; controller = 'CTRL' }; one_pr_outcome = 'reuse'; authority_before = 'a'; authority_after = 'b'; buildable = $true; dependencies = @(); claim_ceiling = 'c'; writer = [pscustomobject]@{ conflict_key = 'k'; parallel_group = 'none'; stack_relation = 'none' }; consumed_authorities = @(); allowed_components = @('c'); forbidden_adjacent_owners = @('x'); spec = [pscustomobject]@{ disposition = 'ISSUE_PLAN_SUFFICIENT'; owner = 'PR8026'; stale_policy = 's'; spec_authority = '#11717' }; first_falsifier = 'f'; controls = [pscustomobject]@{ positive = 'p'; opposite = 'o'; stale = 's'; wrong_subject = 'w'; fault = 'f'; mutation = 'm' }; proof = [pscustomobject]@{ focused = 'p'; routed = 'r' }; review_forward = [pscustomobject]@{ questions = @('q'); lenses = @('l') }; obligations = [pscustomobject]@{ schema = 's'; generated = 'g'; docs = 'd'; changelog = 'c'; receipt = 'r' }; exits = [pscustomobject]@{ old_path = 'o'; compatibility = 'c'; supersession = 's'; transfer = 't' }; rollback = [pscustomobject]@{ rollback = 'r'; return_to_issue = 'i'; not_proven = 'n'; stop = 's' }; successors = @(); identity_fields = @('i'); limitations = @('l') } }
# regression 2: the fixture substrate blocks runner conformance or subject fan-in
Invoke-NegativeControl 'R02-fixture-blocks-foundation' { param($d) (Find-Node $d 'RUNCONF').dependencies += [pscustomobject]@{ target = 'FIXT'; class = 'hard'; provenance = '#10918 body corrected functional DAG' } }
# regression 3: an adapter invents host-observation semantics
Invoke-NegativeControl 'R03-adapter-private-observation' { param($d) $dep = Find-Dep (Find-Node $d 'ADP_E') 'OBS'; (Find-Node $d 'ADP_E').dependencies = @((Find-Node $d 'ADP_E').dependencies | Where-Object { $_.target -cne 'OBS' }) }
# regression 4: a journey leaf emits durable pass cells without the producer
Invoke-NegativeControl 'R04-host-without-producer' { param($d) (Find-Node $d 'HOST_E29').dependencies = @((Find-Node $d 'HOST_E29').dependencies | Where-Object { $_.target -cne 'PROD' }) }
# regression 5: the observation fan-in depends on completed semantic verdicts
Invoke-NegativeControl 'R05-observation-needs-semantics' { param($d) (Find-Node $d 'ROOT_OBS_FAN').dependencies += [pscustomobject]@{ target = 'ROOT_E_SEM'; class = 'hard'; provenance = '#10918 body corrected functional DAG' } }
# regression 6: the Linux substrate waits for semantic journeys
Invoke-NegativeControl 'R06-substrate-waits-for-journeys' { param($d) (Find-Node $d 'LINUX').dependencies += [pscustomobject]@{ target = 'HOST_E29'; class = 'hard'; provenance = '#10918 body corrected functional DAG' } }
# regression 7: substrate success promotes public semantic rows
Invoke-NegativeControl 'R07-substrate-promotes-semantics' { param($d) (Find-Node $d 'LINUX').claim_ceiling = 'proves public semantic rows for every client family' }
# regression 8: a partial denominator enables a complete-cut claim
Invoke-NegativeControl 'R08-partial-denominator' { param($d) (Find-Node $d 'REG').dependencies = @((Find-Node $d 'REG').dependencies | Where-Object { $_.target -cne 'PUB_L_R' }) }
# regression 9: optional breadth becomes an initial-Linux hard dependency
Invoke-NegativeControl 'R09-optional-becomes-hard' { param($d) (Find-Dep (Find-Node $d 'CERT') '#9310').class = 'hard' }
# regression 10: the routing policy becomes a second receipt or support authority
Invoke-NegativeControl 'R10-policy-as-support-authority' { param($d) (Find-Node $d 'POLICY').train_role = 'implementation' }
# regression 11: external action enters the ordinary frontier without authorization
Invoke-NegativeControl 'R11-unauthorized-external-action' { param($d) (Find-Node $d 'SUBJ_CORE').dependencies += [pscustomobject]@{ target = '#EXPLICIT-AUTHORIZATION'; class = 'hard'; provenance = '#10918 body canonical seed graph' } }
# regression 12: live state enters stable bytes (value level, then byte level)
Invoke-NegativeControl 'R12-live-state-value' { param($d) (Find-Node $d 'OBS').limitations += ' rebased onto head 4f5bcb334 deadbeef 00ff11' }
$badLine = 'x deadbeefdeadbeef y'
if ($badLine -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { $controls.Add('R12b-live-state-bytes') } else { throw 'R12b raw-byte live-state scan failed to detect a token' }
# regression 13: a detailed leaf spec is duplicated instead of referenced to #11717
Invoke-NegativeControl 'R13-spec-duplicated' { param($d) (Find-Node $d 'SUBJ_FAN').spec.spec_authority = '#10918' }
# regression 14: a current source file becomes stable semantic identity
Invoke-NegativeControl 'R14-source-path-identity' { param($d) (Find-Node $d 'SUBJ_CORE').allowed_components += 'crates/perl-lsp/src/subject.rs' }

# acceptance-bullet mutation classes beyond the numbered regressions:
# stage inflation, duplicate owner, hard cycle and controller selection
Invoke-NegativeControl 'A01-stage-inflation' { param($d) (Find-Node $d 'HOST_E30').claim_ceiling = 'local passes promote the public artifact directly' }
Invoke-NegativeControl 'A02-duplicate-owner' { param($d) (Find-Node $d 'HOST_E_REL').authority_after = (Find-Node $d 'HOST_E29').authority_after }
Invoke-NegativeControl 'A03-hard-cycle' { param($d) (Find-Node $d 'PROF_E').dependencies += [pscustomobject]@{ target = 'HOST_E29'; class = 'hard'; provenance = '#10918 body corrected functional DAG' } }
Invoke-NegativeControl 'A04-controller-buildable' { param($d) (Find-Node $d 'CTRL').buildable = $true }

# order-invariance control: the canonical digest must not move with input order
$orderDigest = $semanticDigest
$shuffled = Copy-Doc $doc
$shuffled.nodes = @($shuffled.nodes | Sort-Object -Property issue -Descending)
foreach ($n in @($shuffled.nodes)) {
  $n.dependencies = @($n.dependencies | Sort-Object -Property class -Descending)
  $n.successors = @($n.successors | Sort-Object -Descending)
  $n.identity_fields = @($n.identity_fields | Sort-Object -Descending)
}
$shuffledDigest = Invoke-TrainValidation $shuffled
if ($shuffledDigest -cne $orderDigest) { throw 'order-invariance control failed: canonical digest changed with input order' }
$controls.Add('ORDER-CANONICAL-DIGEST')

# Twenty fail-closed mutation controls cover the fourteen #10918 graph
# regressions (regression 12 carries both a value-level and a byte-level
# control), the four acceptance-bullet mutation classes beyond the numbered
# list (stage inflation, duplicate owner, hard cycle, controller selection;
# false total-order and candidate-node are already exercised by R02 and R01),
# and the order-invariance canonicalization control.
if ($controls.Count -ne 20) { throw "expected 20 negative controls, ran $($controls.Count)" }

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
foreach ($term in @('emacs_train.v1','`.spec/11716-emacs-support-architecture/`','#8706','#10918',
                    'authority planes','train roles','not_proven','OD1','OD5','#11768','#11770',
                    'cargo xtask integration emacs train check','#11114')) {
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
foreach ($term in @('emacs_train.v1','#11716','#11764','train.manifest.json','55 nodes','124 typed edges',
                    '#11717','#11361','#8842','#9375','#11718')) {
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
Write-Output "SPEC_10918_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_10918_NEGATIVE_CONTROLS=20/20"
Write-Output "SPEC_10918_SEMANTIC_SHA256=$semanticDigest"
Write-Output "SPEC_10918_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice. Requirements for a valid proof:

1. both runs print `SPEC_10918_STRUCTURAL_CHECK=PASS`;
2. both runs print `SPEC_10918_NEGATIVE_CONTROLS=20/20`;
3. both runs print the same `SPEC_10918_SEMANTIC_SHA256` and
   `SPEC_10918_BUNDLE_SHA256` fingerprints;
4. the full captured output of both runs is byte-identical;
5. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
6. `git diff --check` (staged) is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves manifest shape, node-set completeness, edge
typing, provenance discipline, graph-law freezing, forbidden-edge absence,
uniqueness laws, controller/gate laws, writer capacity classes, claim-ceiling
and spec-authority discipline, durable-byte hygiene, fail-closed behavior of
all fourteen #10918 graph regressions, order-invariant canonicalization, and
byte-level determinism across two runs. It does **not** prove: that the
topology is the semantically correct reading of every leaf body (that is
this PR's review job, and E01R's after); that the xtask validation operations
named by #10918 exist or pass (unbuilt tooling, a separate claim); that the
graph stays current as issues evolve (E01R owns invalidation); or that any
Emacs behavior, subject, journey, public artifact, registry row or support
claim holds (the lanes own those). The repository's absent executable Emacs
train validator remains an open tooling gap recorded here rather than
papered over.

## Flags for builder

- Deviation note: the controlling issue names offline xtask operations
  (`cargo xtask integration emacs train check` / `graph`). Those are
  executable repository tooling and are not built in this bundle-style
  claim; the manifest lands as checked data plus this embedded checker, and
  the absent validator is recorded as `not_proven`. A later tooling claim
  against the same seam must consume `emacs_train.v1` as data, not clone it.
- The canonical existing-candidate rule for #11366 is stable law; its live
  confirmation (whether pull 8026 remains a live candidate for the closed
  substrate leaf) belongs to #10930 and never enters manifest bytes.
- If a downstream check can only pass by weakening a law here, stop and
  return to #10918 rather than editing the law locally.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor manifest version supersedes this one only
  through an E01R-classified revision with an exact successor recorded;
  derived artifacts are re-derived, never patched valid.
- **Stop:** stop before validator commands, current-tree probes, frontier,
  source-context resolution, live observation, packet rendering, GitHub
  metadata work, host execution, dogfood, scheduling, support claims,
  release or publication. If an open decision OD1–OD5 is needed as a
  decision rather than a boundary, stop and route it to its owning issue; do
  not decide it in a builder PR.
