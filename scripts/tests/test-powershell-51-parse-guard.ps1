$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Paths = @(
    (Join-Path $Root "install.ps1"),
    (Join-Path $PSScriptRoot "test-install-ps1-checksum-required.ps1"),
    (Join-Path $PSScriptRoot "test-install-ps1-archive-safety.ps1"),
    (Join-Path $PSScriptRoot "test-install-ps1-product-unit-promotion.ps1"),
    $PSCommandPath
)

function Assert-PowerShellFileParses {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Tokens = $null
    $Errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile(
        $Path,
        [ref]$Tokens,
        [ref]$Errors
    ) | Out-Null

    if ($Errors.Count -ne 0) {
        $Details = ($Errors | ForEach-Object { $_.Message }) -join "; "
        throw "PS51_PARSE_REJECTED:$Path $Details"
    }
}

foreach ($Path in $Paths) {
    Assert-PowerShellFileParses -Path $Path
}

$InvalidPath = Join-Path ([System.IO.Path]::GetTempPath()) ("perl-lsp-ps51-invalid-" + [guid]::NewGuid().ToString("N") + ".ps1")
try {
    Set-Content -LiteralPath $InvalidPath -Value "function {" -Encoding UTF8
    $RejectedInvalidInput = $false
    try {
        Assert-PowerShellFileParses -Path $InvalidPath
    } catch {
        if ($_.Exception.Message.StartsWith("PS51_PARSE_REJECTED:")) {
            $RejectedInvalidInput = $true
        } else {
            throw
        }
    }

    if (-not $RejectedInvalidInput) {
        throw "Windows PowerShell 5.1 parser guard accepted deliberately invalid input"
    }
} finally {
    Remove-Item -LiteralPath $InvalidPath -Force -ErrorAction SilentlyContinue
}

Write-Output "PowerShell 5.1 parser guard accepted candidate files and rejected invalid input"
