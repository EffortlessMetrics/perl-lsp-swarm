$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Discriminating product-unit promotion proof for install.ps1 (#8359).
# PATH-visible names and .perl-lsp/current must observe one complete unit.

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Installer = Join-Path $Root "install.ps1"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("perl-lsp-product-unit-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TempRoot -Force | Out-Null

$Pass = 0
$Fail = 0
$LastStatus = 0
$LastOutput = ""
$LastResult = $null
$InstallDir = $null
$ExtractDir = $null

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

function Hash-BytesFile {
    param([string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Payload {
    param([string]$Path, [string]$Payload)
    [IO.File]::WriteAllText($Path, ($Payload + "`n"))
}

function Stage-Pair {
    param([string]$Dest, [string]$Server, [string]$Dap)
    if (Test-Path -LiteralPath $Dest) {
        Remove-Item -LiteralPath $Dest -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Dest -Force | Out-Null
    Write-Payload -Path (Join-Path $Dest "perllsp.exe") -Payload $Server
    Write-Payload -Path (Join-Path $Dest "perl-dap.exe") -Payload $Dap
}

function Stage-ServerOnly {
    param([string]$Dest, [string]$Server)
    if (Test-Path -LiteralPath $Dest) {
        Remove-Item -LiteralPath $Dest -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Dest -Force | Out-Null
    Write-Payload -Path (Join-Path $Dest "perllsp.exe") -Payload $Server
}

function Setup-Root {
    $script:InstallDir = Join-Path $TempRoot ("install-" + [guid]::NewGuid().ToString("N"))
    $script:ExtractDir = Join-Path $TempRoot ("stage-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $script:InstallDir -Force | Out-Null
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
}

function Invoke-Promote {
    param([string]$Mode = "release")
    $script:LastStatus = 0
    $script:LastOutput = ""
    $script:LastResult = $null
    try {
        $result = Install-StandaloneProductUnit -ExtractDir $script:ExtractDir -InstallDir $script:InstallDir -Mode $Mode
        $script:LastResult = $result
        $script:LastOutput = [string]$result.Receipt
    } catch {
        $script:LastStatus = 1
        $script:LastOutput = [string]$_
    }
}

function Assert-CompletePair {
    param([string]$Server, [string]$Dap)
    $expect = Join-Path $TempRoot "expect"
    New-Item -ItemType Directory -Path $expect -Force | Out-Null
    Write-Payload -Path (Join-Path $expect "perllsp.exe") -Payload $Server
    Write-Payload -Path (Join-Path $expect "perl-dap.exe") -Payload $Dap
    $wantServer = Hash-BytesFile (Join-Path $expect "perllsp.exe")
    $wantDap = Hash-BytesFile (Join-Path $expect "perl-dap.exe")
    $serverPath = Join-Path $script:InstallDir "perllsp.exe"
    $dapPath = Join-Path $script:InstallDir "perl-dap.exe"
    $sitem = Get-Item -LiteralPath $serverPath
    $ditem = Get-Item -LiteralPath $dapPath
    $st = $null
    $dt = $null
    if ($sitem.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        $st = $sitem.Target; if ($st -is [array]) { $st = $st[0] }
    }
    if ($ditem.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        $dt = $ditem.Target; if ($dt -is [array]) { $dt = $dt[0] }
    }
    if ($st -and $dt -and ([IO.Path]::GetDirectoryName([string]$st) -ne [IO.Path]::GetDirectoryName([string]$dt))) { return $false }
    if ((Hash-BytesFile $serverPath) -ne $wantServer) { return $false }
    if ((Hash-BytesFile $dapPath) -ne $wantDap) { return $false }
    $current = Get-StandaloneCurrentObservation -InstallDir $script:InstallDir
    $pathv = Get-StandalonePathVisibleObservation -InstallDir $script:InstallDir
    if ($current -notlike "*server_sha256=$wantServer*") { return $false }
    if ($current -notlike "*dap_sha256=$wantDap*") { return $false }
    if ($current -notlike "state=selected*") { return $false }
    if ($pathv -like "state=mixed*") { return $false }
    return $true
}

try {
    $env:PERL_LSP_INSTALLER_LIBRARY_ONLY = "1"
    . $Installer

    Write-Host "=== standalone product-unit promotion (#8359) ==="

    if (Select-String -Path $Installer -Pattern 'Copy-Item -Path $BinaryPath -Destination $DestPath' -SimpleMatch -Quiet) {
        Fail-Case "independent perllsp destination copy is gone" "install.ps1 still copies perllsp.exe before perl-dap.exe"
    } else {
        Pass-Case "independent perllsp destination copy is gone"
    }

    if (Select-String -Path $Installer -Pattern 'New-Item -ItemType SymbolicLink -Path $serverDest' -SimpleMatch -Quiet) {
        Fail-Case "PATH selectors use atomic replace" "install.ps1 still creates PATH names with a non-atomic New-Item"
    } else {
        Pass-Case "PATH selectors use atomic replace"
    }

    if (-not (Select-String -Path $Installer -Pattern 'ItemType Junction' -SimpleMatch -Quiet)) {
        Fail-Case "unelevated directory pointer fallback exists" "install.ps1 has no junction fallback for current"
    } else {
        Pass-Case "unelevated directory pointer fallback exists"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    $receiptOk = ($LastOutput -like "*product_unit_receipt*") -and ($LastOutput -like "*archive_pair_required*") -and ($LastOutput -notlike "*$InstallDir*")
    $dapPathOk = ($null -ne $LastResult) -and ([string]$LastResult.DapDestPath -eq (Join-Path $InstallDir "perl-dap.exe"))
    if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a") -and $receiptOk -and $dapPathOk) {
        Pass-Case "first archive pair publishes one current complete unit"
    } else {
        Fail-Case "first archive pair publishes one current complete unit" "status=$LastStatus output=$LastOutput dapDest=$dapPathOk"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    Invoke-Promote
    $prev = Join-Path $InstallDir ".perl-lsp\previous"
    if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b") -and (Test-Path -LiteralPath $prev)) {
        Pass-Case "upgrade retains previous complete unit and selects the new pair"
    } else {
        Fail-Case "upgrade retains previous complete unit and selects the new pair" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    if (($LastStatus -ne 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a") -and ($LastOutput -like "*before_commit*")) {
        Pass-Case "commit fault preserves the old complete pair"
    } else {
        Fail-Case "commit fault preserves the old complete pair" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_FAULT = "before_publish"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    $candRoot = Join-Path $InstallDir ".perl-lsp\candidates"
    $candCount = @(Get-ChildItem -LiteralPath $candRoot -Directory -ErrorAction SilentlyContinue).Count
    if (($LastStatus -ne 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a") -and ($candCount -eq 1)) {
        Pass-Case "publish fault does not select or leak a partial new pair"
    } else {
        Fail-Case "publish fault does not select or leak a partial new pair" "status=$LastStatus candidates=$candCount output=$LastOutput"
    }

    Setup-Root
    Stage-ServerOnly -Dest $ExtractDir -Server "server-only"
    Invoke-Promote
    $serverPath = Join-Path $InstallDir "perllsp.exe"
    $dapPath = Join-Path $InstallDir "perl-dap.exe"
    if (($LastStatus -ne 0) -and -not (Test-Path -LiteralPath $serverPath) -and -not (Test-Path -LiteralPath $dapPath) -and ($LastOutput -like "*complete perllsp/perl-dap pair*")) {
        Pass-Case "release mode rejects a missing DAP before current moves"
    } else {
        Fail-Case "release mode rejects a missing DAP before current moves" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-Pair -Dest $InstallDir -Server "legacy-server" -Dap "legacy-dap"
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    $expect = Join-Path $TempRoot "legacy-expect"
    New-Item -ItemType Directory -Path $expect -Force | Out-Null
    Write-Payload -Path (Join-Path $expect "perllsp.exe") -Payload "legacy-server"
    Write-Payload -Path (Join-Path $expect "perl-dap.exe") -Payload "legacy-dap"
    $okLegacy = ($LastStatus -ne 0) -and
        ((Hash-BytesFile (Join-Path $InstallDir "perllsp.exe")) -eq (Hash-BytesFile (Join-Path $expect "perllsp.exe"))) -and
        ((Hash-BytesFile (Join-Path $InstallDir "perl-dap.exe")) -eq (Hash-BytesFile (Join-Path $expect "perl-dap.exe")))
    if ($okLegacy) {
        Pass-Case "legacy regular files stay a complete pair when the new commit fails"
    } else {
        Fail-Case "legacy regular files stay a complete pair when the new commit fails" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-Pair -Dest $InstallDir -Server "legacy-server" -Dap "legacy-dap"
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    Invoke-Promote
    if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b")) {
        Pass-Case "legacy regular pair is imported then atomically replaced by the new pair"
    } else {
        Fail-Case "legacy regular pair is imported then atomically replaced by the new pair" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    Invoke-Promote -Mode source
    $current = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    $pathv = Get-StandalonePathVisibleObservation -InstallDir $InstallDir
    $expectServer = Join-Path $TempRoot "source-expect.exe"
    Write-Payload -Path $expectServer -Payload "source-server"
    $sourceOk = ($LastStatus -eq 0) -and
        (Test-Path -LiteralPath (Join-Path $InstallDir "perllsp.exe")) -and
        -not (Test-Path -LiteralPath (Join-Path $InstallDir "perl-dap.exe")) -and
        ((Hash-BytesFile (Join-Path $InstallDir "perllsp.exe")) -eq (Hash-BytesFile $expectServer)) -and
        ($current -like "*advanced_source_server_only*") -and
        ($current -like "*dap_sha256=-*") -and
        ($pathv -notlike "state=mixed*")
    if ($sourceOk) {
        Pass-Case "source mode publishes an explicit server-only unit, not a pair"
    } else {
        Fail-Case "source mode publishes an explicit server-only unit, not a pair" "status=$LastStatus current=$current path=$pathv output=$LastOutput"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "pair-server" -Dap "pair-dap"
    Invoke-Promote
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    Invoke-Promote -Mode source
    $current = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    if (($LastStatus -eq 0) -and
        ((Hash-BytesFile (Join-Path $InstallDir "perllsp.exe")) -eq (Hash-BytesFile $expectServer)) -and
        -not (Test-Path -LiteralPath (Join-Path $InstallDir "perl-dap.exe")) -and
        ($current -like "*advanced_source_server_only*")) {
        Pass-Case "source upgrade does not keep the previous DAP as current"
    } else {
        Fail-Case "source upgrade does not keep the previous DAP as current" "status=$LastStatus current=$current output=$LastOutput"
    }

    Setup-Root
    $env:PERL_LSP_INSTALL_POINTER = "unprivileged"
    try {
        Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
        Invoke-Promote
        $okUnpriv = ($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a")
        Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
        Invoke-Promote
        $okUnpriv = $okUnpriv -and ($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b")
        if ($okUnpriv) {
            Pass-Case "unelevated junction/hardlink promotion keeps complete pairs"
        } else {
            Fail-Case "unelevated junction/hardlink promotion keeps complete pairs" "status=$LastStatus output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_POINTER -ErrorAction SilentlyContinue
    }

    Setup-Root
    $env:PERL_LSP_INSTALL_POINTER = "copy"
    try {
        Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
        Invoke-Promote
        Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
        Invoke-Promote
        if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b")) {
            Pass-Case "copy fallback still publishes one complete pair"
        } else {
            Fail-Case "copy fallback still publishes one complete pair" "status=$LastStatus output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_POINTER -ErrorAction SilentlyContinue
    }
} finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "=== Results: $Pass passed, $Fail failed ==="
if ($Fail -ne 0) {
    exit 1
}
