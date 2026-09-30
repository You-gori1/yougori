param([Parameter(Mandatory)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
$out=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path "$out/payload" -Force | Out-Null
$vswhere="${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$version=(Get-Content "$vs/VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt" -Raw).Trim()
$vc="$vs/VC/Tools/MSVC/$version"
$sdk=Get-ChildItem "${env:ProgramFiles(x86)}/Windows Kits/10/Include" -Directory | Where-Object { Test-Path "$($_.FullName)/um/windows.h" } | Sort-Object Name -Descending | Select-Object -First 1
& "$vc/bin/Hostx64/x64/cl.exe" /nologo /c /O1 /GS- /Zl /W4 /WX "/I$vc/include" "/I$($sdk.FullName)/ucrt" "/I$($sdk.FullName)/shared" "/I$($sdk.FullName)/um" "/Fo$out/probe.obj" "$PSScriptRoot/windows-setup-probe.c"
if ($LASTEXITCODE) { throw 'Probe compilation failed' }
$lib="${env:ProgramFiles(x86)}/Windows Kits/10/Lib/$($sdk.Name)/um/x64"
& "$vc/bin/Hostx64/x64/link.exe" /nologo /nodefaultlib /entry:mainCRTStartup /subsystem:windows /machine:x64 "/out:$out/payload/probe.exe" "$out/probe.obj" "$lib/kernel32.lib" "$lib/user32.lib" "$lib/advapi32.lib"
if ($LASTEXITCODE) { throw 'Probe linking failed' }
$unix='/mnt/'+$out.Substring(0,1).ToLowerInvariant()+$out.Substring(2).Replace('\','/')
& wsl -d Ubuntu-22.04 -- mformat -i "$unix/probe.img" -C -f 1440 ::
if ($LASTEXITCODE) { throw 'Probe FAT disk generation failed (requires mtools in WSL)' }
& wsl -d Ubuntu-22.04 -- mcopy -i "$unix/probe.img" "$unix/payload/probe.exe" ::
if ($LASTEXITCODE) { throw 'Probe FAT copy failed' }
Write-Output "$out/probe.img"
