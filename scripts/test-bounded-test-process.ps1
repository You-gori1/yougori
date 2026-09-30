#requires -Version 7.0
# Regression tests use only disposable PowerShell processes. No VMs,
# containers, existing processes, network publications or user files are used.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib/bounded-test-process.ps1')

function New-MockInfo([string]$Command) {
    $info = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    foreach ($arg in @('-NoProfile', '-NonInteractive', '-Command', $Command)) { $info.ArgumentList.Add($arg) }
    return $info
}
function Assert([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}

$flood = Invoke-BoundedTestProcess -StartInfo (New-MockInfo @'
[Console]::Out.WriteLine('test result: ok. 1 passed; 0 failed; 0 ignored')
[Console]::Out.Write(('x' * 1048576))
[Console]::Error.Write(('e' * 1048576))
[Console]::Out.WriteLine('OUT_TAIL')
[Console]::Error.WriteLine('ERR_TAIL')
'@) -TimeoutSeconds 10 -TailCharacters 4096 -Quiet
Assert ($flood.ExitCode -eq 0 -and !$flood.TimedOut) 'Flood fixture failed.'
Assert ($flood.SuccessMarker) 'Success marker was lost after output truncation.'
Assert ($flood.OutputTruncated) 'Flood output was not marked truncated.'
Assert ($flood.StdoutCharacters -gt 1MB -and $flood.StderrCharacters -gt 1MB) 'Both output streams were not drained.'
Assert ($flood.Output.Length -lt 10000) 'Retained output exceeded its fixed budget.'
Assert ($flood.Output.Contains('OUT_TAIL') -and $flood.Output.Contains('ERR_TAIL')) 'Latest diagnostic output was lost.'
Write-Output 'PASS bounded stdout/stderr tails and sticky success marker'

$split = Invoke-BoundedTestProcess -StartInfo (New-MockInfo @'
[Console]::Out.Write('test result: ok. 1 pas')
Start-Sleep -Milliseconds 100
[Console]::Out.WriteLine('sed; 0 failed; 0 ignored')
'@) -TimeoutSeconds 10 -Quiet
Assert ($split.SuccessMarker -and $split.ExitCode -eq 0) 'Marker spanning read chunks was not recognized.'
Write-Output 'PASS split output marker'

$failure = Invoke-BoundedTestProcess -StartInfo (New-MockInfo "[Console]::Out.WriteLine('1 passed; 0 failed'); exit 7") -TimeoutSeconds 10 -Quiet
Assert ($failure.ExitCode -eq 7 -and !$failure.TimedOut) 'Nonzero exit code was lost.'
Write-Output 'PASS nonzero exit preserved despite success-looking output'

$clock = [Diagnostics.Stopwatch]::StartNew()
$hung = Invoke-BoundedTestProcess -StartInfo (New-MockInfo 'Start-Sleep -Seconds 20') -TimeoutSeconds 1 -Quiet
Assert ($hung.TimedOut -and $hung.TimeoutPhase -eq 'process') 'Hanging process did not time out.'
Assert ($clock.Elapsed.TotalSeconds -lt 5) 'Process timeout exceeded its bounded cleanup grace.'
Write-Output 'PASS whole-test process timeout'

# The owned mock exits while a short-lived child holds its inherited output
# pipe. A process-only timeout would hang waiting for ReadToEndAsync here.
$clock.Restart()
$drain = Invoke-BoundedTestProcess -StartInfo (New-MockInfo @'
$childInfo = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
$childInfo.UseShellExecute = $false
$childInfo.CreateNoWindow = $true
foreach ($arg in @('-NoProfile', '-NonInteractive', '-Command', "[Console]::Out.WriteLine('owned pipe holder'); Start-Sleep -Seconds 3")) { $childInfo.ArgumentList.Add($arg) }
$child = [Diagnostics.Process]::Start($childInfo)
[Console]::Out.WriteLine('mock parent exiting')
exit 0
'@) -TimeoutSeconds 1 -Quiet
Assert ($drain.TimedOut -and $drain.TimeoutPhase -eq 'output-drain') 'Inherited pipe drain was not covered by the timeout.'
Assert ($clock.Elapsed.TotalSeconds -lt 2.5) 'Post-exit pipe disposal blocked past the deadline.'
Write-Output 'PASS post-exit inherited pipe timeout'

$progressMessages = @()
$progress = Invoke-BoundedTestProcess -StartInfo (New-MockInfo @'
[Console]::Out.WriteLine('first progress')
Start-Sleep -Milliseconds 700
[Console]::Out.WriteLine('second progress')
Start-Sleep -Milliseconds 700
[Console]::Out.WriteLine('test result: ok. 1 passed; 0 failed; 0 ignored')
'@) -TimeoutSeconds 10 -InformationVariable progressMessages
Assert ($progress.ExitCode -eq 0 -and $progress.SuccessMarker) 'Progress fixture failed.'
Assert ($progressMessages.Count -ge 2) 'Progress was not emitted incrementally.'
Write-Output 'PASS live progress'
Write-Output '6 bounded process-runner regressions passed.'
