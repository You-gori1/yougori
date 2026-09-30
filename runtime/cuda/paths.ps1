# Rust's canonical Windows paths use the extended-length prefix. Windows
# PowerShell 5's filesystem provider (notably Join-Path) cannot handle it.
function Get-CudaWindowsPath([string]$LiteralPath) {
    if ($LiteralPath.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
        $LiteralPath = '\\' + $LiteralPath.Substring(8)
    } elseif ($LiteralPath.StartsWith('\\?\', [StringComparison]::Ordinal)) {
        $LiteralPath = $LiteralPath.Substring(4)
    }
    if (![IO.Path]::IsPathRooted($LiteralPath) -or $LiteralPath -notmatch '^(?:[A-Za-z]:[\\/]|\\\\[^\\]+\\[^\\]+)') {
        throw 'CUDA setup requires absolute Windows filesystem paths.'
    }
    return [IO.Path]::GetFullPath($LiteralPath)
}
