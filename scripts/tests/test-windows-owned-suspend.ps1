$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'windows-owned-suspend.ps1'
$heartbeat = Join-Path ([IO.Path]::GetTempPath()) ('perl-lsp-7846-heartbeat-' + [Guid]::NewGuid().ToString('N') + '.txt')
$stderr = $heartbeat + '.err'
$child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\PING.EXE') -ArgumentList '127.0.0.1','-t' -RedirectStandardOutput $heartbeat -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
function Invoke-Helper([string[]]$extra, [bool]$sendResume, [UInt64]$identity) {
  $info = [Diagnostics.ProcessStartInfo]::new()
  $info.FileName = 'powershell.exe'; $info.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $helper + '" -ProcessId ' + $child.Id + ' -CreationTimeFileTime ' + $identity + ' ' + ($extra -join ' ')
  $info.UseShellExecute = $false; $info.CreateNoWindow = $true; $info.RedirectStandardInput = $true; $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
  $worker = [Diagnostics.Process]::new(); $worker.StartInfo = $info; if(!$worker.Start()) { throw 'helper did not start' }
  try {
    $lineTask=$worker.StandardOutput.ReadLineAsync(); if(!$lineTask.Wait(5000)) { throw 'suspend handshake timeout' }; $line=$lineTask.Result
    if($sendResume) { $worker.StandardInput.WriteLine('resume'); $worker.StandardInput.Flush() }
    if(!$worker.WaitForExit(7000)) { $worker.Kill(); throw 'helper did not exit' }
    [pscustomobject]@{ Exit=$worker.ExitCode; Line=$line; Error=$worker.StandardError.ReadToEnd() }
  } finally { $worker.Dispose() }
}
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ((!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if (!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) { throw 'heartbeat did not start' }
  $creation = [Diagnostics.Process]::GetProcessById($child.Id).StartTime.ToFileTimeUtc()
  $before = (Get-Item -LiteralPath $heartbeat).Length
  $proof=Invoke-Helper @('-TimeoutMilliseconds','5000') $true $creation
  if($proof.Exit -ne 0 -or $proof.Line -notlike 'SUSPENDED *') { throw "pinned handshake failed: $($proof | Out-String)" }
  $deadline=[DateTime]::UtcNow.AddSeconds(3); do { Start-Sleep -Milliseconds 50; $after=(Get-Item -LiteralPath $heartbeat).Length } while($after -le $before -and [DateTime]::UtcNow -lt $deadline); if($after -le $before) { throw 'heartbeat did not resume after handshake' }
  $failure=Invoke-Helper @('-InjectFailureAfter','1') $false $creation
  if($failure.Exit -eq 0 -or $failure.Error -notmatch 'injected partial-suspension failure') { throw "rollback control failed: $($failure | Out-String)" }
  $deadline=[DateTime]::UtcNow.AddSeconds(3); do { Start-Sleep -Milliseconds 50; $recovered=(Get-Item -LiteralPath $heartbeat).Length } while($recovered -le $after -and [DateTime]::UtcNow -lt $deadline); if($recovered -le $after) { throw 'rollback did not resume owned child' }
  $wrong=Invoke-Helper @() $false ($creation + 1)
  if($wrong.Exit -eq 0 -or $wrong.Error -notmatch 'creation identity mismatch') { throw "identity control failed: $($wrong | Out-String)" }
  $resumeFailure=Invoke-Helper @('-InjectResumeFailure') $true $creation
  if($resumeFailure.Exit -ne 2 -or $resumeFailure.Error -notmatch 'ROLLBACK_INCOMPLETE') { throw "resume rollback control failed: $($resumeFailure | Out-String)" }
  Write-Output 'WINDOWS_OWNED_SUSPEND_PROTOCOL_PASS'
} finally {
  $child.Refresh(); if(!$child.HasExited) { $child.Kill(); $null=$child.WaitForExit(3000) }; $child.Dispose()
  Remove-Item -LiteralPath $heartbeat,$stderr -Force -ErrorAction SilentlyContinue
}
