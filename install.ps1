# Perl LSP installer for Windows
#
# The piped one-liner is not usable yet. The copy published at
# perl-lsp/master still derives a `perl-lsp-<version>-...zip` asset name while
# releases ship `perllsp-<version>-...zip`, so piping that URL into iex 404s
# (#5461). This file already carries the fix; promoting it to the publication
# repo is #4348.
#
# Until that lands, run it from a clone or a downloaded copy:
#   .\install.ps1                                    # latest, default dir
#   .\install.ps1 -Version 0.17.0 -InstallDir C:\tools\bin

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:USERPROFILE\.local\bin",
    [switch]$NoModifyPath
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = "EffortlessMetrics/perl-lsp"
# The release workflow packages the binary as `perllsp` on every platform
# (see .github/workflows/release.yml — NAME="perllsp"), and every editor doc
# / README / POSIX installer (scripts/install.sh) uses `perllsp`. Install the
# Windows candidate stores `perllsp.exe`; PATH-visible names are `perllsp.cmd`
# / `perl-dap.cmd` shims that follow `.perl-lsp\current` so unelevated hosts
# never publish independent copies.
$Name = "perllsp"
# The release archive also carries the debug adapter (`perl-dap.exe`) — see
# .github/workflows/release.yml, which builds `-p perl-dap` for every target.
# Install it alongside the server so Windows matches every sibling channel:
# scripts/install.sh (optional perl-dap copy), Formula/perllsp.rb,
# distribution/scoop/perl-lsp.json, distribution/winget/perl-lsp.yaml.
$DapName = "perl-dap"

function Write-Info {
    param([string]$Message)
    Write-Host "→ " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Error {
    param([string]$Message)
    Write-Host "Error: " -ForegroundColor Red -NoNewline
    Write-Host $Message
    exit 1
}

function Write-Warn {
    param([string]$Message)
    Write-Host "⚠ " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Write-Success {
    param([string]$Message)
    Write-Host "✓ " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

# Normalize one PATH entry for exact, case-insensitive comparison. Expand
# environment variables before comparing so `%USERPROFILE%\.local\bin` and the
# already-expanded install directory are treated as the same entry.
function Normalize-PathEntry {
    param([string]$PathEntry)

    if ([string]::IsNullOrWhiteSpace($PathEntry)) {
        return $null
    }

    $Expanded = [Environment]::ExpandEnvironmentVariables($PathEntry.Trim())
    if ([IO.Path]::IsPathRooted($Expanded)) {
        try {
            $Expanded = [IO.Path]::GetFullPath($Expanded)
        } catch {
            # Keep the expanded value when the entry is intentionally not a normal
            # filesystem path; it still participates in exact string comparison.
        }
    }

    return $Expanded.TrimEnd([char[]]"\/")
}

function Test-PathContainsEntry {
    param(
        [string]$PathValue,
        [string]$Entry
    )

    $Expected = Normalize-PathEntry $Entry
    if (-not $Expected) {
        return $false
    }

    foreach ($Candidate in @($PathValue -split ';')) {
        $Normalized = Normalize-PathEntry $Candidate
        if ($Normalized -and [string]::Equals($Normalized, $Expected, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }

    return $false
}

function Add-InstallDirToCurrentProcessPath {
    if (Test-PathContainsEntry -PathValue $env:Path -Entry $InstallDir) {
        return
    }

    if ([string]::IsNullOrWhiteSpace($env:Path)) {
        $env:Path = $InstallDir
    } else {
        $env:Path = "$env:Path;$InstallDir"
    }
}

function Ensure-InstallDirOnUserPath {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (Test-PathContainsEntry -PathValue $UserPath -Entry $InstallDir) {
        return $false
    }

    # Persist only the existing *user* PATH plus the install directory. Do not
    # copy `$env:Path`: that value is the merged process/system/user PATH and
    # writing it back to the user scope duplicates system entries permanently.
    $NewUserPath = if ([string]::IsNullOrWhiteSpace($UserPath)) {
        $InstallDir
    } else {
        "$($UserPath.TrimEnd(';'));$InstallDir"
    }
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
    return $true
}

function Write-ManualPathGuidance {
    Write-Host "Add this directory to your user PATH before starting a new editor/Claude process: $InstallDir" -ForegroundColor Cyan
}

# Detect architecture.
#
# The release matrix builds both x86_64-pc-windows-msvc and, since #5208, a
# native aarch64-pc-windows-msvc on the windows-11-arm runner. Prefer the
# native ARM64 asset and fall back to the x64 build under emulation only when
# a given release does not carry it.
#
# The fallback must stay: the ARM64 target was added on 2026-08-03 and no
# release has been cut since, so that asset has never actually been produced.
# Nothing here may *require* it. Releases predating #5208 legitimately have
# only the x64 archive, and installing from an older tag must keep working.
#
# The Windows 11 build-22000 floor applies ONLY to the emulation fallback --
# it exists because x64 emulation needs Windows 11, not because of anything
# about ARM64 itself. A native ARM64 binary runs fine on Windows 10 ARM64, so
# gating the native path on it would refuse an install that works (#6196).
#
# A 32-bit PowerShell host on 64-bit Windows reports "x86" in
# PROCESSOR_ARCHITECTURE and the real architecture in PROCESSOR_ARCHITEW6432,
# so consult the latter first.
$HostArch = if ($env:PROCESSOR_ARCHITEW6432) {
    $env:PROCESSOR_ARCHITEW6432
} else {
    $env:PROCESSOR_ARCHITECTURE
}

$IsArm64Host = $HostArch -eq "ARM64"

function Get-ExpectedAssetHash {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ChecksumPath,
        [Parameter(Mandatory = $true)]
        [string]$Asset
    )

    $HashPattern = [regex]'^[0-9a-f]{64}$'
    $Rows = @(
        Get-Content -LiteralPath $ChecksumPath | ForEach-Object {
            $Parts = $_ -split '\s+', 2
            if ($Parts.Count -eq 2 -and $Parts[0] -and $Parts[1]) {
                [pscustomobject]@{ Hash = $Parts[0]; Name = $Parts[1].Trim().TrimStart('*') }
            }
        } | Where-Object { $_.Name -ceq $Asset }
    )

    if ($Rows.Count -eq 0) {
        throw "SHA256SUMS contains no exact entry for $Asset"
    }
    if ($Rows.Count -ne 1) {
        throw "SHA256SUMS contains duplicate entries for $Asset"
    }
    if (-not $HashPattern.IsMatch($Rows[0].Hash)) {
        throw "SHA256SUMS entry for $Asset is not an exact lowercase SHA-256 hash"
    }
    return $Rows[0].Hash
}

function Get-WindowsBuildNumber {
    try {
        $build = [int](Get-ItemPropertyValue -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -Name "CurrentBuildNumber")
        if ($build -ge 0) {
            return $build
        }
    } catch {
        # Fall back for restricted registry access or older PowerShell hosts.
    }

    try {
        return [int][System.Environment]::OSVersion.Version.Build
    } catch {
        return -1
    }
}

$script:ArchiveSafetyPolicyId = "standalone-archive-safety.v1"
$script:ArchiveSafetyMaxCompressedBytes = 268435456
$script:ArchiveSafetyMaxUncompressedBytes = 536870912
$script:ArchiveSafetyMaxEntryBytes = 268435456
$script:ArchiveSafetyMaxEntries = 32
$script:ArchiveSafetyMaxPathBytes = 255
$script:ArchiveSafetyMaxPathDepth = 3
$script:ArchiveSafetyRequiredWindows = @(
    "perllsp.exe",
    "perl-dap.exe",
    "README.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "SHA256SUMS.txt"
)
$script:ArchiveSafetyAllowedExecutables = @("perllsp.exe", "perl-dap.exe")

function Get-StandaloneArchiveSafetyPolicyId {
    return $script:ArchiveSafetyPolicyId
}

function Get-ArchiveSafetyLimit {
    param(
        [Parameter(Mandatory = $true)][ValidateSet("compressed", "uncompressed", "entry", "entries")][string]$Name
    )

    $override = $null
    switch ($Name) {
        "compressed" {
            $override = $env:PERL_LSP_ARCHIVE_SAFETY_MAX_COMPRESSED_BYTES
            if ($override) { return [int64]$override }
            return [int64]$script:ArchiveSafetyMaxCompressedBytes
        }
        "uncompressed" {
            $override = $env:PERL_LSP_ARCHIVE_SAFETY_MAX_UNCOMPRESSED_BYTES
            if ($override) { return [int64]$override }
            return [int64]$script:ArchiveSafetyMaxUncompressedBytes
        }
        "entry" {
            $override = $env:PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRY_BYTES
            if ($override) { return [int64]$override }
            return [int64]$script:ArchiveSafetyMaxEntryBytes
        }
        "entries" {
            $override = $env:PERL_LSP_ARCHIVE_SAFETY_MAX_ENTRIES
            if ($override) { return [int]$override }
            return [int]$script:ArchiveSafetyMaxEntries
        }
    }
}

function ConvertTo-SafeArchiveMemberPath {
    param([Parameter(Mandatory = $true)][string]$Name)

    if ($Name -match "[\r\n\t]") {
        throw "unsafe archive member path: $Name"
    }
    if ($Name.Contains("\") -or $Name.Contains(":") -or $Name.StartsWith("/") -or $Name.StartsWith("//")) {
        throw "unsafe archive member path: $Name"
    }
    if ($Name -match '^[A-Za-z]:') {
        throw "unsafe archive member path: $Name"
    }
    if ($Name.Length -gt $script:ArchiveSafetyMaxPathBytes) {
        throw "unsafe archive member path: $Name"
    }

    $inspect = $Name.TrimEnd("/")
    if ([string]::IsNullOrEmpty($inspect)) {
        throw "unsafe archive member path: $Name"
    }

    $parts = $inspect.Split("/")
    if ($parts.Count -gt $script:ArchiveSafetyMaxPathDepth) {
        throw "unsafe archive member path: $Name"
    }
    foreach ($part in $parts) {
        if ($part -in @("", ".", "..")) {
            throw "unsafe archive member path: $Name"
        }
        if ($part -notmatch '^[A-Za-z0-9._-]+$') {
            throw "unsafe archive member path: $Name"
        }
        if ($part.EndsWith(".") -or $part.EndsWith(" ")) {
            throw "unsafe archive member path: $Name"
        }
        $folded = $part.ToLowerInvariant()
        if ($folded -match '^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$') {
            throw "unsafe archive member path: $Name"
        }
    }
    return $inspect
}

function Test-ZipEntryIsSymlink {
    param($Entry)
    $mode = ($Entry.ExternalAttributes -shr 16) -band 0xFFFF
    return (($mode -band 0xF000) -eq 0xA000)
}

function Test-ZipEntryIsExecutable {
    param(
        [Parameter(Mandatory = $true)][string]$Basename,
        $Entry
    )
    if ($Basename.ToLowerInvariant() -match '\.(exe|bat|cmd)$') {
        return $true
    }
    $mode = ($Entry.ExternalAttributes -shr 16) -band 0xFFFF
    # Unix exec bits (owner/group/other), decimal 73 == 0o111.
    return (($mode -band 73) -ne 0)
}

function Get-StagedMemberSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Invoke-StandaloneArchiveStaging {
    param(
        [Parameter(Mandatory = $true)][string]$ArchivePath,
        [Parameter(Mandatory = $true)][string]$StagingParent,
        [Parameter(Mandatory = $true)][string]$PackageName
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    if (-not (Test-Path -LiteralPath $ArchivePath)) {
        throw "verified archive is missing"
    }

    $maxCompressed = Get-ArchiveSafetyLimit -Name compressed
    $maxUncompressed = Get-ArchiveSafetyLimit -Name uncompressed
    $maxEntry = Get-ArchiveSafetyLimit -Name entry
    $maxEntries = Get-ArchiveSafetyLimit -Name entries
    $compressed = (Get-Item -LiteralPath $ArchivePath).Length
    if ($compressed -gt $maxCompressed) {
        throw "archive compressed size $compressed exceeds policy ceiling $maxCompressed"
    }

    $stagingRoot = Join-Path $StagingParent ("perl-lsp-stage-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $stagingRoot -Force | Out-Null

    try {
        $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
        try {
            if ($archive.Entries.Count -gt $maxEntries) {
                throw "archive entry count $($archive.Entries.Count) exceeds policy ceiling $maxEntries"
            }

            $seenExact = New-Object 'System.Collections.Generic.HashSet[string]'
            $seenFolded = New-Object 'System.Collections.Generic.HashSet[string]'
            $accepted = New-Object System.Collections.Generic.List[object]
            $basenames = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)

            foreach ($entry in $archive.Entries) {
                $raw = $entry.FullName
                if ($raw.EndsWith("/")) {
                    $normalized = ConvertTo-SafeArchiveMemberPath -Name $raw
                    if ($normalized -ne $PackageName) {
                        throw "unexpected directory member: $normalized"
                    }
                    continue
                }

                $normalized = ConvertTo-SafeArchiveMemberPath -Name $raw
                if (-not $seenExact.Add($normalized)) {
                    throw "duplicate archive member: $normalized"
                }
                $folded = $normalized.ToLowerInvariant()
                if (-not $seenFolded.Add($folded)) {
                    throw "case-fold collision: $normalized"
                }
                if (Test-ZipEntryIsSymlink -Entry $entry) {
                    throw "archive links are not accepted: $normalized"
                }

                $basename = [IO.Path]::GetFileName($normalized)
                if (Test-ZipEntryIsExecutable -Basename $basename -Entry $entry) {
                    if ($script:ArchiveSafetyAllowedExecutables -notcontains $basename) {
                        throw "unexpected executable member: $normalized"
                    }
                }

                $nested = $normalized.Contains("/")
                if ($nested) {
                    $expected = "$PackageName/$basename"
                    if ($normalized -ne $expected) {
                        throw "member is outside the package directory: $normalized"
                    }
                }
                if ($script:ArchiveSafetyRequiredWindows -notcontains $basename) {
                    throw "unexpected archive member: $normalized"
                }
                if ($entry.Length -gt $maxEntry) {
                    throw "archive entry size $($entry.Length) exceeds policy ceiling $maxEntry"
                }
                if (-not $basenames.Add($basename)) {
                    throw "duplicate archive member: $basename"
                }
                $accepted.Add([pscustomobject]@{ Entry = $entry; Normalized = $normalized; Basename = $basename })
            }

            foreach ($required in $script:ArchiveSafetyRequiredWindows) {
                if (-not $basenames.Contains($required)) {
                    throw "missing required member: $required"
                }
            }

            $nestedCount = @($accepted | Where-Object { $_.Normalized.Contains("/") }).Count
            if ($nestedCount -ne 0 -and $nestedCount -ne $accepted.Count) {
                throw "mixed flat and nested archive layout is not accepted"
            }

            $declaredTotal = [int64]0
            foreach ($item in $accepted) {
                $declaredTotal += $item.Entry.Length
                if ($declaredTotal -gt $maxUncompressed) {
                    throw "archive uncompressed size $declaredTotal exceeds policy ceiling $maxUncompressed"
                }
            }

            $extractRoot = $stagingRoot
            if ($nestedCount -eq $accepted.Count) {
                $extractRoot = Join-Path $stagingRoot $PackageName
                New-Item -ItemType Directory -Path $extractRoot -Force | Out-Null
            }

            $actualTotal = [int64]0
            $buffer = New-Object byte[] 65536
            foreach ($item in $accepted) {
                $dest = Join-Path $extractRoot $item.Basename
                $source = $item.Entry.Open()
                $sz = [int64]0
                try {
                    $target = [IO.File]::Open($dest, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                    try {
                        while (($n = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                            if (($sz + $n) -gt $maxEntry) {
                                throw "archive entry size $($sz + $n) exceeds policy ceiling $maxEntry"
                            }
                            if (($actualTotal + $n) -gt $maxUncompressed) {
                                throw "archive uncompressed size $($actualTotal + $n) exceeds policy ceiling $maxUncompressed"
                            }
                            $target.Write($buffer, 0, $n)
                            $sz += $n
                            $actualTotal += $n
                        }
                    } finally {
                        $target.Dispose()
                    }
                } finally {
                    $source.Dispose()
                }
            }

            $extractDir = $extractRoot
            if ($nestedCount -eq 0) {
                $extractDir = $extractRoot
            }

            $server = Join-Path $extractDir "perllsp.exe"
            $dap = Join-Path $extractDir "perl-dap.exe"
            if (-not (Test-Path -LiteralPath $server) -or -not (Test-Path -LiteralPath $dap)) {
                throw "expected binaries were not staged from the release archive"
            }

            $archiveHash = Get-StagedMemberSha256 -Path $ArchivePath
            $memberParts = foreach ($required in $script:ArchiveSafetyRequiredWindows) {
                $hash = Get-StagedMemberSha256 -Path (Join-Path $extractDir $required)
                "$required`:$hash"
            }
            $layout = if ($nestedCount -eq $accepted.Count) { "windows_nested_v1" } else { "windows_flat_v1" }
            $receipt = "archive_safety_receipt policy=$($script:ArchiveSafetyPolicyId) layout=$layout archive_sha256=$archiveHash members=$($memberParts -join ',')"
            if ($receipt -match [regex]::Escape($stagingRoot) -or $receipt -match [regex]::Escape($ArchivePath)) {
                throw "archive safety receipt contained a private path"
            }
            Write-Info $receipt
            Write-Info "staged accepted topology members"
            return $extractDir
        } finally {
            $archive.Dispose()
        }
    } catch {
        if (Test-Path -LiteralPath $stagingRoot) {
            Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
        throw
    }
}

function Get-StandaloneProductStore {
    param([Parameter(Mandatory = $true)][string]$InstallDir)
    return (Join-Path $InstallDir ".perl-lsp")
}

function Invoke-ProductUnitFaultIfRequested {
    param([Parameter(Mandatory = $true)][string]$Barrier, [bool]$AllowFault = $true)
    if (-not $AllowFault) {
        return
    }
    if ($env:PERL_LSP_INSTALL_FAULT -eq $Barrier) {
        throw "injected product-unit fault: $Barrier"
    }
}

function Invoke-ProductUnitObserveIfRequested {
    param(
        [Parameter(Mandatory = $true)][string]$Barrier,
        [Parameter(Mandatory = $true)][string]$InstallDir
    )
    if ($env:PERL_LSP_INSTALL_OBSERVE -ne $Barrier) {
        return
    }
    $file = [string]$env:PERL_LSP_INSTALL_OBSERVE_FILE
    if ([string]::IsNullOrWhiteSpace($file)) {
        throw "PERL_LSP_INSTALL_OBSERVE_FILE is required for observation barrier $Barrier"
    }
    $cur = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    $path = Get-StandalonePathVisibleObservation -InstallDir $InstallDir
    Set-Content -LiteralPath $file -Value ($cur + "`n" + $path) -Encoding ascii
    if ($path -like "state=mixed*") {
        throw "path-visible product unit became mixed at $Barrier"
    }
}

function Get-ProductUnitCandidateId {
    param(
        [Parameter(Mandatory = $true)][string]$Disposition,
        [Parameter(Mandatory = $true)][string]$ServerSha256,
        [Parameter(Mandatory = $true)][string]$DapSha256
    )
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("perl-lsp-unit-" + [guid]::NewGuid().ToString("N"))
    $payload = New-Object System.IO.MemoryStream
    try {
        $enc = [Text.Encoding]::UTF8
        foreach ($part in @(
                "perl-lsp-swarm:standalone-product-unit.v1",
                $Disposition,
                $ServerSha256,
                $DapSha256
            )) {
            $bytes = $enc.GetBytes($part)
            $payload.Write($bytes, 0, $bytes.Length)
            $payload.WriteByte(0)
        }
        [IO.File]::WriteAllBytes($tmp, $payload.ToArray())
        return Get-StagedMemberSha256 -Path $tmp
    } finally {
        $payload.Dispose()
        if (Test-Path -LiteralPath $tmp) {
            Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
        }
    }
}

function Write-ProductUnitManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Directory,
        [Parameter(Mandatory = $true)][string]$Disposition,
        [Parameter(Mandatory = $true)][string]$CandidateId,
        [Parameter(Mandatory = $true)][string]$ServerSha256,
        [Parameter(Mandatory = $true)][string]$DapSha256
    )
    $body = @(
        "schema=standalone_product_unit.v1",
        "disposition=$Disposition",
        "candidate_id=$CandidateId",
        "server_sha256=$ServerSha256",
        "dap_sha256=$DapSha256"
    ) -join "`n"
    Set-Content -LiteralPath (Join-Path $Directory "product_unit.v1") -Value $body -Encoding ascii
}

function Get-StagedProductUnitDisposition {
    param(
        [Parameter(Mandatory = $true)][string]$ExtractDir,
        [Parameter(Mandatory = $true)][ValidateSet("release", "source")][string]$Mode
    )
    $server = Join-Path $ExtractDir "$Name.exe"
    $dap = Join-Path $ExtractDir "$DapName.exe"
    if (-not (Test-Path -LiteralPath $server) -or [IO.File]::GetAttributes($server).HasFlag([IO.FileAttributes]::ReparsePoint)) {
        throw "staged product unit is missing a regular perllsp member"
    }
    if ($Mode -eq "source") {
        return "advanced_source_server_only"
    }
    if (-not (Test-Path -LiteralPath $dap) -or [IO.File]::GetAttributes($dap).HasFlag([IO.FileAttributes]::ReparsePoint)) {
        throw "archive product unit requires a complete perllsp/perl-dap pair"
    }
    return "archive_pair_required"
}

function Initialize-StandaloneNativeFile {
    if (-not ("PerlLspNative.File" -as [type])) {
        Add-Type -Namespace PerlLspNative -Name File -MemberDefinition @"
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern bool MoveFileEx(string lpExistingFileName, string lpNewFileName, uint dwFlags);
"@
    }
}

function Invoke-StandaloneMoveFileReplace {
    param(
        [Parameter(Mandatory = $true)][string]$From,
        [Parameter(Mandatory = $true)][string]$To
    )
    Initialize-StandaloneNativeFile
    $replace = [uint32]1
    $none = [uint32]0
    if (-not (Test-Path -LiteralPath $To)) {
        if ([PerlLspNative.File]::MoveFileEx($From, $To, $none)) {
            return
        }
        $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "atomic pointer publish failed (win32=$code)"
    }
    if ([PerlLspNative.File]::MoveFileEx($From, $To, $replace)) {
        return
    }
    $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    throw "atomic pointer replace failed (win32=$code)"
}

function Write-StandalonePointerFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$RelativeTarget
    )
    $tmp = "$Path.tmp.$PID.$([guid]::NewGuid().ToString("N"))"
    if (Test-Path -LiteralPath $tmp) {
        Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
    Set-Content -LiteralPath $tmp -Value $RelativeTarget -Encoding ascii -NoNewline
    try {
        Invoke-StandaloneMoveFileReplace -From $tmp -To $Path
    } catch {
        if (Test-Path -LiteralPath $tmp) {
            Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
        }
        throw
    }
}

function Read-StandalonePointerRelative {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Store
    )
    if (-not (Test-Path -LiteralPath $Path)) {
        return $null
    }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or $item.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        $old = $item.Target
        if ($old -is [array]) { $old = $old[0] }
        if ([string]::IsNullOrWhiteSpace([string]$old)) {
            return $null
        }
        if ([IO.Path]::IsPathRooted([string]$old)) {
            return "candidates\" + [IO.Path]::GetFileName([string]$old)
        }
        return [string]$old
    }
    return ((Get-Content -LiteralPath $Path -Raw).Trim())
}

function Get-StandaloneCurrentDir {
    param([Parameter(Mandatory = $true)][string]$InstallDir)
    $store = Get-StandaloneProductStore -InstallDir $InstallDir
    $current = Join-Path $store "current"
    $rel = Read-StandalonePointerRelative -Path $current -Store $store
    if ([string]::IsNullOrWhiteSpace($rel)) {
        return $null
    }
    $item = Get-Item -LiteralPath $current -Force
    if ($item.PSIsContainer) {
        return $current
    }
    return [IO.Path]::GetFullPath((Join-Path $store $rel))
}

function Write-StandaloneCmdShim {
    param(
        [Parameter(Mandatory = $true)][string]$ShimPath,
        [Parameter(Mandatory = $true)][string]$ExeName
    )
    $body = @"
@echo off
setlocal EnableExtensions
set "STORE=%~dp0.perl-lsp"
set /p TARGET=<"%STORE%\current"
"%STORE%\%TARGET%\$ExeName" %*
"@
    $tmp = "$ShimPath.tmp.$PID.$([guid]::NewGuid().ToString("N"))"
    Set-Content -LiteralPath $tmp -Value $body -Encoding ascii
    try {
        Invoke-StandaloneMoveFileReplace -From $tmp -To $ShimPath
    } catch {
        if (Test-Path -LiteralPath $tmp) {
            Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
        }
        throw
    }
}

function Publish-ImmutableStandaloneCandidate {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDir,
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [Parameter(Mandatory = $true)][string]$Disposition,
        [bool]$AllowFault = $true
    )
    $store = Get-StandaloneProductStore -InstallDir $InstallDir
    $serverSrc = Join-Path $SourceDir "$Name.exe"
    $serverHash = Get-StagedMemberSha256 -Path $serverSrc
    $dapHash = "-"
    $dapSrc = Join-Path $SourceDir "$DapName.exe"
    if ($Disposition -eq "archive_pair_required") {
        $dapHash = Get-StagedMemberSha256 -Path $dapSrc
    }
    $id = Get-ProductUnitCandidateId -Disposition $Disposition -ServerSha256 $serverHash -DapSha256 $dapHash
    $dest = Join-Path $store "candidates\$id"
    New-Item -ItemType Directory -Path (Join-Path $store "candidates") -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $store "attempts") -Force | Out-Null
    if (Test-Path -LiteralPath $dest) {
        $existingServer = Get-StagedMemberSha256 -Path (Join-Path $dest "$Name.exe")
        if ($existingServer -ne $serverHash) {
            throw "immutable candidate already exists with different perllsp bytes"
        }
        if ($Disposition -eq "archive_pair_required") {
            $existingDap = Get-StagedMemberSha256 -Path (Join-Path $dest "$DapName.exe")
            if ($existingDap -ne $dapHash) {
                throw "immutable candidate already exists with different perl-dap bytes"
            }
        }
        return $id
    }
    Invoke-ProductUnitFaultIfRequested -Barrier "before_publish" -AllowFault $AllowFault
    $attempt = Join-Path $store ("attempts\att-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $attempt -Force | Out-Null
    Copy-Item -LiteralPath $serverSrc -Destination (Join-Path $attempt "$Name.exe")
    if ($Disposition -eq "archive_pair_required") {
        Copy-Item -LiteralPath $dapSrc -Destination (Join-Path $attempt "$DapName.exe")
    }
    Write-ProductUnitManifest -Directory $attempt -Disposition $Disposition -CandidateId $id -ServerSha256 $serverHash -DapSha256 $dapHash
    Move-Item -LiteralPath $attempt -Destination $dest
    return $id
}

function Set-StandaloneCurrentSelection {
    param(
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [Parameter(Mandatory = $true)][string]$CandidateId,
        [bool]$AllowFault = $true
    )
    $store = Get-StandaloneProductStore -InstallDir $InstallDir
    $current = Join-Path $store "current"
    Invoke-ProductUnitFaultIfRequested -Barrier "before_commit" -AllowFault $AllowFault
    $oldRel = Read-StandalonePointerRelative -Path $current -Store $store
    if (-not [string]::IsNullOrWhiteSpace($oldRel)) {
        Write-StandalonePointerFile -Path (Join-Path $store "previous") -RelativeTarget $oldRel
    }
    Write-StandalonePointerFile -Path $current -RelativeTarget "candidates\$CandidateId"
}

function Set-StandalonePathVisibleSelectors {
    param(
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [bool]$AllowFault = $true,
        [string]$Disposition = "",
        [switch]$PermitStaleDapRemoval,
        [switch]$ExistingOnly
    )
    Invoke-ProductUnitFaultIfRequested -Barrier "before_selectors" -AllowFault $AllowFault
    $serverDest = Join-Path $InstallDir "$Name.cmd"
    $dapDest = Join-Path $InstallDir "$DapName.cmd"
    if (-not $ExistingOnly -or (Test-Path -LiteralPath $serverDest)) {
        Write-StandaloneCmdShim -ShimPath $serverDest -ExeName "$Name.exe"
    }
    Invoke-ProductUnitObserveIfRequested -Barrier "between_path_members" -InstallDir $InstallDir
    $wantDap = $Disposition -ne "advanced_source_server_only" -and $Disposition -ne "historical_server_only"
    if ($Disposition -eq "") {
        $currentDir = Get-StandaloneCurrentDir -InstallDir $InstallDir
        $wantDap = $currentDir -and (Test-Path -LiteralPath (Join-Path $currentDir "$DapName.exe"))
    }
    if ($wantDap -and (-not $ExistingOnly -or (Test-Path -LiteralPath $dapDest))) {
        Write-StandaloneCmdShim -ShimPath $dapDest -ExeName "$DapName.exe"
    } elseif (-not $ExistingOnly -and $PermitStaleDapRemoval -and (Test-Path -LiteralPath $dapDest)) {
        # Stale-DAP cleanup is post-commit only: removing the selector before
        # the current-pointer commit would leave the previous paired candidate
        # current with no PATH-visible adapter if the commit fails.
        Remove-Item -LiteralPath $dapDest -Force
    }
    foreach ($legacyExe in @((Join-Path $InstallDir "$Name.exe"), (Join-Path $InstallDir "$DapName.exe"))) {
        if (Test-Path -LiteralPath $legacyExe) {
            Remove-Item -LiteralPath $legacyExe -Force
        }
    }
}

function Get-StandaloneCurrentObservation {
    param([Parameter(Mandatory = $true)][string]$InstallDir)
    $store = Get-StandaloneProductStore -InstallDir $InstallDir
    $current = Join-Path $store "current"
    $dir = Get-StandaloneCurrentDir -InstallDir $InstallDir
    if (-not $dir -or -not (Test-Path -LiteralPath $current)) {
        return "state=none"
    }
    $rel = Read-StandalonePointerRelative -Path $current -Store $store
    $id = if ($rel) { [IO.Path]::GetFileName($rel.TrimEnd('\', '/')) } else { "" }
    $manifest = Join-Path $dir "product_unit.v1"
    $disposition = "unknown"
    if (Test-Path -LiteralPath $manifest) {
        $disposition = ((Select-String -Path $manifest -Pattern '^disposition=(.+)$').Matches[0].Groups[1].Value)
    }
    $server = "-"
    $dap = "-"
    $serverPath = Join-Path $dir "$Name.exe"
    if (Test-Path -LiteralPath $serverPath) {
        $server = Get-StagedMemberSha256 -Path $serverPath
    }
    $dapPath = Join-Path $dir "$DapName.exe"
    if (Test-Path -LiteralPath $dapPath) {
        $dap = Get-StagedMemberSha256 -Path $dapPath
    }
    return "state=selected disposition=$disposition candidate_id=$id server_sha256=$server dap_sha256=$dap"
}

function Get-StandalonePathMemberSha256 {
    param(
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [Parameter(Mandatory = $true)][string]$ExeName
    )
    $exe = Join-Path $InstallDir $ExeName
    $cmd = Join-Path $InstallDir ([IO.Path]::ChangeExtension($ExeName, ".cmd"))
    if (Test-Path -LiteralPath $exe) {
        $item = Get-Item -LiteralPath $exe -Force
        if ($item.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
            return Get-StagedMemberSha256 -Path $exe
        }
        if (-not $item.PSIsContainer) {
            return Get-StagedMemberSha256 -Path $exe
        }
    }
    if (Test-Path -LiteralPath $cmd) {
        $dir = Get-StandaloneCurrentDir -InstallDir $InstallDir
        if ($dir) {
            $resolved = Join-Path $dir $ExeName
            if (Test-Path -LiteralPath $resolved) {
                return Get-StagedMemberSha256 -Path $resolved
            }
        }
        return "-"
    }
    return "-"
}

function Get-StandalonePathVisibleObservation {
    param([Parameter(Mandatory = $true)][string]$InstallDir)
    $server = Get-StandalonePathMemberSha256 -InstallDir $InstallDir -ExeName "$Name.exe"
    $dap = Get-StandalonePathMemberSha256 -InstallDir $InstallDir -ExeName "$DapName.exe"
    $current = Get-StandaloneCurrentObservation -InstallDir $InstallDir
    $curServer = "-"
    $curDap = "-"
    if ($current -match 'server_sha256=([0-9a-f]+|-)') { $curServer = $Matches[1] }
    if ($current -match 'dap_sha256=([0-9a-f]+|-)') { $curDap = $Matches[1] }
    $serverMatches = ($server -eq $curServer)
    $dapMatches = ($dap -eq $curDap)
    if (($curServer -ne "-") -and ($curDap -ne "-") -and (($server -ne "-") -xor ($dap -ne "-"))) {
        return "state=mixed server_sha256=$server dap_sha256=$dap"
    }
    if (($server -ne "-") -and ($dap -ne "-") -and ($serverMatches -xor $dapMatches)) {
        return "state=mixed server_sha256=$server dap_sha256=$dap"
    }
    return "state=path_visible server_sha256=$server dap_sha256=$dap"
}

function ConvertTo-StandaloneLegacyCandidate {
    param([Parameter(Mandatory = $true)][string]$InstallDir)
    $server = Join-Path $InstallDir "$Name.exe"
    $dap = Join-Path $InstallDir "$DapName.exe"
    if (-not (Test-Path -LiteralPath $server)) {
        return $null
    }
    $sitem = Get-Item -LiteralPath $server
    if ($sitem.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        return $null
    }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("perl-lsp-legacy-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    Copy-Item -LiteralPath $server -Destination (Join-Path $tmp "$Name.exe")
    $disposition = "historical_server_only"
    if ((Test-Path -LiteralPath $dap) -and -not (Get-Item -LiteralPath $dap).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) {
        Copy-Item -LiteralPath $dap -Destination (Join-Path $tmp "$DapName.exe")
        $disposition = "archive_pair_required"
    }
    return @{ Dir = $tmp; Disposition = $disposition }
}

function Install-StandaloneProductUnit {
    param(
        [Parameter(Mandatory = $true)][string]$ExtractDir,
        [Parameter(Mandatory = $true)][string]$InstallDir,
        [ValidateSet("release", "source")][string]$Mode = "release"
    )
    if (-not (Test-Path -LiteralPath $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    $disposition = Get-StagedProductUnitDisposition -ExtractDir $ExtractDir -Mode $Mode
    $store = Get-StandaloneProductStore -InstallDir $InstallDir
    New-Item -ItemType Directory -Path $store -Force | Out-Null

    $legacy = ConvertTo-StandaloneLegacyCandidate -InstallDir $InstallDir
    if ($null -ne $legacy) {
        $legacyId = Publish-ImmutableStandaloneCandidate -SourceDir $legacy.Dir -InstallDir $InstallDir -Disposition $legacy.Disposition -AllowFault $false
        Remove-Item -LiteralPath $legacy.Dir -Recurse -Force -ErrorAction SilentlyContinue
        Set-StandaloneCurrentSelection -InstallDir $InstallDir -CandidateId $legacyId -AllowFault $false
        Set-StandalonePathVisibleSelectors -InstallDir $InstallDir -AllowFault $false -Disposition $legacy.Disposition -PermitStaleDapRemoval
    }

    $id = Publish-ImmutableStandaloneCandidate -SourceDir $ExtractDir -InstallDir $InstallDir -Disposition $disposition
    $hadServer = Test-Path -LiteralPath (Join-Path $InstallDir "$Name.cmd")
    $hadDap = Test-Path -LiteralPath (Join-Path $InstallDir "$DapName.cmd")
    try {
        Set-StandalonePathVisibleSelectors -InstallDir $InstallDir -Disposition $disposition
        Set-StandaloneCurrentSelection -InstallDir $InstallDir -CandidateId $id
    } catch {
        if (-not $hadServer) {
            $serverDest = Join-Path $InstallDir "$Name.cmd"
            if (Test-Path -LiteralPath $serverDest) {
                Remove-Item -LiteralPath $serverDest -Force
            }
        }
        if (-not $hadDap) {
            $dapDest = Join-Path $InstallDir "$DapName.cmd"
            if (Test-Path -LiteralPath $dapDest) {
                Remove-Item -LiteralPath $dapDest -Force
            }
        }
        throw
    }
    Set-StandalonePathVisibleSelectors -InstallDir $InstallDir -Disposition $disposition -PermitStaleDapRemoval

    $previous = "none"
    $prevPath = Join-Path $store "previous"
    $prevRel = Read-StandalonePointerRelative -Path $prevPath -Store $store
    if (-not [string]::IsNullOrWhiteSpace($prevRel)) {
        $previous = [IO.Path]::GetFileName($prevRel.TrimEnd('\', '/'))
    }
    $currentDir = Get-StandaloneCurrentDir -InstallDir $InstallDir
    if (-not $currentDir) {
        throw "product-unit current pointer did not resolve after commit"
    }
    $serverHash = Get-StagedMemberSha256 -Path (Join-Path $currentDir "$Name.exe")
    $dapHash = "-"
    $currentDap = Join-Path $currentDir "$DapName.exe"
    if (Test-Path -LiteralPath $currentDap) {
        $dapHash = Get-StagedMemberSha256 -Path $currentDap
    }
    $receipt = "product_unit_receipt disposition=$disposition candidate_id=$id previous=$previous server_sha256=$serverHash dap_sha256=$dapHash state=selected"
    if ($receipt.Contains($InstallDir) -or $receipt.Contains($ExtractDir)) {
        throw "product-unit receipt contained a private path"
    }
    Write-Info $receipt
    Write-Success "Installed $Name to $(Join-Path $InstallDir "$Name.cmd")"
    if ($dapHash -ne "-") {
        Write-Success "Installed $DapName to $(Join-Path $InstallDir "$DapName.cmd")"
    }
    $dapDestPath = $null
    if ($dapHash -ne "-") {
        $dapDestPath = Join-Path $InstallDir "$DapName.cmd"
    }
    return [pscustomobject]@{
        DapInstalled = ($dapHash -ne "-")
        DestPath     = (Join-Path $InstallDir "$Name.cmd")
        DapDestPath  = $dapDestPath
        CandidateId  = $id
        Receipt      = $receipt
    }
}

if ($env:PERL_LSP_INSTALLER_LIBRARY_ONLY -eq '1') {
    return
}

if (-not ($IsArm64Host -or $HostArch -eq "AMD64")) {
    Write-Error "Unsupported architecture: $HostArch. Windows releases ship x86_64 and ARM64 builds only. Build from source: https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/INSTALLATION.md"
}

# Resolve the version before selecting a target. Target selection now depends
# on which assets a specific release actually carries, so the tag has to be
# known first.
if ($Version -eq "latest") {
    try {
        $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Tag = $Release.tag_name
        Write-Info "Latest version: $Tag"
    } catch {
        Write-Error "Failed to fetch latest release: $_"
    }
} else {
    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
}

$VersionNum = $Tag.TrimStart("v")
$ReleaseBaseUrl = "https://github.com/$Repo/releases/download/$Tag"

# Probe for an asset without downloading it. A definitive 404 means the
# release does not carry the native asset; transport/proxy failures are kept
# distinct because they do not establish absence.
function Test-ReleaseAsset {
    param([string]$AssetName)

    try {
        $Response = Invoke-WebRequest -Uri "$ReleaseBaseUrl/$AssetName" -Method Head -UseBasicParsing -ErrorAction Stop
        $StatusCode = [int]$Response.StatusCode
        if ($StatusCode -eq 404) {
            return [pscustomobject]@{ State = "absent" }
        }
        if ($StatusCode -ge 200 -and $StatusCode -lt 400) {
            return [pscustomobject]@{ State = "present" }
        }
        return [pscustomobject]@{ State = "unknown"; StatusCode = $StatusCode }
    } catch {
        $StatusCode = $null
        if ($_.Exception.Response) {
            try {
                $StatusCode = [int]$_.Exception.Response.StatusCode
            } catch {
                # The response may not expose a numeric status code for a
                # transport or proxy failure; retain the unknown state.
            }
        }
        if ($StatusCode -eq 404) {
            return [pscustomobject]@{ State = "absent" }
        }
        return [pscustomobject]@{ State = "unknown"; StatusCode = $StatusCode }
    }
}

# Name each target as a whole literal rather than assembling it from an arch
# variable and a "-pc-windows-msvc" suffix. PowerShell cannot be executed on
# the Linux CI host, so the contract test in
# scripts/tests/test-install-target-selection.sh checks which targets this
# script can request by reading them out of the source. Assembling the triple
# from a variable hides it from that check, which is how the original defect
# ($Arch = "aarch64") stayed invisible.
if ($IsArm64Host) {
    Write-Info "Detected system: Windows (ARM64)"

    $NativeTarget = "aarch64-pc-windows-msvc"
    $NativeAsset = "$Name-$VersionNum-$NativeTarget.zip"
    $AssetProbe = Test-ReleaseAsset $NativeAsset

    if ($AssetProbe.State -eq "present") {
        $Target = $NativeTarget
        Write-Info "Using the native ARM64 build for $Target"
    } elseif ($AssetProbe.State -eq "absent") {
        # Emulation fallback only. The build floor belongs here and nowhere
        # else: it is a property of running x64 code on ARM64, so it must not
        # gate the native path above.
        $WindowsBuild = Get-WindowsBuildNumber
        if ($WindowsBuild -lt 22000) {
            $DetectedBuild = if ($WindowsBuild -ge 0) { "build $WindowsBuild" } else { "an unknown Windows build" }
            Write-Error "$Tag ships no native ARM64 Windows build, and running the x86_64 build under emulation requires Windows 11 (build 22000 or newer); detected $DetectedBuild. Install a release that carries $NativeAsset, or build from source: https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/INSTALLATION.md"
        }

        $Target = "x86_64-pc-windows-msvc"
        Write-Warn "$Tag ships no native ARM64 Windows build; using the x86_64 build, which runs under the x64 emulation in Windows 11 on ARM."
    } else {
        # An unknown probe result is not evidence that the native asset is
        # absent. Windows 10 cannot safely use the x64 fallback, so fail with
        # a retryable diagnosis instead of claiming that the release lacks the
        # native asset. Windows 11 can still use the safe x64 fallback.
        $WindowsBuild = Get-WindowsBuildNumber
        if ($WindowsBuild -lt 22000) {
            Write-Error "Could not determine whether $Tag carries the native ARM64 Windows build because the asset probe failed. Windows 10 ARM64 cannot safely fall back to x64 emulation; retry when the release can be checked, or build from source: https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/how-to/INSTALLATION.md"
        }

        $Target = "x86_64-pc-windows-msvc"
        Write-Warn "Could not verify the native ARM64 asset for $Tag; using the x86_64 build under Windows 11 x64 emulation."
    }
} else {
    $Target = "x86_64-pc-windows-msvc"
    Write-Info "Detected system: Windows ($HostArch) - $Target"
}

# Construct download URL
$Asset = "$Name-$VersionNum-$Target.zip"
$Url = "$ReleaseBaseUrl/$Asset"

Write-Info "Downloading $Name $Tag for $Target"

# Create temp directory
$TempDir = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }

try {
    # Integrity metadata is required and validated before the archive download.
    $ChecksumUrl = "$ReleaseBaseUrl/SHA256SUMS"
    $ChecksumPath = Join-Path $TempDir "SHA256SUMS"
    try {
        Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumPath -UseBasicParsing
    } catch {
        Write-Error "Failed to download required checksum manifest from $ChecksumUrl : $_"
        throw
    }

    $ExpectedHash = Get-ExpectedAssetHash -ChecksumPath $ChecksumPath -Asset $Asset

    # Download binary
    $ZipPath = Join-Path $TempDir $Asset
    Write-Info "Downloading from $Url"
    try {
        Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing
    } catch {
        Write-Error "Failed to download from $Url : $_"
        throw
    }

    # Verify checksum
    $ActualHash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
    if ($ExpectedHash -ne $ActualHash) {
        Write-Error "Checksum mismatch - expected: $ExpectedHash, got: $ActualHash"
        throw
    }
    Write-Success "Checksum verified"

    # Inspect the verified zip and extract only accepted topology members
    # into a private staging root. Expand-Archive is not the safety boundary.
    Write-Info "Inspecting release archive"
    $PackageName = "$Name-$VersionNum-$Target"
    $ExtractedDir = Invoke-StandaloneArchiveStaging -ArchivePath $ZipPath -StagingParent $TempDir -PackageName $PackageName
    
    $BinaryPath = Join-Path $ExtractedDir "$Name.exe"
    if (-not (Test-Path $BinaryPath)) {
        Write-Error "Binary not found at $BinaryPath"
    }
    
    # Create install directory and atomically promote the staged product unit.
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $Promotion = Install-StandaloneProductUnit -ExtractDir $ExtractedDir -InstallDir $InstallDir -Mode release
    $DestPath = $Promotion.DestPath
    $DapInstalled = $Promotion.DapInstalled
    $DapDestPath = $Promotion.DapDestPath

    # Verify installation
    try {
        $VersionOutput = & $DestPath --version 2>&1
        Write-Success "Installation verified: $VersionOutput"
    } catch {
        Write-Warn "Could not verify installation"
    }

    # Persist a user-local install directory safely. The old guidance printed a
    # command that copied the merged process/system/user `$env:Path` back into
    # the User PATH, permanently duplicating system entries. This path updates
    # only the User scope and keeps an explicit opt-out for managed machines.
    #
    # Process-only visibility is not persistence: a prior temporary-session
    # PATH edit must still write User PATH so fresh terminals inherit it.
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $InUserPath = Test-PathContainsEntry -PathValue $UserPath -Entry $InstallDir
    $InProcessPath = Test-PathContainsEntry -PathValue $env:Path -Entry $InstallDir

    if ($InUserPath) {
        Add-InstallDirToCurrentProcessPath
        if ($InProcessPath) {
            $PathDisposition = "path_visible_current_process"
            Write-Success "$InstallDir is already persisted and visible on PATH"
        } else {
            $PathDisposition = "persisted_user_path_restart_required"
            Write-Info "$InstallDir is already persisted in the user PATH"
            Write-Warn "Restart already-running terminals, editors, and Claude Code so they inherit the persisted PATH."
        }
    } elseif ($NoModifyPath) {
        $PathDisposition = "manual_path_action_required"
        Write-Warn "$InstallDir is not in the user PATH and -NoModifyPath was requested"
        Write-ManualPathGuidance
    } else {
        try {
            $Changed = Ensure-InstallDirOnUserPath
            Add-InstallDirToCurrentProcessPath
            $PathDisposition = "persisted_user_path_restart_required"
            if ($Changed) {
                Write-Success "Added $InstallDir to the persistent user PATH"
            } elseif ($InProcessPath) {
                Write-Info "$InstallDir was process-visible; confirmed user PATH persistence"
            }
            Write-Warn "Restart already-running terminals, editors, and Claude Code so they inherit the persisted PATH."
        } catch {
            Add-InstallDirToCurrentProcessPath
            $PathDisposition = "manual_path_action_required"
            Write-Warn "Could not persist $InstallDir on the user PATH: $($_.Exception.Message)"
            Write-ManualPathGuidance
        }
    }
    Write-Info "PATH status: $PathDisposition"
    
    Write-Host ""
    Write-Host "Installation complete! 🎉" -ForegroundColor Green
    Write-Host ""
    Write-Host "To get started with Perl LSP:"
    Write-Host "  • VS Code: Install the Perl LSP extension from the marketplace"
    Write-Host "  • Other editors: Configure to use '$DestPath --stdio'"
    if ($DapInstalled) {
        Write-Host "  • Debugging: Configure your DAP client to use '$DapDestPath'"
    } else {
        Write-Host "  • Debugging: unavailable - $DapName.exe was not in this release archive"
    }
    Write-Host ""
    Write-Host "For more information: https://github.com/$Repo"
    
} finally {
    # Cleanup
    if (Test-Path $TempDir) {
        Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
