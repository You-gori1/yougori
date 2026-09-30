#requires -Version 7.0
# Test-only process runner. Does not discover or stop any external runtime.
if (-not ('Yougori.RuntimeTesting.BoundedOutput' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace Yougori.RuntimeTesting {
    public sealed class BoundedOutput {
        private const string Marker = "1 passed; 0 failed";
        private readonly object gate = new object();
        private readonly StreamReader reader;
        private readonly CancellationTokenSource cancellation = new CancellationTokenSource();
        private readonly LinkedList<string> tail = new LinkedList<string>();
        private readonly LinkedList<string> progress = new LinkedList<string>();
        private readonly int limit;
        private int tailLength, progressLength;
        private long count, progressDropped;
        private int stopped;
        private bool success;
        private string markerSuffix = "", error;
        public Task Completion { get; private set; }

        public BoundedOutput(StreamReader reader, int limit) {
            this.reader = reader;
            this.limit = limit;
            Completion = Read();
        }
        private static int Append(LinkedList<string> parts, ref int length, string text, int capacity) {
            parts.AddLast(text);
            length += text.Length;
            int removed = Math.Max(0, length - capacity);
            int excess = removed;
            while (excess > 0) {
                string first = parts.First.Value;
                if (first.Length <= excess) {
                    parts.RemoveFirst();
                    excess -= first.Length;
                } else {
                    parts.First.Value = first.Substring(excess);
                    excess = 0;
                }
            }
            length -= removed;
            return removed;
        }
        private async Task Read() {
            var buffer = new char[4096];
            try {
                while (!cancellation.IsCancellationRequested) {
                    int size = await reader.ReadAsync(buffer.AsMemory(), cancellation.Token).ConfigureAwait(false);
                    if (size == 0 || cancellation.IsCancellationRequested) break;
                    string text = new string(buffer, 0, size);
                    lock (gate) {
                        count += size;
                        string scan = markerSuffix + text;
                        success |= scan.IndexOf(Marker, StringComparison.Ordinal) >= 0;
                        markerSuffix = scan.Substring(Math.Max(0, scan.Length - Marker.Length + 1));
                        Append(tail, ref tailLength, text, limit);
                        progressDropped += Append(progress, ref progressLength, text, 8192);
                    }
                }
            } catch (OperationCanceledException) { }
              catch (ObjectDisposedException) { }
              catch (Exception exception) {
                if (!cancellation.IsCancellationRequested) lock (gate) { error = exception.Message; }
            }
        }
        public string Tail { get { lock (gate) return String.Concat(tail); } }
        public long Characters { get { lock (gate) return count; } }
        public bool SuccessMarker { get { lock (gate) return success; } }
        public string Error { get { lock (gate) return error; } }
        public string DrainProgress() {
            lock (gate) {
                string text = String.Concat(progress);
                if (progressDropped > 0) text = "[" + progressDropped + " earlier progress characters omitted]\n" + text;
                progress.Clear(); progressLength = 0; progressDropped = 0;
                return text;
            }
        }
        public void Stop() {
            if (Interlocked.Exchange(ref stopped, 1) != 0) return;
            cancellation.Cancel();
            // Some redirected OS pipes cannot cancel an in-flight read. Never
            // let their disposal hold the harness past its wall-clock deadline.
            ThreadPool.QueueUserWorkItem(_ => { try { reader.Dispose(); } catch { } });
        }
        public static void DisposeProcess(Process process) {
            ThreadPool.QueueUserWorkItem(_ => { try { process.Dispose(); } catch { } });
        }
    }
}
'@
}

function Invoke-BoundedTestProcess {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][Diagnostics.ProcessStartInfo]$StartInfo,
        [Parameter(Mandatory)][ValidateRange(1, 7200)][int]$TimeoutSeconds,
        [ValidateRange(1024, 4194304)][int]$TailCharacters = 1048576,
        [string]$Label = 'native test',
        [switch]$Quiet
    )
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    $started = [Diagnostics.Stopwatch]::StartNew()
    $process = [Diagnostics.Process]::Start($StartInfo)
    $stdout = [Yougori.RuntimeTesting.BoundedOutput]::new($process.StandardOutput, $TailCharacters)
    $stderr = [Yougori.RuntimeTesting.BoundedOutput]::new($process.StandardError, $TailCharacters)
    $timedOut = $false
    $timeoutPhase = $null
    $lastProgress = 0.0
    $lastHeartbeat = 0.0
    $cleanupError = $null
    try {
        while ($true) {
            $exited = $process.HasExited
            $drained = $stdout.Completion.IsCompleted -and $stderr.Completion.IsCompleted
            $elapsed = $started.Elapsed.TotalSeconds
            if (!$Quiet -and ($elapsed - $lastProgress -ge 0.5 -or ($exited -and $drained))) {
                foreach ($stream in @(@{ Name = 'stdout'; Capture = $stdout }, @{ Name = 'stderr'; Capture = $stderr })) {
                    $live = $stream.Capture.DrainProgress()
                    if ($live.Length) { Write-Host "[$Label $($stream.Name)] $($live.TrimEnd())" }
                }
                $lastProgress = $elapsed
            }
            if ($exited -and $drained) { break }
            if ($elapsed -ge $TimeoutSeconds) {
                $timedOut = $true
                $timeoutPhase = if ($exited) { 'output-drain' } else { 'process' }
                if (!$exited) {
                    # Only this explicitly started test and its descendants.
                    try { $process.Kill($true) } catch { $cleanupError = $_.Exception.Message }
                    [void]$process.WaitForExit(2000)
                }
                break
            }
            if (!$Quiet -and $elapsed - $lastHeartbeat -ge 5) {
                $phase = if ($exited) { 'draining child output' } else { 'running' }
                Write-Host "PROGRESS $Label $phase ($([math]::Round($elapsed, 1))s / ${TimeoutSeconds}s)"
                $lastHeartbeat = $elapsed
            }
            Start-Sleep -Milliseconds 100
        }
        $exitCode = if ($process.HasExited) { $process.ExitCode } else { $null }
        $stdout.Stop()
        $stderr.Stop()
        $prefix = ''
        if ($stdout.Characters -gt $TailCharacters -or $stderr.Characters -gt $TailCharacters) {
            $prefix = "[Output truncated: retained the last $TailCharacters characters per stream. stdout=$($stdout.Characters), stderr=$($stderr.Characters).]`n"
        }
        if ($timedOut) { $prefix += "[Whole-test timeout during $timeoutPhase. No unrelated process was stopped.]`n" }
        $captureError = (@($stdout.Error, $stderr.Error) | Where-Object { $_ }) -join '; '
        return [pscustomobject]@{
            TimedOut = $timedOut
            TimeoutPhase = $timeoutPhase
            ExitCode = $exitCode
            Seconds = [math]::Round($started.Elapsed.TotalSeconds, 2)
            SuccessMarker = $stdout.SuccessMarker -or $stderr.SuccessMarker
            CaptureError = $captureError
            CleanupError = $cleanupError
            StdoutCharacters = $stdout.Characters
            StderrCharacters = $stderr.Characters
            OutputTruncated = $stdout.Characters -gt $TailCharacters -or $stderr.Characters -gt $TailCharacters
            Output = $prefix + "--- stdout ---`n" + $stdout.Tail + "`n--- stderr ---`n" + $stderr.Tail
        }
    } finally {
        $stdout.Stop()
        $stderr.Stop()
        if (!$process.HasExited) {
            try { $process.Kill($true); [void]$process.WaitForExit(2000) } catch { }
        }
        [Yougori.RuntimeTesting.BoundedOutput]::DisposeProcess($process)
    }
}
