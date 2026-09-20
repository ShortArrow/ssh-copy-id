# Requires PowerShell 7. Runs only against the disposable loopback fixture.
param(
    [string]$IdentityFile = 'work/windows-fixture/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture/known_hosts',
    [int]$Port = 22222
)
$ErrorActionPreference = 'Stop'
$run = [Guid]::NewGuid().ToString('N')
$directory = Join-Path (Get-Location) "work/windows-fixture/standard-$run"
New-Item -ItemType Directory -Path $directory | Out-Null
$key = Join-Path $directory 'identity'
$checks = [Collections.Generic.List[string]]::new()

function Invoke-Client($Executable, [string[]]$ClientArguments, [string]$InputText = '') {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = (Get-Command $Executable).Source
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $ClientArguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    try {
        $process.StandardInput.Write($InputText)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(30000)) {
            $process.Kill($true)
            throw "$Executable timed out"
        }
        [pscustomobject]@{ Code = $process.ExitCode; Out = $stdout.GetAwaiter().GetResult(); Err = $stderr.GetAwaiter().GetResult() }
    } finally { $process.Dispose() }
}
function Get-Options($SelectedKey) {
    @('-F', 'none', '-i', $SelectedKey, '-o', 'IdentitiesOnly=yes',
      '-o', 'IdentityAgent=none', '-o', 'BatchMode=yes',
      '-o', 'StrictHostKeyChecking=yes', '-o', "UserKnownHostsFile=$KnownHostsFile",
      '-o', 'ConnectTimeout=5')
}
function Invoke-Guest($User, $SelectedKey, $Script, $InputText = '') {
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
    Invoke-Client ssh.exe ((Get-Options $SelectedKey) + @('-v', '-T', '-p', "$Port", "$User@127.0.0.1",
        'powershell.exe', '-NoProfile', '-NonInteractive', '-EncodedCommand', $encoded)) $InputText
}
function Assert-Success($Result) {
    if ($Result.Code -ne 0) { throw "Client failed ($($Result.Code)): $($Result.Err) $($Result.Out)" }
}
function Assert-Rejected($Result) {
    if ($Result.Code -ne 255 -or $Result.Err -notmatch 'Permission denied') {
        throw "Expected authentication rejection: $($Result.Code) $($Result.Err) $($Result.Out)"
    }
}

# The marker identifies only resources created by this invocation. Existing .ssh
# directories are refused, and cleanup never recursively removes a profile.
$cleanup = @'
$ErrorActionPreference = 'Stop'
$marker = 'C:\OEM\standard-__RUN__.txt'
if (Test-Path -LiteralPath $marker) {
    $folder = [IO.File]::ReadAllText($marker)
    if ($folder -ne 'C:\Users\fixtureuser\.ssh') { throw 'Unexpected cleanup path' }
    $transfer = 'C:\Users\fixtureuser\ssh-copy-id-sftp-__RUN__.tmp'
    if (Test-Path -LiteralPath $transfer) { Remove-Item -LiteralPath $transfer }
    $authorized = Join-Path $folder 'authorized_keys'
    if (Test-Path -LiteralPath $authorized) { Remove-Item -LiteralPath $authorized }
    if (Test-Path -LiteralPath $folder) {
        if (@(Get-ChildItem -LiteralPath $folder -Force).Count) { throw 'Unexpected files remain in fixture .ssh' }
        Remove-Item -LiteralPath $folder
    }
    Remove-Item -LiteralPath $marker
}
'@
$cleanup = $cleanup.Replace('__RUN__', $run)
$provisionAttempted = $false
try {
    $generated = Invoke-Client ssh-keygen.exe @('-q', '-t', 'ed25519', '-N', '', '-f', $key)
    Assert-Success $generated
    $public = [IO.File]::ReadAllText("$key.pub").Trim()
    Assert-Rejected (Invoke-Guest fixtureuser $key 'exit 0')
    $checks.Add('unregistered key rejected')

    $provision = @'
$ErrorActionPreference = 'Stop'
if (-not (Test-Path C:\OEM\fixture.marker)) { throw 'Not a disposable fixture' }
$user = Get-LocalUser fixtureuser
$profile = Get-CimInstance Win32_UserProfile | Where-Object SID -eq $user.SID.Value
if (-not $profile) { throw 'Sign in to the guest once as fixtureuser to initialize its profile, then rerun.' }
if ($profile.LocalPath -ne 'C:\Users\fixtureuser') { throw 'Unexpected profile path' }
$folder = 'C:\Users\fixtureuser\.ssh'
if (Test-Path -LiteralPath $folder) { throw 'Existing .ssh directory: use a clean fixture' }
[IO.File]::WriteAllText('C:\OEM\standard-__RUN__.txt', $folder)
New-Item -ItemType Directory -Path $folder -Force | Out-Null
$keyText = [Console]::In.ReadToEnd().Trim()
if ($keyText -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: .*)?$') { throw 'Invalid fixture public key' }
$file = Join-Path $folder 'authorized_keys'
[IO.File]::WriteAllText($file, $keyText + "`n", [Text.UTF8Encoding]::new($false))
foreach ($path in @($folder, $file)) {
    & icacls.exe $path /inheritance:r /grant:r "*$($user.SID.Value):(F)" '*S-1-5-18:(F)' '*S-1-5-32-544:(F)'
    if ($LASTEXITCODE) { throw 'ACL grant failed' }
    & icacls.exe $path /setowner "*$($user.SID.Value)"
    if ($LASTEXITCODE) { throw 'ACL owner failed' }
}
'@
    $provisionAttempted = $true
    Assert-Success (Invoke-Guest fixtureadmin $IdentityFile ($provision.Replace('__RUN__', $run)) $public)
    $probe = @'
$ErrorActionPreference = 'Stop'
$text = [Console]::In.ReadToEnd()
if ($text -ne "line one`r`nline two`n") { throw 'stdin bytes changed' }
if ((& whoami) -notmatch '\\fixtureuser$') { throw 'Unexpected identity' }
Write-Output 'standard-user-stdin-ok'
'@
    $result = Invoke-Guest fixtureuser $key $probe "line one`r`nline two`n"
    if ($result.Code -ne 0) {
        $diagnostic = Invoke-Guest fixtureadmin $IdentityFile 'Get-Content C:\Users\fixtureuser\.ssh\authorized_keys; icacls C:\Users\fixtureuser\.ssh; icacls C:\Users\fixtureuser\.ssh\authorized_keys; Get-Acl C:\Users\fixtureuser\.ssh\authorized_keys | Format-List Owner; Get-LocalUser fixtureuser | Select-Object SID | ConvertTo-Json'
        Write-Host $diagnostic.Out
    }
    Assert-Success $result
    if ($result.Out.Trim() -ne 'standard-user-stdin-ok') { throw 'Unexpected probe output' }
    $checks.Add('standard-user authentication and mixed-newline stdin preserved')
    $exitResult = Invoke-Client ssh.exe ((Get-Options $key) + @('-T', '-p', "$Port", 'fixtureuser@127.0.0.1', 'cmd.exe /d /c exit 37'))
    if ($exitResult.Code -ne 37) { throw "Expected remote exit 37, got $($exitResult.Code)" }
    $checks.Add('cmd remote exit code 37 preserved')

    $source = Join-Path $directory 'source.txt'
    $received = Join-Path $directory 'received.txt'
    $payload = [byte[]](0, 10, 13, 34, 39, 128, 255, 65)
    [IO.File]::WriteAllBytes($source, $payload)
    $remote = "ssh-copy-id-sftp-$run.tmp"
    $batch = "put `"$($source.Replace('\', '/'))`" $remote`nget $remote `"$($received.Replace('\', '/'))`"`nrm $remote`nquit`n"
    $transfer = Invoke-Client sftp.exe ((Get-Options $key) + @('-b', '-', '-P', "$Port", 'fixtureuser@127.0.0.1')) $batch
    Assert-Success $transfer
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($received)) -ne [Convert]::ToBase64String($payload)) { throw 'SFTP content changed' }
    $checks.Add('SFTP binary upload/download/delete')
} finally {
    if ($provisionAttempted) { Assert-Success (Invoke-Guest fixtureadmin $IdentityFile $cleanup) }
}
# Keep the key until the post-cleanup negative check has completed.
Assert-Rejected (Invoke-Guest fixtureuser $key 'exit 0')
$checks.Add('removed key rejected')
[ordered]@{ checks = $checks.ToArray(); passed = $checks.Count; evidenceDirectory = $directory } | ConvertTo-Json
