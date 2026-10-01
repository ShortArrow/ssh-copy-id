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
| W01 | Administrator login, remote execution, stdin, guest metadata | Passed | [Test-Smoke.ps1](../tests/environments/windows/Test-Smoke.ps1). |
| W02 | Standard-user key absent / installed / removed | Passed | [Test-StandardUser.ps1](../tests/environments/windows/Test-StandardUser.ps1), initialized profile. |
| W03 | Standard-user CRLF/LF stdin and explicit cmd exit 37 | Passed | Same script; default shell remains cmd. |
| W04 | Standard-user SFTP binary upload, download, byte comparison, deletion | Passed | Same script; not an append or atomic-replacement test. |
| W05 | Windows PowerShell as the sshd default shell | Passed | [Cloned fixture](#windows-powershell-default-shell-2026-09-23), both account tests; a native child command's nonzero exit code reaches the client as 1. |
| W06 | pwsh as the sshd default shell | Pending | Clone a fixture, install pwsh, run `Set-DefaultShell.ps1 -Shell pwsh`, record the pwsh version, and repeat both account tests. |
| W07 | Default administrator shared key-file scope and invalid ACL rejection | Pending | First check whether an inherited profile ACL alone passes sshd's check (needed for stage 2); then positive/negative ACL cases, an extra read-only and an extra writable ACE, and a second administrator. |
| W08 | Custom authorized-key paths and existing parent ACL preservation | Pending | Cover D03 and Windows-specific ACL prerequisites. |
| W09 | Destination shell-family probe outputs under `cmd.exe`, Windows PowerShell, `pwsh`, `sh`, `bash`, and `dash` | Pending | Fix the probe command and its expected outputs for destination detection (D-12); include a login shell that prints a banner. |
| U01 | Stage 1: CLI installs one key on a Unix-like destination; requirements 1, 2, 3, 7, and 8 | Passed | [Stage 1 run](#stage-1-on-a-unix-like-destination-2026-10-01): 14 tests of [unix_destination.rs](../tests/unix_destination.rs) against L02 with both tested Windows clients, also run in CI. |
| A01 | Password and passphrase prompts with public keys on stdin, cancellation, no terminal, agent confirmation | Passed | [Prompt experiment](../tests/prototypes/ssh-prompt/README.md): prompts and cancellation pass with both tested Windows clients; no-terminal behavior differs by client; the Windows agent refuses keys with confirmation. |
| A02 | Agent-selected identities | Pending | Stage 1.5: default key selection from the agent and the selected-identity check with agent keys. |
| F01 | Interrupted writes and uncertain remote state | Pending | Inject disconnects and record actual file state and exit status. |
| P01 | Rust CLI behavior versus pinned upstream | Pending | Implement the CLI before claiming product conformance. |

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
tcsh, and empty-password accounts. Command:

```sh
cargo test --test unix_destination -- --ignored --test-threads=1
```

| Client first on `PATH` | Result |
| --- | --- |
| Git for Windows `OpenSSH_10.0p2` | 14 passed |
| `OpenSSH_for_Windows_9.5p2` | 14 passed |

Removing the rule behind i2, i3, i4, i5, i7, i8, i9, i10, i11, i12, i13, or i14
makes that test fail; i1 and i6 were not mutation-checked. i11 asserts that a
wrong password writes nothing and exits 1, but not the message for an `ssh`
exit 255 without a report; that message is covered by the unit tests only. i12
covers spaces, a single quote, and Japanese in the key path, and asserts that
the key authenticates after installation.

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
