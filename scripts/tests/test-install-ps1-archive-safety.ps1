$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Installer = Join-Path $Root "install.ps1"
$Policy = Join-Path $Root "policy/standalone-archive-safety.v1.toml"
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("perl-lsp-archive-safety-" + [guid]::NewGuid().ToString("N"))
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

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

function New-ZipFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Entries
    )

    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Force
    }
    $Zip = [System.IO.Compression.ZipFile]::Open($Path, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($EntrySpec in $Entries) {
            $Entry = $Zip.CreateEntry($EntrySpec.Name)
            if ($EntrySpec.ContainsKey("UnixMode")) {
                $Entry.ExternalAttributes = ([int]$EntrySpec.UnixMode -shl 16)
            }
            $Bytes = $EntrySpec.Bytes
            if ($null -ne $Bytes) {
                $Stream = $Entry.Open()
                try {
                    $Stream.Write($Bytes, 0, $Bytes.Length)
                } finally {
                    $Stream.Dispose()
                }
            }
        }
    } finally {
        $Zip.Dispose()
    }
}

$RequiredFlat = @(
    @{ Name = "perllsp.exe"; Bytes = [Text.Encoding]::ASCII.GetBytes("win-server`n"); UnixMode = 0x81ED },
    @{ Name = "perl-dap.exe"; Bytes = [Text.Encoding]::ASCII.GetBytes("win-dap`n"); UnixMode = 0x81ED },
    @{ Name = "README.md"; Bytes = [Text.Encoding]::ASCII.GetBytes("readme`n"); UnixMode = 0x81A4 },
    @{ Name = "LICENSE-APACHE"; Bytes = [Text.Encoding]::ASCII.GetBytes("apache`n"); UnixMode = 0x81A4 },
    @{ Name = "LICENSE-MIT"; Bytes = [Text.Encoding]::ASCII.GetBytes("mit`n"); UnixMode = 0x81A4 },
    @{ Name = "SHA256SUMS.txt"; Bytes = [Text.Encoding]::ASCII.GetBytes("sums`n"); UnixMode = 0x81A4 }
)

$env:PERL_LSP_INSTALLER_LIBRARY_ONLY = "1"
. $Installer

$PolicyId = Get-StandaloneArchiveSafetyPolicyId
$TomlId = (Select-String -Path $Policy -Pattern '^policy_id\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
if ($PolicyId -eq $TomlId) {
    Pass-Case "embedded policy id matches policy/standalone-archive-safety.v1.toml"
} else {
    Fail-Case "embedded policy id matches policy/standalone-archive-safety.v1.toml" "adapter=$PolicyId toml=$TomlId"
}

if (Select-String -Path $Installer -Pattern 'Expand-Archive -Path $ZipPath' -SimpleMatch -Quiet) {
    Fail-Case "PowerShell Expand-Archive is not the extract path" "install.ps1 still calls Expand-Archive"
} else {
    Pass-Case "PowerShell Expand-Archive is not the extract path"
}

function Invoke-StagingCase {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)]$Entries,
        [string]$Needle,
        [switch]$ExpectSuccess,
        [string]$PackageName = "perllsp-0.18.0-x86_64-pc-windows-msvc"
    )

    $CaseRoot = Join-Path $TempRoot $Name
    New-Item -ItemType Directory -Path $CaseRoot -Force | Out-Null
    $Sentinel = Join-Path $CaseRoot "sentinel"
    Set-Content -LiteralPath $Sentinel -Encoding ascii -Value "untouched"
    $InstallDir = Join-Path $CaseRoot "install"
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $ZipPath = Join-Path $CaseRoot "archive.zip"
    New-ZipFixture -Path $ZipPath -Entries $Entries

    $Status = 0
    $Output = ""
    $ExtractedDir = $null
    try {
        $ExtractedDir = Invoke-StandaloneArchiveStaging -ArchivePath $ZipPath -StagingParent $CaseRoot -PackageName $PackageName
    } catch {
        $Status = 1
        $Output = [string]$_
    }

    $SentinelOk = ((Get-Content -LiteralPath $Sentinel -Raw).Trim() -eq "untouched")
    $InstallUntouched = -not (Test-Path -LiteralPath (Join-Path $InstallDir "perllsp.exe"))

    if ($ExpectSuccess) {
        $ServerOk = $false
        if ($ExtractedDir) {
            $ServerOk = Test-Path -LiteralPath (Join-Path $ExtractedDir "perllsp.exe")
        }
        if ($Status -eq 0 -and $ServerOk -and $SentinelOk -and $InstallUntouched) {
            Pass-Case $Name
        } else {
            Fail-Case $Name "status=$Status extracted=$ExtractedDir sentinelOk=$SentinelOk"
        }
        return
    }

    $StagingLeft = @(Get-ChildItem -LiteralPath $CaseRoot -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "perl-lsp-stage-*" })
    if ($Status -ne 0 -and $Output -like "*$Needle*" -and $SentinelOk -and $InstallUntouched -and $StagingLeft.Count -eq 0) {
        Pass-Case "$Name fails closed before destination writes"
    } else {
        Fail-Case "$Name fails closed before destination writes" "status=$Status output=$Output leftover=$($StagingLeft.Count)"
    }
}

try {
    Invoke-StagingCase -Name "valid_windows_flat" -Entries $RequiredFlat -ExpectSuccess

    $Nested = foreach ($Item in $RequiredFlat) {
        @{ Name = "perllsp-0.18.0-x86_64-pc-windows-msvc/$($Item.Name)"; Bytes = $Item.Bytes; UnixMode = $Item.UnixMode }
    }
    Invoke-StagingCase -Name "valid_windows_nested" -Entries $Nested -ExpectSuccess

    Invoke-StagingCase -Name "windows_traversal" -Entries ($RequiredFlat + @{ Name = "../sentinel_pwned"; Bytes = [Text.Encoding]::ASCII.GetBytes("escaped`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_absolute" -Entries ($RequiredFlat + @{ Name = "/tmp/sentinel_pwned"; Bytes = [Text.Encoding]::ASCII.GetBytes("escaped`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_drive" -Entries ($RequiredFlat + @{ Name = "C:/Windows/sentinel_pwned"; Bytes = [Text.Encoding]::ASCII.GetBytes("escaped`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_backslash" -Entries ($RequiredFlat + @{ Name = "extra\file.txt"; Bytes = [Text.Encoding]::ASCII.GetBytes("alias`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_duplicate" -Entries ($RequiredFlat + @{ Name = "README.md"; Bytes = [Text.Encoding]::ASCII.GetBytes("second`n") }) -Needle "duplicate archive member"
    Invoke-StagingCase -Name "windows_case_collision" -Entries ($RequiredFlat + @{ Name = "Readme.md"; Bytes = [Text.Encoding]::ASCII.GetBytes("case`n") }) -Needle "case-fold collision"
    Invoke-StagingCase -Name "windows_missing_dap" -Entries ($RequiredFlat | Where-Object { $_.Name -ne "perl-dap.exe" }) -Needle "missing required member"
    Invoke-StagingCase -Name "windows_extra_executable" -Entries ($RequiredFlat + @{ Name = "helper.bat"; Bytes = [Text.Encoding]::ASCII.GetBytes("echo hi`n") }) -Needle "unexpected executable"
    Invoke-StagingCase -Name "windows_reserved" -Entries ($RequiredFlat + @{ Name = "CON.txt"; Bytes = [Text.Encoding]::ASCII.GetBytes("reserved`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_trailing_dot" -Entries ($RequiredFlat + @{ Name = "README.md."; Bytes = [Text.Encoding]::ASCII.GetBytes("trailing`n") }) -Needle "unsafe archive member"
    Invoke-StagingCase -Name "windows_symlink" -Entries ($RequiredFlat + @{ Name = "link"; Bytes = [Text.Encoding]::ASCII.GetBytes("perllsp.exe"); UnixMode = 0xA1FF }) -Needle "archive links are not accepted"

    $env:PERL_LSP_ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES = "16"
    Invoke-StagingCase -Name "windows_compressed_ceiling" -Entries $RequiredFlat -Needle "compressed size"
    Remove-Item Env:PERL_LSP_ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES

    $env:PERL_LSP_ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES = "32"
    Invoke-StagingCase -Name "windows_uncompressed_ceiling" -Entries $RequiredFlat -Needle "uncompressed size"
    Remove-Item Env:PERL_LSP_ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES

    $OversizedFlat = foreach ($Item in $RequiredFlat) {
        if ($Item.Name -eq "README.md") {
            @{ Name = $Item.Name; Bytes = [byte[]]::new(64); UnixMode = $Item.UnixMode }
        } else {
            $Item
        }
    }
    $env:PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES = "32"
    Invoke-StagingCase -Name "windows_oversized_entry" -Entries $OversizedFlat -Needle "entry size"
    Remove-Item Env:PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES

    $Garbage = Join-Path $TempRoot "garbage.zip"
    Set-Content -LiteralPath $Garbage -Encoding ascii -Value "not-a-zip`n"
    $CaseRoot = Join-Path $TempRoot "malformed"
    New-Item -ItemType Directory -Path $CaseRoot -Force | Out-Null
    $Sentinel = Join-Path $CaseRoot "sentinel"
    Set-Content -LiteralPath $Sentinel -Encoding ascii -Value "untouched"
    try {
        Invoke-StandaloneArchiveStaging -ArchivePath $Garbage -StagingParent $CaseRoot -PackageName "perllsp-0.18.0-x86_64-pc-windows-msvc" | Out-Null
        Fail-Case "malformed archive fails closed" "staging succeeded"
    } catch {
        if ((Get-Content -LiteralPath $Sentinel -Raw).Trim() -eq "untouched") {
            Pass-Case "malformed archive fails closed"
        } else {
            Fail-Case "malformed archive fails closed" "sentinel changed"
        }
    }
} finally {
    Remove-Item Env:PERL_LSP_INSTALLER_LIBRARY_ONLY -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "=== Results: $Pass passed, $Fail failed ==="
if ($Fail -ne 0) {
    exit 1
}
exit 0
