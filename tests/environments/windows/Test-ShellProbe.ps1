# Requires PowerShell 7. Runs only against the disposable loopback fixture.
# Validation row W09, Windows side: runs each candidate shell-family probe as the
# administrator's SSH command under cmd.exe, Windows PowerShell, and pwsh as the
# sshd default shell, without a startup step and with one that prints a line
# (cmd's AutoRun; a PowerShell profile under the default execution policy and
# under RemoteSigned). Restores the default shell given by -RestoreShell and
# removes every startup setting it made.
param(
    [string]$IdentityFile = 'work/windows-fixture-ps/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture-pwsh/known_hosts',
    [int]$Port = 22224,
    [ValidateSet('cmd', 'WindowsPowerShell', 'pwsh')]
    [string]$RestoreShell = 'pwsh',
    [string]$EvidenceRoot = 'work/windows-fixture-pwsh'
)
$ErrorActionPreference = 'Stop'
$fixture = @{ IdentityFile = $IdentityFile; KnownHostsFile = $KnownHostsFile; Port = $Port }
$candidates = [ordered]@{ B = 'echo ssh-copy-id-probe:%OS%:$0'; C = 'echo ssh-copy-id-probe:%OS%:$0/$home' }

function Invoke-Ssh([string]$Command) {
    $info = [Diagnostics.ProcessStartInfo]::new((Get-Command ssh.exe).Source)
    foreach ($argument in @('-F', 'none', '-T', '-i', $IdentityFile, '-o', 'IdentitiesOnly=yes',
            '-o', 'IdentityAgent=none', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
            '-o', "UserKnownHostsFile=$KnownHostsFile", '-o', 'ConnectTimeout=5', '-p', "$Port",
            'fixtureadmin@127.0.0.1', $Command)) { $info.ArgumentList.Add($argument) }
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::Latin1
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(30000)) { $process.Kill($true); throw 'ssh timed out' }
    [pscustomobject]@{ Code = $process.ExitCode; Out = $stdout.GetAwaiter().GetResult(); Err = $stderr.GetAwaiter().GetResult() }
}
function Invoke-Admin([string]$Script) {
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
    $result = Invoke-Ssh "powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded"
    if ($result.Code -ne 0) { throw "Guest setup failed ($($result.Code)): $($result.Err)" }
}
function Show-Bytes([string]$Text) {
    ($Text.ToCharArray() | ForEach-Object {
        $code = [int]$_
        if ($code -eq 13) { '\r' } elseif ($code -eq 10) { '\n' } elseif ($code -lt 32 -or $code -gt 126) { '\x{0:x2}' -f $code } else { [string]$_ }
    }) -join ''
}

$profiles = '$profiles = @((Join-Path ([Environment]::GetFolderPath(''MyDocuments'')) ''WindowsPowerShell\Microsoft.PowerShell_profile.ps1''), (Join-Path ([Environment]::GetFolderPath(''MyDocuments'')) ''PowerShell\Microsoft.PowerShell_profile.ps1''))'
$startups = [ordered]@{
    none = '$null'
    'cmd AutoRun' = 'New-Item -Path ''HKCU:\Software\Microsoft\Command Processor'' -Force | Out-Null; Set-ItemProperty -Path ''HKCU:\Software\Microsoft\Command Processor'' -Name AutoRun -Value ''echo startup-line'''
    'profile, default policy' = "$profiles; foreach (`$p in `$profiles) { New-Item -ItemType File -Force -Path `$p -Value ""'startup-line'`n"" | Out-Null }"
    'profile, RemoteSigned' = "$profiles; foreach (`$p in `$profiles) { New-Item -ItemType File -Force -Path `$p -Value ""'startup-line'`n"" | Out-Null }; Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force; & (Join-Path `$env:ProgramFiles 'PowerShell\7\pwsh.exe') -NoProfile -Command 'Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force'"
}
$reset = "if (-not (Test-Path C:\OEM\fixture.marker)) { throw 'Not a disposable fixture' }; Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Command Processor' -Name AutoRun -ErrorAction SilentlyContinue; $profiles; foreach (`$p in `$profiles) { if (Test-Path -LiteralPath `$p) { Remove-Item -LiteralPath `$p } }; Set-ExecutionPolicy -Scope CurrentUser Undefined -Force; & (Join-Path `$env:ProgramFiles 'PowerShell\7\pwsh.exe') -NoProfile -Command 'Set-ExecutionPolicy -Scope CurrentUser Undefined -Force'"

$results = [Collections.Generic.List[object]]::new()
try {
    foreach ($shell in 'cmd', 'WindowsPowerShell', 'pwsh') {
        & "$PSScriptRoot/Set-DefaultShell.ps1" -Shell $shell @fixture | Out-Null
        foreach ($startup in $startups.Keys) {
            if ($shell -eq 'cmd' -and $startup -like 'profile*') { continue }
            if ($shell -ne 'cmd' -and $startup -eq 'cmd AutoRun') { continue }
            Invoke-Admin $reset
            Invoke-Admin $startups[$startup]
            foreach ($name in $candidates.Keys) {
                $result = Invoke-Ssh $candidates[$name]
                $results.Add([ordered]@{
                    shell = $shell; startup = $startup; candidate = $name; exit = $result.Code
                    stdout = Show-Bytes $result.Out; stderr = Show-Bytes $result.Err
                })
            }
        }
    }
} finally {
    Invoke-Admin $reset
    & "$PSScriptRoot/Set-DefaultShell.ps1" -Shell $RestoreShell @fixture | Out-Null
}
$results | ConvertTo-Json | Tee-Object -FilePath (Join-Path $EvidenceRoot 'shell-probes.json')
