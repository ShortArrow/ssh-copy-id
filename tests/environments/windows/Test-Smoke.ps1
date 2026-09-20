param(
    [string]$IdentityFile = 'work/windows-fixture/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture/known_hosts',
    [int]$Port = 22222
)

$ErrorActionPreference = 'Stop'
$guestScript = @'
$ErrorActionPreference = 'Stop'
$inputText = [Console]::In.ReadToEnd().TrimEnd([char[]]"`r`n")
if ($inputText -ne 'ssh-copy-id stdin smoke') { throw 'stdin payload mismatch' }
if (-not (Test-Path -LiteralPath 'C:\OEM\ssh-ready.txt')) { throw 'Bootstrap is incomplete' }
$version = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$standard = Get-LocalUser fixtureuser
if (-not $standard.Enabled) { throw 'fixtureuser must be enabled' }
$admins = Get-LocalGroup -SID 'S-1-5-32-544'
$isAdmin = @(Get-LocalGroupMember -Group $admins.Name | Where-Object { $_.SID -eq $standard.SID }).Count -gt 0
if ($isAdmin) { throw 'fixtureuser must not be an administrator' }
$service = Get-Service sshd
if ($service.Status -ne 'Running') { throw 'sshd is not running' }
$ssh = Join-Path $env:WINDIR 'System32\OpenSSH\ssh.exe'
$sshVersion = (Get-Item -LiteralPath $ssh).VersionInfo.FileVersion
$registry = Get-ItemProperty 'HKLM:\SOFTWARE\OpenSSH'
$defaultShell = if ($registry.PSObject.Properties.Name -contains 'DefaultShell') { $registry.DefaultShell } else { 'cmd.exe (default)' }
[ordered]@{
    user = (& whoami)
    build = $version.CurrentBuild
    revision = $version.UBR
    displayVersion = $version.DisplayVersion
    edition = $version.EditionID
    sshVersion = $sshVersion
    sshd = $service.Status.ToString()
    standardUserEnabled = $standard.Enabled
    standardUserIsAdmin = $isAdmin
    defaultShell = $defaultShell
    stdinRoundTrip = $true
} | ConvertTo-Json -Compress
'@
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($guestScript))
$processInfo = [Diagnostics.ProcessStartInfo]::new()
$processInfo.FileName = (Get-Command ssh.exe).Source
$processInfo.UseShellExecute = $false
$processInfo.RedirectStandardInput = $true
$processInfo.RedirectStandardOutput = $true
$processInfo.RedirectStandardError = $true
foreach ($argument in @('-F', 'none', '-T', '-i', $IdentityFile, '-o', 'IdentitiesOnly=yes',
    '-o', 'IdentityAgent=none',
    '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=accept-new',
    '-o', "UserKnownHostsFile=$KnownHostsFile", '-o', 'ConnectTimeout=5',
    '-p', "$Port", 'fixtureadmin@127.0.0.1',
    'powershell.exe', '-NoProfile', '-EncodedCommand', $encoded)) {
    $processInfo.ArgumentList.Add($argument)
}
$process = [Diagnostics.Process]::Start($processInfo)
$stdout = $process.StandardOutput.ReadToEndAsync()
$stderr = $process.StandardError.ReadToEndAsync()
try {
    $process.StandardInput.WriteLine('ssh-copy-id stdin smoke')
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(30000)) {
        $process.Kill($true)
        throw 'SSH smoke test timed out'
    }
    $errorText = $stderr.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw "SSH failed ($($process.ExitCode)): $errorText" }
    if ($errorText) { Write-Verbose $errorText }
    $result = $stdout.GetAwaiter().GetResult() | ConvertFrom-Json
    if (-not $result.stdinRoundTrip) { throw 'Guest result is missing stdin verification' }
    $result | ConvertTo-Json
} finally {
    $process.Dispose()
}
