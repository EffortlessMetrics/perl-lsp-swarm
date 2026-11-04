param([string]$Installer = (Join-Path $PSScriptRoot '../../install.ps1'))

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Installer = (Resolve-Path $Installer).Path
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ('perl-lsp-verification-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $TempRoot | Out-Null
$Shell = (Get-Process -Id $PID).Path
$Failures = 0

# Only transport is replaced. Real archive inspection, checksum, promotion,
# installed .cmd selector, --version invocation, and script completion execute.
$Harness = Join-Path $TempRoot 'harness.ps1'
@'
param([string]$Installer, [string]$Payload, [string]$InstallDir, [string]$CaseName)
$ErrorActionPreference = 'Stop'
Remove-Item Env:PERL_LSP_INSTALLER_LIBRARY_ONLY -ErrorAction SilentlyContinue
$env:PROCESSOR_ARCHITECTURE = 'AMD64'
Remove-Item Env:PROCESSOR_ARCHITEW6432 -ErrorAction SilentlyContinue
$Asset = 'perllsp-0.18.0-x86_64-pc-windows-msvc.zip'
$PayloadHash = (Get-FileHash -LiteralPath $Payload -Algorithm SHA256).Hash.ToLowerInvariant()
if ($CaseName -eq 'native-error-preference') {
    $PSNativeCommandUseErrorActionPreference = $true
}
function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
    if ($Uri.EndsWith('/SHA256SUMS')) {
        [IO.File]::WriteAllText($OutFile, "$PayloadHash  $Asset`n")
    } elseif ($Uri.EndsWith("/$Asset")) {
        Copy-Item -LiteralPath $Payload -Destination $OutFile
    } else { throw "unexpected request: $Uri" }
}
& $Installer -Version 0.18.0 -InstallDir $InstallDir -NoModifyPath
if (-not $?) { exit 1 }
'@ | Set-Content -LiteralPath $Harness -Encoding utf8

Add-Type -AssemblyName System.IO.Compression.FileSystem
function New-Payload {
    param([string]$Name, [string]$Exe)
    $Stage = Join-Path $TempRoot ('stage-' + $Name)
    New-Item -ItemType Directory -Path $Stage | Out-Null
    Copy-Item -LiteralPath $Exe -Destination (Join-Path $Stage 'perllsp.exe')
    Copy-Item -LiteralPath $Exe -Destination (Join-Path $Stage 'perl-dap.exe')
    foreach ($Member in @('README.md', 'LICENSE-APACHE', 'LICENSE-MIT', 'SHA256SUMS.txt')) {
        [IO.File]::WriteAllText((Join-Path $Stage $Member), 'fixture')
    }
    $Zip = Join-Path $TempRoot ($Name + '.zip')
    [IO.Compression.ZipFile]::CreateFromDirectory($Stage, $Zip)
    return $Zip
}

function Invoke-Case {
    param([string]$Name, [string]$Payload, [string]$InstallDir, [int]$Expected, [string[]]$Needles, [string]$HarnessPath = $Harness)
    # PS 5.1 turns native stderr into ErrorRecords; continue only for the child
    # process capture, then assert its real exit status and complete diagnostics.
    $SavedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $Output = & $Shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $HarnessPath -Installer $Installer -Payload $Payload -InstallDir $InstallDir -CaseName $Name 2>&1
    $Status = $LASTEXITCODE
    $ErrorActionPreference = $SavedPreference
    $Text = $Output -join "`n"
    $Problems = @()
    if ($Status -ne $Expected) { $Problems += "expected exit $Expected, got $Status" }
    foreach ($Needle in $Needles) {
        if (-not $Text.Contains($Needle)) { $Problems += "missing diagnostic: $Needle" }
    }
    if ($Expected -ne 0) {
        $Selector = 'perllsp.cmd'
        if ($Name -eq 'missing-selector-block') { $Selector = 'missing-perllsp.cmd' }
        $DiagnosticPath = Join-Path $InstallDir $Selector
        if (-not $Text.Contains("verification failed at '$DiagnosticPath'")) {
            $Problems += 'verification error omitted the installed path'
        }
        foreach ($Forbidden in @('Installation complete!', 'Installation verified:', 'PATH status:', 'Added ', 'To get started')) {
            if ($Text.Contains($Forbidden)) { $Problems += "false completion: $Forbidden" }
        }
    }
    if ($Problems.Count) {
        $script:Failures++
        Write-Host "FAIL $Name ($($Problems -join '; '))`n$Text"
    } else { Write-Host "PASS $Name exit=$Status" }
}

try {
    $ValidExe = Join-Path $TempRoot 'valid.exe'
    $FailExe = Join-Path $TempRoot 'nonzero.exe'
    $SilentExe = Join-Path $TempRoot 'silent.exe'
    $StderrExe = Join-Path $TempRoot 'stderr.exe'
    $CorruptExe = Join-Path $TempRoot 'corrupt.exe'
    & "$PSScriptRoot/new-install-ps1-native-fixture.ps1" -Path $ValidExe
    & "$PSScriptRoot/new-install-ps1-native-fixture.ps1" -Path $FailExe -ExitCode 23
    & "$PSScriptRoot/new-install-ps1-native-fixture.ps1" -Path $SilentExe -ExitCode 23 -Silent
    & "$PSScriptRoot/new-install-ps1-native-fixture.ps1" -Path $StderrExe -Stderr
    [IO.File]::WriteAllText($CorruptExe, 'not a PE executable')
    $Valid = New-Payload valid $ValidExe
    $Nonzero = New-Payload nonzero $FailExe
    $Silent = New-Payload silent $SilentExe
    $StderrPayload = New-Payload stderr $StderrExe
    $Corrupt = New-Payload corrupt $CorruptExe
    $InstallDir = Join-Path $TempRoot 'install with spaces'
    Invoke-Case valid $Valid $InstallDir 0 @('Installation verified: perllsp fixture 0.18.0', 'Installation complete!', 'PATH status: manual_path_action_required')
    Invoke-Case benign-stderr $StderrPayload (Join-Path $TempRoot 'stderr install') 0 @('Installation verified:', 'fixture benign stderr', 'perllsp fixture 0.18.0', 'Installation complete!')
    $OldId = ([IO.File]::ReadAllText((Join-Path $InstallDir '.perl-lsp/current'))).Trim()
    Invoke-Case native-nonzero $Nonzero $InstallDir 1 @($InstallDir, 'exit code 23', 'perllsp fixture 0.18.0', 'fixture verification failure')
    $PreviousId = ([IO.File]::ReadAllText((Join-Path $InstallDir '.perl-lsp/previous'))).Trim()
    $PreviousExe = Join-Path $InstallDir ".perl-lsp/$PreviousId/perllsp.exe"
    $PreviousOutput = & $PreviousExe --version
    if ($PreviousId -ne $OldId -or $LASTEXITCODE -ne 0 -or $PreviousOutput -ne 'perllsp fixture 0.18.0') {
        throw 'failed verification lost the previous runnable recovery candidate'
    }
    Write-Host 'PASS failed upgrade retains runnable previous candidate'
    Invoke-Case silent-nonzero $Silent (Join-Path $TempRoot 'silent install') 1 @('exit code 23')
    Invoke-Case native-error-preference $Nonzero (Join-Path $TempRoot 'native preference install') 1 @('exit code 23', 'fixture verification failure')
    Invoke-Case corrupt $Corrupt (Join-Path $TempRoot 'corrupt install') 1 @('verification failed', 'perllsp.cmd')

    # Invocation errors need a component control: real main always just created
    # its selector. Execute the actual verification try/catch verbatim with a
    # missing selector and a stale successful native exit code.
    $Tokens = $null
    $Errors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$Tokens, [ref]$Errors)
    if ($Errors.Count) { throw 'installer parse failed' }
    $Verification = @($Ast.FindAll({
        param($Node)
        $Node -is [Management.Automation.Language.TryStatementAst] -and
        $Node.CatchClauses.Count -gt 0 -and $Node.Extent.Text.Contains('$VersionOutput =')
    }, $true) | Sort-Object { $_.Extent.Text.Length })[0].Extent.Text
    $ExceptionHarness = Join-Path $TempRoot 'invocation-error.ps1'
    $ExceptionPreamble = @'
param([string]$InstallDir)
$ErrorActionPreference = 'Stop'
$global:LASTEXITCODE = 0
$DestPath = Join-Path $InstallDir 'missing-perllsp.cmd'
function Write-Error { param([string]$Message) Write-Host $Message; exit 1 }
function Write-Warn { param([string]$Message) Write-Host $Message }
function Write-Success { param([string]$Message) Write-Host $Message }
'@
    [IO.File]::WriteAllText($ExceptionHarness, ($ExceptionPreamble + "`n" + $Verification + "`nWrite-Host 'Installation complete!'"))
    Invoke-Case missing-selector-block '' (Join-Path $TempRoot 'missing install') 1 @('verification failed', 'missing-perllsp.cmd') -HarnessPath $ExceptionHarness
} finally {
    # All mutation is under this freshly allocated, absolute temporary root.
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
if ($Failures) { throw "$Failures installed verification cases failed" }
Write-Host 'All installed verification cases passed'
# GitHub's pwsh wrapper exits with LASTEXITCODE after dot-sourcing this file.
# The last negative control deliberately leaves it at 1; signal suite success.
exit 0
