param(
  [Parameter(Mandatory=$true)][ValidateSet('proof','failure')][string]$Action,
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [Parameter(Mandatory=$true)][UInt64]$CreationTimeFileTime,
  [Parameter(Mandatory=$true)][string]$HeartbeatPath,
  [Parameter(Mandatory=$true)][Int64]$BaselineLength
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
public static class PerlLspOwnedSuspend {
  const uint QUERY_LIMITED = 0x1000, THREAD_SUSPEND_RESUME = 0x0002, THREAD_QUERY_LIMITED = 0x0800;
  const uint SNAP_THREAD = 0x00000004, INVALID = 0xffffffff, NO_MORE_FILES = 18;
  [StructLayout(LayoutKind.Sequential)] struct FILETIME { public uint Lo; public uint Hi; public ulong Value() { return ((ulong)Hi << 32) | Lo; } }
  [StructLayout(LayoutKind.Sequential)] struct THREADENTRY32 { public uint Size, Usage, ThreadId, OwnerProcessId, BasePriority, DeltaPriority, Flags; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr h, out FILETIME create, out FILETIME exit, out FILETIME kernel, out FILETIME user);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool Thread32First(IntPtr snap, ref THREADENTRY32 entry);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool Thread32Next(IntPtr snap, ref THREADENTRY32 entry);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetProcessIdOfThread(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint SuspendThread(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  static void Check(IntPtr h, string what) { if (h == IntPtr.Zero || h == new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error(), what); }
  static ulong Creation(IntPtr process) { FILETIME c,e,k,u; if(!GetProcessTimes(process,out c,out e,out k,out u)) throw new Win32Exception(Marshal.GetLastWin32Error(),"GetProcessTimes"); return c.Value(); }
  public static string Run(uint pid, ulong expected, string heartbeat, long baseline, bool injectFailure) {
    IntPtr process=OpenProcess(QUERY_LIMITED,false,pid); Check(process,"OpenProcess"); var held=new List<IntPtr>(); var resumed=new List<bool>(); IntPtr snap=IntPtr.Zero;
    try {
      if(Creation(process)!=expected) throw new InvalidOperationException("owned process creation identity mismatch");
      snap=CreateToolhelp32Snapshot(SNAP_THREAD,0); Check(snap,"CreateToolhelp32Snapshot"); var e=new THREADENTRY32(); e.Size=(uint)Marshal.SizeOf(typeof(THREADENTRY32));
      if(!Thread32First(snap,ref e)) throw new Win32Exception(Marshal.GetLastWin32Error(),"Thread32First");
      while (true) { if(e.OwnerProcessId==pid) { var t=OpenThread(THREAD_SUSPEND_RESUME|THREAD_QUERY_LIMITED,false,e.ThreadId); Check(t,"OpenThread"); if(GetProcessIdOfThread(t)!=pid) { CloseHandle(t); throw new InvalidOperationException("thread ownership mismatch"); } if(SuspendThread(t)==INVALID) { CloseHandle(t); throw new Win32Exception(Marshal.GetLastWin32Error(),"SuspendThread"); } held.Add(t); resumed.Add(false); if(injectFailure && held.Count==1) throw new InvalidOperationException("injected partial-suspension failure"); } if(Thread32Next(snap,ref e)) continue; int error=Marshal.GetLastWin32Error(); if(error==NO_MORE_FILES) break; throw new Win32Exception(error,"Thread32Next"); }
      if(held.Count==0) throw new InvalidOperationException("owned process has no suspendable threads");
      Thread.Sleep(1500); long during=new FileInfo(heartbeat).Length; if(during!=baseline) throw new InvalidOperationException(String.Format("heartbeat advanced while suspended: {0} -> {1}",baseline,during));
      for(int i=0;i<held.Count;i++) { if(ResumeThread(held[i])==INVALID) throw new Win32Exception(Marshal.GetLastWin32Error(),"ResumeThread"); resumed[i]=true; }
      return String.Format("suspended and resumed {0} owned threads pid {1}; heartbeat stable",held.Count,pid);
    } catch { for(int i=0;i<held.Count;i++) { if(!resumed[i]) { try { ResumeThread(held[i]); } catch {} } } throw; }
    finally { foreach(var t in held) CloseHandle(t); if(snap!=IntPtr.Zero) CloseHandle(snap); CloseHandle(process); }
  }
}
"@
try { $result=[PerlLspOwnedSuspend]::Run([uint32]$ProcessId,$CreationTimeFileTime,$HeartbeatPath,$BaselineLength,($Action -eq 'failure')); Write-Output $result; exit 0 }
catch { Write-Error $_; exit 1 }
