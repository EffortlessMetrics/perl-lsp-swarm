$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Focused classifier proof for Get-StagedProductUnitDisposition (#17968).
# Directory-shaped perllsp.exe / perl-dap.exe names must not satisfy the
# archive-pair or source-only dispositions.

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Installer = Join-Path $Root "install.ps1"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("perl-lsp-staged-disposition-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TempRoot -Force | Out-Null

$Pass = 0
$Fail = 0

function Pass-Case {
    param([string]$Name)
    Write-Host "PASS  $Name"
    $script:Pass++
}

function Fail-Case {
    param([string]$Name, [string]$Detail)
    Write-Host "FAIL  $Name" -ForegroundColor Red
    Write-Host "      $Detail" -ForegroundColor Red
    $script:Fail++
}

function Write-MemberFile {
    param([string]$Path, [string]$Payload = "payload")
    [IO.File]::WriteAllText($Path, ($Payload + "`n"))
}

function New-StageRoot {
    return (Join-Path $TempRoot ("stage-" + [guid]::NewGuid().ToString("N")))
}

function Invoke-Disposition {
    param(
        [Parameter(Mandatory = $true)][string]$StageRoot,
        [Parameter(Mandatory = $true)][ValidateSet("release", "source")][string]$Mode
    )
    try {
        $value = Get-StagedProductUnitDisposition -ExtractDir $StageRoot -Mode $Mode
        return @{ Status = 0; Value = $value; Error = "" }
    } catch {
        return @{ Status = 1; Value = $null; Error = [string]$_ }
    }
}

try {
    $env:PERL_LSP_INSTALLER_LIBRARY_ONLY = "1"
    . $Installer

    Write-Host "=== staged product-unit disposition (#17968) ==="

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perllsp.exe") -Payload "server"
    Write-MemberFile -Path (Join-Path $stage "perl-dap.exe") -Payload "dap"
    $pair = Invoke-Disposition -StageRoot $stage -Mode "release"
    if (($pair.Status -eq 0) -and ($pair.Value -eq "archive_pair_required")) {
        Pass-Case "regular release pair is accepted"
    } else {
        Fail-Case "regular release pair is accepted" "status=$($pair.Status) value=$($pair.Value) error=$($pair.Error)"
    }

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perllsp.exe") -Payload "server-only"
    $source = Invoke-Disposition -StageRoot $stage -Mode "source"
    if (($source.Status -eq 0) -and ($source.Value -eq "advanced_source_server_only")) {
        Pass-Case "source mode accepts regular server-only staging"
    } else {
        Fail-Case "source mode accepts regular server-only staging" "status=$($source.Status) value=$($source.Value) error=$($source.Error)"
    }

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $stage "perllsp.exe") -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perl-dap.exe") -Payload "dap"
    $serverDir = Invoke-Disposition -StageRoot $stage -Mode "release"
    if (($serverDir.Status -ne 0) -and ($serverDir.Error -like "*missing a regular perllsp member*")) {
        Pass-Case "directory-shaped perllsp.exe is rejected in release mode"
    } else {
        Fail-Case "directory-shaped perllsp.exe is rejected in release mode" "status=$($serverDir.Status) value=$($serverDir.Value) error=$($serverDir.Error)"
    }

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perllsp.exe") -Payload "server"
    New-Item -ItemType Directory -Path (Join-Path $stage "perl-dap.exe") -Force | Out-Null
    $dapDir = Invoke-Disposition -StageRoot $stage -Mode "release"
    if (($dapDir.Status -ne 0) -and ($dapDir.Error -like "*complete perllsp/perl-dap pair*")) {
        Pass-Case "directory-shaped perl-dap.exe is rejected in release mode"
    } else {
        Fail-Case "directory-shaped perl-dap.exe is rejected in release mode" "status=$($dapDir.Status) value=$($dapDir.Value) error=$($dapDir.Error)"
    }

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perl-dap.exe") -Payload "dap-only"
    $missingServer = Invoke-Disposition -StageRoot $stage -Mode "release"
    if (($missingServer.Status -ne 0) -and ($missingServer.Error -like "*missing a regular perllsp member*")) {
        Pass-Case "missing perllsp member is rejected"
    } else {
        Fail-Case "missing perllsp member is rejected" "status=$($missingServer.Status) value=$($missingServer.Value) error=$($missingServer.Error)"
    }

    $stage = New-StageRoot
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Write-MemberFile -Path (Join-Path $stage "perllsp.exe") -Payload "server"
    $missingDap = Invoke-Disposition -StageRoot $stage -Mode "release"
    if (($missingDap.Status -ne 0) -and ($missingDap.Error -like "*complete perllsp/perl-dap pair*")) {
        Pass-Case "missing perl-dap member is rejected in release mode"
    } else {
        Fail-Case "missing perl-dap member is rejected in release mode" "status=$($missingDap.Status) value=$($missingDap.Value) error=$($missingDap.Error)"
    }
} finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Results: $Pass passed, $Fail failed"
if ($Fail -ne 0) {
    exit 1
}
