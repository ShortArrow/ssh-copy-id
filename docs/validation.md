# Validation Plan and Results

This document tracks experiments against the accepted [design](design.md).
Passing a fixture test demonstrates the tested OpenSSH behavior; it does not
demonstrate that the Rust product implements it. The Rust entry point is still a
placeholder. Behavioral differences remain indexed in [compatibility](compatibility.md).

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
| W01 | Administrator login, remote execution, stdin, guest metadata | Passed | [Test-Smoke.ps1](../tests/environments/windows/Test-Smoke.ps1). |
| W02 | Standard-user key absent / installed / removed | Passed | [Test-StandardUser.ps1](../tests/environments/windows/Test-StandardUser.ps1), initialized profile. |
| W03 | Standard-user CRLF/LF stdin and explicit cmd exit 37 | Passed | Same script; default shell remains cmd. |
| W04 | Standard-user SFTP binary upload, download, byte comparison, deletion | Passed | Same script; not an append or atomic-replacement test. |
| W05 | Windows PowerShell as the sshd default shell | Pending | Separate fixture; repeat both account tests. Explicit PowerShell invocation under cmd does not cover this. |
| W06 | pwsh as the sshd default shell | Pending | Separate fixture with a recorded pwsh version; repeat both account tests. |
| W07 | Default administrator shared key-file scope and invalid ACL rejection | Pending | Add positive/negative ACL cases and a second administrator. |
| W08 | Custom authorized-key paths and existing parent ACL preservation | Pending | Cover D03 and Windows-specific ACL prerequisites. |
| A01 | Password/passphrase prompts and agent-selected identities | Pending | Interactive client tests; current automation disables agent use. |
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

## Linux Recheck: 2026-09-20

`docker run --rm --network none ssh-copy-id-identity-probe:local` passed all 14
checks. Client/server package: `OpenSSH_9.6p1 Ubuntu-3ubuntu13.19` with OpenSSL
`3.0.13`. Upstream commit: `eabf1987de772f0f2d772fd6bd72b4c84d0ab780`;
script SHA-256: `a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650`.
The experiment remains limited to the fixed direct/jump file-key configuration.
