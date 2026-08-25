# Implementation Checklist: #11716 — durable Emacs support architecture and evidence boundaries

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any Emacs/host process.

### Step 1: Create the architecture context contract

- **File:** `.spec/11716-emacs-support-architecture/context.md`
- **Change:** Record the problem, stable identity decisions, four truth
  planes, authority/ownership split (#10894→#8734, #7777/#10527, #11360/#11361,
  subject/root/public lanes, #11768/#11770), diagnostic cohorts, root/public
  stages, platform cut, agentic execution law, durable dependency ordering,
  stable-vs-mutable rule, current-tree basis, alternatives rejected, and links.
- **Verify:** checker below enforces the identity table, the four truth planes
  in fixed order, the ten-law non-substitution table, the five client families
  in fixed order, the required authority/ordering terms; `git diff --check`.

### Step 2: Create acceptance and negative controls

- **File:** `.spec/11716-emacs-support-architecture/acceptance.md`
- **Change:** Include all canonical `SPEC_TEMPLATE.md` sections, the per-leaf
  contract table covering every blocked leaf, the declarative API names, all
  fifteen issue falsifiers in fixed order, blast radius, and coverage map.
- **Verify:** structural heading, hazard, leaf-table, and falsifier checks
  below; `git diff --check`.

### Step 3: Create the builder and proof contract

- **File:** `.spec/11716-emacs-support-architecture/checklist.md`
- **Change:** Define the bounded change order, deterministic structural
  checking, second-run proof, acceptance gates, and handoff.
- **Verify:** read-only checker runs twice with identical output and no tree
  diff.

## Deterministic structural proof

The repository has no executable `.spec` graph validator. Do not invent a
generated receipt or claim a missing tool passed. From the candidate worktree,
run the following PowerShell 7 check twice after the files are complete. The
command checks the exact three files, the required canonical headings and
contract terms in `context.md` and `acceptance.md` only, the four truth planes
and their ten-law non-substitution table in fixed order, the five client
families in fixed order, the durable dependency-ordering terms, per-leaf
coverage bound to the `§Contracts` table rows in fixed order, and all fifteen
numbered falsifiers in the `acceptance.md` `§Test-Grid` table. Exact-string
comparisons are deliberately case-sensitive (`-cmatch`, `-cne`, `-CaseSensitive`)
so a case-variant path, term, or table cell cannot satisfy an exact assertion.
It enforces fixed order, stable scenario/kind/verdict semantics, table
membership, and a non-empty required verdict for every row; presence of a
marker elsewhere in the bundle is insufficient. Redirecting output to a
temporary file is local proof only; no temporary file belongs in the PR.
Its changed-path assertion is intentionally unscoped: it binds the committed
candidate patch to the explicit `origin/main..HEAD` range, unions that patch
with the unstaged worktree, staged index, and NUL-delimited porcelain paths
(failing closed if any scan itself fails), then requires that union to equal
the exact three-file set. A malformed status record, rename/copy without its
second path, or an unresolvable base/HEAD fails closed.

```powershell
function Get-SpecStatusPaths {
  $statusFile = [IO.Path]::GetTempFileName()
  try {
    # This scan is intentionally unscoped: an exact-scope proof must also see
    # tracked and untracked paths outside the three-file projection.
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

function Invoke-Spec11716Check {
$root = '.spec/11716-emacs-support-architecture'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  'perl-lsp', 'perllsp --stdio', 'generic_lsp', 'bundled Eglot',
  'released standalone Eglot', 'pinned upstream-source Eglot',
  'released lsp-mode', 'pinned upstream-source lsp-mode',
  'perl-mode', 'cperl-mode', 'perl-ts-mode',
  'stock_project_discovery', 'standard_user_project_override',
  'custom_repository_helper', 'exact_source_local', 'release_candidate',
  'public_artifact', 'manual_client_registration',
  'upstream_accepted_unreleased', 'upstream_builtin_released',
  'publishDiagnostics', 'textDocument/diagnostic', 'previousResultId',
  'Flymake', 'diagnostics-supported', 'not_proven',
  '#7777', '#10527', '#10894', '#8734', '#8755', '#11360', '#11361',
  '#8776', '#8795', '#8819', '#8821', '#8822', '#8830', '#11366',
  '#8834', '#8838', '#8842', '#8858', '#9310', '#9413', '#3983',
  '#11744', '#11745', '#11746', '#11747', '#11748', '#11749', '#11750',
  '#11768', '#11770', '#11717', '#11718', '#11719', '#11751', '#11759',
  '#11760', '#10918', '#10923', '#10930', '#10936',
  'E00', 'E01', 'E01R', 'E02', 'E04', 'E06', 'emacs_train.v1',
  'conflict key', 'claim ceiling', 'rollback', 'transfer', 'stop conditions',
  'deterministic'
)
$headings = @('§Behavior', '§Hazards', '§Contracts', '§API-Shape', '§Test-Grid', '§Blast-Radius', '§Coverage-Map')
$leafIds = @('#11717', '#11718', '#11719', '#11744', '#11745', '#11746', '#11747', '#11748', '#11749', '#11750', '#11751', '#11752', '#11753', '#11754', '#11755', '#11756', '#11757', '#11758', '#11759', '#11760', '#11768', '#11770')
$apiNames = @('emacs_train.v1', 'emacs_host_journeys.v1', 'emacs_node_context.v1', 'editor_client_compat.v1', 'agent_implementation_packet.v1')
$text = @($paths | ForEach-Object { Get-Content -Raw $_ })
if ($text.Count -ne 3) { throw 'expected exactly three spec files' }
# Contract assertions deliberately exclude checklist.md. Its checker source is
# self-covered below, but it must not be able to supply missing contract laws.
$contextText = $text[0]
$acceptanceText = $text[1]
$contractText = @($contextText, $acceptanceText)
function Get-SectionBody {
  param([string]$Document, [string]$HeadingPattern)
  $match = [regex]::Match($Document, "(?ms)^${HeadingPattern}\s*\r?\n(?<body>.*?)(?=^#{1,3}\s|\z)")
  if (-not $match.Success) { throw "missing contract section: $HeadingPattern" }
  return $match.Groups['body'].Value
}
$identitySection = Get-SectionBody $contextText '## Stable identity decisions'
$planeSection = Get-SectionBody $contextText '## Four truth planes'
$topologySection = Get-SectionBody $contextText '## Stable implementation topology and dependency ordering'
$acceptanceHazardsSection = Get-SectionBody $acceptanceText '## §Hazards'
$contractsSection = Get-SectionBody $acceptanceText '## §Contracts'
foreach ($term in $required) {
  if (-not ($contextText -cmatch [regex]::Escape($term))) { throw "missing context contract term: $term" }
}
foreach ($term in $apiNames + @('#11716')) {
  if (-not ($contractText -cmatch [regex]::Escape($term))) { throw "missing contract API/identity term: $term" }
}
foreach ($heading in $headings) {
  if (-not ($acceptanceText -cmatch [regex]::Escape($heading))) { throw "missing acceptance heading: $heading" }
}
foreach ($term in @('Receipt duplication', '#3983')) {
  if (-not ($acceptanceHazardsSection -cmatch [regex]::Escape($term))) { throw "missing acceptance hazard term: $term" }
}
# Leaf coverage is bound to the per-leaf contract table rows themselves: a
# prose mention, coverage-map reference, or numeric prefix cannot satisfy it.
$contractLeads = [regex]::Matches($contractsSection, '(?m)^\|\s*(?<id>#\d+)[ \t]')
$leadIds = @($contractLeads | ForEach-Object { $_.Groups['id'].Value })
if (($leadIds -join ',') -cne ($leafIds -join ',')) { throw "per-leaf contract table rows do not match the covered leaf set in fixed order: found $($leadIds -join ',')" }
foreach ($term in @('E00', 'E01', 'E01R', 'E02', 'E04', 'E06', '#10918', '#11770', '#11717', '#11751', '#11718', '#11756', '#11719', '#10936', '#11759', '#11760', '#11744', '#11745', '#11746', '#8755', '#11747', '#11749', '#11748', '#11750', '#8834', '#8838', '#11768')) {
  if (-not ($topologySection -cmatch [regex]::Escape($term))) { throw "missing dependency ordering term: $term" }
}

# Four truth planes: fixed order, exact text.
$planeList = [regex]::Matches($planeSection, '(?m)^\s*(?<n>\d+)\.\s+(?<plane>[^\r\n]+?)\s*$')
$expectedPlanes = @(
  'stable Emacs semantic architecture and implementation topology;',
  'exact current-tree implementation state;',
  'live branch/worktree/PR/check/review/writer state;',
  'behavior/public/support/release evidence.'
)
if ($planeList.Count -ne 4) { throw "expected exactly four truth planes, found $($planeList.Count)" }
for ($i = 0; $i -lt $expectedPlanes.Count; $i++) {
  if ([int]$planeList[$i].Groups['n'].Value -ne ($i + 1)) { throw 'truth planes are not numbered 1-4 in fixed order' }
  if ($planeList[$i].Groups['plane'].Value -cne $expectedPlanes[$i]) { throw "truth plane $($i + 1) has unexpected text" }
}

# Plane non-substitution laws: ten rows, fixed order, exact semantics.
$planeRows = @([regex]::Matches($planeSection, '(?m)^\|\s*(?<left>[^|\r\n]+?)\s*\|\s*(?<right>[^|\r\n]+?)\s*\|\s*$') | Where-Object { $_.Groups['left'].Value.Trim() -cne 'True statement in one plane' -and $_.Groups['left'].Value.Trim() -notmatch '^-+$' })
$expectedPlaneRows = @(
  @{ left = 'issue/PR closed'; right = 'implementation on tree' }
  @{ left = 'runner/profile present'; right = 'actual Emacs behavior' }
  @{ left = 'actual local host pass'; right = 'public artifact' }
  @{ left = 'manual registration pass'; right = 'stock discovery' }
  @{ left = 'correct root URI'; right = 'root-sensitive semantics' }
  @{ left = 'exact-source source head'; right = 'released client' }
  @{ left = 'upstream accepted'; right = 'released built-in client' }
  @{ left = 'Linux pass'; right = 'macOS/Windows/TRAMP' }
  @{ left = 'schema-valid receipt'; right = 'host observation' }
  @{ left = 'host observation'; right = 'support projection' }
)
if ($planeRows.Count -ne $expectedPlaneRows.Count) { throw "plane non-substitution table declares $($planeRows.Count) laws, expected $($expectedPlaneRows.Count)" }
for ($i = 0; $i -lt $expectedPlaneRows.Count; $i++) {
  if ($planeRows[$i].Groups['left'].Value.Trim() -cne $expectedPlaneRows[$i].left) { throw "plane law $($i + 1) is not in the declared slot" }
  if ($planeRows[$i].Groups['right'].Value.Trim() -cne $expectedPlaneRows[$i].right) { throw "plane law $($i + 1) has unexpected non-substitution semantics" }
}

# Client families: five exact subjects, fixed order.
$families = [regex]::Matches($identitySection, '(?m)^\s*(?<n>\d+)\.\s+(?<family>[^\r\n]+?)\s*$')
$expectedFamilies = @('bundled Eglot', 'released standalone Eglot', 'pinned upstream-source Eglot', 'released lsp-mode', 'pinned upstream-source lsp-mode')
if ($families.Count -ne $expectedFamilies.Count) { throw "client families declare $($families.Count) subjects, expected $($expectedFamilies.Count)" }
for ($i = 0; $i -lt $expectedFamilies.Count; $i++) {
  if ([int]$families[$i].Groups['n'].Value -ne ($i + 1)) { throw 'client families are not in fixed order' }
  if ($families[$i].Groups['family'].Value -cne $expectedFamilies[$i]) { throw "client family $($i + 1) has unexpected identity" }
}

# Self-cover the checker and wrapper literals and validation loops. This
# prevents a local edit from silently deleting the very checks that claim to
# validate the spec, including the second-run wrapper mechanics.
$checkerSource = Get-Content -Raw "$root/checklist.md"
$checkerLiterals = @(
  'foreach ($term in $required)', 'foreach ($heading in $headings)',
  'git status --porcelain=v1 -z', '$candidateBaseRef', '$candidateHeadRef',
  '$rows = [regex]::Matches', '$expectedRows', '$expectedPlaneRows',
  '$expectedFamilies', '$expectedPlanes', '$planeRows', '$families',
  '$contractLeads', '$leadIds', '$wrapperFence', 'GetRandomFileName',
  'Compare-Object -CaseSensitive $changed $expected',
  'Get-SpecFingerprints', 'SPEC_11716_STRUCTURAL_CHECK=PASS',
  'SPEC_11716_SECOND_RUN=PASS'
)
foreach ($literal in $checkerLiterals) {
  if (-not ($checkerSource -cmatch [regex]::Escape($literal))) { throw "checker self-cover missing: $literal" }
}
$fence = [string]::new([char]96, 3)
$fences = [regex]::Matches($checkerSource, "(?ms)${fence}powershell\s*(?<body>.*?)\s*${fence}")
if ($fences.Count -ne 2) { throw "expected exactly two powershell fences, found $($fences.Count)" }
$checkerFence = $fences[0].Groups['body'].Value
$wrapperFence = $fences[1].Groups['body'].Value
if (-not $checkerFence -or $checkerFence -notmatch [regex]::Escape('function Invoke-Spec11716Check')) { throw 'checker source fence is missing' }
foreach ($literal in $required + $headings + $leafIds + $apiNames + @('E00', 'E01R', '#10918', '#8755', '#8834', '#8838', '#11768', '#11770')) {
  if (-not ($checkerFence -cmatch [regex]::Escape($literal))) { throw "checker literal is not self-covered: $literal" }
}
foreach ($literal in @('Invoke-Spec11716Check | Set-Content', '$tmp1', '$tmp2', '$tree3', '$h1', '$h2', 'finally', 'GetRandomFileName', 'SPEC_11716_SECOND_RUN=PASS')) {
  if (-not ($wrapperFence -cmatch [regex]::Escape($literal))) { throw "wrapper literal is not self-covered: $literal" }
}

# Fifteen falsifiers: fixed order, unique, table-bound, exact semantics.
$grid = [regex]::Match($acceptanceText, '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 15) { throw "expected exactly fifteen falsifier rows, found $($rows.Count)" }
$ids = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($ids | Sort-Object -Unique).Count -ne $ids.Count) { throw 'falsifier IDs are not unique' }
if (($ids -join ',') -cne ((1..15) -join ',')) { throw 'falsifier IDs are not in fixed order' }
$expectedRows = @(
  @{ id = 1; scenario = 'Runner, profile, or schema presence is represented as actual Emacs host support'; kind = 'negative'; verdict = 'reject; only typed host observation proves host support' }
  @{ id = 2; scenario = 'A client `shutdown_completed` event or exit status 0 alone proves descendant cleanup'; kind = 'negative'; verdict = 'reject; cleanup requires independent observation via #10894/#8734' }
  @{ id = 3; scenario = 'A synthetic capability profile becomes actual-client evidence'; kind = 'negative'; verdict = 'reject; profiles prove negotiation/result shapes only' }
  @{ id = 4; scenario = 'A manually bound fixture root becomes stock project discovery'; kind = 'negative'; verdict = 'reject; stock discovery is observed unprebound (#11747/#11748)' }
  @{ id = 5; scenario = 'A correct rootUri becomes root-sensitive semantics'; kind = 'negative'; verdict = 'reject; semantics require behavior-bearing proof (#11749/#11750)' }
  @{ id = 6; scenario = 'Local exact-source evidence becomes a public artifact claim'; kind = 'negative'; verdict = 'reject; public stages require exact direct evidence' }
  @{ id = 7; scenario = 'An upstream source head becomes a released client subject'; kind = 'negative'; verdict = 'reject; released identity requires package/archive identity' }
  @{ id = 8; scenario = 'Accepted-unreleased upstream integration becomes shipped built-in discovery'; kind = 'negative'; verdict = 'reject; released built-in state requires its own evidence' }
  @{ id = 9; scenario = 'Linux evidence becomes a macOS, Windows, or TRAMP support claim'; kind = 'negative'; verdict = 'reject; platforms and TRAMP require their own proof' }
  @{ id = 10; scenario = 'Eglot evidence becomes lsp-mode, or one client generation fills another'; kind = 'negative'; verdict = 'reject; cohorts stay client- and generation-exact' }
  @{ id = 11; scenario = 'Protocol traffic becomes a host-visible semantic pass without #11360/#11361'; kind = 'negative'; verdict = 'reject; host visibility flows only through observation and producer' }
  @{ id = 12; scenario = 'A controller, fan-in, or external gate becomes an ordinary builder leaf'; kind = 'negative'; verdict = 'reject; roles keep their non-builder dispositions' }
  @{ id = 13; scenario = 'Current SHA/PR/check/model/writer state enters durable spec bytes'; kind = 'negative'; verdict = 'reject; durable specs carry stable identities only' }
  @{ id = 14; scenario = 'An Emacs-local receipt/spec/packet ontology duplicates #7777/#10527/#10872/#10881'; kind = 'negative'; verdict = 'reject; shared authorities are consumed, not cloned' }
  @{ id = 15; scenario = 'Optional breadth becomes an initial-Linux hard prerequisite without a selected claim requiring it'; kind = 'negative'; verdict = 'reject; optional breadth stays optional (#9310)' }
)
for ($i = 0; $i -lt $expectedRows.Count; $i++) {
  $row = $rows[$i]
  $expectedRow = $expectedRows[$i]
  foreach ($field in @('scenario', 'kind', 'verdict')) {
    $actual = $row.Groups[$field].Value.Trim()
    if ($actual -cne $expectedRow[$field]) { throw "falsifier $($expectedRow.id) has unexpected $field" }
  }
}

# Bind the proof to the intended candidate range, rather than recomputing an
# implicit merge-base that could silently change the patch under review.
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

# Each path scan fails closed: a failed git command must abort before a later
# command can mask it with a reset exit code and an incomplete union.
$rangePaths = @(git diff --name-only $candidateRange)
if ($LASTEXITCODE -ne 0) { throw "git diff --name-only failed for $candidateRange" }
$worktreePaths = @(git diff --name-only)
if ($LASTEXITCODE -ne 0) { throw 'git diff --name-only failed for the worktree' }
$cachedPaths = @(git diff --cached --name-only HEAD)
if ($LASTEXITCODE -ne 0) { throw 'git diff --cached --name-only failed' }
$changed = @($rangePaths + $worktreePaths + $cachedPaths + (Get-SpecStatusPaths)) | Sort-Object -Unique -CaseSensitive
$expected = @(
  '.spec/11716-emacs-support-architecture/acceptance.md'
  '.spec/11716-emacs-support-architecture/checklist.md'
  '.spec/11716-emacs-support-architecture/context.md'
)
if ($changed.Count -ne $expected.Count -or (Compare-Object -CaseSensitive $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_11716_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice. Do not create two copies of one
output and hash those copies. Use this wrapper around the exact checker body
above (the body is exposed as `Invoke-Spec11716Check` only to make the
execution boundary explicit). The wrapper fingerprints every expected spec
file before, between, and after the two executions, and compares those
fingerprints with the three global tree snapshots and the two actual
checker-output hashes. Capture is fail-closed (`-ErrorAction Stop` plus
leaf-file checks), runs are isolated in a unique temporary directory so
concurrent executions cannot collide, and the directory is removed in a
`finally` block:

```powershell
function Get-SpecFingerprints {
  $expected = @(
    '.spec/11716-emacs-support-architecture/acceptance.md'
    '.spec/11716-emacs-support-architecture/checklist.md'
    '.spec/11716-emacs-support-architecture/context.md'
  )
  return @($expected | ForEach-Object {
    if (-not (Test-Path -LiteralPath $_ -PathType Leaf)) { throw "missing spec file: $_" }
    "$_=$((Get-FileHash -Algorithm SHA256 -LiteralPath $_ -ErrorAction Stop).Hash)"
  })
}
$ErrorActionPreference = 'Stop'
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("spec-11716-check-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$tmp1 = Join-Path $tmpDir 'run1.out'
$tmp2 = Join-Path $tmpDir 'run2.out'
try {
  $tree1 = @(Get-SpecStatusPaths) -join "`n"
  $fpBefore = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11716Check | Set-Content -LiteralPath $tmp1 -Encoding utf8NoBOM -ErrorAction Stop
  $tree2 = @(Get-SpecStatusPaths) -join "`n"
  $fpBetween = @(Get-SpecFingerprints) -join "`n"
  Invoke-Spec11716Check | Set-Content -LiteralPath $tmp2 -Encoding utf8NoBOM -ErrorAction Stop
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
'SPEC_11716_SECOND_RUN=PASS'
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'working tree diff --check failed' }
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'staged diff --check failed' }
$expected = @(
  '.spec/11716-emacs-support-architecture/acceptance.md'
  '.spec/11716-emacs-support-architecture/checklist.md'
  '.spec/11716-emacs-support-architecture/context.md'
)
if ((Get-SpecStatusPaths | Where-Object { $_ -cnotin $expected })) { throw 'unexpected spec artifact' }
```

The `Invoke-Spec11716Check` function is the exact command body above, not a
copied output; the two invocations each reread the files and revalidate every
table. A future repository-owned checker may replace this proof only through a
separate tooling claim.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are changed.
- [ ] Every stable Emacs identity/evidence/root/public/upstream/platform/
      optional boundary has one durable decision or an exact existing-contract
      reference.
- [ ] #7777/#10527/#11360/#11361 ownership is explicit and non-duplicated;
      #10894 (generic) versus #8734 (Emacs conformance) is explicit.
- [ ] Push/pull/client/source/released/manual/public evidence dimensions
      remain independent.
- [ ] Project discovery, standard override, and behavior-bearing root
      semantics remain independent; `custom_repository_helper` stays future.
- [ ] The initial Linux cut and the later platform/optional/upstream
      programmes remain separate (#9310, #9413 referenced, not re-expanded).
- [ ] Stable/current-tree/live/support truth planes are explicit and
      non-substitutable via the ten-law table.
- [ ] Every concrete leaf blocked behind E00 is covered by one per-leaf
      contract table row with an explicit evidence ceiling, and the dependency
      ordering is durable, so leaves compile later without controller
      archaeology.
- [ ] Shared spec/builder/reviewer/close authorities are consumed rather than
      cloned.
- [ ] All fifteen issue falsifiers are present as rejectable designs in fixed
      order, unique, table-bound, and carrying a required verdict.
- [ ] Deterministic structural proof passes twice and the second run is
      byte-clean.
- [ ] No product behavior, host execution, readiness computation, candidate
      state, registry/support promotion, release, publication, or external
      action is claimed or changed.

## Callers and consumers

- #10918 (`emacs_train.v1`) consumes this bundle as the durable architecture
  it must preserve; its graph never amends these decisions.
- #11717/#11718/#11719 and #11751-#11760 consume the authority split, claim
  ceilings, and dependency ordering when compiling leaf specs, exact-tree
  contexts, packets, routing scenarios, and cohorts.
- #11744-#11750, #8755/#8834/#8838 fan-ins, #11768, and #11770 consume the
  subject, cohort, root, journey, and revision boundaries.
- #10923/#10930 keep exact current-tree and live candidate truth; nothing in
  this bundle certifies current-tree state.
- #7777/#10527/#3983/#10872/#10881/#10554 remain generic authorities; this
  bundle introduces no second schema or engine.

## Flags for builder

- Re-audit live upstream/release truth at each leaf's implementation time;
  version strings in this bundle are naming hints, never subject authority.
- This bundle describes what the train WILL build plus the bounded current
  docs/policy/test-scaffolding basis; it certifies no Emacs capability.
- Mutable state (SHA, PR, check colour, writer, model, live uniqueness) never
  enters durable bytes; route it to #10923/#10930.
- If a downstream check can only pass by weakening a boundary here, stop and
  return to #11716 rather than editing the boundary locally.
- Deviation note: the controlling issue sketched a four-file bundle with
  `plan.md`; current #3983 conventions, `SPEC_TEMPLATE.md`, and every landed
  `.spec/` packet use the three-file projection, so the dependency ordering
  lives in `context.md` §Stable implementation topology and dependency
  ordering and per-leaf coverage in `acceptance.md` §Contracts/§Coverage-Map.

## Scope boundary

Files in scope:

- `.spec/11716-emacs-support-architecture/context.md`
- `.spec/11716-emacs-support-architecture/acceptance.md`
- `.spec/11716-emacs-support-architecture/checklist.md`

Files and surfaces out of scope: all `crates/` and `xtask/` production code,
editor/client adapters, host harnesses, `.github/workflows/`, CI routing,
registry/docs mutations, support/public claims, generated status, dependency
manifests, external processes, and `emacs_train.v1` bytes.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check
output, two-run hash comparison, and `git diff --check` result. Independent
review must challenge whether any per-leaf contract overclaims current main,
whether the dependency ordering is real, and whether authorities are consumed
rather than duplicated. A clean review does not prove Emacs behavior; that
belongs to the implementation lanes. The repository's absent executable
`.spec` graph validator remains `NOT_PROVEN` here and a follow-up tooling
concern, not a reason to widen this PR.
