param([string]$OutputDirectory = "$PSScriptRoot/../src-tauri/boot-helper", [switch]$TestFixture, [switch]$RestartFixture, [switch]$Check)
$ErrorActionPreference = 'Stop'
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Install the Visual Studio C++ build tools to rebuild the boot helper.' }
$version = (Get-Content "$vs/VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt" -Raw).Trim()
$toolchain = "$vs/VC/Tools/MSVC/$version"
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}/Windows Kits/10/Include" -Directory | Where-Object { Test-Path "$($_.FullName)/ucrt/stddef.h" } | Sort-Object Name -Descending | Select-Object -First 1
if (-not $sdk) { throw 'Install the Windows SDK C runtime headers to rebuild the boot helper.' }
if ($TestFixture -and $RestartFixture) { throw 'Select only one test fixture.' }
$sourceName = if ($RestartFixture) { 'test-restart.c' } elseif ($TestFixture) { 'test-os.c' } else { 'main.c' }
$outputName = if ($RestartFixture) { 'test-restart.efi' } elseif ($TestFixture) { 'test-os.efi' } else { 'bootx64.efi' }
$source = [IO.Path]::GetFullPath("$PSScriptRoot/../src-tauri/boot-helper/$sourceName")
$output = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $output | Out-Null
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("yougori-boot-build-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $binaryOutput = if ($Check) { Join-Path $temporary $outputName } else { Join-Path $output $outputName }
    & "$toolchain/bin/Hostx64/x64/cl.exe" /nologo /c /O1 /GS- /Zl /std:c11 /W4 /WX /Brepro "/I$toolchain/include" "/I$($sdk.FullName)/ucrt" "/Fo$temporary/boot.obj" $source
    if ($LASTEXITCODE) { throw 'UEFI compilation failed' }
    & "$toolchain/bin/Hostx64/x64/link.exe" /nologo /nodefaultlib /entry:efi_main /subsystem:efi_application /machine:x64 /fixed:no /dynamicbase:no /Brepro "/out:$binaryOutput" "$temporary/boot.obj"
    if ($LASTEXITCODE) { throw 'UEFI linking failed' }
    $builtHash = Get-FileHash $binaryOutput -Algorithm SHA256
    if ($Check -and $builtHash.Hash -ne (Get-FileHash "$output/$outputName" -Algorithm SHA256).Hash) { throw 'The bundled EFI binary does not match the current source. Rebuild it without -Check.' }
    $builtHash
} finally {
    # Only our unique, verified temporary build directory is removed.
    $resolved = [IO.Path]::GetFullPath($temporary)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf).StartsWith('yougori-boot-build-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
