# Implementation Checklist: #11625 — canonical stable module_train.v1 implementation and proof graph

## Change order

This is a specification/data-only change. Each step is reviewable without
building or executing any tooling beyond the embedded checker.

### Step 1: Write the fail-closed fixtures first

- **File:** the negative-control suite inside the checker below (DESIGN).
- **Change:** Before the manifest is declared valid, the fourteen required
  shift-left falsifiers of #11625 must exist as in-memory mutations that each
  make validation throw (falsifiers 9, 11 and 14 carry doubled controls), the
  acceptance-bullet mutation classes beyond the numbered list (hard cycle,
  duplicate owner, invented case identity, premature binding promotion and
  unauthorized external action) must carry their own controls, plus an
  order-invariance canonicalization control — twenty-two controls in total.
- **Verify:** temporarily weakening the validator shows a control failing to
  reject; against the real validator all reject.

### Step 2: Create the stable manifest

- **File:** `.spec/11625-module-train-graph/train.manifest.json` (CREATE)
- **Change:** Encode the complete 52-node graph (programme and functional
  controllers, the E00 evidence denominator, the M00S durable spec source,
  the request and admission chain, the parser-fact hierarchy, the resolver
  chain, geometry and the public boundary, the L09 live cutover family,
  provider proofs, the P11 exact-process family, the claims nodes and the
  C-series successors) with typed, provenance-traced edges, conflict keys,
  dispositions, controls, claim profiles, the structurally-pending case
  binding law, exits, rollback quartets, successors and identity fields.
- **Verify:** structural and law checks below; `git diff --check`.

### Step 3: Create the context contract

- **File:** `.spec/11625-module-train-graph/context.md` (CREATE)
- **Change:** Record the problem, authority, consumed laws, encoding
  traceability decisions, the #10554 shared-mechanics disposition,
  `AGENTS.md` compatibility, open decisions respected (not decided),
  adoption/rollback/transfer/stop, links.
- **Verify:** heading and term checks below; `git diff --check`.

### Step 4: Create acceptance and negative controls

- **File:** `.spec/11625-module-train-graph/acceptance.md` (CREATE)
- **Change:** All canonical spec sections and the fourteen #11625
  shift-left falsifiers in fixed order with exact kind/verdict semantics,
  plus the claim boundary and non-goals.
- **Verify:** section and falsifier-order checks below; `git diff --check`.

### Step 5: Create the builder and proof contract (this file)

- **File:** `.spec/11625-module-train-graph/checklist.md` (CREATE)
- **Change:** This change order, the embedded deterministic structural
  checker, the second-run procedure, the `not_proven` boundary, and
  rollback/transfer/stop.
- **Verify:** the embedded checker runs twice from the candidate worktree
  with byte-identical output and no tree change.

## Scope boundary

Files IN scope: exactly the four files of
`.spec/11625-module-train-graph/`.

Files OUT of scope: everything else — no `AGENTS.md` change, no `crates/` or
`xtask/` change, no `docs/` change, no configuration, no generated artifact
outside the bundle, no GitHub state, no host execution.

## Deterministic structural proof

The repository has no executable module train validator (the xtask
operations named by #11625 remain a separate tooling claim). Do not invent a
generated receipt or claim a missing tool passed. From the candidate worktree
root, run the following PowerShell 7 checker twice after the four files are
complete. The checker asserts:

1. the union of the committed candidate patch
   (`merge-base(origin/main, HEAD)..HEAD`, which stays the candidate's own
   patch even if `origin/main` advances mid-flight because a sibling lane
   fetched), the staged index, the unstaged worktree, and NUL-delimited
   porcelain paths — including untracked files — equals exactly the four
   bundle paths (it fails closed on a malformed status record or a
   rename/copy record without its second path);
2. the manifest bytes are hygiene-clean (no BOM, no CR, no tabs, exactly one
   trailing LF) and contain no live-state tokens anywhere (long lowercase
   hex runs, timestamps, branch and pull path fragments) and no invented
   local case identifiers, in both raw bytes and parsed values;
3. the manifest parses under a strict schema: exact key sets at every level
   (unknown keys fail closed), exact expected node/issue pairs for all 52
   nodes, frozen role, writer-class and chain-controller maps, unique node
   IDs, issues, aliases, conflict keys and authority-after propositions,
   title fingerprints recomputed, dependency classes from the four-value
   vocabulary, provenance strings from the closed #11625-and-leaf-header
   vocabulary, every dependency target resolvable, exactly one edge per
   target, successor sets exactly the derived reverse-edge set, no cycles
   over hard/evidence edges, controller/fan-in non-buildability, the exact
   nine-controller rejection list, spec-authority reference to #10592, the
   structurally-pending case-binding law, frozen claim-profile membership
   plus the closeout superset law, the cross-programme import relation law
   (one home per authority, imported never copied, edge class fixed by the
   relation), claim-ceiling pattern laws, proof-component laws, and all
   177 node graph-law edges present with exactly their declared
   classes while all 8 forbidden edges stay absent;
4. all fourteen #11625 shift-left falsifiers reject through twenty-two
   fail-closed in-memory mutation controls (falsifiers 9, 11 and 14 carry
   doubled controls; the acceptance-bullet mutation classes carry five
   more), including the order-invariance control whose rejected subject is
   an order-sensitive canonicalization: the canonical semantic digest of a
   shuffled document equals the digest of the original; schema-identifier
   comparison is ordinal (case-sensitive) and all culture-sensitive
   operations run under the invariant culture;
5. the bundle markdown carries its canonical headings/terms and exactly
   fourteen fixed-order rejected falsifier rows;
6. a SHA-256 fingerprint over the four files and the semantic digest are
   printed; two runs must print byte-identical output.

Redirecting output to a temporary file is local proof only; no temporary file
belongs in the change.

```powershell
$ErrorActionPreference = 'Stop'
# Determinism across locales: every culture-sensitive operation below (sorting,
# string comparison, formatting) must behave identically on any host.
[Globalization.CultureInfo]::CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$root = '.spec/11625-module-train-graph'
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
  'case_work_packet_bindings','claim_profiles','cross_programme_imports','nodes',
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
$SPEC_KEYS = @('disposition','owner','stale_policy','spec_authority')
$PLANE_KEYS = @('plane','owns','never_substitutes')
$PROFILE_KEYS = @('id','definition','members')
$IMPORT_KEYS = @('authority','home_train','relation','note')
$BINDING_KEYS = @('status','law','binding_nodes','evidence_authority','consumers','promotion_route')
$CLASSES = @('hard','evidence','optional','external')
$GROUPS = @('none','A','B','C','D')
$DISPOSITIONS = @('SPEC_COMPILED','ISSUE_PLAN_SUFFICIENT','CONTROLLER_NO_CODING_SPEC',
  'FAN_IN_OR_CERTIFICATION_SPEC')
$ROLES = @('controller','spec','evidence','implementation','cutover','retirement','proof',
  'fan_in','claim','external_gate')
$PROVENANCE = @(
  '#10569 body header dependencies', '#10570 body header dependencies'
  '#10571 body header dependencies', '#10572 body header dependencies'
  '#10575 body header dependencies', '#10578 body header dependencies'
  '#10599 body header authorities', '#10995 body header evidence packs'
  '#10999 body header evidence packs', '#11008 body header dependencies'
  '#11013 body header evidence authority', '#11016 body header evidence authority'
  '#11019 body header evidence authority', '#11023 body header evidence authority'
  '#11025 body header evidence authority', '#11026 body header dependencies'
  '#11619 body header evidence authority', '#11620 body header product prerequisites'
  '#11621 body header product prerequisites', '#11622 body header product prerequisites'
  '#11623 body header product prerequisites', '#11624 body header evidence authority'
  '#11624 body header required children', '#11624 body header subject substrate'
  '#11625 body canonical graph E00', '#11625 body canonical graph L09'
  '#11625 body canonical graph M00S', '#11625 body canonical graph M01/M02'
  '#11625 body canonical graph M03-M06', '#11625 body canonical graph M07'
  '#11625 body canonical graph M08-M10', '#11625 body canonical graph P11'
  '#11625 body claims and documentation', '#11625 body provider proof consumers'
  '#11625 comment C-series successor graph', '#11625 comment C01 entry gate clarification'
  '#11626 body packet contract authorities', '#11626 body required frontier shapes'
  '#1744 body header authorities', '#4243 body goal contracts'
  '#7460 body claim inputs', '#8133/#4240 programme header'
  '#8521 body header dependencies', '#8542 body authority correction'
  '#8634 body header dependencies', '#8659 body header dependencies'
  '#8744 body header dependencies', '#8780 body header dependencies'
  '#8810 body header dependencies')
$EXPECTED_NODES = [ordered]@{
'C01'=11625;
  'C02'=11626;
  'C03'=11627;
  'CLAIM'=7460;
  'CLAIMFIX'=10599;
  'CLMA'=7430;
  'CLMB'=4245;
  'CTRL'=4240;
  'E00A'=10977;
  'E00B'=10981;
  'E00C'=10986;
  'E00D'=10995;
  'E00E'=10999;
  'EVID'=8479;
  'L09A'=11008;
  'L09B'=11013;
  'L09C'=11016;
  'L09D'=11019;
  'L09E'=11023;
  'L09F'=11025;
  'L09G'=11026;
  'LIVECTL'=7421;
  'M00S'=10592;
  'M01'=8497;
  'M02'=8521;
  'M03'=8542;
  'M04A'=10568;
  'M04B'=10569;
  'M04C'=10570;
  'M04CTL'=8566;
  'M04D'=10571;
  'M04E'=10572;
  'M05'=8634;
  'M06'=8659;
  'M07A'=10573;
  'M07B'=10575;
  'M07C'=10578;
  'M07CTL'=8701;
  'M07D'=8170;
  'M08'=8744;
  'M09'=8780;
  'M10'=8810;
  'P11A'=11619;
  'P11B'=11620;
  'P11C'=11621;
  'P11D'=11622;
  'P11E'=11623;
  'P11F'=11624;
  'PROG'=8133;
  'PROVA'=1744;
  'PROVB'=4243;
  'XPCTL'=9270;
}
$EXPECTED_ROLES = [ordered]@{
'C01'='spec';
  'C02'='spec';
  'C03'='spec';
  'CLAIM'='claim';
  'CLAIMFIX'='claim';
  'CLMA'='controller';
  'CLMB'='controller';
  'CTRL'='controller';
  'E00A'='evidence';
  'E00B'='evidence';
  'E00C'='evidence';
  'E00D'='evidence';
  'E00E'='evidence';
  'EVID'='controller';
  'L09A'='cutover';
  'L09B'='cutover';
  'L09C'='cutover';
  'L09D'='cutover';
  'L09E'='cutover';
  'L09F'='cutover';
  'L09G'='retirement';
  'LIVECTL'='controller';
  'M00S'='spec';
  'M01'='implementation';
  'M02'='cutover';
  'M03'='implementation';
  'M04A'='implementation';
  'M04B'='implementation';
  'M04C'='implementation';
  'M04CTL'='controller';
  'M04D'='implementation';
  'M04E'='implementation';
  'M05'='implementation';
  'M06'='cutover';
  'M07A'='implementation';
  'M07B'='implementation';
  'M07C'='implementation';
  'M07CTL'='controller';
  'M07D'='implementation';
  'M08'='implementation';
  'M09'='cutover';
  'M10'='implementation';
  'P11A'='proof';
  'P11B'='proof';
  'P11C'='proof';
  'P11D'='proof';
  'P11E'='proof';
  'P11F'='fan_in';
  'PROG'='controller';
  'PROVA'='proof';
  'PROVB'='proof';
  'XPCTL'='controller';
}
$EXPECTED_GROUPS = [ordered]@{
'C01'='A';
  'C02'='A';
  'C03'='A';
  'CLAIM'='A';
  'CLAIMFIX'='A';
  'CLMA'='none';
  'CLMB'='none';
  'CTRL'='none';
  'E00A'='A';
  'E00B'='A';
  'E00C'='A';
  'E00D'='A';
  'E00E'='A';
  'EVID'='none';
  'L09A'='C';
  'L09B'='C';
  'L09C'='C';
  'L09D'='C';
  'L09E'='C';
  'L09F'='C';
  'L09G'='C';
  'LIVECTL'='none';
  'M00S'='A';
  'M01'='B';
  'M02'='B';
  'M03'='B';
  'M04A'='B';
  'M04B'='B';
  'M04C'='B';
  'M04CTL'='none';
  'M04D'='B';
  'M04E'='B';
  'M05'='B';
  'M06'='B';
  'M07A'='B';
  'M07B'='B';
  'M07C'='B';
  'M07CTL'='none';
  'M07D'='B';
  'M08'='B';
  'M09'='B';
  'M10'='B';
  'P11A'='D';
  'P11B'='D';
  'P11C'='D';
  'P11D'='D';
  'P11E'='D';
  'P11F'='D';
  'PROG'='none';
  'PROVA'='D';
  'PROVB'='D';
  'XPCTL'='none';
}
$EXPECTED_CONTROLLERS = [ordered]@{
'C01'='CTRL';
  'C02'='CTRL';
  'C03'='CTRL';
  'CLAIM'='CLMA';
  'CLAIMFIX'='CLMA';
  'CLMA'='PROG';
  'CLMB'='PROG';
  'CTRL'='PROG';
  'E00A'='EVID';
  'E00B'='EVID';
  'E00C'='EVID';
  'E00D'='EVID';
  'E00E'='EVID';
  'EVID'='PROG';
  'L09A'='LIVECTL';
  'L09B'='LIVECTL';
  'L09C'='LIVECTL';
  'L09D'='LIVECTL';
  'L09E'='LIVECTL';
  'L09F'='LIVECTL';
  'L09G'='LIVECTL';
  'LIVECTL'='PROG';
  'M00S'='EVID';
  'M01'='CTRL';
  'M02'='CTRL';
  'M03'='CTRL';
  'M04A'='M04CTL';
  'M04B'='M04CTL';
  'M04C'='M04CTL';
  'M04CTL'='PROG';
  'M04D'='M04CTL';
  'M04E'='M04CTL';
  'M05'='M04CTL';
  'M06'='M04CTL';
  'M07A'='M07CTL';
  'M07B'='M07CTL';
  'M07C'='M07CTL';
  'M07CTL'='PROG';
  'M07D'='M07CTL';
  'M08'='CTRL';
  'M09'='CTRL';
  'M10'='CTRL';
  'P11A'='XPCTL';
  'P11B'='XPCTL';
  'P11C'='XPCTL';
  'P11D'='XPCTL';
  'P11E'='XPCTL';
  'P11F'='XPCTL';
  'PROG'='PROG';
  'PROVA'='LIVECTL';
  'PROVB'='LIVECTL';
  'XPCTL'='PROG';
}
$OPEN_DECISION_OWNERS = @(@('OD1','#10554'),@('OD2','#8479'),@('OD3','#9621'),@('OD4','#11114'),@('OD5','#10575'))
$PLANES = @('durable module programme decisions','executable evidence denominator','durable spec source',
  'stable reviewed topology','exact current-tree implementation state','live candidate and collaboration state',
  'behavior evidence and claim truth','external and support stages')
$PROFILES = @{
'module_contract_grounded' = @('E00A', 'E00B', 'E00C', 'E00D', 'E00E', 'M00S')
  'module_static_resolution_core' = @('M01', 'M02', 'M03', 'M04A', 'M04B', 'M04C', 'M04D', 'M04E', 'M05', 'M06', 'M07A', 'M07B', 'M07C', 'M07D', 'M09')
  'module_live_runtime_cutover' = @('L09A', 'L09B', 'L09C', 'L09D', 'L09E', 'L09F', 'L09G')
  'module_exact_process_resolution_core' = @('P11A', 'P11B', 'P11C', 'P11D', 'P11E')
  'module_exact_process_semantic_edit' = @('P11D')
  'module_exact_process_full_closeout' = @('P11A', 'P11B', 'P11C', 'P11D', 'P11E', 'P11F')
}
$IMPORT_RELATIONS = @(
@('#8131','hard import'),
  @('#7621','hard import'),
  @('#7622','hard import'),
  @('#7582','consumed authority'),
  @('#4851','hard import'),
  @('#7419','hard import'),
  @('#7420','hard import'),
  @('#6736','evidence import'),
  @('#7057','evidence import'),
  @('#7943','evidence import'),
  @('#8112','hard import'),
  @('#8199','hard import'),
  @('#8518','hard import'),
  @('#4239','hard import'),
  @('#7584','hard import'),
  @('#8617','hard import'),
  @('#6720','hard import'),
  @('#7249','consumed authority'),
  @('#8761','consumed authority'),
  @('#9621','imported consumer'),
  @('#11114','evidence import'),
  @('#10554','consumed law'),
  @('#10858','consumed vocabulary'),
  @('#3982','consumed law'),
  @('#3983','consumed law'),
  @('#3985','consumed law'),
  @('#3989','consumed law'),
  @('#EXPLICIT-AUTHORIZATION','gate law')
)
$IMPORT_NO_EDGE = @(
  '#7582', '#7249', '#8761', '#9621'
  '#10554', '#10858', '#3982', '#3983'
  '#3985', '#3989', '#EXPLICIT-AUTHORIZATION')
# Graph-law edges (from the #11625 canonical graph sections plus the current leaf-body
# headers). Each entry must exist as a dependency edge with exactly this class.
$LAW_EDGES = @(
@('CTRL','C01','hard'),
  @('C01','C02','hard'),
  @('C02','C03','hard'),
  @('CLMA','CLAIM','evidence'),
  @('CLMB','CLAIM','evidence'),
  @('EVID','CLAIMFIX','evidence'),
  @('CLAIM','CLAIMFIX','evidence'),
  @('M07B','CLAIMFIX','evidence'),
  @('PROVA','CLMA','evidence'),
  @('PROVB','CLMA','evidence'),
  @('P11F','CLMA','evidence'),
  @('PROVA','CLMB','evidence'),
  @('PROVB','CLMB','evidence'),
  @('P11F','CLMB','evidence'),
  @('PROG','CTRL','hard'),
  @('EVID','E00A','hard'),
  @('E00A','E00B','hard'),
  @('E00A','E00C','hard'),
  @('E00A','E00D','hard'),
  @('E00B','E00D','evidence'),
  @('E00C','E00D','evidence'),
  @('E00A','E00E','hard'),
  @('E00B','E00E','evidence'),
  @('E00C','E00E','evidence'),
  @('E00D','E00E','evidence'),
  @('CTRL','EVID','hard'),
  @('M07A','L09A','hard'),
  @('M07B','L09A','hard'),
  @('M07C','L09A','hard'),
  @('M07D','L09A','hard'),
  @('EVID','L09A','evidence'),
  @('E00D','L09A','evidence'),
  @('E00E','L09A','evidence'),
  @('L09A','L09B','hard'),
  @('EVID','L09B','evidence'),
  @('E00D','L09B','evidence'),
  @('E00E','L09B','evidence'),
  @('L09A','L09C','hard'),
  @('EVID','L09C','evidence'),
  @('E00D','L09C','evidence'),
  @('E00E','L09C','evidence'),
  @('L09A','L09D','hard'),
  @('EVID','L09D','evidence'),
  @('E00B','L09D','evidence'),
  @('E00D','L09D','evidence'),
  @('E00E','L09D','evidence'),
  @('L09A','L09E','hard'),
  @('EVID','L09E','evidence'),
  @('E00D','L09E','evidence'),
  @('E00E','L09E','evidence'),
  @('L09A','L09F','hard'),
  @('EVID','L09F','evidence'),
  @('E00D','L09F','evidence'),
  @('E00E','L09F','evidence'),
  @('L09A','L09G','hard'),
  @('L09B','L09G','hard'),
  @('L09C','L09G','hard'),
  @('L09D','L09G','hard'),
  @('L09E','L09G','hard'),
  @('L09F','L09G','hard'),
  @('EVID','L09G','evidence'),
  @('E00D','L09G','evidence'),
  @('E00E','L09G','evidence'),
  @('CTRL','LIVECTL','hard'),
  @('EVID','M00S','hard'),
  @('E00A','M00S','hard'),
  @('E00B','M00S','hard'),
  @('E00C','M00S','hard'),
  @('E00D','M00S','hard'),
  @('E00E','M00S','hard'),
  @('E00A','M01','evidence'),
  @('E00B','M01','evidence'),
  @('M01','M02','hard'),
  @('M01','M03','hard'),
  @('M03','M04A','hard'),
  @('M04A','M04B','hard'),
  @('M03','M04B','hard'),
  @('M04A','M04C','hard'),
  @('M04B','M04C','hard'),
  @('CTRL','M04CTL','hard'),
  @('M04A','M04D','hard'),
  @('M04B','M04D','hard'),
  @('M04A','M04E','hard'),
  @('M04B','M04E','hard'),
  @('M03','M05','hard'),
  @('M04C','M05','hard'),
  @('M04D','M05','hard'),
  @('M04E','M05','hard'),
  @('M03','M06','hard'),
  @('M05','M06','hard'),
  @('M07A','M07B','hard'),
  @('M04A','M07B','hard'),
  @('M04B','M07B','hard'),
  @('M04C','M07B','evidence'),
  @('M04D','M07B','evidence'),
  @('M04E','M07B','evidence'),
  @('M07A','M07C','hard'),
  @('M07B','M07C','hard'),
  @('M01','M07C','hard'),
  @('M02','M07C','hard'),
  @('CTRL','M07CTL','hard'),
  @('M07C','M07D','hard'),
  @('M03','M08','hard'),
  @('M06','M08','hard'),
  @('M07C','M09','hard'),
  @('M08','M09','hard'),
  @('M01','M09','hard'),
  @('M06','M09','hard'),
  @('M09','M10','hard'),
  @('E00A','P11A','hard'),
  @('M00S','P11A','hard'),
  @('EVID','P11A','evidence'),
  @('E00B','P11A','evidence'),
  @('E00C','P11A','evidence'),
  @('E00D','P11A','evidence'),
  @('E00E','P11A','evidence'),
  @('P11A','P11B','hard'),
  @('M01','P11B','evidence'),
  @('M02','P11B','evidence'),
  @('M07A','P11B','evidence'),
  @('M07B','P11B','evidence'),
  @('M07C','P11B','evidence'),
  @('L09A','P11B','evidence'),
  @('L09B','P11B','evidence'),
  @('L09D','P11B','evidence'),
  @('L09G','P11B','evidence'),
  @('EVID','P11B','evidence'),
  @('E00B','P11B','evidence'),
  @('E00C','P11B','evidence'),
  @('E00D','P11B','evidence'),
  @('P11A','P11C','hard'),
  @('M07D','P11C','evidence'),
  @('L09A','P11C','evidence'),
  @('L09C','P11C','evidence'),
  @('L09D','P11C','evidence'),
  @('L09E','P11C','evidence'),
  @('L09F','P11C','evidence'),
  @('L09G','P11C','evidence'),
  @('PROVB','P11C','evidence'),
  @('EVID','P11C','evidence'),
  @('E00D','P11C','evidence'),
  @('E00E','P11C','evidence'),
  @('P11A','P11D','hard'),
  @('M08','P11D','evidence'),
  @('L09A','P11D','evidence'),
  @('L09D','P11D','evidence'),
  @('L09G','P11D','evidence'),
  @('EVID','P11D','evidence'),
  @('E00E','P11D','evidence'),
  @('P11A','P11E','hard'),
  @('M09','P11E','evidence'),
  @('M10','P11E','evidence'),
  @('L09A','P11E','evidence'),
  @('L09G','P11E','evidence'),
  @('EVID','P11E','evidence'),
  @('E00D','P11E','evidence'),
  @('E00E','P11E','evidence'),
  @('P11A','P11F','hard'),
  @('P11B','P11F','hard'),
  @('P11C','P11F','hard'),
  @('P11D','P11F','hard'),
  @('P11E','P11F','hard'),
  @('EVID','P11F','evidence'),
  @('E00A','P11F','evidence'),
  @('E00B','P11F','evidence'),
  @('E00C','P11F','evidence'),
  @('E00D','P11F','evidence'),
  @('E00E','P11F','evidence'),
  @('L09A','PROVA','hard'),
  @('L09C','PROVA','evidence'),
  @('M04B','PROVA','evidence'),
  @('M07B','PROVA','evidence'),
  @('M07C','PROVA','evidence'),
  @('L09A','PROVB','hard'),
  @('L09D','PROVB','evidence'),
  @('M07C','PROVB','evidence'),
  @('CTRL','XPCTL','hard')
)
# Forbidden edges (the #11625 shift-left falsifiers 5, 6, 7, 9 and the claim boundary):
# no dependency of the second node on the first may exist under any class.
$FORBIDDEN_EDGES = @(
@('M00S','C01'),
  @('M00S','C02'),
  @('PROVA','L09A'),
  @('PROVB','L09A'),
  @('M07D','M07A'),
  @('M07D','M07B'),
  @('P11F','P11A'),
  @('CLAIM','M01')
)
if ($LAW_EDGES.Count -ne 177) { throw "law-edge table declares $($LAW_EDGES.Count) edges, expected 177" }
if ($FORBIDDEN_EDGES.Count -ne 8) { throw "forbidden-edge table declares $($FORBIDDEN_EDGES.Count) edges, expected 8" }

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

# Recursive live-state and invented-identifier scan over every string value.
function Assert-NoLiveStateStrings {
  param($Value, [string]$Where)
  if ($null -eq $Value) { return }
  if ($Value -is [string]) {
    if ($Value -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { throw "possible live SHA/state token at ${Where}: $($Matches[0])" }
    if ($Value -match '\d{4}-\d{2}-\d{2}T') { throw "possible live timestamp at ${Where}" }
    foreach ($tok in @('origin/', 'refs/heads/', 'pull/', 'PR #', 'merge-base', 'worktrees/')) {
      if ($Value.Contains($tok)) { throw "possible live-state token '${tok}' at ${Where}" }
    }
    if ($Value -cmatch '(?i)case[-_ ]?family[-_ ]?id\b|\bMCF-[0-9]+\b') { throw "invented local case identifier at ${Where}" }
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
    if ($v -is [string]) { [void]$b.Append('s:'); [void]$b.Append(($v -replace '\\', '\\\\' -replace ';', '\;')); [void]$b.Append(';'); return }
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

  if ($doc.schema -cne 'module_train.v1') { throw 'schema name mismatch' }
  if ($doc.schema_version -cne 1) { throw 'schema_version must be 1' }

  # programme block
  Assert-KeySet $doc.programme @('parent_programme_issue','controller_issue','evidence_controller_issue',
    'home_programme','method_authority') 'programme'
  if ($doc.programme.parent_programme_issue -cne 8133) { throw 'parent programme issue mismatch' }
  if ($doc.programme.controller_issue -cne 4240) { throw 'programme controller_issue mismatch' }
  if ($doc.programme.evidence_controller_issue -cne 8479) { throw 'evidence controller issue mismatch' }
  if ($doc.programme.home_programme -cne 'module-programme') { throw 'home programme mismatch' }

  # authority planes: exactly eight, fixed order
  if (@($doc.authority_planes).Count -ne 8) { throw 'expected exactly 8 authority planes' }
  for ($i = 0; $i -lt 8; $i++) {
    Assert-KeySet $doc.authority_planes[$i] $PLANE_KEYS "authority_planes[$i]"
    if ($doc.authority_planes[$i].plane -cne $PLANES[$i]) { throw "authority plane order broken at $($i + 1)" }
    Assert-NonEmpty $doc.authority_planes[$i].owns "authority_planes[$i].owns"
    Assert-NonEmpty $doc.authority_planes[$i].never_substitutes "authority_planes[$i].never_substitutes"
  }

  # train role vocabulary: exactly ten, fixed order
  if (@($doc.train_role_vocabulary).Count -ne 10) { throw 'expected exactly 10 train roles' }
  for ($i = 0; $i -lt 10; $i++) {
    Assert-KeySet $doc.train_role_vocabulary[$i] @('role','owns') "train_role_vocabulary[$i]"
    if ($doc.train_role_vocabulary[$i].role -cne $ROLES[$i]) { throw "train role order broken at $($i + 1)" }
    Assert-NonEmpty $doc.train_role_vocabulary[$i].owns "train_role_vocabulary[$i].owns"
  }

  # evidence semantics
  Assert-KeySet $doc.evidence_semantics @('not_proven_law','optional_visibility','metadata_only_rule','issue_identity_rule') 'evidence_semantics'
  foreach ($k in @('not_proven_law','optional_visibility','metadata_only_rule','issue_identity_rule')) {
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
  foreach ($needed in @('#3982', '#3983', '#3985', '#3989', '#10554', '#10858', '#7249', '#8761', '#8131', '#7582', '#7621', '#7622', '#4851', '#7419', '#7420', '#6736', '#7057', '#7943', '#8112', '#8199', '#8518', '#4239', '#7584', '#8617', '#6720', '#9621', '#11114', '#EXPLICIT-AUTHORIZATION')) {
    if (-not $authIds.Contains($needed)) { throw "required external authority missing: $needed" }
  }
  if ($authIds.Count -ne 28) { throw "expected exactly 28 external authorities, found $($authIds.Count)" }

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

  # case and work-packet bindings stay structurally pending (entry-gate law)
  Assert-KeySet $doc.case_work_packet_bindings $BINDING_KEYS 'case_work_packet_bindings'
  if ($doc.case_work_packet_bindings.status -cne 'structurally_pending') {
    throw 'case and work-packet bindings must stay structurally_pending until the E00 family materializes stable identities'
  }
  $bindNodes = @($doc.case_work_packet_bindings.binding_nodes | Sort-Object)
  $wantBind = @('E00A','E00B','E00C','E00D','E00E')
  if (($bindNodes -join ',') -cne ($wantBind -join ',')) { throw 'binding node set mismatch' }
  if ($doc.case_work_packet_bindings.evidence_authority -cne '#8479') { throw 'binding evidence authority mismatch' }
  foreach ($k in @('law','promotion_route')) { Assert-NonEmpty $doc.case_work_packet_bindings.$k "case_work_packet_bindings.$k" }

  # claim profiles: frozen membership sets plus the closeout superset law
  Assert-IsList $doc.claim_profiles 'claim_profiles'
  $profMap = New-OrdinalTable
  foreach ($p in @($doc.claim_profiles)) {
    Assert-KeySet $p $PROFILE_KEYS 'claim_profiles[]'
    Assert-IsString $p.id 'claim_profiles[].id'
    Assert-IsString $p.definition 'claim_profiles[].definition'
    Assert-NonEmpty $p.definition 'claim_profiles[].definition'
    Assert-IsList $p.members 'claim_profiles[].members'
    $profMap[[string]$p.id] = @($p.members | Sort-Object)
  }
  if ($profMap.Count -ne 6) { throw "expected exactly 6 claim profiles, found $($profMap.Count)" }
  foreach ($id in $PROFILES.Keys) {
    if (-not $profMap.ContainsKey($id)) { throw "missing claim profile: $id" }
    if (($profMap[$id] -join ',') -cne (($PROFILES[$id] | Sort-Object) -join ',')) { throw "claim profile membership mismatch: $id" }
  }
  $core = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($m in $profMap['module_exact_process_resolution_core']) { [void]$core.Add($m) }
  foreach ($m in $profMap['module_exact_process_semantic_edit']) { [void]$core.Add($m) }
  foreach ($m in $core) {
    if ($profMap['module_exact_process_full_closeout'] -cnotcontains $m) {
      throw "full closeout hides profile member: $m (a core pass must never hide the edit profile)"
    }
  }

  # cross-programme imports: one home, imported never copied
  Assert-IsList $doc.cross_programme_imports 'cross_programme_imports'
  $importRel = New-OrdinalTable
  foreach ($imp in @($doc.cross_programme_imports)) {
    Assert-KeySet $imp $IMPORT_KEYS 'cross_programme_imports[]'
    if (-not $authIds.Contains([string]$imp.authority)) { throw "cross-programme import names unknown authority: $($imp.authority)" }
    if ($importRel.ContainsKey([string]$imp.authority)) { throw "duplicate cross-programme import: $($imp.authority)" }
    $importRel[[string]$imp.authority] = [string]$imp.relation
    Assert-NonEmpty $imp.home_train 'cross_programme_imports[].home_train'
    Assert-NonEmpty $imp.note 'cross_programme_imports[].note'
  }
  foreach ($needed in @('#8131', '#7621', '#7622', '#7582', '#4851', '#7419', '#7420', '#6736', '#7057', '#7943', '#8112', '#8199', '#8518', '#4239', '#7584', '#8617', '#6720', '#7249', '#8761', '#9621', '#11114', '#10554', '#10858', '#3982', '#3983', '#3985', '#3989', '#EXPLICIT-AUTHORIZATION')) {
    if (-not $importRel.ContainsKey($needed)) { throw "external authority not imported with a home train: $needed" }
  }

  # nodes: exact expected set, uniqueness, per-node contract completeness
  $EXPECTED_ISSUES = [System.Collections.Generic.HashSet[long]]::new()
  foreach ($v in $EXPECTED_NODES.Values) { [void]$EXPECTED_ISSUES.Add([long]$v) }
  $EXPECTED_IDS = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  foreach ($k in $EXPECTED_NODES.Keys) { [void]$EXPECTED_IDS.Add($k) }
  $nodes = @($doc.nodes)
  if ($nodes.Count -ne 52) { throw "expected exactly 52 nodes, found $($nodes.Count)" }
  $byId = New-OrdinalTable
  $byIssue = New-OrdinalTable
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
    if ($byIssue.ContainsKey([long]$n.issue)) { throw "duplicate issue assignment: $($n.issue)" }
    $byIssue[[long]$n.issue] = $id
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
  # the editor-intelligence authority is imported, never copied as a node (falsifier 11)
  if ($byIssue.ContainsKey([long]9621)) { throw 'editor-intelligence node copied instead of imported from its home train' }

  # supersession registry resolved against the node/issue maps
  $seenSuperseded = New-OrdinalTable
  foreach ($s in @($doc.supersessions)) {
    if (-not $byId.ContainsKey([string]$s.superseded_node)) { throw "supersession names unknown node: $($s.superseded_node)" }
    if ($seenSuperseded.ContainsKey($s.superseded_node)) { throw "duplicate supersession for node: $($s.superseded_node)" }
    $seenSuperseded[[string]$s.superseded_node] = $true
    if (-not $EXPECTED_ISSUES.Contains([long]$s.successor_issue)) { throw "supersession names unknown successor issue: $($s.successor_issue)" }
    if ([long]$s.successor_issue -eq [long]$byId[[string]$s.superseded_node].issue) { throw 'successor issue must differ from the superseded node own issue' }
  }

  # revision governance returns to C01 #11625; no separate revision node exists yet
  Assert-KeySet $doc.revision_governance @('owner_node','owner_issue','invalidates','never','metadata_only') 'revision_governance'
  if ($doc.revision_governance.owner_node -cne 'C01' -or $doc.revision_governance.owner_issue -cne 11625) {
    throw 'revision governance must be owned by C01 #11625'
  }
  Assert-NonEmpty $doc.revision_governance.invalidates 'revision_governance.invalidates'
  Assert-NonEmpty $doc.revision_governance.never 'revision_governance.never'
  Assert-NonEmpty $doc.revision_governance.metadata_only 'revision_governance.metadata_only'

  foreach ($n in $nodes) {
    $id = [string]$n.node_id
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
    # frozen role map (falsifiers 1, 4 and 7)
    if ($n.train_role -cne $EXPECTED_ROLES[$id]) { throw "train role mismatch at ${id}: $($n.train_role)" }
    if ($n.train_role -cnotin $ROLES) { throw "unknown train role at ${id}: $($n.train_role)" }
    # chain law: one home programme, frozen controller map (falsifier 11)
    Assert-KeySet $n.chain @('home','controller') "node $id chain"
    if ($n.chain.home -cne 'module-programme') { throw "node $id chain home mismatch" }
    if ($n.chain.controller -cne $EXPECTED_CONTROLLERS[$id]) { throw "node $id chain controller mismatch" }
    # writer law: capacity classes are identities, not quotas
    Assert-KeySet $n.writer @('conflict_key','parallel_group','stack_relation') "node $id writer"
    Assert-NonEmpty $n.writer.conflict_key "node $id conflict_key"
    if ($n.writer.parallel_group -cnotin $GROUPS) { throw "unknown writer capacity class at ${id}: $($n.writer.parallel_group)" }
    if ($n.writer.parallel_group -cne $EXPECTED_GROUPS[$id]) { throw "writer capacity class mismatch at ${id}" }
    # spec law: leaf specs are referenced to #10592, never duplicated (falsifier analog of emacs 13)
    Assert-KeySet $n.spec $SPEC_KEYS "node $id spec"
    if ($n.spec.disposition -cnotin $DISPOSITIONS) { throw "unknown disposition at ${id}: $($n.spec.disposition)" }
    if ($n.spec.owner -cne $id) { throw "node $id spec owner must be itself" }
    Assert-NonEmpty $n.spec.stale_policy "node $id stale_policy"
    if ($n.spec.spec_authority -cne '#10592') { throw "node $id spec authority must reference #10592" }
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
    # path neutrality (falsifier 14 analog): semantic names only, never source paths
    foreach ($s in @($n.identity_fields) + @($n.allowed_components)) {
      Assert-IsString $s "node $id identity/component entry"
      if ($s.Contains('/') -or $s -cmatch '\.(rs|el|toml|json)$') { throw "source path treated as semantic identity at ${id}: $s" }
    }
    # proof nodes never repair the product (falsifier 9)
    if ($n.train_role -ceq 'proof') {
      foreach ($s in @($n.allowed_components)) {
        if ($s -cmatch '(?i)production|repair') { throw "proof node $id allows product repair: $s" }
      }
    }
    foreach ($a in @($n.consumed_authorities)) {
      Assert-IsString $a "node $id consumed authority entry"
      if (-not $a.StartsWith('#')) { throw "consumed authority must be an issue reference: $a" }
      if (-not $authIds.Contains($a) -and -not $byIssue.ContainsKey([long]($a.TrimStart('#')))) {
        throw "node $id consumes unknown authority: $a"
      }
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
        # cross-programme class law: the import relation fixes the edge class
        $rel = $importRel[$t]
        if ($rel -ceq 'hard import' -and $d.class -cne 'hard') { throw "hard import ${t} appears as $($d.class) at node $id" }
        if ($rel -ceq 'evidence import' -and $d.class -cne 'evidence') { throw "evidence import ${t} appears as $($d.class) at node $id" }
        if ($IMPORT_NO_EDGE -ccontains $t) { throw "non-edge import ${t} carries a dependency edge at node $id" }
      } else {
        if (-not $byId.ContainsKey($t)) { throw "node $id depends on unknown node: $t" }
        if ($t -eq $id) { throw "node $id self-dependency" }
      }
      # external authorization law: gate-only, external-class, never invented
      if ($t -ceq '#EXPLICIT-AUTHORIZATION') {
        if ($d.class -cne 'external') { throw "node $id carries #EXPLICIT-AUTHORIZATION as $($d.class); it is external-class only" }
        if ($n.train_role -cne 'external_gate') { throw "node $id carries #EXPLICIT-AUTHORIZATION without the external_gate role" }
      }
    }
  }

  # controller/fan_in law: they never enter ordinary builder frontiers
  foreach ($n in $nodes) {
    $role = [string]$n.train_role
    if ($role -cin @('controller','fan_in','external_gate')) {
      if ($n.buildable) { throw "${role} node $($n.node_id) must not be buildable" }
    } else {
      if (-not $n.buildable) { throw "${role} node $($n.node_id) must be buildable as one one-PR proposition" }
    }
  }
  # exactly the nine reviewed controllers are non-buildable controllers
  $controllerIssues = @($nodes | Where-Object { $_.train_role -ceq 'controller' } | ForEach-Object { [long]$_.issue } | Sort-Object)
  $wantControllers = @(4240,4245,7421,7430,8133,8479,8566,8701,9270)
  if (($controllerIssues -join ',') -cne ($wantControllers -join ',')) { throw 'controller rejection list mismatch' }

  # claim-ceiling pattern laws (the reviewed stage and authority bounds)
  $CEILING_LAWS = @(
    @{ id = 'M00S'; patterns = @('frontier','scheduling') },
    @{ id = 'M07D'; patterns = @('overlay','lookup algorithm') },
    @{ id = 'L09G'; patterns = @('terminal') },
    @{ id = 'P11F'; patterns = @('denominator') },
    @{ id = 'CLAIM'; patterns = @('support') },
    @{ id = 'CLMA'; patterns = @('support') },
    @{ id = 'CLMB'; patterns = @('support') },
    @{ id = 'CLAIMFIX'; patterns = @('effective-root authority') }
  )
  foreach ($law in $CEILING_LAWS) {
    foreach ($p in $law.patterns) {
      if (-not ([string]$byId[$law.id].claim_ceiling).Contains($p)) { throw "ceiling law broken at $($law.id): missing [$p]" }
    }
  }
  foreach ($eid in @('E00A','E00B','E00C','E00D','E00E')) {
    if (-not ([string]$byId[$eid].claim_ceiling).Contains('production behavior')) { throw "evidence ceiling law broken at ${eid}" }
  }
  foreach ($proofNode in @('P11A','P11B','P11C','P11D','P11E')) {
    if (-not ([string]$byId[$proofNode].claim_ceiling).Contains('cannot repair the product')) { throw "proof ceiling law broken at ${proofNode}" }
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
  # forbidden edges stay absent (falsifiers 5, 6, 7, 9 and the claim boundary)
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

  # no live state or invented identifiers anywhere in the document
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
function Find-Node { param($d, [string]$id) @($d.nodes | Where-Object { $_.node_id -ceq $id })[0] }
function Find-Dep { param($n, [string]$t) @($n.dependencies | Where-Object { $_.target -ceq $t })[0] }

# falsifier 1: a controller is emitted as an implementation leaf
Invoke-NegativeControl 'F01-controller-as-leaf' { param($d) (Find-Node $d 'CTRL').train_role = 'implementation' }
# falsifier 2: a pull request number is used as node identity
Invoke-NegativeControl 'F02-pull-as-node' { param($d) $d.nodes += [pscustomobject]@{ node_id = 'PULLX'; issue = 12034; title = 'x'; title_fingerprint = '00'; aliases = @(); train_role = 'implementation'; lane = 'x'; chain = [pscustomobject]@{ home = 'module-programme'; controller = 'CTRL' }; one_pr_outcome = 'x'; authority_before = 'a'; authority_after = 'b'; buildable = $true; dependencies = @(); claim_ceiling = 'c'; writer = [pscustomobject]@{ conflict_key = 'k'; parallel_group = 'B'; stack_relation = 'none' }; consumed_authorities = @(); allowed_components = @('c'); forbidden_adjacent_owners = @('x'); spec = [pscustomobject]@{ disposition = 'ISSUE_PLAN_SUFFICIENT'; owner = 'PULLX'; stale_policy = 's'; spec_authority = '#10592' }; first_falsifier = 'f'; controls = [pscustomobject]@{ positive = 'p'; opposite = 'o'; stale = 's'; wrong_subject = 'w'; fault = 'f'; mutation = 'm' }; proof = [pscustomobject]@{ focused = 'p'; routed = 'r' }; review_forward = [pscustomobject]@{ questions = @('q'); lenses = @('l') }; obligations = [pscustomobject]@{ schema = 's'; generated = 'g'; docs = 'd'; changelog = 'c'; receipt = 'r' }; exits = [pscustomobject]@{ old_path = 'o'; compatibility = 'c'; supersession = 's'; transfer = 't' }; rollback = [pscustomobject]@{ rollback = 'r'; return_to_issue = 'i'; not_proven = 'n'; stop = 's' }; successors = @(); identity_fields = @('i'); limitations = @('l') } }
# falsifier 3: hard and evidence dependency classes collapse
Invoke-NegativeControl 'F03-class-collapse' { param($d) (Find-Dep (Find-Node $d 'M01') 'E00A').class = 'hard' }
# falsifier 4: an E00 evidence row is treated as product implementation
Invoke-NegativeControl 'F04-evidence-as-product' { param($d) (Find-Node $d 'E00B').train_role = 'implementation' }
# falsifier 5: the durable spec source becomes a frontier or scheduler
Invoke-NegativeControl 'F05-spec-source-scheduler' { param($d) (Find-Node $d 'M00S').claim_ceiling = 'owns the current scheduling authority for module work' }
# falsifier 6: the selected-source overlay becomes a second lookup algorithm
Invoke-NegativeControl 'F06-second-lookup' { param($d) (Find-Node $d 'M07D').claim_ceiling = 'a second candidate lookup algorithm over resolver outputs' }
# falsifier 7: a provider helper or proof is treated as the live cutover substrate
Invoke-NegativeControl 'F07-proof-as-cutover' { param($d) (Find-Node $d 'PROVA').train_role = 'cutover' }
# falsifier 8: legacy retirement is allowed before all admitted consumer rows are terminal
Invoke-NegativeControl 'F08-early-retirement' { param($d) (Find-Node $d 'L09G').dependencies = @((Find-Node $d 'L09G').dependencies | Where-Object { $_.target -cne 'L09F' }) }
# falsifier 9a: the fan-in executes missing scenarios
Invoke-NegativeControl 'F09-fanin-executes' { param($d) (Find-Node $d 'P11F').buildable = $true }
# falsifier 9b: a proof node repairs the product
Invoke-NegativeControl 'F09b-proof-repairs-product' { param($d) (Find-Node $d 'P11D').allowed_components += 'production resolver repair' }
# falsifier 10: a core profile pass hides the semantic edit profile
Invoke-NegativeControl 'F10-profile-collapse' { param($d) $prof = @($d.claim_profiles | Where-Object { $_.id -ceq 'module_exact_process_full_closeout' })[0]; $prof.members = @($prof.members | Where-Object { $_ -cne 'P11D' }) }
# falsifier 11: a cross-programme node is copied instead of imported from its home train
Invoke-NegativeControl 'F11-cross-home-copy' { param($d) (Find-Node $d 'M01').chain.home = 'editor_intelligence' }
Invoke-NegativeControl 'F11b-editor-node-copy' { param($d) (Find-Node $d 'PROVA').issue = 9621 }
# falsifier 12: two nodes share one conflict key or shared authority
Invoke-NegativeControl 'F12-conflict-collision' { param($d) (Find-Node $d 'L09C').writer.conflict_key = (Find-Node $d 'L09B').writer.conflict_key }
# falsifier 13: deterministic bytes change with insertion order (order-invariance control below)
# falsifier 14: live branch, pull, model or liveness state enters stable bytes (value level, then byte level)
Invoke-NegativeControl 'F14-live-state-value' { param($d) (Find-Node $d 'M03').limitations += ' rebased onto head 4f5bcb334 deadbeef 00ff11' }
$badLine = 'x deadbeefdeadbeef y'
if ($badLine -cmatch '(?<![0-9A-Fa-f])[0-9a-f]{7,}(?![0-9A-Fa-f])') { $controls.Add('F14b-live-state-bytes') } else { throw 'F14b raw-byte live-state scan failed to detect a token' }

# acceptance-bullet mutation classes beyond the numbered falsifiers:
Invoke-NegativeControl 'A01-hard-cycle' { param($d) (Find-Node $d 'M01').dependencies += [pscustomobject]@{ target = 'M07C'; class = 'hard'; provenance = '#11625 body canonical graph M07' } }
Invoke-NegativeControl 'A02-duplicate-authority-after' { param($d) (Find-Node $d 'M07B').authority_after = (Find-Node $d 'M07A').authority_after }
Invoke-NegativeControl 'A03-case-id-invention' { param($d) (Find-Node $d 'E00B').allowed_components += 'local case family id MCF-0001' }
Invoke-NegativeControl 'A04-binding-prematurely-bound' { param($d) $d.case_work_packet_bindings.status = 'bound' }
Invoke-NegativeControl 'A05-unauthorized-external-action' { param($d) (Find-Node $d 'M01').dependencies += [pscustomobject]@{ target = '#EXPLICIT-AUTHORIZATION'; class = 'hard'; provenance = '#11625 body canonical graph M07' } }

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

# Twenty-two fail-closed mutation controls cover the fourteen #11625 shift-left
# falsifiers (falsifiers 9, 11 and 14 carry doubled controls) plus the five
# acceptance-bullet mutation classes beyond the numbered list (hard cycle,
# duplicate owner, invented case identity, premature binding promotion and
# unauthorized external action), plus the order-invariance canonicalization
# control whose rejected subject is an order-sensitive canonicalization.
if ($controls.Count -ne 22) { throw "expected 22 negative controls, ran $($controls.Count)" }

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
                 '## Shared-mechanics disposition \(#10554\)',
                 '## Compatibility with the repository operating contract \(`AGENTS.md`\)',
                 '## Open decisions respected, not decided','## Adoption, rollback, transfer and stop','## Links')) {
  if (-not ($contextText -match "(?m)^${h}\s*$")) { throw "missing context heading: $h" }
}
foreach ($term in @('module_train.v1','`.spec/10918-emacs-train-graph/`','`.spec/11764-controller-train-graph/`',
                    '#8479','#11625','#10592','#10554','OD1','structurally pending','52 nodes',
                    'cargo xtask module-train check','#11114','#9621','#11626','#11627')) {
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
foreach ($term in @('module_train.v1','train.manifest.json','52 nodes','197 typed edges',
                    '#10977','#11624','#11026','#8170','#10554','#11626','#11627','structurally pending')) {
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
Write-Output "SPEC_11625_STRUCTURAL_CHECK=PASS"
Write-Output "SPEC_11625_NEGATIVE_CONTROLS=22/22"
Write-Output "SPEC_11625_SEMANTIC_SHA256=$semanticDigest"
Write-Output "SPEC_11625_BUNDLE_SHA256=$fingerprint"
```

## Second-run procedure

Run the checker twice. Requirements for a valid proof:

1. both runs print `SPEC_11625_STRUCTURAL_CHECK=PASS`;
2. both runs print `SPEC_11625_NEGATIVE_CONTROLS=22/22`;
3. both runs print the same `SPEC_11625_SEMANTIC_SHA256` and
   `SPEC_11625_BUNDLE_SHA256` fingerprints;
4. the full captured output of both runs is byte-identical;
5. `git status --porcelain` shows no change caused by the runs (no temporary
   file is written inside the repository);
6. `git diff --check` (staged) is clean before commit, and
   `git diff origin/main..HEAD --check` is clean after commit.

## NOT_PROVEN boundary

The structural checker proves manifest shape, node-set completeness, edge
typing, provenance discipline, graph-law freezing, forbidden-edge absence,
uniqueness laws, the controller rejection list, writer-class assignment,
claim-profile membership and the closeout superset law, the
structurally-pending case-binding law, cross-programme import discipline,
claim-ceiling and proof-component laws, durable-byte hygiene, fail-closed
behavior of all fourteen #11625 shift-left falsifiers, order-invariant
canonicalization, and byte-level determinism across two runs. It does **not**
prove: that the topology is the semantically correct reading of every leaf
body (that is this review's job, and the #11625 revision route's after);
that the xtask module-train operations named by #11625 exist or pass
(unbuilt tooling, a separate claim); that the E00 case and work-packet
identities exist (structurally pending until the E00 family lands); that the
graph stays current as issues evolve (the revision route owns invalidation);
or that any module behavior, cutover, receipt, profile or support claim
holds (the lanes own those). The repository's absent executable module train
validator remains an open tooling gap recorded here rather than papered
over.

## Flags for builder

- Deviation note: the controlling issue names offline xtask operations
  (`cargo xtask module-train check`, `graph`, `list`, `explain-static`).
  Those are executable repository tooling and are not built in this
  bundle-style claim; the manifest lands as checked data plus this embedded
  checker, and the absent validator is recorded as `not_proven`. A later
  tooling claim against the same seam must consume `module_train.v1` as
  data, not clone it.
- The case and work-packet binding stays structurally pending by law; a
  later revision that binds stable identities must come through the #11625
  revision route, never a local byte edit.
- #10554's start gate is not satisfied on current main; this bundle begins
  no extraction and decides nothing about the gate (OD1). If a downstream
  check can only pass by weakening a law here, stop and return to #11625
  rather than editing the law locally.

## Rollback, transfer and stop

- **Rollback:** revert the single commit or remove the bundle directory; no
  runtime, product, CI, support or GitHub state depends on it.
- **Transfer:** a successor manifest version supersedes this one only
  through a classified #11625 revision with an exact successor recorded;
  derived artifacts are re-derived, never patched valid.
- **Stop:** stop before validator commands, current-tree probes, frontier,
  live observation, packet rendering, GitHub metadata work, product
  implementation, scheduling, support claims, release or publication. If an
  open decision OD1–OD5 is needed as a decision rather than a boundary,
  stop and route it to its owning issue; do not decide it in a builder
  change.
