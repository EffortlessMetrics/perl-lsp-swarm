$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'windows-owned-suspend.ps1'
$heartbeat = Join-Path ([IO.Path]::GetTempPath()) ('perl-lsp-7846-heartbeat-' + [Guid]::NewGuid().ToString('N') + '.txt')
$stderr = $heartbeat + '.err'
$child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\PING.EXE') -ArgumentList '127.0.0.1','-t' -RedirectStandardOutput $heartbeat -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ((!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if (!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) { throw 'heartbeat did not start' }
  $creation = [Diagnostics.Process]::GetProcessById($child.Id).StartTime.ToFileTimeUtc()
  $before = (Get-Item -LiteralPath $heartbeat).Length
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action proof -ProcessId $child.Id -CreationTimeFileTime $creation -HeartbeatPath $heartbeat -BaselineLength $before
  if ($LASTEXITCODE -ne 0) { throw 'pinned suspend/resume proof failed' }
  $deadline = [DateTime]::UtcNow.AddSeconds(3)
  do { Start-Sleep -Milliseconds 50; $after = (Get-Item -LiteralPath $heartbeat).Length } while ($after -le $before -and [DateTime]::UtcNow -lt $deadline)
  if ($after -le $before) { throw 'heartbeat did not resume after proof' }
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action failure -ProcessId $child.Id -CreationTimeFileTime $creation -HeartbeatPath $heartbeat -BaselineLength $after
  if ($LASTEXITCODE -eq 0) { throw 'injected partial failure unexpectedly succeeded' }
  Start-Sleep -Milliseconds 250
  $recovered = (Get-Item -LiteralPath $heartbeat).Length
  if ($recovered -le $after) { throw 'rollback did not resume the owned child' }
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Action proof -ProcessId $child.Id -CreationTimeFileTime ($creation + 1) -HeartbeatPath $heartbeat -BaselineLength $recovered
  if ($LASTEXITCODE -eq 0) { throw 'wrong creation identity unexpectedly accepted' }
  Write-Output 'WINDOWS_OWNED_SUSPEND_PASS'
} finally {
  $child.Refresh()
  if (!$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(3000) }
  $child.Dispose()
  Remove-Item -LiteralPath $heartbeat,$stderr -Force -ErrorAction SilentlyContinue
}
