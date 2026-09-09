$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'windows-owned-suspend.ps1'
$heartbeat = Join-Path ([IO.Path]::GetTempPath()) ('perl-lsp-7846-heartbeat-' + [Guid]::NewGuid().ToString('N') + '.txt')
$stderr = $heartbeat + '.err'
$child = Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\PING.EXE') -ArgumentList '127.0.0.1','-t' -RedirectStandardOutput $heartbeat -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
function Invoke-Helper([string[]]$extra, [bool]$sendResume, [UInt64]$identity, [bool]$observeHold) {
  $info = [Diagnostics.ProcessStartInfo]::new()
  $info.FileName = 'powershell.exe'; $info.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $helper + '" -ProcessId ' + $child.Id + ' -CreationTimeFileTime ' + $identity + ' ' + ($extra -join ' ')
  $info.UseShellExecute = $false; $info.CreateNoWindow = $true; $info.RedirectStandardInput = $true; $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
  $worker = [Diagnostics.Process]::new(); $worker.StartInfo = $info; if(!$worker.Start()) { throw 'helper did not start' }
  try {
    $lineTask=$worker.StandardOutput.ReadLineAsync(); if(!$lineTask.Wait(5000)) { throw 'suspend handshake timeout' }; $line=$lineTask.Result
    if($observeHold) { $baseline=(Get-Item -LiteralPath $heartbeat).Length; Start-Sleep -Milliseconds 1500; $child.Refresh(); $during=(Get-Item -LiteralPath $heartbeat).Length; if($child.HasExited -or $during -ne $baseline) { throw "heartbeat was not frozen after handshake: $baseline -> $during" } }
    if($sendResume) { $worker.StandardInput.WriteLine('resume'); $worker.StandardInput.Flush(); $worker.StandardInput.Close() }
    if(!$worker.WaitForExit(7000)) { $worker.Kill(); throw 'helper did not exit' }
    [pscustomobject]@{ Exit=$worker.ExitCode; Line=$line; Error=$worker.StandardError.ReadToEnd() }
  } finally {
    if(!$worker.HasExited) {
      try { $worker.StandardInput.Close() } catch { }
      if(!$worker.WaitForExit(500)) {
        try { $worker.Kill() } catch { }
        $null=$worker.WaitForExit(3000)
      }
    }
    $worker.Dispose()
  }
}
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ((!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
  if (!(Test-Path -LiteralPath $heartbeat) -or (Get-Item -LiteralPath $heartbeat).Length -eq 0) { throw 'heartbeat did not start' }
  $creation = [Diagnostics.Process]::GetProcessById($child.Id).StartTime.ToFileTimeUtc()
  $proof=Invoke-Helper @('-TimeoutMilliseconds','5000') $true $creation $true
  if($proof.Exit -ne 0 -or $proof.Line -notlike 'SUSPENDED *') { throw "pinned handshake failed: $($proof | ConvertTo-Json -Compress)" }
  $during = (Get-Item -LiteralPath $heartbeat).Length
  $deadline=[DateTime]::UtcNow.AddSeconds(3); do { Start-Sleep -Milliseconds 50; $after=(Get-Item -LiteralPath $heartbeat).Length } while($after -le $during -and [DateTime]::UtcNow -lt $deadline); if($after -le $during) { throw 'heartbeat did not resume after handshake' }
  $failure=Invoke-Helper @('-InjectFailureAfter','1') $false $creation $false
  if($failure.Exit -eq 0 -or $failure.Error -notmatch 'injected partial-suspension failure') { throw "rollback control failed: $($failure | ConvertTo-Json -Compress)" }
  $deadline=[DateTime]::UtcNow.AddSeconds(3); do { Start-Sleep -Milliseconds 50; $recovered=(Get-Item -LiteralPath $heartbeat).Length } while($recovered -le $after -and [DateTime]::UtcNow -lt $deadline); if($recovered -le $after) { throw 'rollback did not resume owned child' }
  $wrong=Invoke-Helper @() $false ($creation + 1) $false
  if($wrong.Exit -eq 0 -or $wrong.Error -notmatch 'creation identity mismatch') { throw "identity control failed: $($wrong | ConvertTo-Json -Compress)" }
  $timeout=Invoke-Helper @('-TimeoutMilliseconds','100') $false $creation $false
  if($timeout.Exit -eq 0 -or $timeout.Error -notmatch 'resume handshake timeout') { throw "timeout control failed: $($timeout | ConvertTo-Json -Compress)" }
  $deadline=[DateTime]::UtcNow.AddSeconds(3); do { Start-Sleep -Milliseconds 50; $timeoutRecovered=(Get-Item -LiteralPath $heartbeat).Length } while($timeoutRecovered -le $recovered -and [DateTime]::UtcNow -lt $deadline); if($timeoutRecovered -le $recovered) { throw 'timeout finally did not resume owned child' }
  $resumeFailure=Invoke-Helper @('-InjectResumeFailure') $true $creation $false
  if($resumeFailure.Exit -ne 2 -or $resumeFailure.Error -notmatch 'ROLLBACK_INCOMPLETE') { throw "resume rollback control failed: $($resumeFailure | ConvertTo-Json -Compress)" }
  Write-Output 'WINDOWS_OWNED_SUSPEND_PROTOCOL_PASS'
} finally {
  $child.Refresh(); if(!$child.HasExited) { $child.Kill(); $null=$child.WaitForExit(3000) }; $child.Dispose()
  Remove-Item -LiteralPath $heartbeat,$stderr -Force -ErrorAction SilentlyContinue
}
