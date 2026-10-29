$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Discriminating product-unit promotion proof for install.ps1 (#8359) and
# selector-commit atomicity proof for the source-only <-> release transitions
# described by #14052. PATH-visible names and .perl-lsp/current must observe
# one complete unit.

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

# Returns $true when the cmd shim is present but its current pointer
# resolves to a candidate directory that does not contain the expected
# perllsp.exe / perl-dap.exe. Get-StandalonePathVisibleObservation reports
# the same `-` hash for absent and dangling selectors; tests that need to
# discriminate use this helper instead.
function Is-DanglingShim {
    param([string]$ShimPath, [string]$ExeName)
    if (-not (Test-Path -LiteralPath $ShimPath)) { return $false }
    $dir = Get-StandaloneCurrentDir -InstallDir $script:InstallDir
    if (-not $dir) { return $false }
    $exe = Join-Path $dir $ExeName
    return -not (Test-Path -LiteralPath $exe)
}

# Stages a complete pair with executable batch content on both members so the
# PATH-visible shim or symlink can be proven to actually launch the right
# executable body.
function Stage-ExecutablePair {
    param([string]$Dest, [string]$Server, [string]$Dap)
    if (Test-Path -LiteralPath $Dest) {
        Remove-Item -LiteralPath $Dest -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Dest -Force | Out-Null
    $serverExe = Join-Path $Dest "perllsp.exe"
    $dapExe = Join-Path $Dest "perl-dap.exe"
    [IO.File]::WriteAllBytes($serverExe, [Text.Encoding]::ASCII.GetBytes("@echo off`r`necho $Server`r`n"))
    [IO.File]::WriteAllBytes($dapExe, [Text.Encoding]::ASCII.GetBytes("@echo off`r`necho $Dap`r`n"))
}

# Verifies a published pair advertises working selectors on Windows: the cmd
# shim exists, the current pointer resolves to a candidate that carries the
# expected perllsp.exe / perl-dap.exe, and the staged bodies are non-empty so
# the cmd shim would have something to launch.
function Assert-ExecutablePair {
    param([string]$Server, [string]$Dap)
    $serverCmd = Join-Path $script:InstallDir "perllsp.cmd"
    $dapCmd = Join-Path $script:InstallDir "perl-dap.cmd"
    if (-not (Test-Path -LiteralPath $serverCmd)) { return $false }
    if (-not (Test-Path -LiteralPath $dapCmd)) { return $false }
    if (Is-DanglingShim -ShimPath $serverCmd -ExeName "perllsp.exe") { return $false }
    if (Is-DanglingShim -ShimPath $dapCmd -ExeName "perl-dap.exe") { return $false }
    $dir = Get-StandaloneCurrentDir -InstallDir $script:InstallDir
    if (-not $dir) { return $false }
    $serverExe = Join-Path $dir "perllsp.exe"
    $dapExe = Join-Path $dir "perl-dap.exe"
    if (-not (Test-Path -LiteralPath $serverExe)) { return $false }
    if (-not (Test-Path -LiteralPath $dapExe)) { return $false }
    $serverBytes = (Get-Item -LiteralPath $serverExe).Length
    $dapBytes = (Get-Item -LiteralPath $dapExe).Length
    if ($serverBytes -le 0) { return $false }
    if ($dapBytes -le 0) { return $false }
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
            Fail-Case "interleaved PATH reader uses one file pointer and never sees a mixed pair" "status=$LastStatus obs=$obsText output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE -ErrorAction SilentlyContinue
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE_FILE -ErrorAction SilentlyContinue
    }

    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-a" -Dap "dap-a"
        Invoke-Promote
        Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
        Invoke-Promote
        $currentFile = Join-Path $InstallDir ".perl-lsp\current"
        $isFilePointer = (Test-Path -LiteralPath $currentFile) -and -not (Get-Item -LiteralPath $currentFile).PSIsContainer
        if (($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b") -and $isFilePointer) {
            Pass-Case "file pointer plus cmd shims keep complete pairs"
        } else {
            Fail-Case "file pointer plus cmd shims keep complete pairs" "status=$LastStatus output=$LastOutput filePointer=$isFilePointer"
        }

    Setup-Root
    $obsFirst = Join-Path $TempRoot "observe-first.txt"
    Stage-Pair -Dest $ExtractDir -Server "server-first" -Dap "dap-first"
    $env:PERL_LSP_INSTALL_OBSERVE = "between_path_members"
    $env:PERL_LSP_INSTALL_OBSERVE_FILE = $obsFirst
    try {
        Invoke-Promote
        $obsText = ""
        if (Test-Path -LiteralPath $obsFirst) { $obsText = Get-Content -LiteralPath $obsFirst -Raw }
        $okFirst = ($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-first" -Dap "dap-first") -and
            ($obsText -like "*state=none*") -and ($obsText -notlike "*state=selected*") -and
            ($obsText -notlike "*state=mixed*")
        if ($okFirst) {
            Pass-Case "first-install pre-commit observe is kept and is not mixed"
        } else {
            Fail-Case "first-install pre-commit observe is kept and is not mixed" "status=$LastStatus obs=$obsText output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE -ErrorAction SilentlyContinue
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE_FILE -ErrorAction SilentlyContinue
    }

    Setup-Root
    $obsSource = Join-Path $TempRoot "observe-source-to-release.txt"
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    Invoke-Promote -Mode source
    Stage-Pair -Dest $ExtractDir -Server "server-b" -Dap "dap-b"
    $env:PERL_LSP_INSTALL_OBSERVE = "between_path_members"
    $env:PERL_LSP_INSTALL_OBSERVE_FILE = $obsSource
    try {
        Invoke-Promote
        $obsText = ""
        if (Test-Path -LiteralPath $obsSource) { $obsText = Get-Content -LiteralPath $obsSource -Raw }
        $okSource = ($LastStatus -eq 0) -and (Assert-CompletePair -Server "server-b" -Dap "dap-b") -and
            ($obsText -like "*state=selected*") -and
            ($obsText -notlike "*state=mixed*") -and ($obsText -notlike "*state=none*")
        if ($okSource) {
            Pass-Case "source-to-release pre-commit observe is kept and is not mixed"
        } else {
            Fail-Case "source-to-release pre-commit observe is kept and is not mixed" "status=$LastStatus obs=$obsText output=$LastOutput"
        }
    } finally {
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE -ErrorAction SilentlyContinue
        Remove-Item Env:PERL_LSP_INSTALL_OBSERVE_FILE -ErrorAction SilentlyContinue
    }

    # --- #14052: source-only <-> release transition atomicity ------------------
    # The selectors are created before the current-pointer commits so both
    # names become visible together, but a failed commit must roll back the
    # DAP name on the source-only -> release path so a non-pair current never
    # coexists with a dangling PATH-visible adapter. The basic transition
    # rollback is already covered by "source-to-release commit fault preserves
    # the source-only selection" above; the cases below add the executability
    # and pointer-mode coverage #14052 calls out.

    # First-install commit fault must leave no newly advertised selector that
    # cannot execute (#14052 acceptance).
    Setup-Root
    Stage-Pair -Dest $ExtractDir -Server "server-first" -Dap "dap-first"
    $env:PERL_LSP_INSTALL_FAULT = "before_commit"
    Invoke-Promote
    Remove-Item Env:PERL_LSP_INSTALL_FAULT -ErrorAction SilentlyContinue
    $firstServerCmd = Join-Path $script:InstallDir "perllsp.cmd"
    $firstDapCmd = Join-Path $script:InstallDir "perl-dap.cmd"
    if (($LastStatus -ne 0) -and
        (-not (Test-Path -LiteralPath $firstServerCmd)) -and
        (-not (Test-Path -LiteralPath $firstDapCmd)) -and
        ($LastOutput -like "*before_commit*")) {
        Pass-Case "first-install commit fault leaves no newly advertised selector that cannot execute"
    } else {
        Fail-Case "first-install commit fault leaves no newly advertised selector that cannot execute" "status=$LastStatus output=$LastOutput server=$([bool](Test-Path -LiteralPath $firstServerCmd)) dap=$([bool](Test-Path -LiteralPath $firstDapCmd))"
    }

    # A successful source-only -> release must publish a complete executable
    # pair without a mixed, dangling, or missing-member observation
    # (#14052 acceptance).
    Setup-Root
    Stage-ServerOnly -Dest $ExtractDir -Server "source-server"
    Invoke-Promote -Mode source
    Stage-ExecutablePair -Dest $ExtractDir -Server "exec-server-b" -Dap "exec-dap-b"
    Invoke-Promote
    $exeCurrent = Get-StandaloneCurrentObservation -InstallDir $script:InstallDir
    $exePathv = Get-StandalonePathVisibleObservation -InstallDir $script:InstallDir
    if (($LastStatus -eq 0) -and
        (Assert-ExecutablePair -Server "exec-server-b" -Dap "exec-dap-b") -and
        ($exeCurrent -like "*disposition=archive_pair_required*") -and
        ($exePathv -notlike "state=mixed*") -and
        ($exePathv -notlike "state=none*")) {
        Pass-Case "source-only-to-release success publishes a complete executable pair"
    } else {
        Fail-Case "source-only-to-release success publishes a complete executable pair" "status=$LastStatus output=$LastOutput current=$exeCurrent pathv=$exePathv"
    }

    # A first-install pair must publish a complete executable pair
    # (#14052 acceptance: no newly advertised selector can be unexecutable).
    Setup-Root
    Stage-ExecutablePair -Dest $ExtractDir -Server "exec-server-fresh" -Dap "exec-dap-fresh"
    Invoke-Promote
    if (($LastStatus -eq 0) -and
        (Assert-ExecutablePair -Server "exec-server-fresh" -Dap "exec-dap-fresh")) {
        Pass-Case "first-install release publishes a complete executable pair"
    } else {
        Fail-Case "first-install release publishes a complete executable pair" "status=$LastStatus output=$LastOutput current=$(Get-StandaloneCurrentObservation -InstallDir $script:InstallDir)"
    }

    # The accepted test surface must not advertise a PERL_LSP_INSTALL_POINTER
    # switch; the installer no longer reads that variable. A non-deliberate
    # test must not reintroduce it (#14052 acceptance). Scoped to the
    # installer surface so the scan stays bounded.
    $installerHits = @()
    foreach ($f in @("scripts/install.sh", "install.sh", "install.ps1")) {
        $path = Join-Path $Root $f
        if (-not (Test-Path -LiteralPath $path)) { continue }
        $matches = Select-String -Path $path -Pattern 'PERL_LSP_INSTALL_POINTER' -SimpleMatch
        if ($matches) { $installerHits += $matches }
    }
    if ($installerHits.Count -eq 0) {
        Pass-Case "no installer reads PERL_LSP_INSTALL_POINTER"
    } else {
        Fail-Case "no installer reads PERL_LSP_INSTALL_POINTER" ($installerHits | Out-String)
    }
} finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "=== Results: $Pass passed, $Fail failed ==="
if ($Fail -ne 0) {
    exit 1
}
