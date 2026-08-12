[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Server,

    [string]$Dap,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedVersion,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedTarget,

    [string]$ExpectedCandidate,

    [Parameter(Mandatory = $true)]
    [string]$Receipt
)

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$verifier = Join-Path $scriptDir 'verify_binary_identity.py'

$python = Get-Command python3 -ErrorAction SilentlyContinue
if (-not $python) {
    $python = Get-Command python -ErrorAction SilentlyContinue
}
if (-not $python) {
    throw 'Python 3 is required for staged perl-lsp identity verification.'
}

$arguments = @(
    $verifier,
    '--server', $Server,
    '--expected-version', $ExpectedVersion,
    '--expected-target', $ExpectedTarget,
    '--receipt', $Receipt
)
if ($Dap) {
    $arguments += @('--dap', $Dap, '--require-dap')
}
if ($ExpectedCandidate) {
    $arguments += @('--expected-candidate', $ExpectedCandidate)
}

& $python.Source @arguments
if ($LASTEXITCODE -ne 0) {
    throw "Staged perl-lsp identity verification failed with exit code $LASTEXITCODE. Receipt: $Receipt"
}
