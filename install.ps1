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
# Windows binary as `perllsp.exe` so the name matches POSIX and the docs.
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
    # Download binary
    $ZipPath = Join-Path $TempDir $Asset
    Write-Info "Downloading from $Url"
    
    try {
        Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing
    } catch {
        Write-Error "Failed to download from $Url : $_"
    }
    
    # Download and verify checksum (optional)
    $ChecksumUrl = "https://github.com/$Repo/releases/download/$Tag/SHA256SUMS"
    $ChecksumPath = Join-Path $TempDir "SHA256SUMS"
    
    try {
        Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumPath -UseBasicParsing
        
        # Verify checksum
        $ExpectedHash = (Get-Content $ChecksumPath | Select-String $Asset).Line.Split(" ")[0]
        $ActualHash = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLower()
        
        if ($ExpectedHash -eq $ActualHash) {
            Write-Success "Checksum verified"
        } else {
            Write-Error "Checksum mismatch - expected: $ExpectedHash, got: $ActualHash"
        }
    } catch {
        Write-Warn "Could not download or verify checksums"
    }
    
    # Extract archive
    Write-Info "Extracting archive"
    $ExtractDir = Join-Path $TempDir "extract"
    Expand-Archive -Path $ZipPath -DestinationPath $ExtractDir -Force
    
    # Find the binary
    $ExtractedDir = Join-Path $ExtractDir "$Name-$VersionNum-$Target"
    if (-not (Test-Path $ExtractedDir)) {
        # Try without nested directory
        $ExtractedDir = $ExtractDir
    }
    
    $BinaryPath = Join-Path $ExtractedDir "$Name.exe"
    if (-not (Test-Path $BinaryPath)) {
        Write-Error "Binary not found at $BinaryPath"
    }
    
    # Create install directory
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    
    # Install binary
    $DestPath = Join-Path $InstallDir "$Name.exe"
    Write-Info "Installing $Name to $DestPath"
    
    # Remove old binary if exists
    if (Test-Path $DestPath) {
        Remove-Item $DestPath -Force
    }
    
    # Copy binary
    Copy-Item -Path $BinaryPath -Destination $DestPath -Force
    
    Write-Success "Installed $Name to $DestPath"

    # Install the perl-dap companion binary when the archive carries it.
    # Mirrors the optional-DAP copy in scripts/install.sh: present since
    # v0.9.1, so treat absence as a warning rather than a hard failure to stay
    # compatible with older archives.
    $DapInstalled = $false
    $DapSourcePath = Join-Path $ExtractedDir "$DapName.exe"
    $DapDestPath = Join-Path $InstallDir "$DapName.exe"
    if (Test-Path $DapSourcePath) {
        Write-Info "Installing $DapName to $DapDestPath"
        if (Test-Path $DapDestPath) {
            Remove-Item $DapDestPath -Force
        }
        Copy-Item -Path $DapSourcePath -Destination $DapDestPath -Force
        Write-Success "Installed $DapName to $DapDestPath"
        $DapInstalled = $true
    } else {
        Write-Warn "$DapName.exe not found in the release archive - debugging support will be unavailable"
    }

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
