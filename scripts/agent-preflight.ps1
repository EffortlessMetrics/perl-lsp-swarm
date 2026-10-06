<#
.SYNOPSIS
Validate an agent worktree before edit-capable PR work starts.

.DESCRIPTION
This script enforces the Windows worktree and Cargo target layout used by
perl-lsp-swarm agents:

  H:\Code\Rust3\perl-lsp-swarm
    Canonical checkout. It must stay clean and should not be used for edits.

  H:\Code\Rust3\perl-lsp-swarm-worktrees\<issue>-<slug>
    Disposable per-PR worktree.

  H:\Code\Rust3\.cargo-target\perl-lsp-swarm\<issue>-<slug>
    Disposable per-PR Cargo target directory.

Run this from the task worktree before making edits.

A missing or stale installed pre-push hook fails with exit code 7, as does
a missing installed pre-commit hook (check 7 parity with
agent-preflight.sh). Comparison tolerates CR and trailing newlines only;
Windows applies no executable-bit gate.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+$')]
    [string]$Issue,

    [Parameter(Mandatory = $true)]
    [string]$Slug,

    [string]$CanonicalRoot = 'H:\Code\Rust3\perl-lsp-swarm',
    [string]$WorktreeRoot = 'H:\Code\Rust3\perl-lsp-swarm-worktrees',
    [string]$TargetRoot = 'H:\Code\Rust3\.cargo-target\perl-lsp-swarm',

    [switch]$ReadOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail {
    param([string]$Message)
    Write-Error $Message
    exit 1
}

function Get-FullPath {
    param([string]$Path)
    return [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Assert-ChildPath {
    param(
        [string]$Child,
        [string]$Parent,
        [string]$Description
    )

    $childFull = Get-FullPath $Child
    $parentFull = Get-FullPath $Parent
    $parentPrefix = $parentFull
    if (-not $parentPrefix.EndsWith('\')) {
        $parentPrefix = "$parentPrefix\"
    }

    if (-not $childFull.StartsWith($parentPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        Fail "$Description is outside the required root. Path: $childFull Root: $parentFull"
    }

    return $childFull
}

function Invoke-Git {
    param(
        [string]$Repository,
        [string[]]$GitArgs
    )

    $output = & git -C $Repository @GitArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        Fail "git $($GitArgs -join ' ') failed in $Repository. Output: $output"
    }

    return @($output)
}

if ($Slug -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $Slug -match '[\\/]') {
    Fail "Slug must be a path-safe token, not '$Slug'."
}

$canonical = Get-FullPath $CanonicalRoot
$worktreeRootFull = Get-FullPath $WorktreeRoot
$targetRootFull = Get-FullPath $TargetRoot
$worktreePath = Assert-ChildPath -Child (Join-Path $worktreeRootFull "$Issue-$Slug") -Parent $worktreeRootFull -Description 'Worktree path'
$targetPath = Assert-ChildPath -Child (Join-Path $targetRootFull "$Issue-$Slug") -Parent $targetRootFull -Description 'Cargo target path'

if (-not (Test-Path -Path $canonical -PathType Container)) {
    Fail "Canonical checkout does not exist: $canonical"
}

$canonicalStatus = @(Invoke-Git -Repository $canonical -GitArgs @('status', '--porcelain'))
if ($canonicalStatus.Count -gt 0) {
    Fail "Canonical checkout is dirty and must not be used as agent scratch space: $canonical"
}

$canonicalTarget = Join-Path $canonical 'target'
if (Test-Path -Path $canonicalTarget) {
    Fail "Repo-local target directory exists in canonical checkout: $canonicalTarget"
}

$currentRoot = @(Invoke-Git -Repository (Get-Location).Path -GitArgs @('rev-parse', '--show-toplevel'))[0]
$currentRoot = Get-FullPath $currentRoot
$currentBranch = @(Invoke-Git -Repository $currentRoot -GitArgs @('branch', '--show-current'))[0]
$currentTarget = Join-Path $currentRoot 'target'

if (Test-Path -Path $currentTarget) {
    Fail "Repo-local target directory exists in current checkout: $currentTarget"
}

if (-not $ReadOnly) {
    if ([string]::IsNullOrWhiteSpace($currentBranch)) {
        Fail 'Edit-capable agent work must run on a named branch, not detached HEAD.'
    }

    if ($currentBranch -in @('main', 'master')) {
        Fail "Edit-capable agent work must not run on protected branch '$currentBranch'."
    }

    if ($currentRoot.Equals($canonical, [System.StringComparison]::OrdinalIgnoreCase)) {
        Fail "Edit-capable agent work must not run in the canonical checkout: $canonical"
    }

    Assert-ChildPath -Child $currentRoot -Parent $worktreeRootFull -Description 'Current checkout' | Out-Null

    if (-not $currentRoot.Equals($worktreePath, [System.StringComparison]::OrdinalIgnoreCase)) {
        Fail "Current checkout does not match the requested worktree. Current: $currentRoot Requested: $worktreePath"
    }
}

# Hook-currency drift detector (#17413): mirrors check 7 of
# scripts/agent-preflight.sh. The installed pre-push hook must match this
# checkout's hooks/pre-push authority; hook-assumed guards are void until it
# does. Exit code 7 matches the shell preflight.
$checkedInHook = Join-Path $currentRoot 'hooks/pre-push'
if (Test-Path -LiteralPath $checkedInHook -PathType Leaf) {
    $commonDir = @(Invoke-Git -Repository $currentRoot -GitArgs @('rev-parse', '--path-format=absolute', '--git-common-dir'))[0]
    $installedHook = Join-Path $commonDir 'hooks/pre-push'
    $hookProblem = $null
    if (-not (Test-Path -LiteralPath $installedHook -PathType Leaf)) {
        $hookProblem = "pre-push hook is missing ($installedHook)"
    } else {
        # Byte comparison through Latin-1 (one char per byte, no BOM handling):
        # strip CR for CRLF checkouts and trailing LF for the newline the
        # installer appends. Case and every other byte still count, and there
        # is no executable-bit gate on Windows.
        $hookEncoding = [Text.Encoding]::GetEncoding(28591)
        $checkedInText = $hookEncoding.GetString([IO.File]::ReadAllBytes($checkedInHook)).Replace("`r", '').TrimEnd([char]10)
        $installedText = $hookEncoding.GetString([IO.File]::ReadAllBytes($installedHook)).Replace("`r", '').TrimEnd([char]10)
        if (-not [string]::Equals($installedText, $checkedInText, [StringComparison]::Ordinal)) {
            $hookProblem = 'pre-push hook is stale (installed copy differs from hooks/pre-push)'
        }
    }
    if ($null -ne $hookProblem) {
        Write-Host "ERR $hookProblem. Hook-assumed guards are void."
        Write-Host 'Fix: bash scripts/install-githooks.sh (run from the repo root).'
        exit 7
    }
    # pre-commit has no checked-in authority (its bytes are generated inside
    # the installer), so it gets presence only: the installer writes both
    # hooks in one invocation, so a current pre-push implies a current
    # pre-commit unless the latter was deleted outright. No exec-bit gate on
    # Windows (mirrors the shell check's MINGW carve-out).
    $installedCommitHook = Join-Path $commonDir 'hooks/pre-commit'
    if (-not (Test-Path -LiteralPath $installedCommitHook -PathType Leaf)) {
        Write-Host "ERR pre-commit hook is missing ($installedCommitHook). Commits run without the staged gate. Hook-assumed guards are void."
        Write-Host 'Fix: bash scripts/install-githooks.sh (run from the repo root).'
        exit 7
    }
}

New-Item -ItemType Directory -Force -Path $targetPath | Out-Null

Write-Host 'agent preflight ok'
Write-Host "canonical=$canonical"
Write-Host "worktree=$worktreePath"
Write-Host "branch=$currentBranch"
Write-Host "cargo_target=$targetPath"
Write-Host ''
Write-Host 'Set this for every Cargo command in this task:'
Write-Host "`$env:CARGO_TARGET_DIR = '$targetPath'"
Write-Host ''
Write-Host 'Bash equivalent:'
Write-Host "export CARGO_TARGET_DIR='$targetPath'"
