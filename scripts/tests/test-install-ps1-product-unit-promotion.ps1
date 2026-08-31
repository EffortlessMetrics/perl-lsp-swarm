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
    $current = Get-StandaloneCurrentObservation -InstallDir $script:InstallDir
    $pathv = Get-StandalonePathVisibleObservation -InstallDir $script:InstallDir
    if ($current -notlike "*server_sha256=$wantServer*") { return $false }
    if ($current -notlike "*dap_sha256=$wantDap*") { return $false }
    if ($current -notlike "state=selected*") { return $false }
    if ($pathv -like "state=mixed*") { return $false }
    if ($pathv -notlike "*server_sha256=$wantServer*") { return $false }
    if ($pathv -notlike "*dap_sha256=$wantDap*") { return $false }
    if (-not (Test-Path -LiteralPath (Join-Path $script:InstallDir "perllsp.cmd"))) { return $false }
    if (-not (Test-Path -LiteralPath (Join-Path $script:InstallDir "perl-dap.cmd"))) { return $false }
    $dir = Get-StandaloneCurrentDir -InstallDir $script:InstallDir
    if (-not $dir) { return $false }
    if ((Hash-BytesFile (Join-Path $dir "perllsp.exe")) -ne $wantServer) { return $false }
    if ((Hash-BytesFile (Join-Path $dir "perl-dap.exe")) -ne $wantDap) { return $false }
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

    if (Select-String -Path $Installer -Pattern 'To.bak.$PID' -SimpleMatch -Quiet) {
        Fail-Case "pointer replace has no missing-name backup gap" "install.ps1 still moves current aside to a .bak name"
    } else {
        Pass-Case "pointer replace has no missing-name backup gap"
    }

    if (-not (Select-String -Path $Installer -Pattern 'Write-StandaloneCmdShim' -SimpleMatch -Quiet) -or -not (Select-String -Path $Installer -Pattern 'Write-StandalonePointerFile' -SimpleMatch -Quiet)) {
        Fail-Case "PATH names follow a single file pointer" "install.ps1 is missing the file pointer or cmd shim helpers"
    } else {
        Pass-Case "PATH names follow a single file pointer"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    $receiptOk = ($LastOutput -like "*product_unit_receipt*") -and ($LastOutput -like "*archive_pair_required*") -and ($LastOutput -notlike "*$InstallDir*")
    $dapPathOk = ($null -ne $LastResult) -and ([string]$LastResult.DapDestPath -eq (Join-Path $InstallDir "perl-dap.cmd"))
    if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a") -and $receiptOk -and $dapPathOk) {
        Pass-Case "first archive pair publishes one current complete unit"
    } else {
        Fail-Case "first archive pair publishes one current complete unit" "status=$LastStatus output=$LastOutput dapDest=$dapPathOk"
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-first" -Dap "dap-first"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    $firstServerCmd = Join-Path $InstallDir "perllsp.cmd"
    $firstDapCmd = Join-Path $InstallDir "perl-dap.cmd"
    if (($LastStatus -ne 0) -and -not (Test-Path -LiteralPath $firstServerCmd) -and
        -not (Test-Path -LiteralPath $firstDapCmd) -and ($LastOutput -like "*before_commit*")) {
        Pass-Case "first-install commit fault leaves no broken selectors"
    } else {
        Fail-Case "first-install commit fault leaves no broken selectors" "status=$LastStatus output=$LastOutput"
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
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote -Mode source
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    if (($LastStatus -ne 0) -and (Assert-CompletePair -Server "server-a" -Dap "dap-a") -and ($LastOutput -like "*before_commit*")) {
        Pass-Case "release-to-source commit fault preserves the paired selectors"
    } else {
        Fail-Case "release-to-source commit fault preserves the paired selectors" "status=$LastStatus output=$LastOutput"
    }

    Setup-Root
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    Invoke-Promote -Mode source
    $previousCurrent = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    $previousServer = Join-Path $TempRoot "prev-server"
    $currentDirBefore = Get-StandaloneCurrentDir -InstallDir $InstallDir
    Copy-Item -LiteralPath (Join-Path $currentDirBefore "perllsp.exe") -Destination $previousServer
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    $currentAfter = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    $dirAfter = Get-StandaloneCurrentDir -InstallDir $InstallDir
    $serverUnchanged = ($null -ne $dirAfter) -and
        ((Hash-BytesFile (Join-Path $dirAfter "perllsp.exe")) -eq (Hash-BytesFile $previousServer))
    if (($LastStatus -ne 0) -and ($currentAfter -eq $previousCurrent) -and $serverUnchanged -and
        ($LastOutput -like "*before_commit*") -and
        -not (Test-Path -LiteralPath (Join-Path $InstallDir "perl-dap.cmd"))) {
        Pass-Case "source-to-release commit fault preserves the source-only selection"
    } else {
        Fail-Case "source-to-release commit fault preserves the source-only selection" "status=$LastStatus before=$previousCurrent after=$currentAfter output=$LastOutput"
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
    $serverCmd = Join-Path $InstallDir "perllsp.cmd"
    $dapCmd = Join-Path $InstallDir "perl-dap.cmd"
    if (($LastStatus -ne 0) -and -not (Test-Path -LiteralPath $serverPath) -and -not (Test-Path -LiteralPath $dapPath) -and -not (Test-Path -LiteralPath $serverCmd) -and -not (Test-Path -LiteralPath $dapCmd) -and ($LastOutput -like "*complete perllsp/perl-dap pair*")) {
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
        ((Get-StandalonePathMemberSha256 -InstallDir $InstallDir -ExeName "perllsp.exe") -eq (Hash-BytesFile (Join-Path $expect "perllsp.exe"))) -and
        ((Get-StandalonePathMemberSha256 -InstallDir $InstallDir -ExeName "perl-dap.exe") -eq (Hash-BytesFile (Join-Path $expect "perl-dap.exe")))
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
    $dir = Get-StandaloneCurrentDir -InstallDir $InstallDir
    $sourceOk = ($LastStatus -eq 0) -and
        ($null -ne $dir) -and
        (Test-Path -LiteralPath (Join-Path $InstallDir "perllsp.cmd")) -and
        -not (Test-Path -LiteralPath (Join-Path $InstallDir "perl-dap.cmd")) -and
        ((Hash-BytesFile (Join-Path $dir "perllsp.exe")) -eq (Hash-BytesFile $expectServer)) -and
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
    $dir = Get-StandaloneCurrentDir -InstallDir $InstallDir
    if (($LastStatus -eq 0) -and
        ($null -ne $dir) -and
        ((Hash-BytesFile (Join-Path $dir "perllsp.exe")) -eq (Hash-BytesFile $expectServer)) -and
        -not (Test-Path -LiteralPath (Join-Path $InstallDir "perl-dap.cmd")) -and
        ($current -like "*advanced_source_server_only*")) {
        Pass-Case "source upgrade does not keep the previous DAP as current"
    } else {
        Fail-Case "source upgrade does not keep the previous DAP as current" "status=$LastStatus current=$current output=$LastOutput"
    }

    Setup-Root
    $obs = Join-Path $TempRoot "observe.txt"
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
    Invoke-Promote
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_OBSERVE = "between_path_members"
    $env:PERL_LSP_INSTALL_OBSERVE_FILE = $obs
    try {
        Invoke-Promote
        $obsText = ""
        if (Test-Path -LiteralPath $obs) { $obsText = Get-Content -LiteralPath $obs -Raw }
        $okObs = ($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b") -and
            ($obsText -like "*state=selected*") -and ($obsText -like "*state=path_visible*") -and
            ($obsText -notlike "*state=mixed*") -and ($obsText -notlike "*state=none*") -and
            ($obsText -like "*server_sha256=*") -and ($obsText -like "*dap_sha256=*") -and
            ($obsText -notlike "*server_sha256=-*") -and ($obsText -notlike "*dap_sha256=-*")
        if ($okObs) {
            Pass-Case "interleaved PATH reader uses one file pointer and never sees a mixed pair"
        } else {
            Fail-Case "interleaved PATH reader never sees mixed members or a missing current" "status=$LastStatus obs=$obsText output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE -ErrorAction SilentlyContinue
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE_FILE -ErrorAction SilentlyContinue
    }

    Setup-Root
    try {
        Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
        Invoke-Promote
        Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
        Invoke-Promote
        $currentFile = Join-Path $InstallDir ".perl-lsp\current"
        $isFilePointer = (Test-Path -LiteralPath $currentFile) -and -not (Get-Item -LiteralPath $currentFile).PSIsContainer
        if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b") -and $isFilePointer) {
            Pass-Case "file pointer plus cmd shims keep complete pairs"
        } else {
            Fail-Case "unprivileged file pointer plus cmd shims keep complete pairs" "status=$LastStatus output=$LastOutput filePointer=$isFilePointer"
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
