param([Parameter(Mandatory)][int]$RuntimeProcessId)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path.TrimEnd('\')
$runtime = Get-Process -Id $RuntimeProcessId -ErrorAction Stop
if (-not $runtime.Path) { throw 'Cannot inspect the running Yougori executable.' }
$development = $runtime.Path -match '(?i)[\\/]debug[\\/]yougori\.exe$'
$frontend = 'available'
if ($development) {
    $listeners = @(Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue)
    foreach ($owner in @($listeners | Select-Object -ExpandProperty OwningProcess -Unique)) {
        $process = Get-CimInstance Win32_Process -Filter "ProcessId = $owner" -ErrorAction Stop
        $command = [string]$process.CommandLine
        # Never replace or stop an existing server, including a Vite started by
        # another attachment or Tauri session.
        if ($process.Name -ieq 'node.exe' -and
            $command.IndexOf($workspace + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0 -and
            $command -match '(?i)[\\/]vite[\\/]bin[\\/]vite\.js(?:["'']|\s|$)') {
            if ($frontend -ne 'other') { $frontend = 'workspace' }
        } else {
            $frontend = 'other'
        }
    }
}
@{ development = $development; frontend = $frontend } | ConvertTo-Json -Compress
