param(
    [Parameter(Mandatory)][string]$InstallDirectory,
    [ValidateSet('Install', 'Uninstall')][string]$Action = 'Install',
    [switch]$DryRun,
    [string]$ExistingPath
)
$ErrorActionPreference = 'Stop'
$cliDirectory = [IO.Path]::GetFullPath((Join-Path $InstallDirectory 'cli')).TrimEnd('\')
if (-not $DryRun -and $Action -eq 'Install' -and -not (Test-Path -LiteralPath (Join-Path $cliDirectory 'yougori.exe') -PathType Leaf)) {
    throw 'The public Yougori command was not installed.'
}
if ($PSBoundParameters.ContainsKey('ExistingPath') -and -not $DryRun) { throw 'ExistingPath is available only for a dry run.' }
$registry = $null
try {
    if ($DryRun) { $old = $ExistingPath }
    else {
        $registry = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
        $old = [string]$registry.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    }
    $parts = @($old -split ';' | Where-Object { $_.Trim().Trim('"').TrimEnd('\') -ine $cliDirectory })
    if ($Action -eq 'Install') { $parts = @($cliDirectory) + $parts }
    $updated = ($parts -join ';').TrimEnd(';')
    if ($updated.Length -ge 32760) { throw 'PATH is too long; its existing value was preserved.' }
    if ($DryRun) { return $updated }
    if ($old -ne $updated) { $registry.SetValue('Path', $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString) }
} finally { if ($null -ne $registry) { $registry.Dispose() } }
# Notify Explorer so new terminals inherit the public command. Existing terminals
# keep their environment until reopened. Never use setx, which can truncate PATH.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class YougoriEnvironment {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, IntPtr wparam, string lparam, uint flags, uint timeout, out IntPtr result);
}
'@
$broadcastResult = [IntPtr]::Zero
[void][YougoriEnvironment]::SendMessageTimeout([IntPtr]0xffff, 0x001a, [IntPtr]::Zero, 'Environment', 2, 5000, [ref]$broadcastResult)
