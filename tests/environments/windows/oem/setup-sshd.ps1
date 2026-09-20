# Guest provisioning only. Never run this on the development host.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not (Test-Path -LiteralPath 'C:\OEM\fixture.marker')) {
    throw 'The disposable guest fixture marker is missing.'
}

Write-Output 'Checking the OpenSSH Server capability...'
$capability = Get-WindowsCapability -Online -Name 'OpenSSH.Server~~~~0.0.1.0'
if ($capability.State -ne 'Installed') {
    Write-Output 'Installing OpenSSH Server (Windows Update may take several minutes)...'
    $installation = Add-WindowsCapability -Online -Name $capability.Name
    if ($installation.RestartNeeded) {
        throw 'Restart the guest and rerun C:\OEM\setup-sshd.ps1 to finish setup.'
    }
}

Write-Output 'Provisioning the standard fixture account...'
$fixturePassword = ConvertTo-SecureString 'Fixture-Only_47!' -AsPlainText -Force
if (-not (Get-LocalUser -Name 'fixtureuser' -ErrorAction SilentlyContinue)) {
    New-LocalUser -Name 'fixtureuser' -Password $fixturePassword -PasswordNeverExpires | Out-Null
}
$usersGroup = Get-LocalGroup -SID 'S-1-5-32-545'
$fixtureUser = Get-LocalUser -Name 'fixtureuser'
$member = Get-LocalGroupMember -Group $usersGroup.Name |
    Where-Object { $_.SID -eq $fixtureUser.SID }
if (-not $member) {
    Add-LocalGroupMember -Group $usersGroup.Name -Member $fixtureUser
}

# Keep default sshd_config, including its administrator key-file Match block.
Write-Output 'Starting sshd...'
Set-Service -Name sshd -StartupType Automatic
Start-Service -Name sshd

# Optional disposable control key for automated smoke tests. The standard user
# remains unprovisioned so key-installation tests can start from an empty state.
if (Test-Path -LiteralPath 'C:\OEM\smoke-key.pub') {
    $adminKeys = Join-Path $env:ProgramData 'ssh\administrators_authorized_keys'
    $publicKey = [IO.File]::ReadAllText('C:\OEM\smoke-key.pub').Trim()
    if ($publicKey -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: .*)?$') {
        throw 'Unexpected smoke key format.'
    }
    [IO.File]::WriteAllText($adminKeys, $publicKey + "`n", [Text.UTF8Encoding]::new($false))
    & icacls.exe $adminKeys /inheritance:r /grant:r '*S-1-5-32-544:F' '*S-1-5-18:F'
    if ($LASTEXITCODE -ne 0) { throw 'Smoke key ACL setup failed.' }
}
if (-not (Get-NetFirewallRule -Name 'ssh-copy-id-fixture-sshd' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name 'ssh-copy-id-fixture-sshd' -DisplayName 'SSH copy-id fixture' `
        -Direction Inbound -Action Allow -Protocol TCP -LocalPort 22 | Out-Null
}

$sshd = Join-Path $env:WINDIR 'System32\OpenSSH\sshd.exe'
& $sshd -t
if ($LASTEXITCODE -ne 0) { throw 'sshd configuration validation failed.' }
$keygen = Join-Path $env:WINDIR 'System32\OpenSSH\ssh-keygen.exe'
Get-ChildItem -LiteralPath (Join-Path $env:ProgramData 'ssh') -Filter 'ssh_host_*_key.pub' |
    ForEach-Object { & $keygen -lf $_.FullName }
Set-Content -LiteralPath 'C:\OEM\ssh-ready.txt' -Value 'OpenSSH fixture ready' -Encoding ASCII
Write-Output 'OpenSSH fixture ready.'
