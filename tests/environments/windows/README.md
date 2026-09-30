# Windows Destination Fixture (dockur)

This fixture runs a Windows VM inside a Linux container using
[dockur/windows](https://github.com/dockur/windows). It does not require switching
Docker Desktop to Windows containers. Hyper-V remains the fallback if KVM is not
available. Docker and the bootstrap dependencies are test tooling only.

## Current Validation

The current Docker Desktop WSL2 backend (`6.6.87.2-microsoft-standard-WSL2`, x86_64)
passed both opening `/dev/kvm` (API version 12) and `KVM_CREATE_VM` in a disposable
container. The Compose model and PowerShell script syntax are validated.
The Windows 11 Enterprise Evaluation guest reached the desktop. Initial setup
remained at "Almost there." for over seven hours. Recreating the container with
`WINDOWS_HV=N` while retaining its disk allowed it to reach the desktop; this does
not distinguish the effect of restarting from disabling Hyper-V enlightenments.
The guest initially had no `sshd` service. Rerunning the OEM script from an
elevated guest console completed provisioning. A running container is not a
readiness signal.

Validated on 2026-09-20:

- Windows 11 Enterprise Evaluation 25H2, build `26200.6584`.
- Guest OpenSSH executable file version `9.5.5.1`; SSH banner `OpenSSH_for_Windows_9.5`.
- Host client `OpenSSH_for_Windows_9.5p2`.
- Administrator public-key authentication, remote PowerShell execution through
  the default `cmd.exe` shell, and stdin delivery passed `Test-Smoke.ps1`.
- `sshd` is running; `fixtureuser` is enabled and is not an administrator.
- An SFTP batch session succeeded and reported `/C:/Users/fixtureadmin`.

The standard-user test also passed five assertions covering absent/installed/
removed keys, mixed-newline stdin, remote exit 37, and binary SFTP transfer.
It requires an initialized user profile. On 2026-09-23 a second project,
`ssh-copy-id-win11-ps`, was created from a copy of this disk and switched to
Windows PowerShell as the sshd default shell; both account tests passed there.
The `pwsh` default shell, key installation by the product, and the broader ACL
matrix remain pending. See the [validation matrix](../../../docs/validation.md)
for scope and findings.

## Start a Fixture

From the repository root:

To enable the automated smoke test, create its disposable key **before** first
startup (PowerShell 7; use an unused key path):

```powershell
New-Item -ItemType Directory -Force work/windows-fixture | Out-Null
ssh-keygen -q -t ed25519 -N '' -f work/windows-fixture/smoke_key
Copy-Item work/windows-fixture/smoke_key.pub tests/environments/windows/oem/smoke-key.pub
```

Then start the guest:

```powershell
docker compose -p ssh-copy-id-win11 -f tests/environments/windows/compose.yaml up -d
docker compose -p ssh-copy-id-win11 -f tests/environments/windows/compose.yaml logs -f
```

First startup downloads and installs Windows and then provisions OpenSSH. Allow
time for the multi-GB download and guest installation. The default version selector
is `11e` (Windows 11 Enterprise); the container image is pinned, but the downloaded
Windows media is not. Record the guest OS build and OpenSSH version with test results.

- Web console: <http://127.0.0.1:8006>
- SSH: `127.0.0.1:22222`
- Administrator: `fixtureadmin`
- Standard user: `fixtureuser` (created by the OEM script)
- Disposable fixture password for both accounts: `Fixture-Only_47!`

Credentials are public test fixtures. Keep the host bindings on loopback and do
not use real keys or data in the guest. Other containers sharing its Docker
network may reach it. The project source and host SSH configuration are not mounted.

Use the guest console to check `C:\OEM\setup-sshd.log` and
`C:\OEM\ssh-ready.txt`. The log includes SSH host-key fingerprints. Compare them
with the first connection's prompt rather than disabling host-key verification.
Use a separate known-hosts file for this fixture:

```powershell
New-Item -ItemType Directory -Force work/windows-fixture | Out-Null
ssh -o UserKnownHostsFile=work/windows-fixture/known_hosts -p 22222 fixtureuser@127.0.0.1
```

The bootstrap keeps the default administrator key-file configuration. It creates
the standard account, enables the guest SSH service, and opens the guest firewall
for SSH. These operations provision a disposable test server; they are not
operations performed by the ssh-copy-id product. Run the OEM script only inside
the guest. By default it installs no authorized keys. If the ignored
`oem/smoke-key.pub` exists before first setup, it installs that disposable Ed25519
public key into the administrator key file for automation. The corresponding
private key stays on the host, and the standard user starts without authorized
keys. Rerunning bootstrap with this optional file resets the administrator key
file to that single fixture key; use it only in this disposable environment.

## Readiness and Troubleshooting

Check each layer in order:

1. In the guest, check `C:\OEM\setup-sshd.log` and `Get-Service sshd`.
   OpenSSH Server is installed by the bootstrap; do not assume Windows includes it.
   Windows capability installation can take several minutes. Its diagnostics are
   in `C:\Windows\Logs\DISM\dism.log` and `C:\Windows\Logs\CBS\CBS.log`.
2. If provisioning was interrupted, run `powershell.exe -NoProfile
   -ExecutionPolicy Bypass -File C:\OEM\setup-sshd.ps1` from an elevated guest
   console. Wait for `C:\OEM\ssh-ready.txt`.
3. Check the guest listener and firewall before diagnosing Docker's host mapping.
   The configured path is host loopback TCP 22222 to container TCP 22 to guest TCP 22.
4. After provisioning a disposable control key, run the host-side smoke test:

   ```powershell
   ./tests/environments/windows/Test-Smoke.ps1
   ```

   It uses `work/windows-fixture/smoke_key` and a separate known-hosts file by
   default. It accepts a previously unknown host key for this loopback fixture,
   rejects a changed key, and checks SSH execution, stdin delivery, the standard
   account's group membership, and the guest OS/OpenSSH versions. It requires
   PowerShell 7 on the host. It does not test the ssh-copy-id product.

   After the SSH check, test the SFTP subsystem using the recorded host key:

   ```powershell
   @('pwd', 'quit') | sftp -F none -b - -i work/windows-fixture/smoke_key `
     -o IdentitiesOnly=yes -o IdentityAgent=none -o BatchMode=yes `
     -o StrictHostKeyChecking=yes -o UserKnownHostsFile=work/windows-fixture/known_hosts `
     -o ConnectTimeout=5 -P 22222 fixtureadmin@127.0.0.1
   ```

5. If the client reports `Connection closed` before key exchange, check the guest
   from inside the container: `nc -z 172.30.0.2 22`, then `ip neigh` (an
   `INCOMPLETE` entry for the guest IP means no ARP reply), and a QEMU monitor
   `screendump` that does not change after `sendkey`. The baseline guest hung this
   way on 2026-09-20 after about one hour of uptime. `docker compose stop` then
   times out and kills QEMU after the grace period, and the guest booted normally
   afterwards. Capture the screen and note the `data.img` timestamp before
   restarting.
6. If `ssh` cannot load the control key (`Permission denied` on the key file),
   check its ACL. A key generated by another account or sandbox is readable only
   by that account. `Install-ControlKey.ps1` generates a key at the given path
   if needed and installs it through the documented fixture password.

For the observed initial-setup stall, the restart configuration was:

```powershell
$env:WINDOWS_HV = 'N'
docker compose -p ssh-copy-id-win11 -f tests/environments/windows/compose.yaml up -d
```

`HV` controls guest Hyper-V enlightenments, not the host's Hyper-V feature or KVM
acceleration. Its default remains `Y`; keep `WINDOWS_HV=N` in the current shell
when managing a fixture that uses this workaround. See the upstream
[environment variables](https://github.com/dockur/windows/blob/master/docs/environment.md).

## Separate Scenarios

For the standard-user baseline, sign in once through the guest console as
`fixtureuser` and sign out to initialize its Windows profile. Leave its `.ssh`
directory absent, then run from the host:

```powershell
./tests/environments/windows/Test-StandardUser.ps1
```

Run this test serially. It uses the administrator control key to provision one
temporary standard-user key and removes its guest test files in `finally`.
It refuses an existing `.ssh` directory. Local disposable keys and transfer
evidence remain under ignored `work/windows-fixture/standard-<run-id>`.

Use a different Compose project name for each independent guest disk. Set
`WINDOWS_WEB_PORT` and `WINDOWS_SSH_PORT` to unused ports when running them together.
Set `WINDOWS_VERSION` before creating a fresh project to select another version;
changing it does not convert an existing installed disk.

Start with the default shell and test standard and administrator accounts.
Then use separate fixtures for Windows PowerShell and `pwsh` default shells,
custom key-file paths, and ACL scenarios. The default-shell variants are
described below; the others are not provisioned yet.
To reset a test, prefer a new project/volume; `down` preserves the existing disk.

```powershell
docker compose -p ssh-copy-id-win11 -f tests/environments/windows/compose.yaml down
```

Use `down --volumes` only when intentionally discarding that project's entire
Windows disk. The fixture does not mount host physical disks.

## Default Shell Variants

Create one project per default shell from a copy of a validated disk instead of
reinstalling Windows. Stop the source guest first; copying a running guest's
disk is not supported. From the repository root:

```powershell
docker compose -p ssh-copy-id-win11 -f tests/environments/windows/compose.yaml stop
docker volume create ssh-copy-id-win11-ps_windows-storage
docker run --rm -v ssh-copy-id-win11_windows-storage:/s:ro -v ssh-copy-id-win11-ps_windows-storage:/d alpine sh -c 'apk add -q coreutils && cp -a --sparse=always /s/. /d/'
$env:WINDOWS_HV = 'N'; $env:WINDOWS_WEB_PORT = '8007'; $env:WINDOWS_SSH_PORT = '22223'
docker compose -p ssh-copy-id-win11-ps -f tests/environments/windows/compose.yaml up -d
```

The copy keeps the host keys, accounts, initialized profiles, and the
administrator control key. Use a separate evidence directory and known-hosts
file per project, for example `work/windows-fixture-ps/`. Change the default
shell through the control key, then run both tests with the expected
interpreting shell process:

```powershell
$fixture = @{ Port = 22223; IdentityFile = 'work/windows-fixture-ps/smoke_key'; KnownHostsFile = 'work/windows-fixture-ps/known_hosts' }
./tests/environments/windows/Test-Smoke.ps1 @fixture
./tests/environments/windows/Set-DefaultShell.ps1 -Shell WindowsPowerShell @fixture
./tests/environments/windows/Test-Smoke.ps1 -ExpectedShellProcess powershell.exe @fixture
./tests/environments/windows/Test-StandardUser.ps1 -ExpectedShellProcess powershell.exe -ExpectedCmdExitCode 1 -EvidenceRoot work/windows-fixture-ps @fixture
```

`Set-DefaultShell.ps1` writes `HKLM:\SOFTWARE\OpenSSH\DefaultShell` (`cmd`
removes the value; `pwsh` requires PowerShell 7 installed in the guest) and
reports the resulting value and shell file version. New sessions used the new
shell without restarting `sshd`. The tests report the process that sshd started
to interpret the command and, for the standard user, the exit status observed
for `cmd.exe /d /c exit 37`. Results and the exit-code measurements are in the
[validation record](../../../docs/validation.md#windows-powershell-default-shell-2026-09-23).
