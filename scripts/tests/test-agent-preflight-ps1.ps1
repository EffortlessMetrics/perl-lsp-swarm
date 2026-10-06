<#
.SYNOPSIS
Fixture suite for the Windows agent-preflight hook-currency check (#17413).

.DESCRIPTION
Builds a real git repo plus a linked worktree under a unique temp directory,
then drives scripts/agent-preflight.ps1 in a child process through missing,
stale, current, installer-shaped, CRLF, pre-commit-missing, and
no-authority hook states.
Override -PreflightScript to falsify the suite against a pre-fix script.
#>
param(
    [string]$PreflightScript,
    [string]$PowerShellExecutable
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrEmpty($PreflightScript)) {
    $PreflightScript = Join-Path $PSScriptRoot '../agent-preflight.ps1'
}
$PreflightScript = (Resolve-Path -LiteralPath $PreflightScript).Path
if ([string]::IsNullOrEmpty($PowerShellExecutable)) {
    $PowerShellExecutable = (Get-Process -Id $PID).Path
}
$GitExecutable = (Get-Command git -CommandType Application | Select-Object -First 1).Source

$FixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('preflight hook 17413-' + [guid]::NewGuid().ToString('N'))
$Canonical = Join-Path $FixtureRoot 'canonical'
$Worktrees = Join-Path $FixtureRoot 'worktrees'
$Worktree = Join-Path $Worktrees '17413-hook'
$Targets = Join-Path $FixtureRoot 'targets'
$Installed = Join-Path $Canonical '.git/hooks/pre-push'
$InstalledCommit = Join-Path $Canonical '.git/hooks/pre-commit'
$Authority = Join-Path $Worktree 'hooks/pre-push'

$Passed = 0
$Failed = 0

function Invoke-Native {
    param(
        [string]$Executable,
        [string[]]$Arguments,
        [string]$WorkDir
    )
    $savedPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        if ($WorkDir) { Push-Location -LiteralPath $WorkDir }
        try {
            $records = @(& $Executable @Arguments 2>&1)
            $status = $LASTEXITCODE
        } finally {
            if ($WorkDir) { Pop-Location }
        }
    } finally {
        $ErrorActionPreference = $savedPreference
    }
    return @{ Status = $status; Text = ($records -join "`n") }
}

function Invoke-FixtureGit {
    param([string[]]$GitArgs)
    $result = Invoke-Native -Executable $GitExecutable -Arguments $GitArgs
    if ($result.Status -ne 0) { throw "fixture git failed ($($result.Status)): $($result.Text)" }
}

function Invoke-Preflight {
    return Invoke-Native -Executable $PowerShellExecutable -Arguments @(
        '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $PreflightScript,
        '-Issue', '17413', '-Slug', 'hook',
        '-CanonicalRoot', $Canonical,
        '-WorktreeRoot', $Worktrees,
        '-TargetRoot', $Targets
    ) -WorkDir $Worktree
}

Write-Output '=== agent-preflight.ps1 hook-currency fixtures ==='

try {
    New-Item -ItemType Directory -Path $FixtureRoot | Out-Null
    Invoke-FixtureGit @('init', '-q', '-b', 'main', $Canonical)
    New-Item -ItemType Directory -Path (Join-Path $Canonical 'hooks') | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $Canonical 'hooks/pre-push'),
        "#!/bin/sh`necho HOOK-AUTHORITY`n",
        [Text.UTF8Encoding]::new($false))
    Invoke-FixtureGit @('-C', $Canonical, 'add', '--', 'hooks/pre-push')
    Invoke-FixtureGit @('-C', $Canonical, '-c', 'user.name=Preflight fixture',
        '-c', 'user.email=preflight@example.invalid', 'commit', '-q', '-m', 'fixture authority')
    New-Item -ItemType Directory -Path $Worktrees | Out-Null
    Invoke-FixtureGit @('-C', $Canonical, 'worktree', 'add', '-q', '-b', 'fixture/17413-hook', $Worktree)

    # Derive installed-hook variants from the bytes git actually checked out,
    # so the suite is deterministic under any core.autocrlf setting.
    $latin1 = [Text.Encoding]::GetEncoding(28591)
    $utf8NoBom = [Text.UTF8Encoding]::new($false)
    $authorityBytes = [IO.File]::ReadAllBytes($Authority)
    $authorityText = $latin1.GetString($authorityBytes)
    $staleBytes = $utf8NoBom.GetBytes("#!/bin/sh`necho STALE-HOOK`n")
    $shapedBytes = New-Object byte[] ($authorityBytes.Length + 1)
    [Array]::Copy($authorityBytes, $shapedBytes, $authorityBytes.Length)
    $shapedBytes[$shapedBytes.Length - 1] = 10
    $crlfBytes = $utf8NoBom.GetBytes($authorityText.Replace("`n", "`r`n"))
    $spaceBytes = $utf8NoBom.GetBytes($authorityText.TrimEnd([char]13, [char]10) + ' ')

    $cases = @(
        @{ Name = 'missing'; Expected = 7; Diagnostic = 'pre-push hook is missing'; Installed = $null },
        @{ Name = 'stale'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $staleBytes },
        @{ Name = 'current'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $authorityBytes },
        @{ Name = 'installer-trailing-newline'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $shapedBytes },
        @{ Name = 'crlf-installation'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $crlfBytes },
        @{ Name = 'trailing-space-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $spaceBytes },
        @{ Name = 'pre-commit-missing'; Expected = 7; Diagnostic = 'pre-commit hook is missing'; Installed = $authorityBytes; NoCommitHook = $true },
        @{ Name = 'no-authority-noop'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $staleBytes; NoAuthority = $true }
    )

    foreach ($case in $cases) {
        [IO.File]::WriteAllBytes($Authority, $authorityBytes)
        if (Test-Path -LiteralPath $Installed) { Remove-Item -LiteralPath $Installed -Force }
        if ($case.ContainsKey('NoAuthority')) {
            Remove-Item -LiteralPath $Authority -Force
        } elseif ($null -ne $case.Installed) {
            [IO.File]::WriteAllBytes($Installed, $case.Installed)
        }
        if (Test-Path -LiteralPath $InstalledCommit) { Remove-Item -LiteralPath $InstalledCommit -Force }
        if (-not $case.ContainsKey('NoCommitHook')) {
            [IO.File]::WriteAllBytes($InstalledCommit, $utf8NoBom.GetBytes("#!/bin/sh`necho COMMIT-HOOK`n"))
        }
        $result = Invoke-Preflight
        $valid = ($result.Status -eq $case.Expected) -and $result.Text.Contains($case.Diagnostic)
        if ($case.Expected -eq 7) {
            $valid = $valid -and $result.Text.Contains('install-githooks') -and (-not $result.Text.Contains('agent preflight ok'))
        }
        if ($valid) {
            Write-Output "PASS $($case.Name)"
            $Passed++
        } else {
            Write-Output ("FAIL {0}: expected={1} actual={2}`n{3}" -f $case.Name, $case.Expected, $result.Status, $result.Text)
            $Failed++
        }
    }
    Write-Output "$Passed passed, $Failed failed"
} finally {
    # Only this unique, owned fixture tree is removed; it contains no user work.
    $resolvedFixture = [IO.Path]::GetFullPath($FixtureRoot)
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedFixture.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'refusing to clean a fixture outside the temp directory'
    }
    if (Test-Path -LiteralPath $resolvedFixture) {
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}

if ($Failed -ne 0) { exit 1 }
