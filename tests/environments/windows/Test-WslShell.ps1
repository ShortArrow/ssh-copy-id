# Requires PowerShell 7. Runs only against a disposable fixture that has WSL 2
# and a distribution whose default user has the test key in its
# ~/.ssh/authorized_keys.
# Validation row W10: runs the shell-family probe, lists the SSH and WSL
# environment variables, and tries the test key, first with the Windows sshd's
# default shell set to each way of starting WSL, then against an sshd running
# inside WSL, reached through the Windows sshd's TCP forwarding. Every switch of
# the default shell is backed by a scheduled task that restores Windows
# PowerShell, since a default shell that cannot run the control command leaves
# no other way back.
param(
    [string]$IdentityFile = 'work/windows-fixture-ps/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture-w10/known_hosts',
    [int]$Port = 22225,
    [string]$WslKey = 'work/windows-fixture-w10/wsl_key',
    [string]$WslUser = 'wsluser',
    [string]$Distribution = 'Ubuntu-24.04',
    [int]$WslSshPort = 2222,
    [string]$EvidenceRoot = 'work/windows-fixture-w10'
)
$ErrorActionPreference = 'Stop'
$probe = 'echo ssh-copy-id-probe:%OS%:$0/$home'
$environment = 'echo user=$(id -un) home=$HOME; env | sort | grep -E "^(SSH_|WSL)"'
$powershell = 'C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe'
$variants = @(
    [ordered]@{ name = 'wsl.exe'; shell = 'C:\Windows\System32\wsl.exe'; option = $null }
    [ordered]@{ name = 'wsl.exe, option --'; shell = 'C:\Windows\System32\wsl.exe'; option = '--' }
    [ordered]@{ name = 'bash.exe'; shell = 'C:\Windows\System32\bash.exe'; option = $null }
)

function Invoke-Ssh([string[]]$Options, [string]$Command) {
    $info = [Diagnostics.ProcessStartInfo]::new((Get-Command ssh.exe).Source)
    foreach ($argument in @('-F', 'none', '-T', '-o', 'IdentitiesOnly=yes', '-o', 'IdentityAgent=none',
            '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10') + $Options + @($Command)) { $info.ArgumentList.Add($argument) }
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::Latin1
    $info.StandardErrorEncoding = [Text.Encoding]::Latin1
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(60000)) {
        $process.Kill($true)
        return [pscustomobject]@{ Code = 'timeout'; Out = ''; Err = '' }
    }
    [pscustomobject]@{ Code = $process.ExitCode; Out = $stdout.GetAwaiter().GetResult(); Err = $stderr.GetAwaiter().GetResult() }
}
function Get-WindowsOptions([string]$Key) {
    @('-i', $Key, '-o', 'StrictHostKeyChecking=yes', '-o', "UserKnownHostsFile=$KnownHostsFile", '-p', "$Port", 'fixtureadmin@127.0.0.1')
}
function Get-WslOptions {
    $jump = "ssh -F none -i $IdentityFile -o IdentitiesOnly=yes -o IdentityAgent=none -o BatchMode=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$KnownHostsFile -p $Port -W 127.0.0.1:$WslSshPort fixtureadmin@127.0.0.1"
    @('-i', $WslKey, '-o', "ProxyCommand=$jump", '-o', 'StrictHostKeyChecking=accept-new',
      '-o', "UserKnownHostsFile=$EvidenceRoot/wsl_known_hosts", "$WslUser@wsl")
}
# Runs under any default shell that can start powershell.exe: Windows PowerShell
# itself, or a WSL shell through interop. A WSL shell drops the backslashes of
# an unquoted Windows path, so the command finds powershell.exe on the PATH.
function Invoke-Admin([string]$Script) {
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
    Invoke-Ssh (Get-WindowsOptions $IdentityFile) "powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded"
}
function Assert-Admin([string]$Script) {
    $result = Invoke-Admin $Script
    if ($result.Code -ne 0) { throw "Guest command failed ($($result.Code)): $($result.Err)" }
    $result.Out
}
function Show-Bytes([string]$Text) {
    ($Text.ToCharArray() | ForEach-Object {
        $code = [int]$_
        if ($code -eq 13) { '\r' } elseif ($code -eq 10) { '\n' } elseif ($code -lt 32 -or $code -gt 126) { '\x{0:x2}' -f $code } else { [string]$_ }
    }) -join ''
}
function Get-Run([string]$Target, [string]$Label, [string[]]$Options, [string]$Command) {
    Write-Host "$Target | $Label"
    $result = Invoke-Ssh $Options $Command
    Write-Host "  exit $($result.Code)"
    [ordered]@{ target = $Target; run = $Label; exit = $result.Code; stdout = Show-Bytes $result.Out; stderr = Show-Bytes $result.Err }
}
function Wait-PowerShell {
    foreach ($attempt in 1..40) {
        $result = Invoke-Ssh (Get-WindowsOptions $IdentityFile) $probe
        if ($result.Out -match '%OS%:/[A-Za-z]:\\') { Write-Host 'Windows PowerShell is the default shell'; return }
        Write-Host "  waiting for Windows PowerShell ($($result.Code))"
        Start-Sleep -Seconds 15
    }
    throw 'Windows PowerShell did not come back as the default shell'
}

$guard = "if (-not (Test-Path C:\OEM\fixture.marker)) { throw 'Not a disposable fixture' }"
$restore = "Set-ItemProperty HKLM:\SOFTWARE\OpenSSH -Name DefaultShell -Value $powershell; Remove-ItemProperty HKLM:\SOFTWARE\OpenSSH -Name DefaultShellCommandOption -ErrorAction SilentlyContinue"
$encodedRestore = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($restore))
$schedule = "$guard; `$action = New-ScheduledTaskAction -Execute powershell.exe -Argument '-NoProfile -EncodedCommand $encodedRestore'; Register-ScheduledTask -TaskName w10-restore-shell -Action `$action -Trigger (New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(3)) -User SYSTEM -RunLevel Highest -Force | Out-Null"
$unschedule = 'Unregister-ScheduledTask -TaskName w10-restore-shell -Confirm:$false -ErrorAction SilentlyContinue; exit 0'

$results = [Collections.Generic.List[object]]::new()
Wait-PowerShell
try {
    foreach ($variant in $variants) {
        $set = "Set-ItemProperty HKLM:\SOFTWARE\OpenSSH -Name DefaultShell -Value '$($variant.shell)'"
        if ($variant.option) { $set += "; Set-ItemProperty HKLM:\SOFTWARE\OpenSSH -Name DefaultShellCommandOption -Value '$($variant.option)'" }
        Assert-Admin "$schedule; $set" | Out-Null
        try {
            foreach ($run in @(
                    @('probe', (Get-WindowsOptions $IdentityFile), $probe),
                    @('environment', (Get-WindowsOptions $IdentityFile), $environment),
                    @('test key', (Get-WindowsOptions $WslKey), 'exit 0'))) {
                $results.Add((Get-Run "Windows sshd, default shell $($variant.name)" @run))
            }
        } finally {
            Invoke-Admin "$restore; $unschedule" | Out-Null
            Wait-PowerShell
            Assert-Admin $unschedule | Out-Null
        }
    }

    # WSL stops its VM once no wsl.exe process is left, so this session keeps
    # one running while the sshd inside WSL is measured.
    $info = [Diagnostics.ProcessStartInfo]::new((Get-Command ssh.exe).Source)
    foreach ($argument in @('-F', 'none', '-T', '-o', 'IdentitiesOnly=yes', '-o', 'IdentityAgent=none', '-o', 'BatchMode=yes') +
            (Get-WindowsOptions $IdentityFile) +
            @("wsl.exe -d $Distribution -u root -- sh -c 'mkdir -p /run/sshd && /usr/sbin/sshd -p $WslSshPort && sleep 300'")) {
        $info.ArgumentList.Add($argument)
    }
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $keepalive = [Diagnostics.Process]::Start($info)
    try {
        Start-Sleep -Seconds 20
        foreach ($run in @(
                @('probe', (Get-WslOptions), $probe),
                @('environment', (Get-WslOptions), $environment),
                @('test key', (Get-WslOptions), 'exit 0'))) {
            $results.Add((Get-Run 'sshd inside WSL' @run))
        }
    } finally {
        Assert-Admin "& wsl.exe --terminate $Distribution" | Out-Null
        if (-not $keepalive.WaitForExit(30000)) { $keepalive.Kill($true) }
    }
} finally {
    Invoke-Admin "$restore; $unschedule" | Out-Null
    $results | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'wsl-shell.json')
}
Get-Content -LiteralPath (Join-Path $EvidenceRoot 'wsl-shell.json')
