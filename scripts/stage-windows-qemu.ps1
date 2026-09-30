[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$OutputDirectory,
  [string]$QemuBuildDirectory = "$PSScriptRoot/../build/secure-runtime/qemu-build",
  [string]$SourceDirectory = "$PSScriptRoot/../build/runtime-cache/qemu-secure-src",
  [string]$AngleDirectory = "$PSScriptRoot/../build/angle-runtime/angle/out/Yougori-D3D11"
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath "$PSScriptRoot/..").Path
$output = [IO.Path]::GetFullPath($OutputDirectory)
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$build = (Resolve-Path -LiteralPath $QemuBuildDirectory).Path
$angle = & "$PSScriptRoot/check-angle-build.ps1" -AngleDirectory $AngleDirectory
if (-not $output.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'Stage runtime files inside this checkout.'
}
if (Test-Path -LiteralPath $output) { throw 'Use a new staging directory; existing runtime files are preserved.' }
$revision = (& git -C $source rev-parse HEAD).Trim()
if ($LASTEXITCODE -or $revision -ne '84f07211cc5b4fc6a371559bf8a5de4fb068e648') { throw 'Unexpected QEMU source revision.' }
New-Item -ItemType Directory -Path $output | Out-Null
$ucrt = "$repo/build/secure-runtime/toolchain/msys64/ucrt64/bin"
$queue = [Collections.Generic.Queue[string]]::new()
foreach ($file in @("$build/qemu-system-x86_64.exe", "$build/qemu-img.exe", "$angle/libEGL.dll", "$angle/libGLESv2.dll")) {
  $queue.Enqueue($file)
}
$seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
while ($queue.Count) {
  $file = $queue.Dequeue()
  $name = [IO.Path]::GetFileName($file)
  if (-not $seen.Add($name)) { continue }
  if (-not (Test-Path -LiteralPath $file)) { throw "Missing runtime dependency: $file" }
  Copy-Item -LiteralPath $file -Destination (Join-Path $output $name)
  $imports = & "$ucrt/objdump.exe" -p $file
  if ($LASTEXITCODE) { throw "Cannot inspect PE dependencies: $file" }
  foreach ($line in $imports) {
    if ($line -match 'DLL Name:\s*(\S+)') {
      $dll = $Matches[1]
      if (Test-Path -LiteralPath "$ucrt/$dll") { $queue.Enqueue("$ucrt/$dll") }
      elseif (-not (Test-Path -LiteralPath "$env:SystemRoot/System32/$dll") -and $dll -notmatch '^api-ms-') {
        throw "Unresolved PE dependency: $dll"
      }
    }
  }
}
foreach ($forbidden in @('libdb-6.2.dll', 'libjack64.dll', 'brlapi-0.8.dll', 'SDL2.dll', 'libssp-0.dll')) {
  if ($seen.Contains($forbidden)) { throw "Unexpected stock runtime dependency: $forbidden" }
}
& "$ucrt/strip.exe" "$output/qemu-system-x86_64.exe" "$output/qemu-img.exe"
if ($LASTEXITCODE) { throw 'Cannot strip runtime executables.' }
$share = Join-Path $output 'share'
New-Item -ItemType Directory -Path "$share/keymaps" -Force | Out-Null
foreach ($name in @('bios-256k.bin', 'bios-microvm.bin', 'bios.bin', 'edk2-licenses.txt', 'efi-e1000e.rom', 'efi-virtio.rom',
    'kvmvapic.bin', 'linuxboot_dma.bin', 'pvh.bin', 'qboot.rom', 'vgabios-bochs-display.bin', 'vgabios-ramfb.bin',
    'vgabios-stdvga.bin', 'vgabios-virtio.bin', 'vgabios.bin')) {
  Copy-Item -LiteralPath "$source/pc-bios/$name" -Destination $share
}
Copy-Item -LiteralPath "$source/pc-bios/keymaps/en-us" -Destination "$share/keymaps/en-us"
# Decompress the upstream firmware bytes; never regenerate existing VM identities.
$env:YOUGORI_QEMU_SOURCE = $source
$env:YOUGORI_QEMU_SHARE = $share
try {
  @'
import bz2, os
from pathlib import Path
source = Path(os.environ['YOUGORI_QEMU_SOURCE']) / 'pc-bios'
output = Path(os.environ['YOUGORI_QEMU_SHARE'])
for name in ('edk2-i386-vars.fd', 'edk2-x86_64-code.fd'):
    (output / name).write_bytes(bz2.decompress((source / (name + '.bz2')).read_bytes()))
'@ | python -
  if ($LASTEXITCODE) { throw 'Cannot unpack pinned firmware.' }
} finally {
  Remove-Item Env:YOUGORI_QEMU_SOURCE, Env:YOUGORI_QEMU_SHARE -ErrorAction SilentlyContinue
}
foreach ($name in @('COPYING', 'COPYING.LIB', 'LICENSE', 'README.rst')) {
  Copy-Item -LiteralPath "$source/$name" -Destination $output
}
& "$PSScriptRoot/build-gpu-bridge.ps1" -QemuDirectory $output
Copy-Item -LiteralPath "$angle/ANGLE_BUILD.json", "$angle/ANGLE-NOTICES.txt" -Destination $output
Copy-Item -LiteralPath "$ucrt/../share/licenses" -Destination "$output/licenses" -Recurse
Copy-Item -LiteralPath "$repo/runtime/security/licenses" -Destination "$output/licenses/extra" -Recurse
$pacman = "$repo/build/secure-runtime/toolchain/msys64/usr/bin/pacman.exe"
$dlls = @($seen | Where-Object { $_ -notin @('libEGL.dll', 'libGLESv2.dll') -and (Test-Path -LiteralPath "$ucrt/$_") } | ForEach-Object { "/ucrt64/bin/$_" })
$owners = @(& $pacman -Qoq @dlls | Sort-Object -Unique)
if ($LASTEXITCODE) { throw 'Cannot identify runtime packages.' }
$packages = & $pacman -Qi @owners
if ($LASTEXITCODE) { throw 'Cannot record runtime packages.' }
[IO.File]::WriteAllLines("$output/PACKAGES.txt", $packages, [Text.UTF8Encoding]::new($false))
$patches = @('qemu-windows-tpm.patch', 'qemu-whpx-tpm-ppi.patch', 'qemu-whpx-reboot.patch', 'qemu-license-notices.patch', 'tpm-qemu.c', 'tpm-api.h')
$sourceRecord = [ordered]@{
  schemaVersion = 1
  source = 'https://github.com/qemu/qemu'
  revision = $revision
  buildScript = 'scripts/build-secure-qemu.sh'
  buildScriptSha256 = (Get-FileHash -LiteralPath "$repo/scripts/build-secure-qemu.sh" -Algorithm SHA256).Hash.ToLowerInvariant()
  profile = 'Windows x86_64 WHPX/TCG, VNC, OpenGL; disk utility enabled; JACK/SDL/GTK/spice/remote disk backends disabled'
  patches = @($patches | ForEach-Object { [ordered]@{ path = "runtime/security/$_"; sha256 = (Get-FileHash -LiteralPath "$repo/runtime/security/$_" -Algorithm SHA256).Hash.ToLowerInvariant() } })
  firmware = 'Unmodified pc-bios files from the recorded QEMU revision, with bz2 firmware decompressed'
  dependencyProvenance = 'PACKAGES.txt for package DLLs; ANGLE_BUILD.json and ANGLE-NOTICES.txt for source-built D3D11 ANGLE'
}
[IO.File]::WriteAllText("$output/SOURCE_BUILD.json", ($sourceRecord | ConvertTo-Json -Depth 8) + "`n", [Text.UTF8Encoding]::new($false))
$lines = Get-ChildItem -LiteralPath $output -Recurse -File | Where-Object Name -ne SHA256SUMS | Sort-Object FullName | ForEach-Object {
  $relative = [IO.Path]::GetRelativePath($output, $_.FullName).Replace('\', '/')
  "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
}
[IO.File]::WriteAllLines("$output/SHA256SUMS", $lines, [Text.UTF8Encoding]::new($false))
Write-Output "Staged source-built QEMU: $output"
