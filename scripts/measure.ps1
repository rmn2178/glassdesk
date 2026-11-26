<#
.SYNOPSIS
  Measures Nosih resource usage.

.DESCRIPTION
  Starts a release build of the dock, measures startup time, then records memory,
  CPU, threads, handles, and GDI/USER objects for the idle dock and watchdog.
  The script closes the dock afterward so it restores the taskbar.

  The script does not move the mouse or take focus, so it can run while the
  computer is in use. CPU measurements are more reliable while the computer is idle.

.EXAMPLE
  cargo build --release -p nosih; ./scripts/measure.ps1
#>
param(
    [string]$Exe = (Join-Path $env:USERPROFILE '.cargo-target\nosih\release\nosih.exe'),
    [int]$IdleSeconds = 10,
    # Startup work (icon cache, taskbar, status watches) settles over the first
    # seconds; sampling earlier counts it as idle use.
    [int]$WarmSeconds = 20
)
$ErrorActionPreference = 'Stop'

Add-Type @'
using System; using System.Runtime.InteropServices;
public static class NosihMeasure {
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr p, uint flags);
}
'@

if (-not (Test-Path $Exe)) { throw "No existe $Exe. Compila antes: cargo build --release -p nosih" }
if (Get-Process nosih -ErrorAction SilentlyContinue) { throw 'Ya hay un dock en marcha: ciérralo con `nosih --quit`.' }

$clock = [Diagnostics.Stopwatch]::StartNew()
Start-Process $Exe -WindowStyle Hidden | Out-Null
$visibleMs = $null
while ($clock.ElapsedMilliseconds -lt 10000) {
    $h = [NosihMeasure]::FindWindow('NosihWindow', [NullString]::Value)
    if ($h -ne [IntPtr]::Zero -and [NosihMeasure]::IsWindowVisible($h)) { $visibleMs = $clock.ElapsedMilliseconds; break }
    Start-Sleep -Milliseconds 5
}

Start-Sleep -Seconds $WarmSeconds   # Let startup and memory trimming finish.
$procs = Get-CimInstance Win32_Process -Filter "Name='nosih.exe'"
$cpuBefore = @{}
foreach ($p in $procs) { $cpuBefore[$p.ProcessId] = (Get-Process -Id $p.ProcessId).TotalProcessorTime }
Start-Sleep -Seconds $IdleSeconds

$rows = foreach ($p in $procs) {
    $proc = Get-Process -Id $p.ProcessId
    $role = if ($p.CommandLine -match '--guard') { 'guardián' } else { 'dock' }
    $pws = (Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$($p.ProcessId)").WorkingSetPrivate
    [pscustomobject]@{
        Proceso               = $role
        'Privada (MB)'        = [math]::Round($proc.PrivateMemorySize64 / 1MB, 1)
        'WS privado (MB)'     = [math]::Round($pws / 1MB, 1)
        'Working set (MB)'    = [math]::Round($proc.WorkingSet64 / 1MB, 1)
        'CPU en reposo (ms)'  = [math]::Round(($proc.TotalProcessorTime - $cpuBefore[$p.ProcessId]).TotalMilliseconds, 1)
        Hilos                 = $proc.Threads.Count
        Handles               = $proc.HandleCount
        GDI                   = [NosihMeasure]::GetGuiResources($proc.Handle, 0)
        USER                  = [NosihMeasure]::GetGuiResources($proc.Handle, 1)
    }
}

"Arranque hasta dock visible: $(if ($visibleMs) { "$visibleMs ms" } else { 'no apareció en 10 s' })"
"Reposo medido: $IdleSeconds s"
$rows | Format-Table -AutoSize | Out-String -Width 200
"Privada = memoria comprometida solo por el proceso (la métrica real)."
"WS privado = lo que muestra el Administrador de tareas en 'Memoria'."

& $Exe --quit
Start-Sleep -Seconds 2
