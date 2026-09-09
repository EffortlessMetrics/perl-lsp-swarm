param(
  [Parameter(Mandatory=$true)][int]$ProcessId,
  [Parameter(Mandatory=$true)][UInt64]$CreationTimeFileTime,
  [int]$TimeoutMilliseconds = 120000,
  [int]$MaxRounds = 4,
  [int]$InjectFailureAfter = 0,
  [switch]$InjectResumeFailure,
  [switch]$Cleanup
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
public static class PerlLspOwnedSuspend {
  const uint QueryLimited=0x1000, ThreadSuspendResume=0x0002, ThreadQueryLimited=0x0800;
  const uint SnapshotThread=0x00000004, Invalid=0xffffffff, NoMoreFiles=18;
  const uint Synchronize=0x00100000, WaitObject0=0;
  [StructLayout(LayoutKind.Sequential)] struct FileTime { public uint Lo; public uint Hi; public ulong Value() { return ((ulong)Hi << 32) | Lo; } }
  [StructLayout(LayoutKind.Sequential)] struct ThreadEntry { public uint Size, Usage, ThreadId, OwnerProcessId, BasePriority, DeltaPriority, Flags; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint a,bool i,uint p);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr h,out FileTime c,out FileTime e,out FileTime k,out FileTime u);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateToolhelp32Snapshot(uint f,uint p);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool Thread32First(IntPtr s,ref ThreadEntry e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool Thread32Next(IntPtr s,ref ThreadEntry e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenThread(uint a,bool i,uint t);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetProcessIdOfThread(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint SuspendThread(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr h,uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr h,uint milliseconds);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  static void Check(IntPtr h,string what) { if(h==IntPtr.Zero || h==new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error(),what); }
  static ulong Creation(IntPtr h) { FileTime c,e,k,u; if(!GetProcessTimes(h,out c,out e,out k,out u)) throw new Win32Exception(Marshal.GetLastWin32Error(),"GetProcessTimes"); return c.Value(); }
  static void ResumeAll(Dictionary<uint,IntPtr> held, bool inject, ref bool incomplete) {
    bool injected=false;
    foreach(var pair in held) { if(inject && !injected) { injected=true; incomplete=true; continue; } if(ResumeThread(pair.Value)==Invalid) incomplete=true; }
  }
  public static int Run(uint pid,ulong expected,int timeout,int rounds,int failAfter,bool injectResumeFailure) {
    IntPtr process=OpenProcess(QueryLimited,false,pid); Check(process,"OpenProcess"); var held=new Dictionary<uint,IntPtr>(); IntPtr snap=IntPtr.Zero; bool incomplete=false;
    try {
      if(Creation(process)!=expected) throw new InvalidOperationException("owned process creation identity mismatch");
      bool stable=false;
      for(int round=0; round<rounds && !stable; round++) {
        snap=CreateToolhelp32Snapshot(SnapshotThread,0); Check(snap,"CreateToolhelp32Snapshot"); var e=new ThreadEntry(); e.Size=(uint)Marshal.SizeOf(typeof(ThreadEntry));
        if(!Thread32First(snap,ref e)) throw new Win32Exception(Marshal.GetLastWin32Error(),"Thread32First");
        int added=0;
        while(true) {
          if(e.OwnerProcessId==pid && !held.ContainsKey(e.ThreadId)) { var t=OpenThread(ThreadSuspendResume|ThreadQueryLimited,false,e.ThreadId); Check(t,"OpenThread"); if(GetProcessIdOfThread(t)!=pid) { CloseHandle(t); throw new InvalidOperationException("thread ownership mismatch"); } if(SuspendThread(t)==Invalid) { CloseHandle(t); throw new Win32Exception(Marshal.GetLastWin32Error(),"SuspendThread"); } held.Add(e.ThreadId,t); added++; if(failAfter>0 && held.Count>=failAfter) throw new InvalidOperationException("injected partial-suspension failure"); }
          if(Thread32Next(snap,ref e)) continue; int error=Marshal.GetLastWin32Error(); if(error==NoMoreFiles) break; throw new Win32Exception(error,"Thread32Next");
        }
        CloseHandle(snap); snap=IntPtr.Zero; stable=added==0;
      }
      if(!stable || held.Count==0) throw new InvalidOperationException("owned thread set did not stabilize");
      Console.WriteLine(String.Format("SUSPENDED {0} THREADS",held.Count)); Console.Out.Flush();
      var input=Task.Run(()=>Console.ReadLine()); if(!input.Wait(timeout) || !String.Equals(input.Result,"resume",StringComparison.OrdinalIgnoreCase)) throw new TimeoutException("resume handshake timeout or invalid command");
      ResumeAll(held,injectResumeFailure,ref incomplete); if(incomplete) { Console.Error.WriteLine("ROLLBACK_INCOMPLETE"); return 2; }
      return 0;
    } catch { ResumeAll(held,false,ref incomplete); if(incomplete) Console.Error.WriteLine("ROLLBACK_INCOMPLETE"); throw; }
    finally { foreach(var t in held.Values) CloseHandle(t); if(snap!=IntPtr.Zero) CloseHandle(snap); CloseHandle(process); }
  }
  public static int Cleanup(uint pid,ulong expected) {
    IntPtr process=OpenProcess(QueryLimited|0x0001|Synchronize,false,pid); Check(process,"OpenProcess");
    try { if(Creation(process)!=expected) throw new InvalidOperationException("owned process creation identity mismatch"); if(!TerminateProcess(process,1)) throw new Win32Exception(Marshal.GetLastWin32Error(),"TerminateProcess"); if(WaitForSingleObject(process,10000)!=WaitObject0) throw new TimeoutException("owned process termination did not complete"); return 0; }
    finally { CloseHandle(process); }
  }
}
"@
try { $exit=if($Cleanup) { [PerlLspOwnedSuspend]::Cleanup([uint32]$ProcessId,$CreationTimeFileTime) } else { [PerlLspOwnedSuspend]::Run([uint32]$ProcessId,$CreationTimeFileTime,$TimeoutMilliseconds,$MaxRounds,$InjectFailureAfter,$InjectResumeFailure.IsPresent) }; exit $exit }
catch { Write-Error $_; exit 1 }
