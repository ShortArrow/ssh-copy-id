# Validation Plan and Results

This document tracks experiments against the accepted [design](design.md).
Passing a fixture test demonstrates the tested OpenSSH behavior; it does not
demonstrate that the Rust product implements it; product behavior is covered by
the U rows. Behavioral differences remain indexed in [compatibility](compatibility.md).

## Recording Policy

- Record the date, OS/build, client/server versions, command, and assertions.
- Use `passed`, `failed`, or `pending`; never count an unrun scenario as passed.
- Keep reproducible scripts beside their fixtures. Keep generated private keys,
  raw logs, and transient results under ignored `work/`, outside documentation.
- Describe setup failures separately from product differences. Link accepted
  decisions to the design rather than duplicating their rationale here.
- Use isolated fixtures for default-shell, server-configuration, and destructive
  permission scenarios. Preserve failed-run evidence and report cleanup failures.

## Matrix

| ID | Scenario | Status | Evidence / next action |
| --- | --- | --- | --- |
| L01 | Selected B versus authorized A, direct and one jump | Passed | [Linux prototype](../tests/prototypes/identity-isolation/README.md): 14 checks; fixed configuration only. |
| L02 | Linux sshd fixture for Rust CLI integration tests | Passed | [smoke.sh](../tests/environments/linux/smoke.sh): key-only and password accounts, run by the CI `linux-fixture` job. |
| L03 | OpenWrt destination fixture: BusyBox ash and Dropbear | Passed | [smoke.sh](../tests/environments/openwrt/smoke.sh): root with a per-run password and a control key on stock OpenWrt 24.10.8, whose BusyBox has no `od`. [OpenWrt run](#openwrt-destination-and-key-line-endings-2026-10-09): the script then needed `od`. With `hexdump` in its place, the 6 tests of [openwrt_destination.rs](../tests/openwrt_destination.rs) pass, a CRLF key file included; run in the CI `linux-fixture` job. |
| W01 | Administrator login, remote execution, stdin, guest metadata | Passed | [Test-Smoke.ps1](../tests/environments/windows/Test-Smoke.ps1). |
| W02 | Standard-user key absent / installed / removed | Passed | [Test-StandardUser.ps1](../tests/environments/windows/Test-StandardUser.ps1), initialized profile. |
| W03 | Standard-user CRLF/LF stdin and explicit cmd exit 37 | Passed | Same script; default shell remains cmd. |
| W04 | Standard-user SFTP binary upload, download, byte comparison, deletion | Passed | Same script; not an append or atomic-replacement test. |
| W05 | Windows PowerShell as the sshd default shell | Passed | [Cloned fixture](#windows-powershell-default-shell-2026-09-23), both account tests; a native child command's nonzero exit code reaches the client as 1. |
| W06 | pwsh as the sshd default shell | Passed | [Cloned fixture with PowerShell 7.6.7](#pwsh-default-shell-w06-2026-10-14), both account tests; exit statuses as under Windows PowerShell (W05). |
| W07 | Default administrator shared key-file scope and invalid ACL rejection | Shared file passed | [An inherited profile ACL alone passes sshd's check](#inherited-profile-acl-w07-first-check-2026-10-13), and an extra writable entry for `BUILTIN\Users` fails it. [The shared file](#shared-administrator-file-w07-2026-10-22) is appended to with its ACL kept, or created with Administrators and SYSTEM only, and the keys log in. [A second administrator](#release-review-for-020-w07-w11-2026-10-22) appends to a file the first one created, and both keys log in. Pending: an existing shared file with an extra writable entry, and an extra read-only ACE. |
| W08 | Custom authorized-key paths and existing parent ACL preservation | Pending | Cover D03 and Windows-specific ACL prerequisites. |
| W09 | Destination shell-family probe outputs under `cmd.exe`, Windows PowerShell, `pwsh`, `sh`, `bash`, and `dash` | Passed | [Probe outputs](#shell-family-probe-w09-2026-10-14) under each, also `tcsh`, `pwsh` as a Linux login shell, and startup steps that print a line. |
| W10 | A Windows sshd whose default shell starts WSL, and an sshd inside WSL | Passed | [Probe outputs and key files](#wsl-default-shell-and-sshd-inside-wsl-w10-2026-10-15): with `wsl.exe` or `bash.exe` as the default shell the probe stops the run, and a key in the WSL home does not authenticate; an sshd inside WSL answers as a Unix-like shell and reads the WSL home. |
| W11 | Windows key input past about 4 KB on stdin | Passed | [The end of input does not arrive; the loader stops at its terminator](#release-review-for-020-w07-w11-2026-10-22). |
| U01 | Stage 1: CLI installs one key on a Unix-like destination; requirements 1, 2, 3, 7, and 8 | Passed | [Stage 1 run](#stage-1-on-a-unix-like-destination-2026-10-01): 19 tests of [unix_destination.rs](../tests/unix_destination.rs) against L02 with both tested Windows clients, also run in CI. |
| U02 | Stage 1.5: `-n`, `-f`, `-t`, `-i` without a file, default key selection, agent keys, and `-x` on a Unix-like destination; requirements 4 and 5 | Passed | [Stage 1.5 run](#fixture-tests-u02): 35 tests of [unix_destination.rs](../tests/unix_destination.rs) against L02 with both tested Windows clients, except i22 with Windows OpenSSH, whose agent A02 covers; also run in CI. |
| U03 | Stage 2: CLI installs one key for a standard Windows user; requirements 1, 2, 3, 7, and 8 under `cmd.exe`, Windows PowerShell, and `pwsh` default shells | Passed | [Stage 2 run](#stage-2-on-a-windows-destination-2026-10-21): 7 tests of [windows_destination.rs](../tests/windows_destination.rs) under each default shell, also the administrator stop; [rechecked](#recheck-after-the-release-review-2026-10-22) under `cmd.exe` and Windows PowerShell after the release review. |
| K01 | Line endings and encoding sshd accepts in authorized_keys (line endings, D-07) | Passed | 2026-10-09, checked by logging in with the key: a key line ending in CR authenticates, with or without a comment, on OpenSSH 9.6p1 (L02 image), [OpenSSH 7.6p1, Dropbear 2022.82, 2024.86 and 2025.89, and Windows OpenSSH 9.5p2](#key-line-endings-k01). A line starting with a UTF-8 BOM is rejected on all of them, except that Windows OpenSSH accepts one at the start of the file. |
| A01 | Password and passphrase prompts with public keys on stdin, cancellation, no terminal, agent confirmation | Passed | [Prompt experiment](../tests/prototypes/ssh-prompt/README.md): prompts and cancellation pass with both tested Windows clients; no-terminal behavior differs by client; the Windows agent refuses keys with confirmation. [Through the CLI](#authentication-prompts-through-the-cli-a01), with `ssh`'s streams on files, prompts and Ctrl-C pass with both clients and a run without a console fails at once. |
| C01 | Windows client run without a console, from an sshd session | Passed | In the dockur Windows 11 guest's sshd session, Windows OpenSSH 9.5p2 hung on `ssh -G` with stderr on a pipe and on the installation with stdin and stdout on pipes; with every stream on a file, the run appends and verifies the key, and a second run skips it. The same run passes in the desktop session (A02). |
| A02 | Agent-selected identities | Passed | Linux: a private `ssh-agent` with two keys, one installed, appends only the other (`unix_destination` i22 and golden scenario 16). Windows: in a dockur Windows 11 guest's desktop session, with `SSH_AUTH_SOCK` unset and two keys in the guest's own OpenSSH 9.5p2 agent, one installed on L02, the run appends and verifies only the other, and a second run skips both (D-20). |
| F01 | Interrupted writes and uncertain remote state | Pending | Inject disconnects and record actual file state and exit status. |
| P01 | Rust CLI behavior versus pinned upstream | Passed for the covered scenarios | [p01_golden.rs](../tests/p01_golden.rs) runs 29 scenarios with both tools against L02 in the CI `linux-fixture` job; every difference left is tagged with its D-xx, STAGE-1.5 (the usage while `-s` is missing), or SHELL reason. |

## Windows Baseline: 2026-09-20

Environment: Docker Desktop Linux/WSL2, dockur with KVM, Windows 11 Enterprise
Evaluation 25H2 `26200.6584`. Guest OpenSSH file version `9.5.5.1`; host client
`OpenSSH_for_Windows_9.5p2`. Default shell: `cmd.exe`. See the
[fixture record](../tests/environments/windows/README.md) for the pinned image,
startup recovery, addresses, and provisioning instructions.

Commands from the repository root (PowerShell 7):

```powershell
./tests/environments/windows/Test-Smoke.ps1
./tests/environments/windows/Test-StandardUser.ps1
```

The standard-user test creates a unique disposable key, refuses an existing
`.ssh` directory, installs the public key through the administrator fixture
connection, and removes its guest files in `finally`. It then verifies that the
removed key is rejected. The local evidence directory retains the disposable
key and transferred data; it is not a production credential. Run one test at a
time against this guest.

Five assertions passed: unregistered-key rejection, registered-key authentication
with exact mixed-newline stdin, remote exit 37, binary SFTP round trip with
deletion, and removed-key rejection. The administrator smoke test also passed.

### Profile Preparation Finding

The account initially existed without a Windows user profile. Creating
`C:\Users\fixtureuser\.ssh\authorized_keys` with the intended owner and ACL was
insufficient in this fixture: OpenSSH rejected the key. After profile
initialization, the same test passed. The diagnostic `Start-Process
-Credential -LoadUserProfile` created a profile but its child returned nonzero,
so it is not adopted as an automated setup recipe. The supported test prerequisite
is a completed guest-console sign-in as `fixtureuser`, followed by sign-out.

The test now checks that the profile exists at the expected path before creating
key files. This is a fixture prerequisite and an observed setup issue, not a new
product design difference. Test first-login behavior separately if it becomes a
supported product scenario. Windows OpenSSH documents relative key paths against
the [user's profile directory](https://github.com/PowerShell/Win32-OpenSSH/wiki/sshd_config).

## Windows PowerShell Default Shell: 2026-09-23

Environment: a second dockur project, `ssh-copy-id-win11-ps`, created from a
copy of the validated baseline disk while both guests were stopped. Same guest
build `26200.6584` and guest OpenSSH `9.5.5.1`; host client
`OpenSSH_for_Windows_9.5p2`. Default shell:
`C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe`, file version
`10.0.26100.5074`, set through `Set-DefaultShell.ps1`. Host bindings: web `8007`,
SSH `22223`. Evidence remains under ignored `work/windows-fixture-ps/`.

Commands from the repository root (PowerShell 7). The smoke test had first run
against the copy with its default `cmd.exe` shell to record the host key:

```powershell
$fixture = @{ Port = 22223; IdentityFile = 'work/windows-fixture-ps/smoke_key'; KnownHostsFile = 'work/windows-fixture-ps/known_hosts' }
./tests/environments/windows/Set-DefaultShell.ps1 -Shell WindowsPowerShell @fixture
./tests/environments/windows/Test-Smoke.ps1 -ExpectedShellProcess powershell.exe @fixture
./tests/environments/windows/Test-StandardUser.ps1 -ExpectedShellProcess powershell.exe -ExpectedCmdExitCode 1 -EvidenceRoot work/windows-fixture-ps @fixture
```

Both tests now assert the name of the process that sshd started to interpret
the SSH command. The administrator smoke test passed and recorded that process's
command line. The standard-user test passed five assertions: unregistered-key
rejection, authentication with exact mixed-newline stdin under `powershell.exe`,
remote `cmd.exe /d /c exit 37` reported as exit 1, binary SFTP round trip with
deletion, and removed-key rejection.

### Command Delivery and Exit Codes

sshd wraps the SSH command in the default shell's command option:

| Default shell | Observed shell command line |
| --- | --- |
| `cmd.exe` | `"c:\windows\system32\cmd.exe" /c "<command>"` |
| Windows PowerShell | `"c:\windows\system32\windowspowershell\v1.0\powershell.exe" -c "<command>"` |

Under Windows PowerShell the SSH command is parsed as a PowerShell statement.
Administrator probes on the same fixture, recorded in
`work/windows-fixture-ps/exit-code-probes.json`:

| SSH command | Exit status |
| --- | --- |
| `exit 37` | 37 |
| `cmd.exe /d /c exit 37` | 1 |
| `cmd.exe /d /c exit 0` | 0 |
| `powershell.exe -NoProfile -Command "exit 37"` | 1 |
| `cmd.exe /d /c exit 37; exit $LASTEXITCODE` | 37 |
| `$host.SetShouldExit(37)` | 37 |
| `nonexistent-command-xyz` | 1 |

Success and failure remain distinguishable. A specific nonzero exit code of a
native child command reaches the client only when the command is PowerShell
syntax that exits explicitly. SFTP is unaffected because the subsystem does not
start the default shell. How the product forwards the installation script's
exit code without assuming the default shell is tracked in the design.

### Setup Findings

- The baseline guest had hung since 2026-09-20 21:51 JST: the QEMU framebuffer
  no longer changed after keyboard and mouse input, the guest IP had no ARP
  reply inside the container, one core stayed busy, and `data.img` was last
  written at that time. The graceful stop timed out, the container was killed
  after its grace period, and the guest booted normally afterwards. The cause is
  not identified; the fixture runs with `HV=N`.
- The earlier control key files were created by a different sandbox account, and
  their ACLs deny the current account. `Install-ControlKey.ps1` installed a new
  control key through the documented fixture password. This is a host-side
  setup issue, not a guest or product finding.

## Evaluation License: 2026-09-30

The Windows guests run Windows 11 Enterprise Evaluation, whose license expires.
On 2026-09-30 at 05:19 guest time, `ssh-copy-id-win11-ps` reported the following
for the product with a partial product key and application ID
`55c92734-d682-4d71-983e-d6ec3f16059f`, read with
`Get-CimInstance SoftwareLicensingProduct` over SSH:

| Field | Value |
| --- | --- |
| Name | `Windows(R), EnterpriseEval edition` |
| GracePeriodRemaining | 114,769 minutes |
| OS install date | 2026-09-19 22:07 |

The grace period ends on 2026-12-18 at about 22:08 guest time, which matches the
install date plus 90 days. The baseline guest shares the install,
because the second guest is a copy of its disk. Rerun the query before relying on
this date; activation state can change after a rebuild.

## Stage 1 on a Unix-like Destination: 2026-10-01

Host: Windows 11 Pro 26200, rustc 1.97.1, Docker Engine 28.5.1. Destination: the
L02 fixture image, Ubuntu 24.04 with OpenSSH 9.6p1, with key-only, password,
tcsh, and empty-password accounts; the empty-password account's banner contains
`using "publickey"` and a forged `Authenticated to … using "publickey"` line. Command:

```sh
cargo test --test unix_destination -- --ignored --test-threads=1
```

| Client first on `PATH` | Result |
| --- | --- |
| Git for Windows `OpenSSH_10.0p2` | 19 passed |
| `OpenSSH_for_Windows_9.5p2` | 19 passed |

Removing the rule behind any of the 19 tests makes that test fail, and every run
checks that no `ssh-copy-id.*` scratch directory is left in the local `~/.ssh`. i1 checks
the modes 700 and 600; i6 a key comment with `%s` and a backslash; i12 logs in
with the installed key; i14 a `none` login after a banner that forges the
`publickey` line, which only the `-E` log file tells apart (D-01); i15 lines
that are trimmed, trailing blank lines that are dropped, and comment and blank
lines that are appended but not counted (D-18); i16 a FIFO target (D-16); i17 a write
cut short on a full tmpfs, which leaves the file byte-identical, and i18 the same
with a comment line before the key, which is removed with it (D-17); i19 a key
restricted by `command="exit 1"`, which a second run skips instead of adding
again (D-01). i11 asserts that a wrong password writes nothing and
exits 1, but not the message for an `ssh` exit 255 without a report; that message
is covered by the unit tests only.

## Authentication Prompts on Windows: 2026-10-01

Host: Windows 11 Pro 26200, the CLI stand-in under Python 3.12.11 in a ConPTY
pseudo console created by pywinpty 3.0.5. Destination: the L02 fixture image,
Ubuntu 24.04 with OpenSSH 9.6p1. The stdin payload was 66 bytes with LF and CRLF
lines; the remote side returned its SHA-256 and the account name.

| Case | `OpenSSH_for_Windows_9.5p2` | Git for Windows `OpenSSH_10.0p2` |
| --- | --- | --- |
| Password typed at the console prompt while stdin is a pipe | Exit 0, payload hash matched | Exit 0, payload hash matched |
| Passphrase of an encrypted key typed at the console prompt | Exit 0, payload hash matched | Exit 0, payload hash matched |
| Ctrl-C at the password prompt | `ssh` exited 255; the parent survived and saw 255 | The parent also received Ctrl-C and died with `KeyboardInterrupt` |
| No console, password authentication | `ssh` did not exit within 20 s and was killed | Exit 255 after 0.5 s |
| No console, password authentication, `BatchMode=yes` | Exit 255 after 0.5 s | Exit 255 after 0.6 s |

Prompts are read from the console, not from stdin, by both clients, so public
key data on stdin and interactive authentication coexist. Two consequences for
the CLI: with Git's client, console Ctrl-C reaches the CLI as well as `ssh`; and
without a console, Windows OpenSSH waits indefinitely at a password prompt.

Agent confirmation, measured the same day against the Windows OpenSSH agent
service: `ssh-add -c` with a disposable unencrypted Ed25519 key exited 1 with
`agent refused operation`, and the key was not listed. The same key without `-c`
was added and listed, then removed with `ssh-add -d`. The agent's key list
before and after the experiment was identical. The Windows agent therefore
never asks for confirmation; agents that support `-c`, such as one run under Git
for Windows, were not measured.

## OpenWrt Destination and Key Line Endings: 2026-10-09

Host: Windows 11 Pro 26300, rustc 1.97.1, Docker Engine 28.5.1, and WSL 2 Arch
Linux with `OpenSSH_10.3p1`. Windows clients: Git for Windows `OpenSSH_10.0p2`
and `OpenSSH_for_Windows_9.5p2`. Images, by index digest:

| Image | Digest | Server | Shell and commands |
| --- | --- | --- | --- |
| `openwrt/rootfs:x86-64-24.10.8` (L03) | `sha256:9972a4b4747cd136abd597475d7b88c51a49fd849d0d53f069a2f4bf446061b9` | Dropbear v2024.86 | BusyBox v1.36.1 |
| `openwrt/rootfs:x86-64-23.05.5` | `sha256:a44cce5d8f3619e30b0b7cf74e236d4e62cb3bf118953f57e044dc684d7cfc36` | Dropbear v2022.82 | BusyBox v1.36.1 |
| `openwrt/rootfs:x86-64-25.12.5` | `sha256:c5d5f05bab4ce06a4e840b3573671a06ec8ae9842273188ffc53f7021836b8e2` | Dropbear v2025.89 | BusyBox v1.37.0 |
| `ubuntu:18.04` with `openssh-server` | `sha256:152dc042452c496007f07ca9127571cb9c29697f42acbfad72324b2bb2e43c98` | `OpenSSH_7.6p1 Ubuntu-4ubuntu0.7` | not used |
| `alpine` (control) | `sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6` | none | BusyBox v1.37.0 with `od` |

### OpenWrt Image Findings

- None of the three OpenWrt images has `od`. `tail`, `dd`, `expr`, `wc`,
  `mkdir`, `dirname`, `tr`, and `hexdump` exist; `restorecon`, `xxd`, `stat`,
  and `truncate` do not. `busybox` without an applet prints `applet not found`,
  so the BusyBox version above is the first line of `ls --help`.
- root's password is empty in the images, and Dropbear then logs
  `Auth succeeded with blank password for 'root'` for a client that offers only
  a key that is not authorized, under `BatchMode=yes`. The first key runs on
  23.05.5 and 25.12.5 were made in that state and accepted every case; they are
  discarded. The L03 entrypoint refuses to start without `ROOT_PASSWORD`.
- Dropbear on 24.10.8 reads root's keys only from
  `/etc/dropbear/authorized_keys`: a key only in `/root/.ssh/authorized_keys`
  (`/root` 750, `.ssh` 700, file 600) is rejected. smoke.sh asserts this.
- Dropbear 2022.82 started with `-R` exited with `Early exit: Bad buf_getptr`;
  with an Ed25519 host key made by `dropbearkey` and passed with `-r`, it ran.
  This is a setup issue of the 23.05.5 probe only.

### Key Line Endings (K01)

Each case used a new Ed25519 key from the client's own `ssh-keygen`. The key file
was rewritten to hold only that case, then the client logged in:

```sh
docker exec <container> sh -c "printf '<format>' '<type> <base64>' > <file>; chmod 600 <file>"
ssh -F none -p <port> -o BatchMode=yes -o IdentitiesOnly=yes -i <key> <user>@127.0.0.1 true
```

On Dropbear the account was root, with a password set, and the file
`/etc/dropbear/authorized_keys`; on OpenSSH 7.6p1 it was an account with a
locked password and `~/.ssh/authorized_keys`. On every server a key absent
from the file was rejected, and the file's first and last bytes were read back
to confirm the CR and BOM bytes.

| Case | Format | Dropbear 2022.82, 2024.86, 2025.89 | OpenSSH 7.6p1 | Windows OpenSSH 9.5p2 |
| --- | --- | --- | --- | --- |
| LF | `%s c\n` | accepted | accepted | accepted |
| CR, with comment | `%s comment\r\n` | accepted | accepted | accepted |
| CR, without comment | `%s\r\n` | accepted | accepted | accepted |
| CR at the end of the file, no LF | `%s\r` | accepted | accepted | accepted |
| CR line before a comment line | `%s\r\n# trailing\n` | accepted | accepted | accepted |
| BOM, first line | `\357\273\277%s c\n` | rejected | rejected | accepted |
| BOM, after a comment line | `# x\n\357\273\277%s c\n` | rejected | rejected | rejected |

Git's client ran every case on all four servers; the Windows client ran them on
Dropbear 2024.86 and OpenSSH 7.6p1, with the same results.

Windows OpenSSH 9.5p2 (`sshd.exe` product version `OpenSSH_9.5p2 for Windows`) ran
on the W05 guest (port 22223). The account was the standard user `fixtureuser`,
whose `.ssh` and `authorized_keys` were created for the run with the owner and ACL
that Test-StandardUser.ps1 sets, and removed afterwards; each case rewrote the file's bytes over
SSH as the administrator, read back its first and last three bytes, and logged
in with `OpenSSH_for_Windows_9.5p2`.

### Installation Script under BusyBox

The library's unit tests were built in `rust:1-alpine` (rustc 1.99.0, musl) and
run inside each container as a non-root account, so `sh` and every command the
script calls are the image's:

```sh
cargo test --lib --no-run
docker exec -u tester -w /tmp/tester -e HOME=/tmp/tester <container> /tmp/unittests remote_script:: --test-threads=1
```

| Environment | `remote_script` tests |
| --- | --- |
| OpenWrt 24.10.8 | 43 passed, 6 failed |
| Alpine | 49 passed |
| Alpine after `rm /usr/bin/od` | 43 passed, the same 6 failed |

Every call of the script's `hex`, `od -v -An -tx1 | tr -d ' \t\n'`, prints
`sh: od: not found` and an empty string. The last-byte check then never sees
`0a`, so a newline is written before the first line even when the file ends
with one: s05 and s36 get `existing\n\nssh-ed25519 …`. The check before a
truncation compares two empty strings and always truncates: s28, s32, and s34
report `failed` with summary `unchanged`, and s33 summary `partial`, where
`uncertain` is expected.

The same command, printed by a scratch program from
`remote_script::install_command`, ran as root under OpenWrt's `/bin/sh`:

```sh
cd /root; printf '<keys>' | LOGNAME=root HOME=/root sh -c "$(cat cmd.txt)"
```

| Scenario | Report | `/etc/dropbear/authorized_keys` afterwards |
| --- | --- | --- |
| Missing file | key added, `installed` | the key line |
| `existing` without a final newline | key added, `installed` | `existing\n<key>\n` |
| `existing\n` | key added, `installed` | `existing\n\n<key>\n`, a blank line added |
| `# note`, blank line, key, `  # indented` | 1 key added, `installed` | the four lines as given |
| `ulimit -f 2` with `trap '' XFSZ`, a 3 KB key and a second key, after `existing\n` | both `failed`, `unchanged` | `existing\n`, rolled back |
| Same limit; a wrapper `tail` appends `other-writer-line` before the key is written, after `e\n` | `failed`, `unchanged` | `e\n`: the other writer's line was removed |

On Alpine the last scenario reported `uncertain` and kept the other line.
BusyBox ash accepts `ulimit -f`. The explicit-target command with the path
``sub dir/it's!x $(id) `id` `` on stdin, as `install_input` writes it, reported the
key added with ``path=sub%20dir/it's!x%20$(id)%20`id` ``, and created
`/root/sub dir` (700) holding a file with that literal name (600).

### CLI on the Fixture

From Git Bash, with Git's client first on `PATH` and an askpass program
answering root's password:

```sh
SSH_ASKPASS=askpass.cmd SSH_ASKPASS_REQUIRE=force target/debug/ssh-copy-id.exe -i new \
    -p <port> -F none -o UserKnownHostsFile=kh -o StrictHostKeyChecking=accept-new root@127.0.0.1
```

Without `-t` the run exited 0 with `Number of key(s) added: 1` and `the key
authenticates: it is installed and verified`; stderr also held `sh: od: not
found` twice. `/etc/dropbear/authorized_keys` became the control line, an empty
line, and the new line, and the new key logged in. With `-t
.ssh/authorized_keys` the run exited 0 with one key added:
`/root/.ssh/authorized_keys` held the key, `/etc/dropbear/authorized_keys` kept
its SHA-256, and the CLI warned `the key was installed but could not be
verified: the server still rejects it`, since Dropbear does not read that file
for root.

```sh
docker build -t ssh-copy-id-l03:local tests/environments/openwrt
cargo test --test openwrt_destination -- --ignored --test-threads=1
```

| Client first on `PATH` | Result |
| --- | --- |
| Git for Windows `OpenSSH_10.0p2` | o1, o3, o4 passed; o2, o5 failed |
| `OpenSSH_for_Windows_9.5p2` | o1, o3, o4 passed; o2, o5 failed |
| WSL `OpenSSH_10.3p1`, three runs of about 10 s | o1, o3, o4 passed; o2, o5 failed |

o2 found the blank line; o5 found `the key was not written to
/etc/dropbear/authorized_keys` and the other writer's line removed. With the
image rebuilt with a static BusyBox 1.37.0 from `busybox:1.37.0-musl` copied to
`/usr/bin/od`, all five passed on Windows. o4 passes without `od` because no
other writer touched the file. smoke.sh passed on Windows and WSL.

## Stage 2 on a Windows Destination: 2026-10-21

Host: Windows 11 Pro 26300, `OpenSSH_for_Windows_9.5p2` first on `PATH` for the
CLI. Since 2026-10-16 the Docker Desktop WSL 2 VM on this host has started
without nested virtualization (`Hyper-V: Nested features: 0x0`, no `/dev/kvm`),
also after `wsl --shutdown`, so the guests ran on the second Linux host used for
W10, one at a time: copies of the `ssh-copy-id-win11` (`cmd.exe`),
`ssh-copy-id-win11-ps` (Windows PowerShell), and `ssh-copy-id-win11-pwsh`
(`pwsh` 7.6.7) disks under the pinned dockur image with `VMX: "Y"`, ports on
that host's loopback and reached through an SSH tunnel. Guest: Windows 11
Enterprise Evaluation `26200.6584`, OpenSSH `9.5.5.1`. The `cmd.exe` copy got a
new administrator control key through `Install-ControlKey.ps1`. Command from the
repository root, with the variables naming the guest:

```powershell
cargo test --test windows_destination -- --ignored --test-threads=1
```

| Default shell | Result |
| --- | --- |
| `cmd.exe` | 7 passed |
| Windows PowerShell | 7 passed |
| `pwsh` 7.6.7 | 7 passed |

Each test starts from `fixtureuser`'s profile without `.ssh` and runs the CLI
with a temporary `USERPROFILE`, its own known-hosts file, no agent, and an
`IdentityFile`, since Windows OpenSSH takes `~` from the account's profile
rather than from `USERPROFILE`. v01 logs in with the installed key and sees the
verification line; v02 an empty file and a file without a final newline; v03 a
second run that skips the key and a second key that is added; v04 a file whose
ACL denies `fixtureuser` writing, which stays byte-identical with exit 1; v05 a
key path with a space, Japanese, and `'`, and a comment with `"`, `%OS%`, and
`$home`, which logs in; v06 a wrong password and a host key mismatch, which
leave no file; v07 `fixtureadmin`, whose shared key file stays unchanged with
exit 1. Afterwards each guest was stopped; the last one is kept on that host
with its volume.

## Release Review for 0.2.0 (W07, W11): 2026-10-22

Three refuting reviewers examined commit 3ca3c07, one of them on the `cmd.exe`
copy described [below](#shared-administrator-file-w07-2026-10-22); each finding
here was reproduced again before the fix.

- Input past about 4 KB never ended (W11). With the script ahead of the keys, a
  key file of 1106 bytes, a 5000-character comment cut by `ssh-keygen`, left
  the run waiting after "1 key(s) remain to be installed" until it was stopped
  at 120 s. Probes sent from the client straight to `powershell.exe` showed the
  cause: every byte arrives, but past about 4 KB the end of input does not;
  `CopyTo` and a plain read loop waited for it in each round, while a loop that
  stopped at its own terminator read 3 KB to 200 KB in 24 of 24 rounds. The
  loader now stops at a second NUL. v10 installs such a key under both default
  shells, and an RSA-8192 key file of 1415 bytes is written.
- Runs started at the same instant lost a line while each reported `added`.
  Unit test w32 reproduced it locally with eight runs before the fix and passed
  five times in a row after it.
- A second administrator: after `fixtureadmin` created the shared file, owned
  by Administrators (`O:BA`), the account `fixtureadmin2` appended its key, and
  both keys logged in. The account was removed; its profile was in use and is
  left until the guest restarts.

| Tests | Default shell | Result |
| --- | --- | --- |
| `windows_destination` v01–v10 | `cmd.exe` | 10 passed |
| `windows_destination` v01–v10 | Windows PowerShell | 10 passed |

## Shared Administrator File (W07): 2026-10-22

The Windows installation script now travels on stdin behind a fixed
`-EncodedCommand` loader, and an administrator's keys go to
`administrators_authorized_keys`. Host, client, and guest are as in the
[recheck below](#recheck-after-the-release-review-2026-10-22): the `cmd.exe`
copy on the second Linux host, Windows build 26200, `OpenSSH_9.5p2 for Windows`,
its default shell switched to Windows PowerShell for the second run and back
afterwards.

| Tests | Default shell | Result |
| --- | --- | --- |
| `windows_destination` v01–v09 | `cmd.exe` | 9 passed |
| `windows_destination` v01–v09 | Windows PowerShell | 9 passed |

- v07: a key appended to the existing shared file, which held the control key,
  logs in; the file's DACL is unchanged, and standard error names the file as
  shared before writing.
- v08: after the shared file was removed, the run creates it with the DACL
  `D:PAI(A;;FA;;;SY)(A;;FA;;;BA)`, and its key logs in. Both tests put the file's
  bytes and DACL back through a password login.
- v09: `fixtureadmin`'s SSH session holds `S-1-16-12288` (High) and not
  `S-1-16-8192` (Medium), with `LocalAccountTokenFilterPolicy` unset, so the
  write needs no elevation on this guest.
- Microsoft Defender had real-time protection and AMSI on, signature
  1.461.143.0, and `Get-MpThreatDetection` listed nothing after both runs, the
  loader's `ScriptBlock::Create` included.

The `pwsh` default shell was not run: this guest has no PowerShell 7, and the
command is the same fixed line under every default shell (O-03). The unit tests
w10 and w26–w29 cover the script's branches with a stand-in `ProgramData`.

## Recheck After the Release Review: 2026-10-22

After the release review, the shell-family probe became
`echo ssh-copy-id-probe:%OS%:$home` and only its Windows forms are recognized
(D-12), and the Windows script turns off progress records. Host, client, and
guest are as in the [stage 2 run](#stage-2-on-a-windows-destination-2026-10-21);
only the `cmd.exe` copy ran, on the second Linux host, and its default shell was
switched to Windows PowerShell with `Set-DefaultShell.ps1` for the second run and
back to `cmd.exe` afterwards.

| Tests | Destination and client | Result |
| --- | --- | --- |
| `windows_destination` | `cmd.exe` default shell | 7 passed |
| `windows_destination` | Windows PowerShell default shell | 7 passed |
| `unix_destination` | L02, `OpenSSH_for_Windows_9.5p2` | 34 passed; i20 and i22 not run, since without `-i` they reach the account's own agent |
| `unix_destination` | L02, Linux client in the `linux-fixture` job of the v0.1.0-pre release run | 36 passed |

v01 now also checks that standard error holds no CLIXML; on the guest it failed
with the script from before the change. i8b installs for a fish login shell,
whose probe reply has no marker line; it failed with the code from before the
change. The `pwsh` default shell was not rerun: its probe reply has the Windows
PowerShell form, `%OS%:` and a Windows home path (W09), and the command sent
after detection is the one both runs above passed.

## Stage 1.5 on a Unix-like Destination: 2026-10-11

Host: Windows 11 Pro 26300, rustc 1.97.1, Docker Engine 28.5.1. Destination:
the L02 fixture image. The CLI gives `ssh` and `ssh-add` files as their standard
streams.

### Fixture Tests (U02)

Command, with each client's directory first on `PATH` and `SSH_AUTH_SOCK` unset:

```sh
cargo test --test unix_destination -- --ignored --test-threads=1
```

| Client first on `PATH` | Result |
| --- | --- |
| Git for Windows `OpenSSH_10.0p2` | 35 passed |
| `OpenSSH_for_Windows_9.5p2` | 34 passed; i22 cannot run |

The 35 tests include requirement 4 (i25 to i28: `-n` checks, lists the key, and
creates or changes nothing; with `-f` it makes no connection) and requirement 5
(i23 and i24: `-f` installs without the private key file before `-i` and appends
an installed key again). i22 starts a private `ssh-agent` on a socket, which the
Windows OpenSSH agent, a service, does not create; the Windows agent is covered
by A02. Windows OpenSSH's `ssh-keygen` ends the `.pub` file with CRLF and Git's
with LF; the tests expect the line as the file gives it ([line endings](design.md#line-endings)).

### Authentication Prompts through the CLI (A01)

[cli_probe.py](../tests/prototypes/ssh-prompt/cli_probe.py) ran
`ssh-copy-id.exe` in a ConPTY pseudo console (Python 3.12.11, pywinpty 3.0.5);
each case installs a new key and then logs in with it.

| Case | `OpenSSH_for_Windows_9.5p2` | Git for Windows `OpenSSH_10.0p2` |
| --- | --- | --- |
| Password typed at the console prompt | Exit 0, the key logs in | Exit 0, the key logs in |
| Passphrase of the encrypted control key typed at the console prompt | Exit 0, the key logs in; asked twice, for the check and the installation | Exit 0, the key logs in; asked once |
| Ctrl-C at the password prompt | Exit 1, the key is not installed | Exit 1, the key is not installed |
| No console, password authentication | Exit 1 after 1.7 s, the key is not installed | Exit 1 after 1.7 s, the key is not installed |

### Recheck after the Release Review: 2026-10-12

After the stream files moved under `~/.ssh` and a console without a window
started counting as no console, the fixture tests gave the same results as
above with both clients, and every case of the CLI prompt probe passed again
with both. With Windows OpenSSH 9.5p2 against L02:

| Case | Result |
| --- | --- |
| `TEMP` and `TMP` naming a path under a regular file | The key is installed and verified, exit 0 |
| A private key readable by Everyone, first run | Checked as not installed, since `ssh` reads the private key only after the server accepts the public key; installed, then the verification warns with `ssh`'s `Load key "…": bad permissions` line |
| The same key, second run | The check is inconclusive with that line and the key is appended again, as upstream appends it |
| Started with `CREATE_NO_WINDOW` or `DETACHED_PROCESS` | `ssh` runs with `BatchMode=yes` ([windows_console.rs](../tests/windows_console.rs), also run in CI) |

## Inherited Profile ACL (W07, First Check): 2026-10-13

Environment: the `ssh-copy-id-win11-ps` guest (build `26200`, guest OpenSSH
`9.5.5.1`, `StrictModes` not set, so its default `yes`, and
`AuthorizedKeysFile .ssh/authorized_keys`), host client
`OpenSSH_for_Windows_9.5p2`. Command from the repository root (PowerShell 7):

```powershell
./tests/environments/windows/Test-InheritedAcl.ps1 -IdentityFile work/windows-fixture-ps/smoke_key -KnownHostsFile work/windows-fixture-ps/known_hosts -Port 22223 -EvidenceRoot work/windows-fixture-ps
```

`fixtureuser` had no `.ssh`. In its own password-authenticated session, as
ssh-copy-id has it before a key is installed, it created `.ssh` and
`authorized_keys` with `New-Item` and `WriteAllText`, changing no ACL. The
three checks passed:

| Check | Result |
| --- | --- |
| Public-key login with that key file | Accepted |
| The same login after an administrator granted `BUILTIN\Users:(M)` on the file | Rejected; sshd logged `Bad permissions. Try removing permissions for user: BUILTIN\\Users (S-1-5-32-545) on file C:/Users/fixtureuser/.ssh/authorized_keys.` and `Authentication refused.` |
| The same login after cleanup removed the file and `.ssh` | Rejected |

The ACLs as created:

| Object | Owner | Entries |
| --- | --- | --- |
| `C:\Users\fixtureuser` | `SYSTEM` | `SYSTEM`, `Administrators`, `fixtureuser`: `(OI)(CI)(F)`; a capability SID with `(S,X)`, not inheritable |
| `.ssh` | `fixtureuser` | `SYSTEM`, `Administrators`, `fixtureuser`: `(I)(OI)(CI)(F)` |
| `.ssh\authorized_keys` | `fixtureuser` | `SYSTEM`, `Administrators`, `fixtureuser`: `(I)(F)` |

On a default profile the inherited entries are the same principals and rights
as the explicit ACL in the design, and the owner is already the account. A
profile whose inheritable entries grant write to another principal would pass
that entry on to new objects; this run did not cover such a profile.

## pwsh Default Shell (W06): 2026-10-14

Environment: a third dockur project, `ssh-copy-id-win11-pwsh`, created from a
copy of the `ssh-copy-id-win11-ps` disk while that guest was stopped. Same
guest build `26200.6584` and guest OpenSSH `9.5.5.1`; host client
`OpenSSH_for_Windows_9.5p2`. Host bindings: web `8008`, SSH `22224`. PowerShell
7.6.7 was installed from `PowerShell-7.6.7-win-x64.msi` of the PowerShell
GitHub release `v7.6.7`, whose SHA-256 matched the release's `hashes.sha256` on
the host and again in the guest, with `msiexec /i … /qn /norestart ADD_PATH=1`
(exit 0). Windows PowerShell stays installed. Default shell:
`C:\Program Files\PowerShell\7\pwsh.exe`, file version `7.6.7.500`. Evidence remains under
ignored `work/windows-fixture-pwsh/`; the administrator control key is the one
in `work/windows-fixture-ps/`, since the disk is a copy.

Commands from the repository root (PowerShell 7):

```powershell
$fixture = @{ Port = 22224; IdentityFile = 'work/windows-fixture-ps/smoke_key'; KnownHostsFile = 'work/windows-fixture-pwsh/known_hosts' }
./tests/environments/windows/Set-DefaultShell.ps1 -Shell pwsh @fixture
./tests/environments/windows/Test-Smoke.ps1 -ExpectedShellProcess pwsh.exe @fixture
./tests/environments/windows/Test-StandardUser.ps1 -ExpectedShellProcess pwsh.exe -ExpectedCmdExitCode 1 -EvidenceRoot work/windows-fixture-pwsh @fixture
```

The administrator smoke test passed with `pwsh.exe` as the process that
interprets the SSH command, whose command line was
`"c:\program files\powershell\7\pwsh.exe" -c "<command>"`. The standard-user
test passed its five assertions with the expected exit code 1; a first run
expecting 37 failed only at that assertion, after the earlier ones had passed.

Administrator probes, recorded in `work/windows-fixture-pwsh/exit-code-probes.json`,
gave the same statuses as under Windows PowerShell:

| SSH command | Exit status |
| --- | --- |
| `exit 37` | 37 |
| `cmd.exe /d /c exit 37` | 1 |
| `cmd.exe /d /c exit 0` | 0 |
| `powershell.exe -NoProfile -Command "exit 37"` | 1 |
| `powershell.exe -NoProfile -NonInteractive -EncodedCommand <exit 37>` | 1 |
| `cmd.exe /d /c exit 37; exit $LASTEXITCODE` | 37 |
| `$host.SetShouldExit(37)` | 37 |
| `nonexistent-command-xyz` | 1 |

The `-EncodedCommand` row has the form the CLI sends to a Windows destination,
so its script's exit status does not reach the client under a PowerShell
default shell either. Under `pwsh` the error for an unknown command reached the
client with ANSI color sequences although no terminal was requested.

## Shell-Family Probe (W09): 2026-10-14

Windows side: the `ssh-copy-id-win11-pwsh` guest of W06, with the default shell
switched to `cmd.exe`, Windows PowerShell, and `pwsh` in turn and restored to
`pwsh` afterwards. Unix side: a throwaway container of the L02 image, with
`keyuser`'s login shell switched to `/bin/sh` (dash), `/bin/dash`, `/bin/bash`,
`/usr/bin/tcsh`, and `pwsh` 7.6.7 extracted from `powershell-7.6.7-linux-x64.tar.gz`
of the PowerShell GitHub release, whose SHA-256 matched the release's
`hashes.sha256`. Commands from the repository root:

```powershell
./tests/environments/windows/Test-ShellProbe.ps1
docker run --rm --entrypoint sh -v "$PWD/tests/environments/linux:/probe:ro" -v "<dir with pwsh.tar.gz>:/pwsh:ro" ssh-copy-id-l02:local /probe/probe-shells.sh
```

Each probe ran as the SSH command without a startup step and with one that
prints `startup-line`: `.bashrc`, `.cshrc`, cmd's `AutoRun` value, and a
PowerShell profile under the default execution policy and under
`RemoteSigned`. Every run exited 0. Standard output after the
`ssh-copy-id-probe:` marker, CRLF line endings on Windows:

| Default or login shell | `echo ssh-copy-id-probe:%OS%:$0` | `echo ssh-copy-id-probe:%OS%:$0/$home` |
| --- | --- | --- |
| `cmd.exe` | `Windows_NT:$0` | `Windows_NT:$0/$home` |
| Windows PowerShell | `%OS%:` | `%OS%:/C:\Users\fixtureadmin` |
| `pwsh` on Windows | `%OS%:` | `%OS%:/C:\Users\fixtureadmin` |
| `pwsh` on Linux | `%OS%:` | `%OS%://home/keyuser` |
| `sh`, `dash`, `bash` | `%OS%:sh`, `%OS%:dash`, `%OS%:bash` | `%OS%:sh/`, `%OS%:dash/`, `%OS%:bash/` |
| `tcsh` | `%OS%:tcsh` | `%OS%:tcsh//home/keyuser` |

Every startup line came before the probe line. Under Windows PowerShell's
default execution policy the profile was not loaded and its error went to
standard error, leaving standard output unchanged. Without `$home`, `pwsh` on
Linux prints the same as PowerShell on Windows; with it, the home path tells
them apart. Afterwards the guest had no `AutoRun` value or profile, the
current user's execution policy was `Undefined`, and the default shell was
`pwsh` again.

## WSL Default Shell and sshd Inside WSL (W10): 2026-10-15

A copy of the `ssh-copy-id-win11-ps` disk (Windows PowerShell default shell) ran
under the pinned dockur image on a second Linux host with nested KVM (Intel
Celeron J4125), with `VMX: "Y"` in a Compose override: with dockur's default
`-vmx`, WSL 2 failed with `HCS_E_HYPERV_NOT_INSTALLED`. Its ports were bound to
that host's loopback and reached through an SSH tunnel. In the guest, the
Windows Subsystem for Linux and Virtual Machine Platform features were enabled,
WSL 3.0.3 was installed from `wsl.3.0.3.0.x64.msi` of the microsoft/WSL GitHub
release (SHA-256 `AE977286974255BD26BF925F00F40F5692C6B2084155CBC0563C09AEA091F211`),
since the inbox `wsl.exe` would not install it from an sshd session, and
Ubuntu 24.04.5 was installed for `fixtureadmin` with `wsl --install --web-download`.
Its default user `wsluser` had a fresh test key in `~/.ssh/authorized_keys`.
Command from the repository root:

```powershell
./tests/environments/windows/Test-WslShell.ps1
```

The script set the Windows sshd's `DefaultShell` to `wsl.exe`, to `wsl.exe`
with `DefaultShellCommandOption` `--`, and to `bash.exe`, each time with a
scheduled task that restores Windows PowerShell, and then started an sshd on
port 2222 inside WSL, reached through the Windows sshd's TCP forwarding. The
probe and the environment listing ran as `fixtureadmin` with the control key,
whose key file is the shared administrator file; the test key logged in as
`fixtureadmin` on the Windows sshd and as `wsluser` on the sshd inside WSL.

| Destination | Probe exit and output | `SSH_`/`WSL` variables | Test key |
| --- | --- | --- | --- |
| Windows sshd, `wsl.exe` | 255; `Invalid command line argument: -c` in UTF-16 on standard output | Command failed the same way | Rejected |
| Windows sshd, `wsl.exe`, option `--` | 127; nothing on standard output; `bash` reports the whole expanded line, `echo ssh-copy-id-probe:%OS%:/bin/bash/`, as a missing command | Command failed the same way | Rejected |
| Windows sshd, `bash.exe` | 0; `ssh-copy-id-probe:%OS%:/bin/bash/` | No `SSH_` variable; `WSL_DISTRO_NAME`, `WSL_INTEROP`, empty `WSLENV` | Rejected |
| sshd inside WSL | 0; `ssh-copy-id-probe:%OS%:bash/` | `SSH_CLIENT`, `SSH_CONNECTION`; no `WSL` variable | Accepted |

Wherever a shell ran inside WSL, it was `wsluser`'s, with `HOME` `/home/wsluser`.
With a default shell that starts WSL, a key in the WSL home does not
authenticate, since the Windows sshd reads the Windows account's key file. The
probe stops each of those runs before any write: two print no marker line, and
`bash.exe` puts the shell's path in `$0`, which the Unix-like form excludes,
while an sshd inside WSL passes the bare name, as on Linux (W09). WSL also
printed `Failed to start the systemd user session` to standard error in the
`wsl.exe` run with `--`. Afterwards the default shell was Windows
PowerShell with no `DefaultShellCommandOption`, the restore task was gone, and
the guest was stopped.

## Linux Selected-Identity Experiment: 2026-09-20

The [Docker experiment](../tests/prototypes/identity-isolation/README.md) (L01)
ran the pinned upstream script, SHA-256
`a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650`, with Ubuntu
24.04 and OpenSSH `9.6p1 Ubuntu-3ubuntu13.19`, and passed 14 checks. It tests the
fixed script with Ubuntu's SSH binaries, not a build of the OpenSSH source tree
at the reference commit.

| Scenario | Observed result, both direct and through one jump host |
| --- | --- |
| Only A is authorized, B is selected, client config contains A | Upstream exits 0, reports all keys skipped, and leaves B uninstalled |
| Generated configuration contains only B; B is absent | Probe is rejected with exit 255 and `Permission denied` |
| B is added to the target's authorized keys | The same isolated probe succeeds |
| Target host key is removed from the known-hosts fixture; jump host remains trusted | Connection is rejected by host key verification |

The experiment evaluated settings with `ssh -G`, removed additive identity and
certificate lists, and wrote a temporary `-F` configuration containing only B.
For the jump fixture it replaced ProxyJump with a separate `ssh -W` process
reading the original configuration, so the jump kept its own key. Server logs
confirmed successful A and B authentication in the respective sequence.
Target-agent use was disabled in this experiment only. Windows SSH client
behavior and interactive authentication were not tested. The Docker image stays
a local test artifact; each run removes its container and generated keys.

## Linux Recheck: 2026-09-20

`docker run --rm --network none ssh-copy-id-identity-probe:local` passed all 14
checks. Client/server package: `OpenSSH_9.6p1 Ubuntu-3ubuntu13.19` with OpenSSL
`3.0.13`. Upstream commit: `eabf1987de772f0f2d772fd6bd72b4c84d0ab780`;
script SHA-256: `a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650`.
The experiment remains limited to the fixed direct/jump file-key configuration.
