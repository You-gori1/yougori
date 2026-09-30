#requires -Version 7.0
$ErrorActionPreference = 'Stop'

if (!$IsWindows) { throw 'The Windows verification suite requires Windows.' }
if (!(Test-Path -LiteralPath 'C:/Program Files/Git/bin/bash.exe' -PathType Leaf)) {
    throw 'Installer fixture tests require Git Bash. Install it before running the complete suite.'
}
& py -3 --version
if ($LASTEXITCODE) { throw 'Tutorial HTTP tests require Python 3 through the py launcher.' }
Write-Output 'Windows test prerequisites are available; shell/Python tests must not be silently skipped.'
