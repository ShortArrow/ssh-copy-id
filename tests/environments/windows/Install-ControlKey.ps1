# Requires PowerShell 7. Runs only against the disposable loopback fixture.
# Replaces the administrator control key through the documented fixture password
# when the existing control key is unusable. This is fixture provisioning; the
# ssh-copy-id product never authenticates with a password from a script.
param(
    [string]$IdentityFile = 'work/windows-fixture/smoke_key',
    [string]$KnownHostsFile = 'work/windows-fixture/known_hosts',
    [int]$Port = 22222
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $IdentityFile)) {
    & ssh-keygen.exe -q -t ed25519 -N '' -f $IdentityFile
    if ($LASTEXITCODE -ne 0) { throw 'Control key generation failed.' }
}
$publicKey = [IO.File]::ReadAllText("$IdentityFile.pub").Trim()
$guestScript = @'
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath 'C:\OEM\fixture.marker')) { throw 'Not a disposable fixture' }
$adminKeys = Join-Path $env:ProgramData 'ssh\administrators_authorized_keys'
$publicKey = [Console]::In.ReadToEnd().Trim()
if ($publicKey -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: .*)?$') { throw 'Unexpected control key format.' }
[IO.File]::WriteAllText($adminKeys, $publicKey + "`n", [Text.UTF8Encoding]::new($false))
$aclArguments = @($adminKeys, '/inheritance:r', '/grant:r', '*S-1-5-32-544:F', '*S-1-5-18:F')
& icacls.exe @aclArguments | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Control key ACL setup failed.' }
Write-Output 'control key installed'
'@
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($guestScript))
$askpass = Join-Path ([IO.Path]::GetTempPath()) "ssh-copy-id-fixture-askpass-$PID.cmd"
Set-Content -LiteralPath $askpass -Value '@echo Fixture-Only_47!' -Encoding ascii
try {
    $env:SSH_ASKPASS = $askpass
    $env:SSH_ASKPASS_REQUIRE = 'force'
    $output = $publicKey | & ssh.exe -F none -T -o PreferredAuthentications=password -o PubkeyAuthentication=no `
        -o NumberOfPasswordPrompts=1 -o StrictHostKeyChecking=accept-new -o "UserKnownHostsFile=$KnownHostsFile" `
        -o ConnectTimeout=5 -p $Port fixtureadmin@127.0.0.1 `
        powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Control key installation failed ($LASTEXITCODE): $output" }
    $output
} finally {
    $env:SSH_ASKPASS = $null
    $env:SSH_ASKPASS_REQUIRE = $null
    Remove-Item -LiteralPath $askpass -ErrorAction SilentlyContinue
}
