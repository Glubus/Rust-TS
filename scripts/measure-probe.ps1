# Build first: cargo build --release --example operational_probe
# Samples the probe process during its explicitly announced idle phase.
$ErrorActionPreference = 'Stop'
$probePath = Join-Path $PSScriptRoot '../target/release/examples/operational_probe.exe'
$probePath = (Resolve-Path -LiteralPath $probePath).Path
$startInfo = [System.Diagnostics.ProcessStartInfo]::new($probePath)
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$probeProcess = [System.Diagnostics.Process]::Start($startInfo)
try {
    $idleSample = $null
    while ($null -ne ($line = $probeProcess.StandardError.ReadLine())) {
        if (-not $line.StartsWith('idle measurement:')) {
            Write-Warning $line
            continue
        }
        $probeProcess.Refresh()
        $cpuBefore = $probeProcess.TotalProcessorTime.TotalMilliseconds
        $rssBefore = $probeProcess.WorkingSet64
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Milliseconds 1500
        $probeProcess.Refresh()
        $elapsedMs = $stopwatch.Elapsed.TotalMilliseconds
        $cpuMs = $probeProcess.TotalProcessorTime.TotalMilliseconds - $cpuBefore
        $idleSample = @{
            wall_ms = $elapsedMs
            cpu_ms = $cpuMs
            percent_one_core = 100 * $cpuMs / $elapsedMs
            rss_before_bytes = $rssBefore
            rss_after_bytes = $probeProcess.WorkingSet64
        }
    }
    $report = $probeProcess.StandardOutput.ReadToEnd() | ConvertFrom-Json
    $probeProcess.WaitForExit()
    if ($probeProcess.ExitCode -ne 0) { throw "Probe failed: $($probeProcess.ExitCode)" }
    @{ probe = $report; windows_idle_sample = $idleSample } | ConvertTo-Json -Depth 10
} finally {
    $probeProcess.Dispose()
}
