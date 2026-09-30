param(
  [ValidateRange(1, 25)]
  [int]$Samples = 5,
  [switch]$Json
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$runtimeRoot = Join-Path $repositoryRoot "src-tauri\resources\runtime"
$qemuRoot = Join-Path $runtimeRoot "qemu"
$applianceRoot = Join-Path $runtimeRoot "appliance"
$qemuSystem = Join-Path $qemuRoot "qemu-system-x86_64.exe"

function Get-TreeStats([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    return [pscustomobject]@{ Files = 0; Bytes = 0; MiB = 0.0 }
  }
  $files = @(Get-ChildItem -LiteralPath $Path -File -Recurse)
  $bytes = [long](($files | Measure-Object -Property Length -Sum).Sum)
  [pscustomobject]@{
    Files = $files.Count
    Bytes = $bytes
    MiB = [math]::Round($bytes / 1MB, 2)
  }
}

function Measure-ManifestVerification([string]$Root, [string]$Manifest) {
  $elapsed = Measure-Command {
    foreach ($line in Get-Content -LiteralPath $Manifest) {
      if (-not $line.Trim()) { continue }
      $parts = $line -split '\s+', 2
      $path = Join-Path $Root $parts[1].Trim().TrimStart('*')
      $algorithm = [Security.Cryptography.SHA256]::Create()
      $stream = [IO.File]::OpenRead($path)
      try {
        $null = $algorithm.ComputeHash($stream)
      } finally {
        $stream.Dispose()
        $algorithm.Dispose()
      }
    }
  }
  $elapsed.TotalMilliseconds
}

function Measure-ManifestMetadataScan([string]$Root, [string]$Manifest) {
  $elapsed = Measure-Command {
    $manifestBytes = [IO.File]::ReadAllBytes($Manifest)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
      $null = $algorithm.ComputeHash($manifestBytes)
    } finally {
      $algorithm.Dispose()
    }
    foreach ($line in Get-Content -LiteralPath $Manifest) {
      if (-not $line.Trim()) { continue }
      $parts = $line -split '\s+', 2
      $item = Get-Item -LiteralPath (Join-Path $Root $parts[1].Trim().TrimStart('*'))
      $null = $item.Length
      $null = $item.CreationTimeUtc.Ticks
      $null = $item.LastWriteTimeUtc.Ticks
    }
  }
  $elapsed.TotalMilliseconds
}

function Get-Median([double[]]$Values) {
  $sorted = @($Values | Sort-Object)
  $middle = [math]::Floor($sorted.Count / 2)
  if ($sorted.Count % 2 -eq 1) { return $sorted[$middle] }
  ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

$qemuStats = Get-TreeStats $qemuRoot
$applianceStats = Get-TreeStats $applianceRoot
$frontendStats = Get-TreeStats (Join-Path $repositoryRoot "dist")
$fullVerificationMilliseconds =
  (Measure-ManifestVerification $qemuRoot (Join-Path $qemuRoot "SHA256SUMS")) +
  (Measure-ManifestVerification $applianceRoot (Join-Path $applianceRoot "SHA256SUMS"))
$metadataScanMilliseconds =
  (Measure-ManifestMetadataScan $qemuRoot (Join-Path $qemuRoot "SHA256SUMS")) +
  (Measure-ManifestMetadataScan $applianceRoot (Join-Path $applianceRoot "SHA256SUMS"))

$qemuStartupSamples = @()
if (Test-Path -LiteralPath $qemuSystem) {
  for ($index = 0; $index -lt $Samples; $index++) {
    $elapsed = Measure-Command { & $qemuSystem --version 2>$null | Out-Null }
    $qemuStartupSamples += $elapsed.TotalMilliseconds
  }
}

$appDataRoot = Join-Path $env:APPDATA "com.yougori.desktop"
$appDataStats = Get-TreeStats $appDataRoot
$result = [ordered]@{
  measuredAt = [DateTimeOffset]::Now.ToString("o")
  qemuBundleMiB = $qemuStats.MiB
  applianceBundleMiB = $applianceStats.MiB
  totalRuntimeMiB = [math]::Round($qemuStats.MiB + $applianceStats.MiB, 2)
  frontendMiB = $frontendStats.MiB
  # The desktop uses this metadata-bound path after one complete verification.
  runtimeMetadataWarmScanMs = [math]::Round($metadataScanMilliseconds, 2)
  # Kept visible as the expected one-time cost after install or payload changes.
  runtimeFullChecksumScanMs = [math]::Round($fullVerificationMilliseconds, 2)
  qemuProcessStartupMedianMs = if ($qemuStartupSamples.Count) { [math]::Round((Get-Median $qemuStartupSamples), 2) } else { $null }
  qemuProcessStartupSamplesMs = @($qemuStartupSamples | ForEach-Object { [math]::Round($_, 2) })
  localRuntimeDataMiB = $appDataStats.MiB
}

if ($Json) {
  $result | ConvertTo-Json -Depth 4
} else {
  [pscustomobject]$result | Format-List
}
