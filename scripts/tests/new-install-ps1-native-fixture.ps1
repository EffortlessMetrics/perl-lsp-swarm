# A real Windows PE executable for installer proofs, built without Cargo or downloads.
# .NET Framework's compiler is present on supported Windows/Windows CI hosts.
param(
    [Parameter(Mandatory = $true)][string]$Path,
    [int]$ExitCode = 0,
    [switch]$Silent,
    [switch]$Stderr
)

$ErrorActionPreference = 'Stop'
$Compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $Compiler)) {
    $Compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
}
$Source = "$Path.cs"
$OutputStatements = ''
if (-not $Silent) {
    $OutputStatements = 'System.Console.WriteLine("perllsp fixture 0.18.0");'
    if ($ExitCode -ne 0) {
        $OutputStatements += 'System.Console.Error.WriteLine("fixture verification failure");'
    } elseif ($Stderr) {
        $OutputStatements += 'System.Console.Error.WriteLine("fixture benign stderr");'
    }
}
try {
    [IO.File]::WriteAllText($Source, "class Fixture { static int Main() { $OutputStatements return $ExitCode; } }")
    & $Compiler /nologo /target:exe "/out:$Path" $Source
    if ($LASTEXITCODE -ne 0) { throw "native fixture compilation failed: $LASTEXITCODE" }
} finally {
    Remove-Item -LiteralPath $Source -Force -ErrorAction SilentlyContinue
}
