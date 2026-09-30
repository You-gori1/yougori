[CmdletBinding()]
param([Parameter(Mandatory)][string]$AngleDirectory)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath "$AngleDirectory/ANGLE_BUILD.json")) {
  throw 'Build and inspect D3D11 ANGLE first: python scripts/build-angle-runtime.py. See docs/rebuilding-third-party.txt.'
}
$directory = (Resolve-Path -LiteralPath $AngleDirectory).Path
$record = Get-Content -LiteralPath "$directory/ANGLE_BUILD.json" -Raw | ConvertFrom-Json
if ($record.schemaVersion -ne 1 -or $record.profile -ne 'angle-d3d11-only' -or
    $record.revision -ne '890b5d8fa2988e3719e0d80421bf3e927db9cd5c' -or
    $record.fontPreprocessing.dataPresent -ne $false -or $record.inputs.Count -lt 1) {
  throw 'ANGLE must be built and inspected with scripts/build-angle-runtime.py.'
}
foreach ($name in @('libEGL.dll', 'libGLESv2.dll')) {
  $entry = @($record.binaries | Where-Object file -eq $name)
  if ($entry.Count -ne 1 -or (Get-FileHash -LiteralPath "$directory/$name" -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry[0].sha256) {
    throw "ANGLE build identity mismatch: $name"
  }
}
$noticeText = [IO.File]::ReadAllText("$directory/ANGLE-NOTICES.txt").Replace("`r`n", "`n").Replace("`r", "`n")
$noticeHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($noticeText))).ToLowerInvariant()
if ($noticeHash -ne $record.notices.sha256) { throw 'ANGLE third-party notice identity mismatch.' }
Write-Output $directory
