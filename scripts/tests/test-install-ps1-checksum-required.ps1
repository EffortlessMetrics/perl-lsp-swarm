$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Installer = Join-Path $Root "install.ps1"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("perl-lsp-install-ps1-" + [guid]::NewGuid().ToString("N"))
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

$Harness = Join-Path $TempRoot "case-harness.ps1"
@'
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$CaseName,
    [Parameter(Mandatory = $true)][string]$CaseRoot,
    [Parameter(Mandatory = $true)][string]$Payload,
    [Parameter(Mandatory = $true)][string]$PayloadHash
)

$ErrorActionPreference = "Stop"
$env:PROCESSOR_ARCHITECTURE = "AMD64"
Remove-Item Env:PROCESSOR_ARCHITEW6432 -ErrorAction SilentlyContinue

$Asset = "perllsp-0.18.0-x86_64-pc-windows-msvc.zip"
$Log = Join-Path $CaseRoot "requests.log"
$ExpandSentinel = Join-Path $CaseRoot "expanded"
$InstallDir = Join-Path $CaseRoot "install"

function Invoke-WebRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [switch]$UseBasicParsing
    )

    Add-Content -LiteralPath $Log -Value $Uri
    if ($Uri.EndsWith("/SHA256SUMS")) {
        switch ($CaseName) {
            "missing-manifest" { throw "simulated missing manifest" }
            "missing-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  other.zip`n"
                return
            }
            "substring-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  prefix-$Asset`n"
                return
            }
            "duplicate-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  $Asset`n$PayloadHash *$Asset`n"
                return
            }
            "uppercase-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$($PayloadHash.ToUpperInvariant())  $Asset`n"
                return
            }
            "uppercase-asset-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  $($Asset.ToUpperInvariant())`n"
                return
            }
            "malformed-duplicate-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  $Asset`nabc123  $Asset`n"
                return
            }
            "short-row" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "abc123  $Asset`n"
                return
            }
            "mismatch" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$(('0' * 64))  $Asset`n"
                return
            }
            "valid-binary-crlf" {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash *$Asset`r`n"
                return
            }
            default {
                Set-Content -LiteralPath $OutFile -Encoding ascii -NoNewline -Value "$PayloadHash  $Asset`n"
                return
            }
        }
    }

    if ($Uri.EndsWith("/$Asset")) {
        Copy-Item -LiteralPath $Payload -Destination $OutFile -Force
        return
    }

    throw "unexpected request: $Uri"
}

function Expand-Archive {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [switch]$Force
    )

    Set-Content -LiteralPath $ExpandSentinel -Encoding ascii -Value "expanded"
    $Extracted = Join-Path $DestinationPath "perllsp-0.18.0-x86_64-pc-windows-msvc"
    New-Item -ItemType Directory -Path $Extracted -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $Extracted "perllsp.exe") -Encoding ascii -Value "test server"
    Set-Content -LiteralPath (Join-Path $Extracted "perl-dap.exe") -Encoding ascii -Value "test dap"
}

& $Installer -Version "0.18.0" -InstallDir $InstallDir
$InstallerSucceeded = $?
if (-not $InstallerSucceeded) {
    exit 1
}
'@ | Set-Content -LiteralPath $Harness -Encoding utf8

$Payload = Join-Path $TempRoot "archive.zip"
Set-Content -LiteralPath $Payload -Encoding ascii -NoNewline -Value "bounded archive bytes`n"
$PayloadHash = (Get-FileHash -LiteralPath $Payload -Algorithm SHA256).Hash.ToLowerInvariant()

$Cases = @(
    @{ Name = "valid"; Expected = 0; AssetRequested = $true; Expanded = $true },
    @{ Name = "valid-binary-crlf"; Expected = 0; AssetRequested = $true; Expanded = $true },
    @{ Name = "missing-manifest"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "missing-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "substring-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "duplicate-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "uppercase-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "uppercase-asset-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "malformed-duplicate-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "short-row"; Expected = 1; AssetRequested = $false; Expanded = $false },
    @{ Name = "mismatch"; Expected = 1; AssetRequested = $true; Expanded = $false }
)

try {
    foreach ($Case in $Cases) {
        $CaseRoot = Join-Path $TempRoot $Case.Name
        New-Item -ItemType Directory -Path $CaseRoot -Force | Out-Null
        $Output = & pwsh -NoProfile -NonInteractive -File $Harness `
            -Installer $Installer `
            -CaseName $Case.Name `
            -CaseRoot $CaseRoot `
            -Payload $Payload `
            -PayloadHash $PayloadHash 2>&1
        $Status = $LASTEXITCODE

        $LogPath = Join-Path $CaseRoot "requests.log"
        $Requests = @(
            if (Test-Path -LiteralPath $LogPath) {
                Get-Content -LiteralPath $LogPath
            }
        )
        $AssetRequest = @($Requests | Where-Object { $_ -like "*.zip" }).Count -eq 1
        $Expanded = Test-Path -LiteralPath (Join-Path $CaseRoot "expanded")
        $ServerInstalled = Test-Path -LiteralPath (Join-Path $CaseRoot "install/perllsp.exe")
        $DapInstalled = Test-Path -LiteralPath (Join-Path $CaseRoot "install/perl-dap.exe")

        $Problems = [System.Collections.Generic.List[string]]::new()
        if ($Status -ne $Case.Expected) {
            $Problems.Add("expected status $($Case.Expected), got $Status")
        }
        if ($Requests.Count -lt 1 -or -not $Requests[0].EndsWith("/SHA256SUMS")) {
            $Problems.Add("checksum manifest was not the first request")
        }
        if ($AssetRequest -ne $Case.AssetRequested) {
            $Problems.Add("asset request expected=$($Case.AssetRequested) actual=$AssetRequest")
        }
        if ($Expanded -ne $Case.Expanded) {
            $Problems.Add("extraction expected=$($Case.Expanded) actual=$Expanded")
        }
        if ($Case.Expected -eq 0) {
            if (-not ($ServerInstalled -and $DapInstalled)) {
                $Problems.Add("successful case did not install both binaries")
            }
        } elseif ($ServerInstalled -or $DapInstalled) {
            $Problems.Add("failed case changed the install destination")
        }

        if ($Problems.Count -eq 0) {
            Pass-Case $Case.Name
        } else {
            Fail-Case $Case.Name (($Problems -join "; ") + "`n" + ($Output -join "`n"))
        }
    }
} finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "=== Results: $Pass passed, $Fail failed ==="
if ($Fail -ne 0) {
    exit 1
}
exit 0
