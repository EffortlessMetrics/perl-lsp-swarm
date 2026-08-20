# Implementation Checklist: #11766 — shared editor-host reliability and adoption contract

## Change order

This is a documentation/specification-only change. Each step is reviewable without
building or executing a host/editor process.

### Step 1: Create the context contract

- **File:** `.spec/10894-editor-host-reliability/context.md`
- **Change:** Record the problem, #10894 versus consumer ownership, platform-neutral
  identity/freshness/deadline/process/cleanup/artifact/four-plane laws, adoption,
  rollback/transfer/stop boundaries, links, and claim limits.
- **Verify:** `Select-String` checks each required authority and law; `git diff --check`.

### Step 2: Create acceptance and negative controls

- **File:** `.spec/10894-editor-host-reliability/acceptance.md`
- **Change:** Include all canonical `SPEC_TEMPLATE.md` sections, all fourteen issue
  falsifiers, representative consumers, adoption dispositions, and explicit
  non-goals.
- **Verify:** Structural heading and falsifier-count checks below; `git diff --check`.

### Step 3: Create the builder and proof contract

- **File:** `.spec/10894-editor-host-reliability/checklist.md`
- **Change:** Define the bounded implementation order, deterministic structural
  checking, second-run proof, rollback/transfer/stop conditions, and handoff.
- **Verify:** Read-only checker runs twice with identical output and no tree diff.

## Deterministic structural proof

The repository has no executable `.spec` graph validator. Do not invent a generated
receipt or claim a missing tool passed. From the candidate worktree, run the following
PowerShell 7 check twice after the files are complete. The command checks the exact
three files, required canonical headings, required terms, and all fourteen numbered
falsifiers in the `acceptance.md` `§Test-Grid` table. It enforces fixed order,
uniqueness, table membership, and a non-empty required verdict for every row;
presence of a marker elsewhere in the bundle is insufficient. Redirecting the
output to a temporary file is local proof only; no temporary file belongs in the PR.
Its changed-path assertion is intentionally unscoped: it binds the committed
candidate patch to the explicit `origin/main..HEAD` range, unions that patch with
the unstaged worktree, staged index, and NUL-delimited porcelain paths, then
requires that union to equal the exact three-file set. A malformed status record,
rename/copy without its second path, or an unresolvable base/HEAD fails closed.

```powershell
function Get-SpecStatusPaths {
  $root = '.spec/10894-editor-host-reliability'
  $statusFile = [IO.Path]::GetTempFileName()
  try {
    & git status --porcelain=v1 -z --untracked-files=all -- $root > $statusFile 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'git status porcelain failed' }
    $bytes = [IO.File]::ReadAllBytes($statusFile)
    if ($bytes.Length -ge 2 -and $bytes[$bytes.Length - 2] -eq 0x0D -and $bytes[$bytes.Length - 1] -eq 0x0A) {
      $bytes = $bytes[0..($bytes.Length - 3)]
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

function Invoke-Spec10894Check {
$root = '.spec/10894-editor-host-reliability'
$paths = @("$root/context.md", "$root/acceptance.md", "$root/checklist.md")
$required = @(
  'HostRunSubject', 'FreshReceiptTarget', 'parent-owned deadline',
  'process domain', 'complete process ledger', 'independent cleanup',
  'bounded artifact', 'product, instrument, reporting, and cleanup',
  'not_proven', '#7777', '#10527', 'Emacs', 'LSP4IJ', 'Coc', 'Lite XL',
  'Vim', 'rollback', 'transfer', 'legacy', 'Stop', 'deterministic',
  'executable path', 'content hash', 'version', 'run ID', 'start time',
  'stage', 'run-bound nonce', 'subject digest', 'write-after-start',
  'schema identity', 'candidate identity', 'driver identity',
  'direct-host', 'candidate', 'descendant', 'replacement', 'ambient',
  'required denominator', 'representative subset is insufficient'
)
$headings = @('§Behavior', '§Hazards', '§Contracts', '§API-Shape', '§Test-Grid', '§Blast-Radius')
$cleanupDomains = @('direct-host', 'candidate', 'descendant', 'replacement', 'ambient')
$text = $paths | ForEach-Object { Get-Content -Raw $_ }
if ($text.Count -ne 3) { throw 'expected exactly three spec files' }
foreach ($term in $required) { if (-not ($text -match [regex]::Escape($term))) { throw "missing term: $term" } }
foreach ($heading in $headings) { if (-not ($text -match [regex]::Escape($heading))) { throw "missing heading: $heading" } }

# Self-cover the checker literals and validation loops. This prevents a local
# edit from silently deleting the very checks that claim to validate the spec.
$checkerSource = Get-Content -Raw "$root/checklist.md"
$checkerLiterals = @(
  'foreach ($term in $required)', 'foreach ($heading in $headings)',
  '$cleanupDomains = @(', 'git status --porcelain=v1 -z',
  '$candidateBaseRef', '$candidateHeadRef', '$rows = [regex]::Matches',
  'Compare-Object $changed $expected', 'SPEC_10894_STRUCTURAL_CHECK=PASS'
)
foreach ($literal in $checkerLiterals) {
  if (-not ($checkerSource -match [regex]::Escape($literal))) { throw "checker self-cover missing: $literal" }
}
$fence = [string]::new([char]96, 3)
$checkerFence = [regex]::Match($checkerSource, "(?ms)${fence}powershell\s*(?<body>.*?)\s*${fence}").Groups['body'].Value
if (-not $checkerFence -or $checkerFence -notmatch [regex]::Escape('function Invoke-Spec10894Check')) { throw 'checker source fence is missing' }
foreach ($literal in $required + $headings + $cleanupDomains) {
  if (-not ($checkerFence -match [regex]::Escape($literal))) { throw "checker literal is not self-covered: $literal" }
}
$grid = [regex]::Match($text[1], '(?ms)^## §Test-Grid\s*(?<body>.*?)(?=^## |\z)').Groups['body'].Value
$rows = [regex]::Matches($grid, '(?m)^\|\s*(?<id>\d+)\s*\|\s*(?<scenario>[^|]+?)\s*\|\s*(?<kind>[^|]+?)\s*\|\s*(?<verdict>[^|]+?)\s*\|')
if ($rows.Count -ne 14) { throw "expected exactly fourteen falsifier rows, found $($rows.Count)" }
$ids = @($rows | ForEach-Object { [int]$_.Groups['id'].Value })
if (($ids | Sort-Object -Unique).Count -ne $ids.Count) { throw 'falsifier IDs are not unique' }
if (($ids -join ',') -ne ((1..14) -join ',')) { throw 'falsifier IDs are not in fixed order' }
foreach ($row in $rows) {
  $kind = $row.Groups['kind'].Value.Trim()
  $verdict = $row.Groups['verdict'].Value.Trim()
  if (-not $verdict) { throw "falsifier $($row.Groups['id'].Value) has no required verdict" }
  if ($kind -eq 'negative' -and $verdict -notmatch '(?i)\breject\b') { throw "falsifier $($row.Groups['id'].Value) is not rejectable" }
}
$cleanup = [regex]::Match($text[0], '(?ms)^### Cleanup denominator declaration\s*(?<body>.*?)(?=^### |\z)').Groups['body'].Value
$domainRows = [regex]::Matches($cleanup, '(?m)^\|\s*`(?<domain>[^`]+)`\s*\|\s*(?<observation>[^|]+?)\s*\|\s*(?<rule>[^|]+?)\s*\|')
if ($domainRows.Count -ne $cleanupDomains.Count) { throw "cleanup denominator declares $($domainRows.Count) domains, expected $($cleanupDomains.Count)" }
$actualDomains = @($domainRows | ForEach-Object { $_.Groups['domain'].Value.Trim() })
if (($actualDomains -join ',') -ne ($cleanupDomains -join ',')) { throw 'cleanup denominator domains are incomplete or reordered' }
if (($cleanup -notmatch 'include every known member') -or ($cleanup -notmatch 'ambient.*excluded')) { throw 'cleanup denominator coverage is not fail-closed' }

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
$candidateDiffCheck = git diff --check $candidateRange
if ($LASTEXITCODE -ne 0) { throw "candidate diff --check failed for $candidateRange" }

$changed = @(
  git diff --name-only $candidateRange
  git diff --name-only
  git diff --cached --name-only HEAD
  Get-SpecStatusPaths
) | Sort-Object -Unique
$expected = @(
  '.spec/10894-editor-host-reliability/acceptance.md'
  '.spec/10894-editor-host-reliability/checklist.md'
  '.spec/10894-editor-host-reliability/context.md'
)
if ($changed.Count -ne $expected.Count -or (Compare-Object $changed $expected)) { throw 'unexpected changed paths' }
'SPEC_10894_STRUCTURAL_CHECK=PASS'
}
```

The proof must execute the checker twice. Do not create two copies of one output
and hash those copies. Use this wrapper around the exact checker body above (the
body is exposed as `Invoke-Spec10894Check` only to make the execution boundary
explicit):

```powershell
$tmp = Join-Path $env:TEMP 'spec-10894-check'
Remove-Item -LiteralPath "$tmp.1","$tmp.2" -Force -ErrorAction SilentlyContinue
$tree1 = @(Get-SpecStatusPaths) -join "`n"
Invoke-Spec10894Check | Set-Content -LiteralPath "$tmp.1" -Encoding utf8NoBOM
$tree2 = @(Get-SpecStatusPaths) -join "`n"
Invoke-Spec10894Check | Set-Content -LiteralPath "$tmp.2" -Encoding utf8NoBOM
$tree3 = @(Get-SpecStatusPaths) -join "`n"
if ($tree1 -ne $tree2 -or $tree2 -ne $tree3) { throw 'checker changed the spec tree' }
$h1 = (Get-FileHash -Algorithm SHA256 -LiteralPath "$tmp.1").Hash
$h2 = (Get-FileHash -Algorithm SHA256 -LiteralPath "$tmp.2").Hash
if ($h1 -ne $h2) { throw 'second run is not deterministic' }
'SPEC_10894_SECOND_RUN=PASS'
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'working tree diff --check failed' }
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'staged diff --check failed' }
$expected = @(
  '.spec/10894-editor-host-reliability/acceptance.md'
  '.spec/10894-editor-host-reliability/checklist.md'
  '.spec/10894-editor-host-reliability/context.md'
)
if ((Get-SpecStatusPaths | Where-Object { $_ -notin $expected })) { throw 'unexpected spec artifact' }
```

The `Invoke-Spec10894Check` function is the exact command body above, not a copied output;
the two invocations each reread the files and revalidate the table. A future
repository-owned checker may replace this proof only through a separate tooling
claim.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are changed.
- [ ] #10894 shared authority and consumer ownership are explicit and non-overlapping.
- [ ] Identity/freshness, parent deadline, process-domain/ledger, independent
      cleanup, artifact integrity, and four-plane laws are platform-neutral.
- [ ] Missing capability/instrumentation is `not_proven`; no status-0/client-event
      cleanup inference is allowed.
- [ ] All fourteen issue falsifiers are present as rejectable designs in fixed order,
      unique, table-bound, and carrying a required verdict.
- [ ] #7777/#10527 remain generic receipt authority; no copied receipt framework exists.
- [ ] Emacs/Eglot/lsp-mode, LSP4IJ, Coc, Lite XL, and Vim/DAP can reference one
      contract without local generic-policy copies.
- [ ] Rollback, transfer, compatibility, legacy adoption, and stop conditions exist.
- [ ] Deterministic structural proof passes twice and the second run is byte-clean.
- [ ] No host execution, editor behavior, CI route, support promotion, or external
      mutation is claimed or changed.

## Callers and consumers

- The future #10894 shared substrate is the sole owner of `HostRunSubject`,
  `FreshReceiptTarget`, the identity tuple, process ledger, cleanup observation,
  and four terminal planes; its implementation and proof are tracked by #10894.
- The parent controller and recurrence owners (#9800 and #10899) consume the
  run-level contract without redefining it.
- Emacs/Eglot and lsp-mode, LSP4IJ, Coc, Lite XL, and Vim/DAP host leaves are
  representative consumers. Their client/provider actions, fixtures, and
  user-facing receipt mappings remain consumer-owned; each may reference this
  bundle but must not copy its generic freshness or cleanup policy.
- #7777 and #10527 remain the generic receipt consumers/authorities. This
  checklist introduces no receipt call site or replacement schema.

## Flags for builder

- Resolve the representation of executable path/hash/version and the
  run-ID/start/stage/nonce/subject-digest tuple without weakening exact identity.
- Prove write-after-start and exact schema/candidate/driver identity before
  accepting a receipt; stale receipts and wrong-executable receipts must fail
  closed, including when filenames and schema versions match.
- Select platform ownership mechanisms independently. Missing capability or
  instrumentation remains `not_proven`; do not infer cleanup from exit status or
  a client event.
- Preserve four terminal planes and bounded-artifact evidence separately. Do not
  turn this spec-only claim into host, editor, CI, support, or release work.
- Explicitly inventory untouched legacy drivers and record reviewed exceptions
  when modified drivers cannot migrate. No legacy inventory is a support claim.

## Scope boundary

Files in scope:

- `.spec/10894-editor-host-reliability/context.md`
- `.spec/10894-editor-host-reliability/acceptance.md`
- `.spec/10894-editor-host-reliability/checklist.md`

Files and surfaces out of scope: all host/editor/client code, `crates/`, generic
receipt implementations, workflows, CI routing, policy/support/release claims,
generated status, dependency manifests, and external processes.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check output,
two-run hash comparison, and `git diff --check` result. Independent review must
challenge claim-versus-spec coverage, proof discrimination, ownership and consumer
reachability, and rollback/legacy adoption. A clean review does not prove host
execution or OS cleanup; those belong to the separate #10894 implementation and
consumer conformance lanes. Any missing tooling remains `NOT_PROVEN` and is a
follow-up issue, not a reason to widen this PR.
