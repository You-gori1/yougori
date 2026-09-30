param([string]$Thumbprint = $env:YOUGORI_SIGN_CERT_SHA1)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
& "$PSScriptRoot/sign-windows-release.ps1" -Action Preflight -Thumbprint $Thumbprint
if ($LASTEXITCODE) { throw 'Signing preflight failed.' }
$previousSigner = $env:YOUGORI_SIGN_CERT_SHA1
$env:YOUGORI_SIGN_CERT_SHA1 = $Thumbprint
$configDirectory = Join-Path $repoRoot 'build/release-signing'
New-Item -ItemType Directory -Force -Path $configDirectory | Out-Null
$configPath = Join-Path $configDirectory 'tauri.signing.json'
$config = @{ bundle = @{ windows = @{ signCommand = @{ cmd = 'powershell.exe'; args = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',"$PSScriptRoot/sign-windows-release.ps1",'-Action','Sign','-Path','%1') } } } }
[IO.File]::WriteAllText($configPath, ($config | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
try {
    Push-Location $repoRoot
    npm run tauri -- build --config src-tauri/tauri.windows.conf.json --config $configPath --bundles nsis,msi
    if ($LASTEXITCODE) { throw 'Signed Windows packaging failed.' }
} finally {
    Pop-Location
    $env:YOUGORI_SIGN_CERT_SHA1 = $previousSigner
}
