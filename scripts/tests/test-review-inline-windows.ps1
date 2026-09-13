# Offline native PowerShell -> WSL proof for #14313. Requires WSL bash and jq.
# Only paths cross the process boundary; never pass review text to bash -c.
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Run this boundary test on native Windows PowerShell 7.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$fixtureDir = Join-Path ([IO.Path]::GetTempPath()) ('review-inline-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($fixtureDir) | Out-Null
try {
    $bodyPath = Join-Path $fixtureDir 'literal review body.md'
    $body = @'
-- leading dashes, "double", 'single', \path\ and café 日本語
## No material findings
`cargo sentinel` $(git sentinel) `sentinel` $(sentinel)
```bash
cargo test
```
'@
    $body += "`r`nCRLF line`r`n`n`n"
    [IO.File]::WriteAllText($bodyPath, $body, [Text.UTF8Encoding]::new($false))
    $linuxBody = & wsl.exe --exec wslpath -a -u $bodyPath
    if ($LASTEXITCODE -ne 0) { throw 'wslpath could not translate the body fixture path' }
    $suitePath = Join-Path $repoRoot 'scripts/tests/test-review-threads-inline.sh'
    $linuxSuite = & wsl.exe --exec wslpath -a -u $suitePath
    if ($LASTEXITCODE -ne 0) { throw 'wslpath could not translate the test suite path' }
    # --exec bypasses the WSL default shell. bash receives a script filename and
    # a fixture filename as separate arguments, never a command string or body.
    & wsl.exe --exec bash $linuxSuite $linuxBody
    if ($LASTEXITCODE -ne 0) { throw 'Offline review suite failed through the Windows/WSL boundary' }
    if ([IO.File]::ReadAllText($bodyPath) -cne $body) { throw 'Body fixture changed during the test' }
    Write-Output 'PASS native Windows -> WSL file transport (offline stub GitHub only)'
} finally {
    Remove-Item -LiteralPath $bodyPath -Force -ErrorAction SilentlyContinue
    # This directory contains only the uniquely named test fixture; no recursion.
    Remove-Item -LiteralPath $fixtureDir -Force
}
