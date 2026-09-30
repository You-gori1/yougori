$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'install-cli-path.ps1'
$long = '%USERPROFILE%\bin;' + (('C:\Other' * 150) -join '')
$added = & $script -InstallDirectory 'C:\Apps\Yougori' -DryRun -ExistingPath $long
if ($added -ne ('C:\Apps\Yougori\cli;' + $long)) { throw 'Install must preserve long paths and expansion tokens' }
$again = & $script -InstallDirectory 'C:\Apps\Yougori' -DryRun -ExistingPath $added
if ($again -ne $added) { throw 'Install must be idempotent' }
$removed = & $script -InstallDirectory 'C:\Apps\Yougori' -Action Uninstall -DryRun -ExistingPath $added
if ($removed -ne $long) { throw 'Uninstall must remove only the CLI entry' }
'CLI PATH tests passed without changing this computer.'
