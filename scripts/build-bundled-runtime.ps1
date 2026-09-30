[CmdletBinding()]
param(
  [switch]$SkipQemu,
  [switch]$SkipAppliance,
  [switch]$BuildSecureRuntime,
  [string]$RuntimeDirectory = "$PSScriptRoot/../src-tauri/resources/runtime"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$runtimeRoot = [IO.Path]::GetFullPath($RuntimeDirectory)
if (-not $runtimeRoot.StartsWith($repositoryRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'Runtime build output must be inside this checkout.'
}
if ($BuildSecureRuntime -and $runtimeRoot -ne (Join-Path $repositoryRoot 'src-tauri\resources\runtime')) {
  throw 'For a custom RuntimeDirectory, stage the secure runtime separately. BuildSecureRuntime installs into the default bundled directory.'
}
$cacheRoot = Join-Path $repositoryRoot "build\runtime-cache"
$qemuRoot = Join-Path $runtimeRoot "qemu"
$applianceRoot = Join-Path $runtimeRoot "appliance"

function ConvertTo-WslPath([string]$WindowsPath) {
  $fullPath = [IO.Path]::GetFullPath($WindowsPath)
  if ($fullPath -notmatch '^([A-Za-z]):\\(.*)$') {
    throw "The appliance build expects a local drive path: $fullPath"
  }
  $drive = $Matches[1].ToLowerInvariant()
  $tail = $Matches[2] -replace '\\', '/'
  return "/mnt/$drive/$tail"
}

function Assert-ChildPath([string]$Parent, [string]$Child) {
  $resolvedParent = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $resolvedChild = [IO.Path]::GetFullPath($Child)
  if (-not $resolvedChild.StartsWith($resolvedParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to modify a path outside $resolvedParent`: $resolvedChild"
  }
  return $resolvedChild
}

function Remove-QemuBuildExtras([string]$Root) {
  $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
  $removeFiles = @(
    Get-ChildItem -LiteralPath $resolvedRoot -File -Filter "qemu-system-*.exe" |
      Where-Object Name -ne "qemu-system-x86_64.exe"
    Get-ChildItem -LiteralPath $resolvedRoot -File |
      Where-Object Name -in @("qemu-edid.exe", "qemu-ga.exe", "qemu-io.exe", "qemu-nbd.exe", "qemu-storage-daemon.exe")
  )
  foreach ($file in $removeFiles) {
    $target = Assert-ChildPath $resolvedRoot $file.FullName
    Remove-Item -LiteralPath $target -Force
  }

  # These optional ANGLE capture/Vulkan fallback libraries are outside the
  # transitive PE dependency closure and are not dynamically loaded by the
  # supported q35/TCG/VNC/egl-headless profiles. libEGL.dll and libGLESv2.dll
  # are intentionally retained: egl-headless loads them dynamically even
  # though they do not appear in the static import graph.
  foreach ($name in @(
    "libGLESv2_with_capture.dll",
    "libGLESv2_vulkan_secondaries.dll",
    "libvk_swiftshader.dll",
    "libfeature_support.dll",
    "libjsoncpp-26.dll",
    "libEGL_vulkan_secondaries.dll",
    "libVkICD_mock_icd.dll",
    "libGLESv1_CM.dll"
  )) {
    $target = Join-Path $resolvedRoot $name
    if (Test-Path -LiteralPath $target) {
      $target = Assert-ChildPath $resolvedRoot (Resolve-Path -LiteralPath $target).Path
      Remove-Item -LiteralPath $target -Force
    }
  }
  foreach ($relative in @("share\applications", "share\doc", "share\dtb", "share\icons", "share\locale", "share\man")) {
    $target = Join-Path $resolvedRoot $relative
    if (Test-Path -LiteralPath $target) {
      $target = Assert-ChildPath $resolvedRoot (Resolve-Path -LiteralPath $target).Path
      Remove-Item -LiteralPath $target -Recurse -Force
    }
  }
  # Yougori ships only the x86_64 system emulator and launches an explicit
  # set of x86 firmware/option ROMs. Treat that payload as an allowlist so new
  # upstream cross-architecture firmware cannot silently bloat installers.
  $supportedShareFiles = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
  )
  foreach ($name in @(
    "bios-256k.bin",
    "bios-microvm.bin",
    "bios.bin",
    "edk2-i386-vars.fd",
    "edk2-licenses.txt",
    "edk2-x86_64-code.fd",
    "efi-e1000e.rom",
    "efi-virtio.rom",
    "kvmvapic.bin",
    "linuxboot_dma.bin",
    "pvh.bin",
    "qboot.rom",
    "vgabios-bochs-display.bin",
    "vgabios-ramfb.bin",
    "vgabios-stdvga.bin",
    "vgabios-virtio.bin",
    "vgabios.bin"
  )) {
    [void]$supportedShareFiles.Add($name)
  }
  $shareRoot = Join-Path $resolvedRoot "share"
  Get-ChildItem -LiteralPath $shareRoot -File | ForEach-Object {
    if (-not $supportedShareFiles.Contains($_.Name)) {
      $target = Assert-ChildPath $resolvedRoot $_.FullName
      Remove-Item -LiteralPath $target -Force
    }
  }
  foreach ($relative in @("share\firmware")) {
    $target = Join-Path $resolvedRoot $relative
    if (Test-Path -LiteralPath $target) {
      $target = Assert-ChildPath $resolvedRoot (Resolve-Path -LiteralPath $target).Path
      Remove-Item -LiteralPath $target -Recurse -Force
    }
  }
  $keymapRoot = Join-Path $shareRoot "keymaps"
  if (Test-Path -LiteralPath $keymapRoot) {
    Get-ChildItem -LiteralPath $keymapRoot -File |
      Where-Object Name -ne "en-us" |
      ForEach-Object {
        $target = Assert-ChildPath $resolvedRoot $_.FullName
        Remove-Item -LiteralPath $target -Force
      }
  }
}

function Invoke-QemuQmpSmoke([string]$Executable, [string]$WorkingDirectory, [string]$Arguments, [string]$Label) {
  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $Executable
  $startInfo.WorkingDirectory = $WorkingDirectory
  $startInfo.Arguments = $Arguments
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardInput = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $process = [Diagnostics.Process]::new()
  $process.StartInfo = $startInfo
  $didStart = $false
  try {
    [void]$process.Start()
    $didStart = $true
    $greeting = $process.StandardOutput.ReadLineAsync()
    if (-not $greeting.Wait(10000)) {
      throw "$Label QMP greeting timed out"
    }
    if ($greeting.Result -notmatch '"QMP"') {
      throw "$Label returned an invalid QMP greeting: $($greeting.Result)"
    }
    $process.StandardInput.WriteLine('{"execute":"qmp_capabilities"}')
    $process.StandardInput.Flush()
    $capabilities = $process.StandardOutput.ReadLineAsync()
    if (-not $capabilities.Wait(10000) -or $capabilities.Result -notmatch '"return"') {
      throw "$Label QMP capabilities negotiation timed out"
    }
    $process.StandardInput.WriteLine('{"execute":"quit"}')
    $process.StandardInput.Flush()
    $quitAcknowledged = $false
    for ($attempt = 0; $attempt -lt 4; $attempt++) {
      $quitResponse = $process.StandardOutput.ReadLineAsync()
      if (-not $quitResponse.Wait(5000)) {
        break
      }
      if ($quitResponse.Result -match '"return"') {
        $quitAcknowledged = $true
        break
      }
    }
    if (-not $quitAcknowledged) {
      throw "$Label did not acknowledge the QMP quit command"
    }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(10000)) {
      throw "$Label QMP quit timed out"
    }
    $stderr = $process.StandardError.ReadToEnd()
    if ($process.ExitCode -ne 0) {
      throw "$Label exited with code $($process.ExitCode): $stderr"
    }
    if ($stderr -match 'failed to find romfile|Could not load PC BIOS|eglInitialize failed') {
      throw "$Label reported a missing runtime dependency: $stderr"
    }
  } finally {
    if ($didStart -and -not $process.HasExited) {
      $process.Kill()
      $process.WaitForExit()
    }
    $process.Dispose()
  }
}

function Assert-QemuBuildStarts([string]$Root) {
  $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
  $qemu = Join-Path $resolvedRoot "qemu-system-x86_64.exe"
  $qemuImg = Join-Path $resolvedRoot "qemu-img.exe"
  foreach ($relative in @(
    "share\bios-256k.bin",
    "share\edk2-i386-vars.fd",
    "share\edk2-x86_64-code.fd",
    "share\efi-e1000e.rom",
    "share\linuxboot_dma.bin",
    "share\vgabios-stdvga.bin",
    "share\vgabios-virtio.bin"
  )) {
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedRoot $relative))) {
      throw "Pruned QEMU is missing required payload $relative"
    }
  }
  & $qemuImg --version | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "Pruned QEMU image utility failed its startup check"
  }
  & $qemu --version | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "Pruned QEMU system emulator failed its startup check"
  }

  # Device discovery loads the modules used by Yougori's q35 and GPU launch
  # profiles while still exiting immediately and remaining host-independent.
  $deviceOutput = & $qemu -display help 2>&1
  if ($LASTEXITCODE -ne 0 -or ($deviceOutput -join "`n") -notmatch "egl-headless") {
    throw "Pruned QEMU does not expose the required egl-headless display"
  }
  foreach ($device in @("VGA", "e1000e", "virtio-gpu-gl-pci", "virtio-rng-pci")) {
    & $qemu -device "$device,help" -machine q35 -display none 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "Pruned QEMU does not expose required device $device"
    }
  }
  Invoke-QemuQmpSmoke $qemu $resolvedRoot "-L share -machine q35 -accel tcg,thread=multi -nodefaults -S -device virtio-gpu-gl-pci,max_outputs=1 -display egl-headless -qmp stdio -monitor none -serial none" "q35 egl-headless smoke"
  Invoke-QemuQmpSmoke $qemu $resolvedRoot "-L share -machine microvm,graphics=off,acpi=off,pcie=off,usb=off,x-option-roms=off -accel tcg,thread=multi -nodefaults -S -device virtio-rng-device -display none -qmp stdio -monitor none -serial none" "microvm virtio-mmio smoke"
}

function Write-RuntimeChecksums([string]$Root) {
  $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
  $manifest = Join-Path $resolvedRoot "SHA256SUMS"
  $lines = Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse |
    Where-Object FullName -ne $manifest |
    Sort-Object FullName |
    ForEach-Object {
      $relative = $_.FullName.Substring($resolvedRoot.Length + 1).Replace('\', '/')
      "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    }
  [IO.File]::WriteAllLines($manifest, $lines, [Text.UTF8Encoding]::new($false))
}

New-Item -ItemType Directory -Force -Path $runtimeRoot, $cacheRoot | Out-Null

if (-not $SkipQemu) {
  $qemuBuild = Join-Path $repositoryRoot 'build/secure-runtime/qemu-build'
  & "$PSScriptRoot/build-secure-runtime.ps1" -QemuOnly -WithTools -QemuBuildDirectory $qemuBuild
  $staged = Join-Path $runtimeRoot ("qemu-staged-" + [guid]::NewGuid().ToString('N'))
  & "$PSScriptRoot/stage-windows-qemu.ps1" -OutputDirectory $staged -QemuBuildDirectory $qemuBuild
  Assert-QemuBuildStarts $staged
  $mapped = Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith($qemuRoot + '\', [StringComparison]::OrdinalIgnoreCase)
  }
  if ($mapped) { throw "Close environments using this checkout's QEMU before replacing it. Tested build preserved at $staged" }
  $preserved = Join-Path $cacheRoot ("previous-qemu-" + [guid]::NewGuid().ToString('N'))
  $checkedStaged = Assert-ChildPath $repositoryRoot $staged
  $checkedTarget = Assert-ChildPath $repositoryRoot $qemuRoot
  $checkedPrevious = Assert-ChildPath $repositoryRoot $preserved
  if (Test-Path -LiteralPath $checkedTarget) { Move-Item -LiteralPath $checkedTarget -Destination $checkedPrevious }
  try { Move-Item -LiteralPath $checkedStaged -Destination $checkedTarget }
  catch {
    if ((Test-Path -LiteralPath $checkedPrevious) -and -not (Test-Path -LiteralPath $checkedTarget)) {
      Move-Item -LiteralPath $checkedPrevious -Destination $checkedTarget
    }
    throw
  }
}

Remove-QemuBuildExtras $qemuRoot
& (Join-Path $PSScriptRoot 'build-gpu-bridge.ps1') -QemuDirectory $qemuRoot
Assert-QemuBuildStarts $qemuRoot
Write-RuntimeChecksums $qemuRoot

if (-not $SkipAppliance) {
  New-Item -ItemType Directory -Force -Path $applianceRoot | Out-Null
  $linuxRepository = ConvertTo-WslPath $repositoryRoot
  $linuxOutput = ConvertTo-WslPath $applianceRoot
  $linuxScript = "$linuxRepository/scripts/build-appliance.sh"
  wsl -d Ubuntu-22.04 -u root -- bash $linuxScript $linuxRepository $linuxOutput
  if ($LASTEXITCODE -ne 0) {
    throw "Appliance build failed with exit code $LASTEXITCODE"
  }
}

$required = @(
  (Join-Path $qemuRoot "qemu-system-x86_64.exe"),
  (Join-Path $qemuRoot "qemu-img.exe"),
  (Join-Path $applianceRoot "appliance-base.qcow2"),
  (Join-Path $applianceRoot "vmlinuz-virt"),
  (Join-Path $applianceRoot "initramfs-virt")
)
foreach ($path in $required) {
  if (-not (Test-Path -LiteralPath $path)) {
    throw "Bundled runtime is incomplete: $path"
  }
}

if ($BuildSecureRuntime) {
  & "$PSScriptRoot/build-secure-runtime.ps1" -Install
}
if (-not (Test-Path -LiteralPath "$runtimeRoot/qemu-secure/SHA256SUMS")) {
  throw 'Windows VM security runtime is missing. Run scripts/build-secure-runtime.ps1 -Install, or pass -BuildSecureRuntime after preparing the documented build tools.'
}
Write-Output "Bundled Yougori runtime is ready at $runtimeRoot"
