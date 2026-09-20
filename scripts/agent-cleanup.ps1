<#
.SYNOPSIS
Remove a disposable agent worktree and its external Cargo target directory.

.DESCRIPTION
This script is the stop-the-line cleanup gate for per-PR agent work. It removes
only the computed worktree and target paths for the supplied issue/slug, and it
fails instead of guessing when the worktree is dirty, the PR is not merged, or a
path falls outside the configured roots.
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+$')]
    [string]$Issue,

    [Parameter(Mandatory = $true)]
    [string]$Slug,

    [int]$PrNumber,
    [switch]$Abandoned,
    [string]$Branch,

    [string]$CanonicalRoot = 'H:\Code\Rust3\perl-lsp-swarm',
    [string]$WorktreeRoot = 'H:\Code\Rust3\perl-lsp-swarm-worktrees',
    [string]$TargetRoot = 'H:\Code\Rust3\.cargo-target\perl-lsp-swarm'
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

function Convert-ToBashPath {
    param([string]$Path)

    $fullPath = Get-FullPath $Path
    if ($fullPath -match '^([A-Za-z]):\\(.*)$') {
        $drive = $matches[1].ToLowerInvariant()
        $rest = $matches[2] -replace '\\', '/'
        return "/mnt/$drive/$rest"
    }

    return ($fullPath -replace '\\', '/')
}

function Quote-BashArgument {
    param([string]$Value)
    return "'" + ($Value -replace "'", "'\''") + "'"
}

function Test-BranchDeletionAdmission {
    param(
        [int]$PullRequest,
        [string]$Repository
    )

    $admissionScript = Join-Path $Repository 'scripts\branch-deletion-admission'
    if (-not (Test-Path -Path $admissionScript -PathType Leaf)) {
        Write-Host "branch not deleted because shared admission is unavailable: $admissionScript"
        return $false
    }

    $bash = Get-Command bash -ErrorAction SilentlyContinue
    if ($null -ne $bash) {
        $admissionOutput = & bash (Convert-ToBashPath $admissionScript) plan --pr $PullRequest --remote origin 2>&1
    }
    else {
        Push-Location $Repository
        try {
            $admissionOutput = & cargo run --quiet --locked -p xtask --bin branch-deletion-admission -- plan --pr $PullRequest --remote origin 2>&1
        }
        finally {
            Pop-Location
        }
    }

    $admissionExitCode = $LASTEXITCODE
    if ($admissionExitCode -eq 0) {
        return $true
    }

    Write-Host "branch not deleted because branch-deletion admission retained it (exit $admissionExitCode): $($admissionOutput -join ' ')"
    return $false
}

if ($Slug -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $Slug -match '[\\/]') {
    Fail "Slug must be a path-safe token, not '$Slug'."
}

if (-not $Abandoned -and $PrNumber -le 0) {
    Fail 'Provide -PrNumber for merged work, or pass -Abandoned for intentionally abandoned work.'
}

$canonical = Get-FullPath $CanonicalRoot
$worktreeRootFull = Get-FullPath $WorktreeRoot
$targetRootFull = Get-FullPath $TargetRoot
$worktreePath = Assert-ChildPath -Child (Join-Path $worktreeRootFull "$Issue-$Slug") -Parent $worktreeRootFull -Description 'Worktree path'
$targetPath = Assert-ChildPath -Child (Join-Path $targetRootFull "$Issue-$Slug") -Parent $targetRootFull -Description 'Cargo target path'
$verifiedMergedPr = $false

if (-not (Test-Path -Path $canonical -PathType Container)) {
    Fail "Canonical checkout does not exist: $canonical"
}

if ($PrNumber -gt 0) {
    # Bind the lookup to the repository origin points at. Without --repo, gh
    # infers it from the working directory, so the PR consulted may not belong
    # to the repository whose branch is about to be deleted.
    $originUrl = (& git -C $canonical remote get-url origin 2>$null)
    if ([string]::IsNullOrWhiteSpace($originUrl)) {
        Fail "Unable to derive origin for $canonical. Refusing: the PR lookup would not be bound to a repository."
    }
    $repoSlug = ([string]$originUrl).Trim() -replace '\.git$', '' -replace '^.*[:/]([^/]+/[^/]+)$', '$1'
    if ($repoSlug -notmatch '^[^/]+/[^/]+$') {
        Fail "Unable to derive owner/name from origin '$originUrl'. Refusing."
    }

    $prJson = & gh pr view $PrNumber --repo $repoSlug --json state,mergedAt,headRefName,isCrossRepository 2>&1
    if ($LASTEXITCODE -ne 0) {
        Fail "Unable to verify PR #$PrNumber. Output: $prJson"
    }

    $pr = $prJson | ConvertFrom-Json

    # A cross-repository (fork) PR's headRefName names a branch in the FORK.
    # A local branch of the same name is a different branch, so binding to
    # headRefName would authorize deleting the wrong ref. Absent metadata is
    # treated as cross-repository: a response that does not say must not be
    # read as saying "not a fork".
    if ($null -eq $pr.isCrossRepository -or [bool]$pr.isCrossRepository) {
        Fail "PR #$PrNumber is cross-repository, or its fork metadata is missing. Refusing: its head branch does not live in this repository."
    }
    if (-not $Abandoned -and [string]::IsNullOrWhiteSpace([string]$pr.mergedAt)) {
        Fail "PR #$PrNumber is not merged. Cleanup is blocked."
    }
    elseif (-not [string]::IsNullOrWhiteSpace([string]$pr.mergedAt)) {
        $verifiedMergedPr = $true
    }

    if ([string]::IsNullOrWhiteSpace($Branch)) {
        $Branch = [string]$pr.headRefName
    }
    elseif (-not [string]::IsNullOrWhiteSpace([string]$pr.headRefName) -and
            $Branch -ne [string]$pr.headRefName) {
        # The admission is granted for this PR's head branch. Deleting a
        # different branch the caller named would apply an authorization for
        # branch A to branch B. Refuse rather than coerce to headRefName: a
        # caller who named another branch holds a belief worth surfacing.
        Fail "-Branch '$Branch' is not PR #$PrNumber's head branch '$($pr.headRefName)'. Refusing: the admission would be granted for a different branch than the one deleted."
    }
}

if (Test-Path -Path $worktreePath -PathType Container) {
    $worktreeStatus = @(Invoke-Git -Repository $worktreePath -GitArgs @('status', '--porcelain'))
    if ($worktreeStatus.Count -gt 0) {
        Fail "Worktree is dirty; refusing cleanup: $worktreePath"
    }

    if ([string]::IsNullOrWhiteSpace($Branch)) {
        $Branch = @(Invoke-Git -Repository $worktreePath -GitArgs @('branch', '--show-current'))[0]
    }

    if ($PSCmdlet.ShouldProcess($worktreePath, 'git worktree remove')) {
        Invoke-Git -Repository $canonical -GitArgs @('worktree', 'remove', $worktreePath) | Out-Null
    }
}
else {
    Write-Host "worktree already absent: $worktreePath"
}

if (Test-Path -Path $targetPath) {
    if ($PSCmdlet.ShouldProcess($targetPath, 'remove external Cargo target directory')) {
        Remove-Item -Path $targetPath -Recurse -Force
    }
}
else {
    Write-Host "target already absent: $targetPath"
}

if (-not [string]::IsNullOrWhiteSpace($Branch) -and -not $Abandoned) {
    $localBranches = @(Invoke-Git -Repository $canonical -GitArgs @('branch', '--format', '%(refname:short)', '--list', $Branch))
    if ($localBranches -contains $Branch) {
        $mergedBranches = @(Invoke-Git -Repository $canonical -GitArgs @('branch', '--merged', 'origin/main', '--format', '%(refname:short)'))
        $branchDeleteAction = 'delete merged local branch'
        if ($mergedBranches -notcontains $Branch -and $verifiedMergedPr) {
            $branchDeleteAction = 'delete squash-merged local branch'
        }

        if ($mergedBranches -contains $Branch -or $verifiedMergedPr) {
            # Re-read origin immediately before admitting and require it to be
            # the URL this run bound its PR lookup to. Origin is mutable config;
            # a value read once at the top is not evidence about the endpoint the
            # deletion will reach.
            $originNow = (& git -C $canonical remote get-url origin 2>$null)
            if (([string]$originNow).Trim() -ne ([string]$originUrl).Trim()) {
                Write-Host "branch not deleted because origin changed during this run: $Branch"
            }
            elseif (Test-BranchDeletionAdmission -PullRequest $PrNumber -Repository $canonical) {
                # The admission covers the REMOTE branch. Deleting the local ref
                # is a separate act, so prove the local tip is the admitted
                # remote tip first; unpushed commits are unsalvaged work no
                # admission authorized.
                $localTip = (& git -C $canonical rev-parse --verify --quiet "refs/heads/$Branch" 2>$null)
                $remoteTip = (& git -C $canonical ls-remote origin "refs/heads/$Branch" 2>$null)
                if (-not [string]::IsNullOrWhiteSpace($remoteTip)) {
                    $remoteTip = ([string]$remoteTip).Split()[0]
                }
                else {
                    $remoteTip = (& git -C $canonical rev-parse --verify --quiet "refs/remotes/origin/$Branch" 2>$null)
                }

                if ([string]::IsNullOrWhiteSpace($localTip) -or [string]::IsNullOrWhiteSpace($remoteTip)) {
                    Write-Host "branch not deleted because its tip could not be read on both sides: $Branch"
                }
                elseif (([string]$localTip).Trim() -ne ([string]$remoteTip).Trim()) {
                    Write-Host "branch not deleted because the local tip is not the admitted remote tip: $Branch"
                }
                elseif ($PSCmdlet.ShouldProcess($Branch, $branchDeleteAction)) {
                    # Atomic compare-and-delete on the admitted tip. `branch -D`
                    # deletes whatever the ref points at now, so a ref that
                    # advanced between the check above and here would lose that
                    # work; `update-ref -d <ref> <old-oid>` fails closed instead.
                    & git -C $canonical update-ref -d "refs/heads/$Branch" ([string]$localTip).Trim() 2>$null
                    if ($LASTEXITCODE -ne 0) {
                        Write-Host "branch not deleted because it moved between admission and deletion: $Branch"
                    }
                }
            }
        }
        else {
            Write-Host "branch not deleted because it is not merged into origin/main: $Branch"
        }
    }
}

Invoke-Git -Repository $canonical -GitArgs @('worktree', 'prune') | Out-Null

$canonicalTarget = Join-Path $canonical 'target'
if (Test-Path -Path $canonicalTarget) {
    Fail "Repo-local target directory still exists in canonical checkout: $canonicalTarget"
}

$canonicalStatus = @(Invoke-Git -Repository $canonical -GitArgs @('status', '--porcelain'))
if ($canonicalStatus.Count -gt 0) {
    Fail "Canonical checkout is dirty after cleanup: $canonical"
}

$storageDoctor = Join-Path $canonical 'scripts\storage-doctor'
if (Test-Path -Path $storageDoctor) {
    $bash = Get-Command bash -ErrorAction SilentlyContinue
    if ($null -ne $bash) {
        $bashCanonical = Quote-BashArgument (Convert-ToBashPath $canonical)
        & bash -lc "cd $bashCanonical && ./scripts/storage-doctor"
    }
    else {
        Push-Location $canonical
        try {
            & '.\scripts\storage-doctor'
        }
        finally {
            Pop-Location
        }
    }

    if ($LASTEXITCODE -ne 0) {
        Fail 'storage-doctor failed after cleanup.'
    }
}

Write-Host 'agent cleanup ok'
Write-Host "canonical=$canonical"
Write-Host "removed_worktree=$worktreePath"
Write-Host "removed_target=$targetPath"
if (-not [string]::IsNullOrWhiteSpace($Branch)) {
    Write-Host "branch=$Branch"
}
