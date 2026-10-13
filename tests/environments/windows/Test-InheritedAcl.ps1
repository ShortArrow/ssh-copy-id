# Requires PowerShell 7. Runs only against the disposable loopback fixture.
# Validation row W07, first check: whether a key file that the standard user
# creates in its own session, keeping the ACL inherited from its profile, passes
# sshd's permission check. An extra writable entry for BUILTIN\Users must then
# make the same key fail, showing that the check ran.
param(
    [string]$IdentityFile = 'work/windows-fixture/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture/known_hosts',
    [int]$Port = 22222,
    [string]$EvidenceRoot = 'work/windows-fixture'
)
$ErrorActionPreference = 'Stop'
$run = [Guid]::NewGuid().ToString('N')
$directory = Join-Path (Get-Location) "$EvidenceRoot/inherited-acl-$run"
New-Item -ItemType Directory -Path $directory | Out-Null
$key = Join-Path $directory 'identity'
$checks = [Collections.Generic.List[string]]::new()
$evidence = [ordered]@{}

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
function Get-Options {
    @('-F', 'none', '-o', 'IdentityAgent=none', '-o', 'StrictHostKeyChecking=yes',
      '-o', "UserKnownHostsFile=$KnownHostsFile", '-o', 'ConnectTimeout=5', '-T', '-p', "$Port")
}
function Get-Encoded($Script) {
    @('powershell.exe', '-NoProfile', '-NonInteractive', '-EncodedCommand',
      [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script)))
}
function Invoke-WithKey($User, $SelectedKey, $Script, $InputText = '') {
    Invoke-Client ssh.exe ((Get-Options) + @('-i', $SelectedKey, '-o', 'IdentitiesOnly=yes',
        '-o', 'BatchMode=yes', "$User@127.0.0.1") + (Get-Encoded $Script)) $InputText
}
# The user's own session, as ssh-copy-id has it before a key is installed.
function Invoke-WithPassword($User, $Script, $InputText = '') {
    $askpass = Join-Path $directory 'askpass.cmd'
    Set-Content -LiteralPath $askpass -Value '@echo Fixture-Only_47!' -Encoding ascii
    try {
        $env:SSH_ASKPASS = $askpass
        $env:SSH_ASKPASS_REQUIRE = 'force'
        Invoke-Client ssh.exe ((Get-Options) + @('-o', 'PreferredAuthentications=password',
            '-o', 'PubkeyAuthentication=no', '-o', 'NumberOfPasswordPrompts=1', "$User@127.0.0.1") +
            (Get-Encoded $Script)) $InputText
    } finally {
        $env:SSH_ASKPASS = $null
        $env:SSH_ASKPASS_REQUIRE = $null
        Remove-Item -LiteralPath $askpass -ErrorAction SilentlyContinue
    }
}
function Assert-Success($Result) {
    if ($Result.Code -ne 0) { throw "Client failed ($($Result.Code)): $($Result.Err) $($Result.Out)" }
}
function Assert-Rejected($Result) {
    if ($Result.Code -ne 255 -or $Result.Err -notmatch 'Permission denied') {
        throw "Expected authentication rejection: $($Result.Code) $($Result.Err) $($Result.Out)"
    }
}
function Get-Acls {
    $result = Invoke-WithKey fixtureadmin $IdentityFile @'
foreach ($path in 'C:\Users\fixtureuser', 'C:\Users\fixtureuser\.ssh', 'C:\Users\fixtureuser\.ssh\authorized_keys') {
    "== $path owner $((Get-Acl -LiteralPath $path).Owner)"
    & icacls.exe $path
}
'@
    Assert-Success $result
    $result.Out
}
function Get-SshdEvents {
    $result = Invoke-WithKey fixtureadmin $IdentityFile @'
Get-WinEvent -LogName OpenSSH/Operational -MaxEvents 8 | ForEach-Object { "$($_.TimeCreated.ToString('o')) $($_.Message)" }
'@
    Assert-Success $result
    $result.Out
}

# The marker identifies only resources created by this invocation. An existing
# .ssh directory is refused, and cleanup never recursively removes a profile.
$cleanup = @'
$ErrorActionPreference = 'Stop'
$marker = 'C:\OEM\inherited-acl-__RUN__.txt'
if (Test-Path -LiteralPath $marker) {
    $folder = [IO.File]::ReadAllText($marker)
    if ($folder -ne 'C:\Users\fixtureuser\.ssh') { throw 'Unexpected cleanup path' }
    $authorized = Join-Path $folder 'authorized_keys'
    if (Test-Path -LiteralPath $authorized) { Remove-Item -LiteralPath $authorized }
    if (Test-Path -LiteralPath $folder) {
        if (@(Get-ChildItem -LiteralPath $folder -Force).Count) { throw 'Unexpected files remain in fixture .ssh' }
        Remove-Item -LiteralPath $folder
    }
    Remove-Item -LiteralPath $marker
}
'@.Replace('__RUN__', $run)
$reserved = $false
try {
    Assert-Success (Invoke-Client ssh-keygen.exe @('-q', '-t', 'ed25519', '-N', '', '-f', $key))
    $public = [IO.File]::ReadAllText("$key.pub").Trim()

    $reserve = @'
$ErrorActionPreference = 'Stop'
if (-not (Test-Path C:\OEM\fixture.marker)) { throw 'Not a disposable fixture' }
$folder = 'C:\Users\fixtureuser\.ssh'
if (Test-Path -LiteralPath $folder) { throw 'Existing .ssh directory: use a clean fixture' }
[IO.File]::WriteAllText('C:\OEM\inherited-acl-__RUN__.txt', $folder)
'@.Replace('__RUN__', $run)
    $reserved = $true
    Assert-Success (Invoke-WithKey fixtureadmin $IdentityFile $reserve)

    $create = @'
$ErrorActionPreference = 'Stop'
$folder = Join-Path $env:USERPROFILE '.ssh'
New-Item -ItemType Directory -Path $folder | Out-Null
$keyText = [Console]::In.ReadToEnd().Trim()
if ($keyText -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: .*)?$') { throw 'Invalid fixture public key' }
[IO.File]::WriteAllText((Join-Path $folder 'authorized_keys'), $keyText + "`n", [Text.UTF8Encoding]::new($false))
'@
    Assert-Success (Invoke-WithPassword fixtureuser $create $public)
    $evidence.inheritedAcls = Get-Acls

    Assert-Success (Invoke-WithKey fixtureuser $key 'exit 0')
    $checks.Add('key file created by the user with the inherited profile ACL accepted')

    $widen = @'
$ErrorActionPreference = 'Stop'
& icacls.exe 'C:\Users\fixtureuser\.ssh\authorized_keys' /grant '*S-1-5-32-545:(M)'
if ($LASTEXITCODE) { throw 'ACL grant failed' }
'@
    Assert-Success (Invoke-WithKey fixtureadmin $IdentityFile $widen)
    $evidence.widenedAcls = Get-Acls
    Assert-Rejected (Invoke-WithKey fixtureuser $key 'exit 0')
    $evidence.widenedSshdEvents = Get-SshdEvents
    $checks.Add('the same key rejected after granting BUILTIN\Users modify')
} finally {
    if ($reserved) { Assert-Success (Invoke-WithKey fixtureadmin $IdentityFile $cleanup) }
}
Assert-Rejected (Invoke-WithKey fixtureuser $key 'exit 0')
$checks.Add('removed key rejected')
$report = [ordered]@{ checks = $checks.ToArray(); passed = $checks.Count; evidence = $evidence; evidenceDirectory = $directory }
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $directory 'result.json')
$report | ConvertTo-Json
