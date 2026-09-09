$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'windows-owned-suspend.ps1'
$heartbeat = Join-Path ([IO.Path]::GetTempPath()) ('perl-lsp-7846-heartbeat-' + [Guid]::NewGuid().ToString('N') + '.txt')
$child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\PING.EXE') -ArgumentList '127.0.0.1','-t' -RedirectStandardOutput $heartbeat -RedirectStandardError ($heartbeat + '.err') -PassThru -WindowStyle Hidden
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ((!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if (!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) { throw 'heartbeat did not start' }
  $creation = [Diagnostics.Process]::GetProcessById($child.Id).StartTime.ToFileTimeUtc()
  $before = (Get-Item -LiteralPath $heartbeat).Length
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action suspend -ProcessId $child.Id -CreationTimeFileTime $creation
  if ($LASTEXITCODE -ne 0) { throw 'suspend failed' }
  Start-Sleep -Milliseconds 1500
  $child.Refresh()
  Write-Output ("AFTER_SUSPEND_EXITED=" + $child.HasExited)
  $during = (Get-Item -LiteralPath $heartbeat).Length
  if ($during -ne $before) { throw "heartbeat advanced while suspended: $before -> $during" }
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action resume -ProcessId $child.Id -CreationTimeFileTime $creation
  if ($LASTEXITCODE -ne 0) { throw 'resume failed' }
  $deadline = [DateTime]::UtcNow.AddSeconds(3)
  do { Start-Sleep -Milliseconds 50; $after = (Get-Item -LiteralPath $heartbeat).Length } while ($after -le $during -and [DateTime]::UtcNow -lt $deadline)
  if ($after -le $during) { throw 'heartbeat did not resume' }
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action suspend -ProcessId $child.Id -CreationTimeFileTime ($creation + 1)
  if ($LASTEXITCODE -eq 0) { throw 'wrong creation identity unexpectedly accepted' }
  Write-Output 'WINDOWS_OWNED_SUSPEND_PASS'
} finally {
  if (!$child.HasExited) { Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue }
  $child.Dispose()
  Remove-Item -LiteralPath $heartbeat,($heartbeat + '.err') -Force -ErrorAction SilentlyContinue
}
