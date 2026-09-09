param(
  [Parameter(Mandatory=$true)][ValidateSet('suspend','resume')][string]$Action,
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [Parameter(Mandatory=$true)][UInt64]$CreationTimeFileTime
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class PerlLspOwnedSuspend {
  const uint QUERY_LIMITED = 0x1000, THREAD_SUSPEND_RESUME = 0x0002, THREAD_QUERY_LIMITED = 0x0800;
  const uint SNAP_THREAD = 0x00000004, INVALID = 0xffffffff;
  [StructLayout(LayoutKind.Sequential)] struct FILETIME { public uint Lo; public uint Hi; public ulong GetValue() { return ((ulong)Hi << 32) | Lo; } }
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
  public static ulong Creation(uint pid) { var p=OpenProcess(QUERY_LIMITED,false,pid); Check(p,"OpenProcess"); try { FILETIME c,e,k,u; if(!GetProcessTimes(p,out c,out e,out k,out u)) throw new Win32Exception(Marshal.GetLastWin32Error(),"GetProcessTimes"); return c.GetValue(); } finally { CloseHandle(p); } }
  public static string Change(uint pid, ulong expected, bool suspend) {
    if (Creation(pid) != expected) throw new InvalidOperationException("owned process creation identity mismatch");
    var snap=CreateToolhelp32Snapshot(SNAP_THREAD,0); Check(snap,"CreateToolhelp32Snapshot"); var held=new List<IntPtr>();
    try { var e=new THREADENTRY32(); int changed=0; e.Size=(uint)Marshal.SizeOf(typeof(THREADENTRY32)); if(!Thread32First(snap,ref e)) throw new Win32Exception(Marshal.GetLastWin32Error(),"Thread32First"); do {
      if(e.OwnerProcessId==pid) { var t=OpenThread(THREAD_SUSPEND_RESUME|THREAD_QUERY_LIMITED,false,e.ThreadId); Check(t,"OpenThread"); if(GetProcessIdOfThread(t)!=pid) { CloseHandle(t); throw new InvalidOperationException("thread ownership mismatch"); } if(suspend) { if(SuspendThread(t)==INVALID) { CloseHandle(t); throw new Win32Exception(Marshal.GetLastWin32Error(),"SuspendThread"); } held.Add(t); changed++; } else { if(ResumeThread(t)==INVALID) { CloseHandle(t); throw new Win32Exception(Marshal.GetLastWin32Error(),"ResumeThread"); } changed++; CloseHandle(t); } }
    } while(Thread32Next(snap,ref e)); if(changed==0) throw new InvalidOperationException("owned process has no suspendable threads"); return String.Format("{0} {1} threads pid {2}", suspend ? "suspended" : "resumed", changed, pid); }
    catch { foreach(var t in held) { try { ResumeThread(t); } catch {} CloseHandle(t); } throw; } finally { foreach(var t in held) CloseHandle(t); CloseHandle(snap); }
  }
}
"@
try { $result=[PerlLspOwnedSuspend]::Change([uint32]$ProcessId,$CreationTimeFileTime,($Action -eq 'suspend')); Write-Output $result; exit 0 }
catch { Write-Error $_; exit 1 }


