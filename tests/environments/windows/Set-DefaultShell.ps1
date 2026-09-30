# Requires PowerShell 7. Runs only against the disposable loopback fixture.
# Configures the sshd default shell through the administrator control key. This
# is fixture provisioning; the ssh-copy-id product never changes server settings.
param(
    [Parameter(Mandatory)]
    [ValidateSet('cmd', 'WindowsPowerShell', 'pwsh')]
    [string]$Shell,
    [string]$IdentityFile = 'work/windows-fixture/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture/known_hosts',
    [int]$Port = 22222
)
$ErrorActionPreference = 'Stop'
$guestScript = @'
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath 'C:\OEM\fixture.marker')) { throw 'Not a disposable fixture' }
$path = switch ('__SHELL__') {
    'cmd' { $null }
    'WindowsPowerShell' { Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe' }
    'pwsh' { Join-Path $env:ProgramFiles 'PowerShell\7\pwsh.exe' }
}
$key = 'HKLM:\SOFTWARE\OpenSSH'
if ($null -eq $path) {
    Remove-ItemProperty -Path $key -Name DefaultShell -ErrorAction SilentlyContinue
} else {
    if (-not (Test-Path -LiteralPath $path)) { throw "Shell executable is missing: $path" }
    New-ItemProperty -Path $key -Name DefaultShell -Value $path -PropertyType String -Force | Out-Null
}
$registry = Get-ItemProperty -Path $key
[ordered]@{
    requestedShell = '__SHELL__'
    defaultShell = if ($registry.PSObject.Properties.Name -contains 'DefaultShell') { $registry.DefaultShell } else { $null }
    shellVersion = if ($path) { (Get-Item -LiteralPath $path).VersionInfo.FileVersion } else { $null }
} | ConvertTo-Json -Compress
'@
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($guestScript.Replace('__SHELL__', $Shell)))
$output = & ssh.exe -F none -T -i $IdentityFile -o IdentitiesOnly=yes -o IdentityAgent=none `
    -o BatchMode=yes -o StrictHostKeyChecking=yes -o "UserKnownHostsFile=$KnownHostsFile" `
    -o ConnectTimeout=5 -p $Port fixtureadmin@127.0.0.1 `
    powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded 2>&1
if ($LASTEXITCODE -ne 0) { throw "Default shell configuration failed ($LASTEXITCODE): $output" }
($output | Where-Object { "$_" -like '{*' }) | ConvertFrom-Json | ConvertTo-Json
