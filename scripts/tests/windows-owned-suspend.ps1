<#
Test-only fault injection for a disposable, caller-owned process. The caller must
retain the creation FILETIME and pair suspension with resume/cleanup. SUSPENDED
means a bounded thread enumeration stabilized; stdin resume, EOF, or the hold
deadline ends the suspension. Cleanup terminates only the pinned matching identity.

Proof: from vscode-extension, compile tsconfig.published-smoke.json with
scripts/governed-tsc.js, then run node --test scripts/windows-owned-suspension.test.js.
The companion test-windows-owned-suspend.ps1 exercises partial rollback controls.
These tests establish the fault mechanism, not installed or release acceptance.
#>
param(
  [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$ProcessId,
  [Parameter(Mandatory=$true)][UInt64]$CreationTimeFileTime,
  [ValidateRange(1,120000)][int]$TimeoutMilliseconds = 120000,
  [ValidateRange(1,8)][int]$MaxRounds = 4,
  [ValidateRange(0,4096)][int]$InjectFailureAfter = 0,
  [switch]$InjectResumeFailure,
  [switch]$Cleanup
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading.Tasks;

public static class PerlLspOwnedSuspend {
  const uint QueryLimited = 0x1000;
  const uint ThreadSuspendResume = 0x0002;
  const uint ThreadQueryLimited = 0x0800;
  const uint SnapshotThread = 0x00000004;
  const uint Invalid = 0xffffffff;
  const int NoMoreFiles = 18;
  const uint Synchronize = 0x00100000;
  const uint WaitObject0 = 0;
  const uint ProcessTerminate = 0x0001;

  [StructLayout(LayoutKind.Sequential)]
  struct FileTime {
    public uint Lo;
    public uint Hi;
    public ulong Value() { return ((ulong)Hi << 32) | Lo; }
  }

  [StructLayout(LayoutKind.Sequential)]
  struct ThreadEntry {
    public uint Size, Usage, ThreadId, OwnerProcessId, BasePriority, DeltaPriority, Flags;
  }

  [DllImport("kernel32.dll", SetLastError=true)]
  static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool GetProcessTimes(IntPtr handle, out FileTime creation, out FileTime exit, out FileTime kernel, out FileTime user);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool Thread32First(IntPtr snapshot, ref ThreadEntry entry);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool Thread32Next(IntPtr snapshot, ref ThreadEntry entry);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern uint GetProcessIdOfThread(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern uint SuspendThread(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern uint ResumeThread(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool TerminateProcess(IntPtr handle, uint code);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll")]
  static extern bool CloseHandle(IntPtr handle);

  static void Check(IntPtr handle, string operation) {
    if (handle == IntPtr.Zero || handle == new IntPtr(-1)) {
      throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }
  }

  static ulong Creation(IntPtr handle) {
    FileTime creation, exit, kernel, user;
    if (!GetProcessTimes(handle, out creation, out exit, out kernel, out user)) {
      throw new Win32Exception(Marshal.GetLastWin32Error(), "GetProcessTimes");
    }
    return creation.Value();
  }

  static bool ResumeAll(Dictionary<uint, IntPtr> held, bool injectFailure) {
    bool incomplete = false;
    foreach (var pair in held) {
      if (injectFailure) {
        injectFailure = false;
        incomplete = true;
        continue;
      }
      if (ResumeThread(pair.Value) == Invalid) incomplete = true;
    }
    return incomplete;
  }

  static int AcquireRound(uint pid, Dictionary<uint, IntPtr> held, int failAfter) {
    IntPtr snapshot = CreateToolhelp32Snapshot(SnapshotThread, 0);
    Check(snapshot, "CreateToolhelp32Snapshot");
    try {
      var entry = new ThreadEntry();
      entry.Size = (uint)Marshal.SizeOf(typeof(ThreadEntry));
      if (!Thread32First(snapshot, ref entry)) {
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Thread32First");
      }
      int added = 0;
      while (true) {
        if (entry.OwnerProcessId == pid && !held.ContainsKey(entry.ThreadId)) {
          IntPtr thread = OpenThread(ThreadSuspendResume | ThreadQueryLimited, false, entry.ThreadId);
          Check(thread, "OpenThread");
          if (GetProcessIdOfThread(thread) != pid) {
            CloseHandle(thread);
            throw new InvalidOperationException("thread ownership mismatch");
          }
          if (SuspendThread(thread) == Invalid) {
            int error = Marshal.GetLastWin32Error();
            CloseHandle(thread);
            throw new Win32Exception(error, "SuspendThread");
          }
          held.Add(entry.ThreadId, thread);
          added++;
          if (failAfter > 0 && held.Count >= failAfter) {
            throw new InvalidOperationException("injected partial-suspension failure");
          }
        }
        if (Thread32Next(snapshot, ref entry)) continue;
        int nextError = Marshal.GetLastWin32Error();
        if (nextError == NoMoreFiles) return added;
        throw new Win32Exception(nextError, "Thread32Next");
      }
    } finally {
      CloseHandle(snapshot);
    }
  }

  public static int Run(uint pid, ulong expected, int timeout, int rounds, int failAfter, bool injectResumeFailure) {
    IntPtr process = OpenProcess(QueryLimited | Synchronize, false, pid);
    Check(process, "OpenProcess");
    var held = new Dictionary<uint, IntPtr>();
    bool resumeAttempted = false;
    try {
      if (Creation(process) != expected) {
        throw new InvalidOperationException("owned process creation identity mismatch");
      }
      bool stable = false;
      for (int round = 0; round < rounds && !stable; round++) {
        stable = AcquireRound(pid, held, failAfter) == 0;
      }
      if (!stable || held.Count == 0) {
        throw new InvalidOperationException("owned thread set did not stabilize");
      }
      Console.WriteLine("SUSPENDED {0} THREADS", held.Count);
      Console.Out.Flush();
      var input = Task.Run(() => Console.ReadLine());
      if (!input.Wait(timeout)) {
        throw new TimeoutException("resume handshake timeout after " + timeout + "ms");
      }
      var command = input.Result;
      if (command == null) {
        throw new InvalidOperationException("resume handshake stdin closed before a command arrived");
      }
      if (!String.Equals(command, "resume", StringComparison.OrdinalIgnoreCase)) {
        throw new InvalidOperationException("resume handshake received an unexpected command");
      }
      if (WaitForSingleObject(process, 0) == WaitObject0) {
        Console.WriteLine("ALREADY_GONE");
        return 3;
      }
      resumeAttempted = true;
      if (ResumeAll(held, injectResumeFailure)) {
        Console.Error.WriteLine("ROLLBACK_INCOMPLETE");
        return 2;
      }
      return 0;
    } catch {
      if (!resumeAttempted && ResumeAll(held, false)) {
        Console.Error.WriteLine("ROLLBACK_INCOMPLETE");
      }
      throw;
    } finally {
      foreach (var thread in held.Values) CloseHandle(thread);
      CloseHandle(process);
    }
  }

  public static int Cleanup(uint pid, ulong expected) {
    IntPtr process = OpenProcess(QueryLimited | ProcessTerminate | Synchronize, false, pid);
    if (process == IntPtr.Zero) {
      int error = Marshal.GetLastWin32Error();
      if (error == 87 && pid != 0) {
        Console.WriteLine("ALREADY_GONE");
        return 0;
      }
      throw new Win32Exception(error, "OpenProcess cleanup");
    }
    try {
      if (Creation(process) != expected || WaitForSingleObject(process, 0) == WaitObject0) {
        Console.WriteLine("ALREADY_GONE");
        return 0;
      }
      if (!TerminateProcess(process, 1)) {
        int error = Marshal.GetLastWin32Error();
        if (WaitForSingleObject(process, 0) != WaitObject0) {
          throw new Win32Exception(error, "TerminateProcess");
        }
      }
      if (WaitForSingleObject(process, 10000) != WaitObject0) {
        throw new TimeoutException("owned process termination did not complete");
      }
      Console.WriteLine("TERMINATED");
      return 0;
    } finally {
      CloseHandle(process);
    }
  }
}
"@
try {
  if ($Cleanup) {
    $result = [PerlLspOwnedSuspend]::Cleanup([uint32]$ProcessId, $CreationTimeFileTime)
  } else {
    $result = [PerlLspOwnedSuspend]::Run([uint32]$ProcessId, $CreationTimeFileTime, $TimeoutMilliseconds, $MaxRounds, $InjectFailureAfter, $InjectResumeFailure.IsPresent)
  }
  exit $result
} catch {
  Write-Error $_
  exit 1
}
