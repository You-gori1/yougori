param(
    [ValidateSet('Preflight','Sign','Verify')][string]$Action = 'Preflight',
    [string]$Path,
    [string]$Thumbprint = $env:YOUGORI_SIGN_CERT_SHA1,
    [string]$TimestampUrl = 'https://timestamp.digicert.com'
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop
if ($Action -eq 'Verify') {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'Choose an existing signed executable or installer.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw "A trusted, timestamped signature is required: $Path ($($signature.Status))" }
    if ($Thumbprint -and $signature.SignerCertificate.Thumbprint -ne ($Thumbprint -replace '\s','')) { throw 'Unexpected release signer.' }
    Write-Output "Verified trusted signature and timestamp: $Path"
    exit 0
}
if ($Thumbprint -notmatch '^[a-fA-F0-9]{40}$') { throw 'Set YOUGORI_SIGN_CERT_SHA1 to your installed code-signing certificate thumbprint. A self-signed test certificate cannot sign a public release.' }
$certificate = @(Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object Thumbprint -eq $Thumbprint)
if ($certificate.Count -ne 1 -or -not $certificate[0].HasPrivateKey -or $certificate[0].NotAfter -le (Get-Date) -or $certificate[0].NotBefore -gt (Get-Date)) { throw 'The selected current-user code-signing certificate/private key is missing or outside its validity period.' }
$chain = New-Object System.Security.Cryptography.X509Certificates.X509Chain
try { if (-not $chain.Build($certificate[0])) { throw 'The signing certificate does not chain to a trusted issuer.' } } finally { $chain.Dispose() }
$sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$signtool = Get-ChildItem -LiteralPath $sdkRoot -Directory | Sort-Object Name -Descending | ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $signtool) { throw 'Install the Windows SDK signing tools before building a signed release.' }
if ($Action -eq 'Preflight') { Write-Output 'Code-signing certificate, private key and Windows SDK are available.'; exit 0 }
if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or [IO.Path]::GetExtension($Path) -notin @('.exe','.dll','.msi')) { throw 'Choose an existing Windows executable, DLL or MSI.' }
$timestamp = [Uri]$TimestampUrl
if ($timestamp.Scheme -ne 'https' -or $timestamp.UserInfo) { throw 'Use an HTTPS timestamp service without embedded credentials.' }
& $signtool sign /sha1 $Thumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $Path
if ($LASTEXITCODE) { throw "Windows signing failed (exit $LASTEXITCODE)." }
& $signtool verify /pa /all /tw $Path
if ($LASTEXITCODE) { throw 'Windows signature verification failed.' }
& $PSCommandPath -Action Verify -Path $Path -Thumbprint $Thumbprint
