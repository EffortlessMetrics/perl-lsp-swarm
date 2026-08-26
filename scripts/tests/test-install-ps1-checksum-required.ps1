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

& $Installer -Version "0.18.0" -InstallDir $InstallDir -NoModifyPath
$InstallerSucceeded = $?
if (-not $InstallerSucceeded) {
    exit 1
}
'@ | Set-Content -LiteralPath $Harness -Encoding utf8

$Payload = Join-Path $TempRoot "archive.zip"
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path -LiteralPath $Payload) {
    Remove-Item -LiteralPath $Payload -Force
}
$Zip = [System.IO.Compression.ZipFile]::Open($Payload, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    $Files = @{
        "perllsp.exe" = [Text.Encoding]::ASCII.GetBytes("test server")
        "perl-dap.exe" = [Text.Encoding]::ASCII.GetBytes("test dap")
        "README.md" = [Text.Encoding]::ASCII.GetBytes("readme`n")
        "LICENSE-APACHE" = [Text.Encoding]::ASCII.GetBytes("apache`n")
        "LICENSE-MIT" = [Text.Encoding]::ASCII.GetBytes("mit`n")
        "SHA256SUMS.txt" = [Text.Encoding]::ASCII.GetBytes("sums`n")
    }
    foreach ($Name in $Files.Keys) {
        $Entry = $Zip.CreateEntry($Name)
        $Stream = $Entry.Open()
        try {
            $Stream.Write($Files[$Name], 0, $Files[$Name].Length)
        } finally {
            $Stream.Dispose()
        }
    }
} finally {
    $Zip.Dispose()
}
$PayloadHash = (Get-FileHash -LiteralPath $Payload -Algorithm SHA256).Hash.ToLowerInvariant()

$Cases = @(
    @{ Name = "valid"; Expected = 0; AssetRequested = $true },
    @{ Name = "valid-binary-crlf"; Expected = 0; AssetRequested = $true },
    @{ Name = "missing-manifest"; Expected = 1; AssetRequested = $false },
    @{ Name = "missing-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "substring-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "duplicate-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "uppercase-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "uppercase-asset-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "malformed-duplicate-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "short-row"; Expected = 1; AssetRequested = $false },
    @{ Name = "mismatch"; Expected = 1; AssetRequested = $true }
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
        $installDir = Join-Path $CaseRoot "install"
        $ServerCmd = Test-Path -LiteralPath (Join-Path $installDir "perllsp.cmd")
        $DapCmd = Test-Path -LiteralPath (Join-Path $installDir "perl-dap.cmd")
        $ServerExe = Test-Path -LiteralPath (Join-Path $installDir "perllsp.exe")
        $DapExe = Test-Path -LiteralPath (Join-Path $installDir "perl-dap.exe")

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
        if ($Case.Expected -eq 0) {
            if (-not ($ServerCmd -and $DapCmd)) {
                $Problems.Add("successful case did not install both PATH selectors")
            }
            if ($ServerExe -or $DapExe) {
                $Problems.Add("successful case published independent PATH copies")
            }
        } elseif ($ServerCmd -or $DapCmd -or $ServerExe -or $DapExe) {
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
