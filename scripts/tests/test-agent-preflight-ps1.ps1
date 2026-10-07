<#
.SYNOPSIS
Exercise Windows agent preflight with small, real linked-worktree fixtures.

.DESCRIPTION
Override -PreflightScript to falsify the suite against a pre-fix script.
The optional Git fault modes inject diagnostics without reading or changing user
configuration. Ordinary cases use Git's actual common-dir discovery.
All fixture commands and children use disposable configuration; caller settings
are restored even when setup or a case fails.
#>
[CmdletBinding()]
param(
    [string]$PreflightScript,
    [string]$PowerShellExecutable = (Get-Process -Id $PID).Path
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrEmpty($PreflightScript)) { $PreflightScript = Join-Path $PSScriptRoot '../agent-preflight.ps1' }
$PreflightScript = (Resolve-Path -LiteralPath $PreflightScript).Path
$GitExecutable = (Get-Command git -CommandType Application | Select-Object -First 1).Source
$FixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('perl-preflight-ps1-' + [guid]::NewGuid().ToString('N'))
$Canonical = Join-Path $FixtureRoot 'canonical checkout'
$Worktrees = Join-Path $FixtureRoot 'worktrees'
$Worktree = Join-Path $Worktrees '17413-fixture'
$Targets = Join-Path $FixtureRoot 'targets'
$AuthorityText = "#!/bin/sh`necho AUTHORITY`n"
$CommitText = "#!/bin/sh`necho COMMIT-HOOK`n"
$Passed = 0
$Failed = 0
New-Item -ItemType Directory -Path $FixtureRoot | Out-Null
$GitEnvironmentNames = @('GIT_CONFIG', 'GIT_CONFIG_GLOBAL', 'GIT_CONFIG_SYSTEM',
    'GIT_CONFIG_NOSYSTEM', 'GIT_CONFIG_COUNT', 'GIT_CONFIG_PARAMETERS',
    'GIT_TEMPLATE_DIR', 'GIT_TRACE', 'GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR',
    'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES')
$OriginalGitEnvironment = @{}
foreach ($Name in $GitEnvironmentNames) {
    $OriginalGitEnvironment[$Name] = [Environment]::GetEnvironmentVariable($Name, 'Process')
}

function Invoke-FixtureGit {
    param([string[]]$GitArgs)
    $SavedPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $Records = @(& $GitExecutable @GitArgs 2>&1)
        $Status = $LASTEXITCODE
    } finally { $ErrorActionPreference = $SavedPreference }
    # Retain diagnostics; they are not porcelain output or a nonzero status.
    foreach ($Record in $Records) {
        Add-Content -LiteralPath (Join-Path $FixtureRoot 'git.log') -Value "$Record"
        if ($Record -is [Management.Automation.ErrorRecord]) { Write-Warning "$Record" }
    }
    if ($Status -ne 0) { throw "fixture git failed ($Status): $($Records -join '; ')" }
}

function Write-FixtureText {
    param([string]$Path, [string]$Text)
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

$Harness = Join-Path $FixtureRoot 'invoke-preflight.ps1'
@'
param([string]$Preflight, [string]$Canonical, [string]$Worktrees,
      [string]$Worktree, [string]$Targets, [string]$GitExecutable, [string]$GitMode)
$ErrorActionPreference = 'Stop'
function git {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$GitArgs)
    if ($GitArgs -contains 'status' -and $GitMode -ne 'native') {
        $global:LASTEXITCODE = 0
        switch ($GitMode) {
            'clean' { return }
            'warning' { Write-Error 'fixture git diagnostic: exit zero, empty porcelain' -ErrorAction Continue; return }
            'dirty' { Write-Output ' M hooks/pre-push'; return }
            'failure' { $global:LASTEXITCODE = 23; Write-Error 'fixture git failed' -ErrorAction Continue; return }
            default { throw "unknown Git mode $GitMode" }
        }
    }
    & $GitExecutable @GitArgs
}
# Native cases must reach the executable directly, without a function changing
# native stream types at the preflight call boundary.
if ($GitMode -eq 'native') {
    Remove-Item Function:git
    # Deterministic native stderr at exit zero, independent of ambient warnings.
    $env:GIT_TRACE = '1'
}
Set-Location -LiteralPath $Worktree
& $Preflight -Issue 17413 -Slug fixture -CanonicalRoot $Canonical -WorktreeRoot $Worktrees -TargetRoot $Targets
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $Harness -Encoding UTF8

try {
    $FixtureConfig = Join-Path $FixtureRoot 'gitconfig'
    $FixtureIgnore = Join-Path $FixtureRoot 'empty ignore'
    $FixtureSystemConfig = Join-Path $FixtureRoot 'empty system config'
    $FixtureTemplate = Join-Path $FixtureRoot 'empty template'
    $UnavailableSigner = (Join-Path $FixtureRoot 'absent signer').Replace('\', '/')
    New-Item -ItemType Directory -Path $FixtureTemplate | Out-Null
    Write-FixtureText $FixtureIgnore ''
    Write-FixtureText $FixtureSystemConfig ''
    $IgnoreForGit = $FixtureIgnore.Replace('\', '/')
    Write-FixtureText $FixtureConfig "[core]`n excludesFile = `"$IgnoreForGit`"`n autocrlf = false`n[commit]`n gpgSign = true`n[gpg]`n program = `"$UnavailableSigner`"`n"
    # Clear environment overrides before bootstrap, including repository paths
    # and templates that could import an unrelated checkout's hooks.
    foreach ($Name in $GitEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$Name" -ErrorAction SilentlyContinue
    }
    $env:GIT_CONFIG_GLOBAL = $FixtureConfig
    $env:GIT_CONFIG_SYSTEM = $FixtureSystemConfig
    $env:GIT_CONFIG_NOSYSTEM = '1'
    $env:GIT_TEMPLATE_DIR = $FixtureTemplate
    Invoke-FixtureGit -GitArgs @('init', '-q', '-b', 'main', $Canonical)
    $CanonicalHooks = Join-Path $Canonical 'hooks'
    New-Item -ItemType Directory -Path $CanonicalHooks | Out-Null
    Write-FixtureText (Join-Path $CanonicalHooks 'pre-push') $AuthorityText
    Invoke-FixtureGit -GitArgs @('-C', $Canonical, 'add', '--', 'hooks/pre-push')
    Invoke-FixtureGit -GitArgs @('-C', $Canonical, '-c', 'user.name=Preflight fixture', '-c', 'user.email=preflight@example.invalid', 'commit', '--no-gpg-sign', '-q', '-m', 'fixture authority')
    New-Item -ItemType Directory -Path $Worktrees | Out-Null
    Invoke-FixtureGit -GitArgs @('-C', $Canonical, 'worktree', 'add', '-q', '-b', 'fixture/17413', $Worktree)
    # The empty template deliberately imports no default hook directory/files.
    New-Item -ItemType Directory -Path (Join-Path $Canonical '.git/hooks') | Out-Null
    $Installed = Join-Path $Canonical '.git/hooks/pre-push'
    $InstalledCommit = Join-Path $Canonical '.git/hooks/pre-commit'
    $Authority = Join-Path $Worktree 'hooks/pre-push'
    # A linked-worktree-local decoy must never satisfy common-dir installation.
    $PrivateHookDir = Join-Path $Canonical '.git/worktrees/17413-fixture/hooks'
    New-Item -ItemType Directory -Path $PrivateHookDir | Out-Null
    Write-FixtureText (Join-Path $PrivateHookDir 'pre-push') $AuthorityText
    $CustomHooks = Join-Path $FixtureRoot 'custom hooks'
    $RelativeHooks = Join-Path $Worktree 'relative hooks'
    New-Item -ItemType Directory -Path $CustomHooks, $RelativeHooks | Out-Null

    $Cases = @(
        @{ Name = 'missing'; Expected = 7; Diagnostic = 'pre-push hook is missing'; Installed = $null },
        @{ Name = 'current'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $AuthorityText },
        @{ Name = 'stale-common-dir-despite-private-decoy'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = "#!/bin/sh`necho STALE`n" },
        @{ Name = 'case-only-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText.Replace('AUTHORITY', 'authority') },
        @{ Name = 'installer-trailing-newlines'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $AuthorityText + "`n`n" },
        @{ Name = 'crlf-installation'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $AuthorityText.Replace("`n", "`r`n") },
        @{ Name = 'utf8-bom-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = [string][char]0xfeff + $AuthorityText },
        @{ Name = 'utf16-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText; Utf16 = $true },
        @{ Name = 'nul-shebang-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText.Replace('#!', '#!' + [char]0) },
        @{ Name = 'raw-soft-hyphen-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText.Replace('#!', '#!' + [char]0xad); Latin1 = $true },
        @{ Name = 'leading-whitespace-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = ' ' + $AuthorityText },
        @{ Name = 'trailing-space-drift'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText.TrimEnd([char]10) + ' ' },
        @{ Name = 'installed-directory'; Expected = 7; Diagnostic = 'pre-push hook is missing'; Installed = $null; Directory = $true },
        @{ Name = 'no-authority'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = "echo STALE`n"; NoAuthority = $true; NoCommit = $true },
        @{ Name = 'worktree-revision-authority'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = "#!/bin/sh`necho BRANCH_REVISION`n"; Authority = "#!/bin/sh`necho BRANCH_REVISION`n" },
        @{ Name = 'absolute-hooks-path-missing'; Expected = 7; Diagnostic = 'pre-push hook is missing'; Installed = $AuthorityText; HooksPath = $CustomHooks; ActiveInstalled = $null },
        @{ Name = 'absolute-hooks-path-stale'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText; HooksPath = $CustomHooks; ActiveInstalled = "echo STALE`n" },
        @{ Name = 'absolute-hooks-path-current'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = "echo STALE`n"; HooksPath = $CustomHooks; ActiveInstalled = $AuthorityText },
        @{ Name = 'relative-hooks-path-missing'; Expected = 7; Diagnostic = 'pre-push hook is missing'; Installed = $AuthorityText; HooksPath = 'relative hooks'; ActiveInstalled = $null },
        @{ Name = 'relative-hooks-path-stale'; Expected = 7; Diagnostic = 'pre-push hook is stale'; Installed = $AuthorityText; HooksPath = 'relative hooks'; ActiveInstalled = "echo STALE`n" },
        @{ Name = 'relative-hooks-path-current'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = "echo STALE`n"; HooksPath = 'relative hooks'; ActiveInstalled = $AuthorityText },
        @{ Name = 'no-authority-custom-hooks-path'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $null; HooksPath = 'relative hooks'; ActiveInstalled = $null; NoAuthority = $true; NoCommit = $true; NoActiveCommit = $true },
        @{ Name = 'pre-commit-missing'; Expected = 7; Diagnostic = 'pre-commit hook is missing'; Installed = $AuthorityText; NoCommit = $true },
        @{ Name = 'pre-commit-directory'; Expected = 7; Diagnostic = 'pre-commit hook is missing'; Installed = $AuthorityText; CommitDirectory = $true },
        @{ Name = 'absolute-pre-commit-missing-despite-common-copy'; Expected = 7; Diagnostic = 'pre-commit hook is missing'; Installed = "echo STALE`n"; HooksPath = $CustomHooks; ActiveInstalled = $AuthorityText; NoActiveCommit = $true },
        @{ Name = 'relative-pre-commit-missing-despite-common-copy'; Expected = 7; Diagnostic = 'pre-commit hook is missing'; Installed = "echo STALE`n"; HooksPath = 'relative hooks'; ActiveInstalled = $AuthorityText; NoActiveCommit = $true },
        @{ Name = 'native-git-current'; Expected = 0; Diagnostic = 'agent preflight ok'; Installed = $AuthorityText; GitMode = 'native' },
        @{ Name = 'native-dirty-porcelain-refused'; Expected = 1; Diagnostic = 'Canonical checkout is dirty'; Installed = $AuthorityText; GitMode = 'native'; DirtyCanonical = $true },
        @{ Name = 'git-warning-not-dirty'; Expected = 0; Diagnostic = 'fixture git diagnostic: exit zero, empty porcelain'; Installed = $AuthorityText; GitMode = 'warning' },
        @{ Name = 'dirty-porcelain-refused'; Expected = 1; Diagnostic = 'Canonical checkout is dirty'; Installed = $AuthorityText; GitMode = 'dirty' },
        @{ Name = 'failed-git-refused'; Expected = 1; Diagnostic = 'fixture git failed'; Installed = $AuthorityText; GitMode = 'failure' }
    )

    $CustomPathConfigured = $false
    foreach ($Case in $Cases) {
        $CaseTarget = Join-Path $Targets '17413-fixture'
        # Preflight creates an empty directory only; refuse unexpected contents.
        if (Test-Path -LiteralPath $CaseTarget) { Remove-Item -LiteralPath $CaseTarget }
        # Change only this disposable repository's configuration. The parent
        # directory exists for every configured path, including paths with spaces.
        if ($CustomPathConfigured) {
            Invoke-FixtureGit -GitArgs @('-C', $Canonical, 'config', '--local', '--unset', 'core.hooksPath')
            $CustomPathConfigured = $false
        }
        if ($Case.ContainsKey('HooksPath')) {
            Invoke-FixtureGit -GitArgs @('-C', $Canonical, 'config', '--local', 'core.hooksPath', $Case.HooksPath)
            $CustomPathConfigured = $true
            $ActiveDirectory = $Case.HooksPath
            if (-not [IO.Path]::IsPathRooted($ActiveDirectory)) { $ActiveDirectory = Join-Path $Worktree $ActiveDirectory }
            $ActiveHook = Join-Path $ActiveDirectory 'pre-push'
            if (Test-Path -LiteralPath $ActiveHook) { Remove-Item -LiteralPath $ActiveHook }
            if ($null -ne $Case.ActiveInstalled) { Write-FixtureText $ActiveHook $Case.ActiveInstalled }
            $ActiveCommit = Join-Path $ActiveDirectory 'pre-commit'
            if (Test-Path -LiteralPath $ActiveCommit) { Remove-Item -LiteralPath $ActiveCommit }
            if (-not $Case.ContainsKey('NoActiveCommit')) { Write-FixtureText $ActiveCommit $CommitText }
        }
        Write-FixtureText (Join-Path $CanonicalHooks 'pre-push') $AuthorityText
        if ($Case.ContainsKey('DirtyCanonical')) { Write-FixtureText (Join-Path $CanonicalHooks 'pre-push') "echo DIRTY`n" }
        if (Test-Path -LiteralPath $Installed) { Remove-Item -LiteralPath $Installed -Force }
        if (Test-Path -LiteralPath $InstalledCommit) { Remove-Item -LiteralPath $InstalledCommit }
        if ($Case.ContainsKey('CommitDirectory')) { New-Item -ItemType Directory -Path $InstalledCommit | Out-Null }
        elseif (-not $Case.ContainsKey('NoCommit')) { Write-FixtureText $InstalledCommit $CommitText }
        if (-not (Test-Path -LiteralPath (Split-Path -Parent $Authority))) { New-Item -ItemType Directory -Path (Split-Path -Parent $Authority) | Out-Null }
        Write-FixtureText $Authority $AuthorityText
        if ($Case.ContainsKey('Authority')) { Write-FixtureText $Authority $Case.Authority }
        if ($Case.ContainsKey('NoAuthority')) { Remove-Item -LiteralPath $Authority }
        if ($Case.ContainsKey('Directory')) { New-Item -ItemType Directory -Path $Installed | Out-Null }
        elseif ($Case.ContainsKey('Utf16')) { [IO.File]::WriteAllText($Installed, $Case.Installed, [Text.Encoding]::Unicode) }
        elseif ($Case.ContainsKey('Latin1')) { [IO.File]::WriteAllBytes($Installed, [Text.Encoding]::GetEncoding(28591).GetBytes($Case.Installed)) }
        elseif ($null -ne $Case.Installed) { Write-FixtureText $Installed $Case.Installed }
        $GitMode = 'clean'
        if ($Case.ContainsKey('GitMode')) { $GitMode = $Case.GitMode }
        $Output = @(& $PowerShellExecutable -NoProfile -NonInteractive -File $Harness -Preflight $PreflightScript -Canonical $Canonical -Worktrees $Worktrees -Worktree $Worktree -Targets $Targets -GitExecutable $GitExecutable -GitMode $GitMode 2>&1)
        $Status = $LASTEXITCODE
        $Text = $Output -join "`n"
        $Valid = $Status -eq $Case.Expected -and $Text.Contains($Case.Diagnostic)
        if ($GitMode -eq 'native') { $Valid = $Valid -and $Text.Contains('trace: built-in: git') }
        if ($Case.Expected -eq 7) {
            $Valid = $Valid -and $Text.Contains('bash scripts/install-githooks.sh') -and -not $Text.Contains('agent preflight ok') -and -not (Test-Path -LiteralPath $CaseTarget)
        }
        if ($Case.Expected -eq 0) { $Valid = $Valid -and (Test-Path -LiteralPath $CaseTarget -PathType Container) }
        if ($Valid) { Write-Output "PASS $($Case.Name)"; $Passed++ }
        else { Write-Output "FAIL $($Case.Name): expected=$($Case.Expected), actual=$Status`n$Text"; $Failed++ }
    }
    Write-Output "$Passed passed, $Failed failed"
} finally {
    # Preserve absent versus empty values on PowerShell versions supporting both.
    foreach ($Name in $GitEnvironmentNames) {
        if ($null -eq $OriginalGitEnvironment[$Name]) {
            Remove-Item -LiteralPath "Env:$Name" -ErrorAction SilentlyContinue
        } else {
            Set-Item -LiteralPath "Env:$Name" -Value $OriginalGitEnvironment[$Name]
        }
    }
    # Only this unique, owned fixture tree is removed; it contains no user work.
    $ResolvedFixture = [IO.Path]::GetFullPath($FixtureRoot)
    $TempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $ResolvedFixture.StartsWith($TempPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture cleanup escaped the temporary directory' }
    Remove-Item -LiteralPath $ResolvedFixture -Recurse -Force
}
if ($Failed -ne 0) { exit 1 }
