<# Native production-path fixtures. All data stays in an explicit unique proof root. #>
param(
    [string]$CleanupScript,
    [Parameter(Mandatory = $true)][string]$ProofRoot,
    [string]$PowerShellExecutable
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $CleanupScript) { $CleanupScript = Join-Path $PSScriptRoot '../agent-cleanup.ps1' }
$CleanupScript = (Resolve-Path -LiteralPath $CleanupScript).Path
if (-not $PowerShellExecutable) { $PowerShellExecutable = (Get-Process -Id $PID).Path }
$GitExecutable = (Get-Command git -CommandType Application | Select-Object -First 1).Source
$ProofRoot = [IO.Path]::GetFullPath($ProofRoot)
if (Test-Path -LiteralPath $ProofRoot) { throw "Proof root already exists: $ProofRoot" }
New-Item -ItemType Directory -Path $ProofRoot | Out-Null

function Invoke-Native {
    param([string]$Executable, [string[]]$Arguments)
    $saved = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $records = @(& $Executable @Arguments 2>&1)
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $saved }
    return @{ Code = $code; Text = ($records -join "`n") }
}
function Git {
    param([string[]]$GitArgs)
    $r = Invoke-Native -Executable $GitExecutable -Arguments $GitArgs
    if ($r.Code -ne 0) { throw "Fixture Git failed: $($r.Text)" }
    return $r.Text
}

$cases = @(
    @{ Name = 'populated-target'; Kind = 'directory'; Exists = $true; Worktree = $true },
    @{ Name = 'empty-target'; Kind = 'empty'; Exists = $true; Worktree = $true },
    @{ Name = 'file-target'; Kind = 'file'; Exists = $true; Worktree = $true },
    @{ Name = 'junction-target'; Kind = 'junction'; Exists = $true; Worktree = $true },
    @{ Name = 'target-without-worktree'; Kind = 'directory'; Exists = $true; Worktree = $false },
    @{ Name = 'whatif-still-refuses-unknown-target'; Kind = 'directory'; Exists = $true; Worktree = $true; WhatIf = $true },
    @{ Name = 'absent-target-compatible'; Kind = 'absent'; Exists = $false; Worktree = $true }
)
$results = @()
foreach ($case in $cases) {
    $caseRoot = Join-Path $ProofRoot $case.Name
    $canonical = Join-Path $caseRoot 'canonical'
    $worktreeRoot = Join-Path $caseRoot 'worktrees with spaces'
    $worktree = Join-Path $worktreeRoot '11671-retention'
    $targetRoot = Join-Path $caseRoot 'targets with spaces'
    $target = Join-Path $targetRoot '11671-retention'
    New-Item -ItemType Directory -Path $caseRoot,$worktreeRoot,$targetRoot | Out-Null
    Git @('init', '-q', '-b', 'main', $canonical) | Out-Null
    [IO.File]::WriteAllText((Join-Path $canonical 'source.txt'), 'fixture source')
    Git @('-C', $canonical, 'add', '--', 'source.txt') | Out-Null
    Git @('-C', $canonical, '-c', 'user.name=Retention fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-q', '-m', 'fixture') | Out-Null
    if ($case.Worktree) {
        Git @('-C', $canonical, 'worktree', 'add', '-q', '-b', 'fixture/retention', $worktree) | Out-Null
    }
    $sentinels = @()
    if ($case.Kind -in @('directory','empty')) {
        New-Item -ItemType Directory -Path $target | Out-Null
        if ($case.Kind -eq 'directory') {
            foreach ($name in @('reusable-library.rlib','proof-receipt.json','retained-owner-marker')) {
                $p = Join-Path $target $name
                [IO.File]::WriteAllText($p, "independent sentinel $name")
                $sentinels += @{ Path = $p; Hash = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash }
            }
        }
    } elseif ($case.Kind -eq 'junction') {
        $outside = Join-Path $caseRoot 'separate retained source'
        New-Item -ItemType Directory -Path $outside | Out-Null
        $outsideSource = Join-Path $outside 'unique-source.txt'
        [IO.File]::WriteAllText($outsideSource, 'unique source beyond target junction')
        New-Item -ItemType Junction -Path $target -Target $outside | Out-Null
        $sentinels += @{ Path = $outsideSource; Hash = (Get-FileHash -LiteralPath $outsideSource -Algorithm SHA256).Hash }
    } elseif ($case.Kind -eq 'file') {
        [IO.File]::WriteAllText($target, 'source-like file at inferred target')
        $sentinels += @{ Path = $target; Hash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash }
    }
    $beforeTrees = Git @('-C', $canonical, 'worktree', 'list', '--porcelain')
    $beforeRefs = Git @('-C', $canonical, 'show-ref')
    $argv = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$CleanupScript,
              '-Issue','11671','-Slug','retention','-Abandoned',
              '-CanonicalRoot',$canonical,'-WorktreeRoot',$worktreeRoot,'-TargetRoot',$targetRoot)
    if ($case.ContainsKey('WhatIf')) { $argv += '-WhatIf' }
    $r = Invoke-Native -Executable $PowerShellExecutable -Arguments $argv
    $errors = @()
    if ($case.Exists) {
        if ($r.Code -eq 0 -or $r.Text -notmatch 'External target cleanup is retired') { $errors += "Expected explicit target-retention refusal, got $($r.Code): $($r.Text)" }
        if (-not (Test-Path -LiteralPath $target)) { $errors += 'Target disappeared' }
        if ($case.Worktree -and -not (Test-Path -LiteralPath (Join-Path $worktree 'source.txt'))) { $errors += 'Worktree source disappeared before target refusal' }
        if ((Git @('-C',$canonical,'worktree','list','--porcelain')) -ne $beforeTrees) { $errors += 'Worktree topology changed during refusal' }
        if ((Git @('-C',$canonical,'show-ref')) -ne $beforeRefs) { $errors += 'Refs changed during refusal' }
        foreach ($sentinel in $sentinels) {
            if (-not (Test-Path -LiteralPath $sentinel.Path) -or (Get-FileHash -LiteralPath $sentinel.Path -Algorithm SHA256).Hash -ne $sentinel.Hash) { $errors += "Sentinel bytes changed: $($sentinel.Path)" }
        }
    } else {
        if ($r.Code -ne 0 -or $r.Text -notmatch 'agent cleanup ok') { $errors += "Absent-target compatibility failed: $($r.Code): $($r.Text)" }
        if (Test-Path -LiteralPath $worktree) { $errors += 'Absent-target fixture worktree was not removed by existing behavior' }
        if (Test-Path -LiteralPath $target) { $errors += 'Absent target was created' }
    }
    $results += [pscustomobject]@{ Name=$case.Name; Passed=($errors.Count -eq 0); Exit=$r.Code; Errors=$errors; Output=$r.Text }
    Write-Output "$($case.Name): $($errors.Count -eq 0)"
}
$results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $ProofRoot 'results.json')
$failed = @($results | Where-Object { -not $_.Passed })
Write-Output "Retention fixtures: $($results.Count - $failed.Count)/$($results.Count) passed; retained proof root=$ProofRoot"
if ($failed.Count) { exit 1 }
