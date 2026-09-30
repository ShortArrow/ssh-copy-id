# ssh-copy-id for Windows Design

[日本語](design.jp.md)

[Differences from upstream](compatibility.md)

Status: pre-implementation design. Records decisions and open questions as of 2026-09-18.

See the [2026-09-20 adversarial review](design-review.md) for unresolved failure
cases and proposed decisions. Recommendations are not accepted requirements unless
explicitly recorded as subsequent decisions.

## Goals and Principles

Build a Rust CLI that installs public keys on remote hosts from Windows. Keep its
arguments, key selection, detection of installed keys, and results as close as
possible to the existing Linux `ssh-copy-id` experience. This document defines
implementation and validation targets, not a list of implemented features.

- By default, install keys by executing remote commands over SSH.
- `-s` selects SFTP. Do not execute remote commands, including for discovery or ACL configuration.
- Never send private keys to the remote host. Send public keys as data rather than embedding them in command strings.
- Respect host key verification and existing SSH connection settings.
- Perform the file operations and permission changes needed to install keys. Do not start, stop, or restart services.
- Minimize runtime and build dependencies to support future distribution through APT and other package managers.

## Project Scope

The project installs existing SSH public keys for a specified account on one
remote host per invocation. It includes the selection, authentication checks,
file updates, and permissions needed for that operation. It is a replacement
for the `ssh-copy-id` command, not for the OpenSSH package.

The specified account is the connection target, not a guarantee of exclusive
authorization: Windows OpenSSH's default administrator key file is shared among
administrator accounts. See [Windows administrator scope](#windows-administrator-scope).

### In Scope

- Select existing public keys from files or an available agent, following the compatibility rules below.
- Connect using an existing authentication method, such as a password or an already authorized key. Initial access must already be possible.
- Check whether selected keys can authenticate, and install the remaining keys. Support forced installation and dry runs.
- Create the destination directory and key file when needed, preserve existing entries, and append keys with correct newline handling.
- Apply permissions needed for the destination key file and its directory, within the authenticated account's privileges and the selected transport's capabilities.
- Support remote command installation and SFTP installation, with clear errors and results.
- Preserve familiar SSH connection options and document compatibility differences.
- Build, test, document, and package this CLI with minimal dependencies, including coexistence with the existing Linux command.

### Platform and Delivery Boundaries

| Area | Scope and stage |
| --- | --- |
| Local Windows client | Primary platform and initial implementation target |
| Local Linux client | Planned extension for distribution through APT, pacman, and similar systems; use `ssh-copy-id-rs` |
| Unix-like remote hosts | First implementation target, using the shell and file layout described below |
| Windows OpenSSH remote hosts | Planned extension covering user and administrator key files and their ACL requirements |
| SFTP-only remote access | In scope for `-s`, subject to accessible paths and permission capabilities |
| Other local operating systems or specialized SSH appliances | No support commitment in the initial scope; evaluate separately |

These are implementation targets, not current support claims. The first
milestone remains Windows to a Unix-like host with one explicitly selected key.
Broader CLI compatibility, SFTP, Windows destinations, and Linux distribution
follow as separate increments; they are not prerequisites for that first milestone.

### Out of Scope

- Generating key pairs, copying or backing up private keys, issuing certificates, or managing a certificate authority.
- Removing, revoking, rotating, or synchronizing authorized keys, including pruning existing entries.
- Installing or provisioning SSH servers, changing `sshd_config`, enabling authentication methods, or configuring firewalls and accounts.
- Starting, stopping, enabling, or restarting `ssh-agent` or `sshd`, or invoking `ssh-add` to add keys. User-configured additions by SSH are covered by the agent policy below.
- Automatically elevating privileges or repairing permissions outside the destination key file and its directory.
- Providing a general SSH terminal, file transfer client, fleet management system, or replacement for OpenSSH executables.
- Implementing SSH or cryptography from scratch, or bypassing host key verification or authentication to obtain access.

When prerequisites are missing, report the problem rather than silently expanding
the operation into system setup. Adding a feature outside these boundaries
requires an explicit scope decision and an update to both language versions of
this design.

## Dependency and Distribution Policy

Keep dependencies small and justified, including transitive dependencies, native
libraries, external commands, and build tools. Prefer the Rust standard library
for straightforward tasks. Add a dependency when it provides a clear correctness,
security, compatibility, or maintenance benefit; minimizing dependencies must not
lead to implementing cryptography or SSH from scratch.

- Evaluate the system OpenSSH client first for connection handling. Count required executables as runtime dependencies and document them; invoking an external command is not dependency-free.
- Review each new crate's purpose, transitive dependencies, license, maintenance status, and packaging impact. Disable unused optional features where practical.
- Avoid additional language runtimes, large frameworks, and bundled native libraries unless required by a documented need. Keep platform-specific dependencies scoped to their target platforms.
- Keep test and development tools separate from dependencies needed to build and run the distributed program.
- Support builds without network access once declared sources and dependencies have been supplied. Do not download tools or code implicitly during build scripts or installation.
- Document local runtime requirements separately from requirements on the remote host, such as a shell or `icacls.exe`.

### Coexistence with OpenSSH Packages

Distribute the executable as `ssh-copy-id.exe` on Windows and install it as
`/usr/bin/ssh-copy-id-rs` in Linux packages. Leave the existing
`/usr/bin/ssh-copy-id` owned by OpenSSH untouched. Use `ssh-copy-id-rs` for Linux
manual pages and shell completions as well, so their paths do not collide.

Do not declare a conflict with or replacement of `openssh-client` or `openssh`
merely to take over the command name, or claim to provide those complete packages.
Declare the required OpenSSH runtime dependency if that connection backend is selected.

Users who want the familiar command name in their shell can explicitly opt in:

```sh
alias ssh-copy-id=ssh-copy-id-rs
```

Packages must not add this alias or modify the user's shell configuration
automatically. Package names remain undecided and need not match executable
names. This policy does not imply that an APT or pacman package or Linux client
support is already available.

## Compatibility with the Linux Version

### Pinned Upstream Reference

Use OpenSSH portable commit
[`eabf1987de772f0f2d772fd6bd72b4c84d0ab780`](https://github.com/openssh/openssh-portable/commit/eabf1987de772f0f2d772fd6bd72b4c84d0ab780)
as the compatibility baseline, resolved from upstream `master` using `git ls-remote`.
The reference files are
[`contrib/ssh-copy-id`](https://github.com/openssh/openssh-portable/blob/eabf1987de772f0f2d772fd6bd72b4c84d0ab780/contrib/ssh-copy-id)
and [its manual](https://github.com/openssh/openssh-portable/blob/eabf1987de772f0f2d772fd6bd72b4c84d0ab780/contrib/ssh-copy-id.1).

Compare argument parsing, exit codes, and output against this fixed revision.
For accepted differences, test the project design instead of reproducing upstream
behavior. Distribution-specific patches, including Ubuntu changes, require a
separate decision; they do not automatically change the baseline. Upstream updates
must be reviewed before explicitly updating this reference and affected tests.

The commit ID is fixed. A selected-identity experiment against it has passed;
the full compatibility suite has not been implemented.
The following are compatibility targets, not a claim of full compatibility.

### CLI Compatibility Targets

| Operation or argument | Target behavior |
| --- | --- |
| `[user@]host` | Accept the destination, including host aliases from SSH configuration |
| `-i [identity_file]` | Select the specified public key file, adding `.pub` if absent. Match upstream parsing when the argument is omitted |
| No `-i` | Prefer public keys from the agent; otherwise select the most recently modified `~/.ssh/id*.pub`, excluding `*-cert.pub` |
| `-p port` | Set the destination port |
| `-o option`, `-F config` | Specify SSH options or a configuration file. Allow repeated `-o` arguments |
| `-f` | Skip the installed-key check. Allow installation with only the public key; duplicates may result |
| `-n` | Display the keys that would be installed without performing installation operations. Connect for installed-key checks unless skipped with `-f` |
| `-s` | Install keys using SFTP |
| `-t target_path` | Specify the destination file |
| `-h`, `-?` | Display help |

Normally, determine whether a key is already installed by checking whether it can
authenticate. Do not substitute a text comparison against `authorized_keys`.
Avoid false positives caused by authentication with another key or reuse of an
existing multiplexed connection. Do not interpret connection failures or host key
errors as evidence that a key is not installed.

### Recorded Difference: Identity Used for the Installed-Key Check

Decision recorded on 2026-09-20: describe this as a behavioral difference from
upstream, without classifying upstream behavior as a defect or this change as a
bug fix. The original author's intent has not been established.

In the upstream source reviewed, a successful probe causes the selected key to
be skipped; the script does not independently verify which identity authenticated.
Another configured identity may remain a candidate. This project will not use
authentication with another key as evidence that the selected key is installed.
Using an existing key to authenticate the actual installation remains valid.

The implementation mechanism and handling of inconclusive probes remain open.
Record any additional compatibility impact of the chosen mechanism separately.
The scenario has been reproduced in Docker; see the
[selected-identity experiment](#selected-identity-experiment-results).

### Recorded Difference: SFTP Installed-Key Check

Decision recorded on 2026-09-20: retain the upstream remote `exit` check in
normal mode. In `-s` mode, check by establishing an SFTP session with the selected
key, without sending `exit` or any other remote command. A successful SFTP
protocol session, not merely a TCP connection, is required.

The reviewed upstream implementation attempts `exit` even for `-s` and also
treats the message `allows sftp connections only` as a successful check.
This project will not use that message as a substitute for an SFTP session.
Record this as a design difference, without classifying it as a bug fix.

The selected-identity rule above applies to both modes. `-f` skips the check.
Failure to establish SFTP must not automatically mean that the key is absent;
distinguishing authentication rejection from other failures remains an
implementation question. This decision does not adopt a general replacement
of normal-mode exit-status checking with direct authentication-result inspection.

### Recorded Difference: Existing Parent Directory of a Custom Target

Decision recorded on 2026-09-20: when `-t` specifies a target, preserve the
permissions of its existing parent directory in both normal and SFTP modes.
If permissions prevent installation, report an error instead of changing the
parent directory's mode or ACL.

In the reviewed upstream implementation, normal mode uses `umask 077` for
creation without an unconditional chmod of the existing parent. SFTP mode sets
the parent directory to mode 700. Preserving that directory in this project's
SFTP mode is recorded as a design difference, not a bug fix.

This decision covers an existing parent directory selected through `-t`.
Permissions for newly created directories, the destination key file, and the
default destination, as well as symbolic link and reparse point handling, remain
separate implementation decisions. Test with a custom target in a shared
directory and verify that its permissions are unchanged in both modes, including
when installation fails due to insufficient access.

### Remaining Compatibility Questions

Exact exit codes, diagnostic output, handling of `-x`, and support for specialized
remote systems remain open. At minimum, exit successfully when all keys are
already installed, and never report a failed write as success.

### Dry-Run Behavior

`-n` performs installed-key checks and displays the keys that would be installed.
It does not create the destination directory or key file, append keys, or change
their permissions. Connections for the check still occur unless `-f` skips it.
Authentication logs, login hooks, host key handling, and user-configured
`AddKeysToAgent` behavior may still occur as part of those connections.

Describe `-n` in help as "Display the keys that would be installed without
performing installation operations," not as an offline or side-effect-free mode.
This clarifies the upstream dry-run boundary and adds no new behavioral difference;
the separately recorded SFTP check difference still applies. Whether `-n -f`
performs any other connection remains an implementation question.

## Execution Flow

1. Parse arguments and select keys and connection settings.
2. Unless `-f` is set, check authentication for each key and exclude installed keys.
3. Exit successfully if all keys are installed. For `-n`, display the planned keys and exit.
4. Determine the destination and required permission handling.
5. Preserve existing contents, add a trailing newline if necessary, and append the public keys.
6. Check required permission changes and the results of writes and closes, then report the outcome.

### Public Key Input

Preserve key options such as `restrict` and `command=`, and comments. Do not
reconstruct installation entries from only the key type and key blob. The
following accepted differences apply to both installation transports, including
`-f`; they are design decisions, not implemented or verified features.

#### Private Key Input Rejection

The reviewed upstream script has no explicit check rejecting private key content
in the installation input; with `-f`, that content may be transmitted. Reject
private key content supplied as public key installation input before transmitting
any of that input, and report an error. `-f` skips installed-key checks, not this
input check. This does not prohibit SSH from using a local private key for authentication.

The detection method and supported private key formats remain implementation
questions. Test representative OpenSSH and PEM private key inputs, including
mixed public/private content and forced mode, without sending their contents.

#### CRLF Normalization

The reviewed upstream script does not explicitly strip CR from CRLF input.
Normalize CRLF line endings in the public key installation input to LF for
transmission. Preserve options and comments; do not rewrite existing remote
file contents or modify the local source file. Test LF and CRLF inputs with
restricted entries and comments through both transports and forced mode.

Strict public key parsing is not adopted by this decision. Encoding, BOMs,
standalone CR characters, other malformed input, and input size limits remain
open questions.

## Remote Operating Systems and Shells

Implement the basic path for Unix-like destinations first, then extend it to
Windows destinations. Do not require an OS option for normal use; assume a
Unix-like destination by default, as the Linux version does.

| Destination | Usual key file | Permission handling |
| --- | --- | --- |
| Unix-like system | `.ssh/authorized_keys` | Use directory mode 700 and file mode 600 as the baseline; decide how to handle existing permissions by comparison with upstream |
| Standard Windows user | `.ssh/authorized_keys` in the user profile | Follow the ACL requirements for a user-specific key file |
| Windows user in the Administrators group | `%ProgramData%/ssh/administrators_authorized_keys` under the default configuration | Restrict access to SYSTEM and Administrators |

Server configuration can change the destination. The OS alone cannot establish
administrator group membership or the actual key file location. Respect an
explicit `-t` path; separately decide how to select the administrators' key file.

Distinguish the default shell that initially interprets the SSH command from the
shell that executes the installation script. For example:
`SSH → cmd.exe → powershell.exe → key installation`.

- Use POSIX `sh` for Unix-like installation scripts without requiring Bash-specific features.
- Use Windows PowerShell as the initial candidate for Windows scripts, and verify compatibility with `pwsh`.
- Test launching through `cmd`. This does not necessarily require implementing all installation logic in batch syntax.
- Do not infer the OS from the shell name; `pwsh`, for example, also runs on Linux.
- `--target-os` and `--remote-shell` are proposed names. Their semantics, values, and any automatic detection remain undecided.

## Windows Permissions and Services

### Windows Administrator Scope

Follow Windows OpenSSH's key-file configuration. When installing into the shared
`administrators_authorized_keys` file, display the destination path and explain
that it is shared among administrator accounts before writing. Do not describe
the installation as granting access exclusively to the named account.

Do not modify server configuration to separate administrator accounts' key files.
This is a Windows destination specification, not a behavioral difference from
the Linux implementation. Selection of the effective destination under custom
server settings and the exact ACL update procedure remain open.

Validate that shared-file installations display the path and shared scope before
mutation, leave server configuration unchanged, and are distinguished from
installations into a standard user's key file.

### Permissions and Service Boundaries

In the default mode, use the remote host's `icacls.exe` when necessary.
Consider using SIDs to avoid localized group names, and check exit codes.
Define which existing ACL entries to remove or preserve before implementation.
Explain permission failures without automatically elevating privileges or
managing services.

`ssh-agent` is a helper service for private keys. Installing public keys on the
remote host does not require starting or restarting it. Do not automatically
start the local agent either; fall back to key file selection when it is
unavailable. Do not restart `sshd` solely for an `authorized_keys` update.

## Agent Use and User Configuration

The CLI does not invoke `ssh-add` to add keys. Listing public keys with
`ssh-add -L` remains in scope. Respect the user's `AddKeysToAgent` setting:
do not override it to prevent SSH from adding a key during authentication.
This preserves the reviewed upstream behavior and is not a recorded difference.
Agent services are not started, stopped, or restarted by this tool.

Verify that the configured `AddKeysToAgent` behavior is preserved and that the
CLI itself issues no key-addition or service-management commands. Other SSH
option precedence rules and agent-selection details remain separate decisions.

## SFTP Mode

SFTP v3 provides `SSH_FXF_APPEND`. When using an API that supports appending, do
not specify `TRUNC`, which discards existing contents. OpenSSH `sftp`'s `put -a`
resumes a transfer; it does not append a file containing only the new data.
Upstream `-s` downloads the file, edits it locally, and uploads it again.

- Distinguish a missing file from permission and communication failures.
- The choice between downloading and uploading the entire file or appending directly remains open.
- Rewriting the entire file can lose concurrent updates. Direct appending also does not guarantee atomicity across duplicate checks and newline handling. Test and define the scope of concurrency support.
- SFTP v3 Unix permission attributes alone cannot configure Windows ACLs adequately. Limit Windows SFTP support and document prerequisites such as existing ACLs.
- Do not automatically fall back to remote commands when `-s` cannot establish the required permissions.

## Implementation Boundaries and Open Questions

### Authentication Interaction

Delegate password and private-key passphrase prompts to the SSH client, and agent
confirmation requests to SSH and the agent. This CLI does not collect or store
passwords or passphrases. This follows upstream behavior and adds no compatibility
difference.

Prototype the `ssh.exe` process path on Windows to verify that sending public
keys through stdin can coexist with authentication prompts. Cover password
authentication, encrypted private keys, agent confirmation, cancellation, and
execution without an interactive terminal. Keep authentication interaction
separate from the public key data stream; do not solve prompt handling by
collecting credentials in this CLI or globally enabling `BatchMode=yes`.
This is a feasibility check, not a claim of implemented support.

### Connection Backend Evaluation

Separate the CLI, key selection, installed-key checks, connections, destination
policy, and file updates. Compare the following connection approaches with small prototypes,
starting with the system OpenSSH client under the dependency policy above.

| Candidate | What to verify |
| --- | --- |
| Invoke `ssh.exe` from Rust | Reuse existing SSH settings and authentication; support interactive prompts while sending public key data |
| Use a Rust SSH/SFTP library | Reconsider only if it can satisfy the accepted authentication delegation policy without collecting credentials in this CLI; also check SSH config, agents, jump hosts, and host key verification |
| Combine the SFTP subsystem through `ssh.exe` with an SFTP implementation in Rust | Check whether SFTP operations can be handled while preserving SSH compatibility |

Implementing the SSH protocol from scratch is not the plan. Library and external
command choices remain open. Record dependencies and supported environments
once the connection approach is selected.
Compare the full build and runtime dependency footprint, including packaging
costs, alongside compatibility and authentication behavior before making that choice.

### Selected-Identity Experiment Results

The [reproducible Docker experiment](../tests/prototypes/identity-isolation/README.md)
passed 14 checks with Ubuntu 24.04 and Linux OpenSSH
`9.6p1 Ubuntu-3ubuntu13.19`. It ran the pinned upstream script; its SHA-256 is
`a331afd275d386fd1a699e42fa1514699cf86c744ef62df3e5240d89e1443650`.
This tests the fixed script with Ubuntu's SSH binaries, not a build of the full
OpenSSH source tree at the reference commit.

| Scenario | Observed result, both direct and through one jump host |
| --- | --- |
| Only A is authorized, B is selected, client config contains A | Upstream exits 0, reports all keys skipped, and leaves B uninstalled |
| Generated configuration contains only B; B is absent | Probe is rejected with exit 255 and `Permission denied` |
| B is added to the target's authorized keys | The same isolated probe succeeds |
| Target host key is removed from the known-hosts fixture; jump host remains trusted | Connection is rejected by host key verification |

The experiment evaluates settings using `ssh -G`, removes additive identity and
certificate lists, and writes a temporary `-F` configuration containing only B.
For the single jump fixture it replaces ProxyJump with a separate `ssh -W`
process reading the original configuration, so the jump retains its own key.
Server logs confirm successful A and B authentication in the respective test sequence.

This establishes feasibility for the fixture, not a production implementation.
Target-agent use is disabled in this experiment only. Arbitrary config
serialization, `Match`, percent expansion, quoted paths, multiple jumps,
certificates, agent selection, and multiplexing still require validation.
Windows SSH client behavior and interactive authentication were not tested;
Windows destination fixtures are described below. No backend choice is finalized by
these results. The Docker image remains a local test artifact; each run removes
its container and generated keys.

## Initial Validation

The first implementation milestone is to install one explicitly selected key on
a Unix-like destination without adding it again on a subsequent run.
Turn the following requirements into behavioral tests.

1. Do not append a key that can already authenticate, and do not mistake success with another key for success with the selected key.
2. Correctly append to an empty file, a missing file, and a file without a trailing newline.
3. Preserve existing contents and do not treat permission or communication errors as a missing file.
4. `-n` must not create the destination directory or key file, append keys, or change their permissions. Verify that installed-key checks still run unless `-f` skips them and that normal connection side effects are not described as absent.
5. `-f` must skip installed-key checks and must not require the corresponding private key file.
6. `-s` must not execute remote commands, including for discovery, authentication checks, or ACL configuration.
7. Treat paths and key comments containing spaces, Japanese characters, or quotes as data.
8. Do not report authentication failures, host key mismatches, write failures, or permission configuration failures as success.

Use unit tests for argument parsing and key selection, and integration tests
against isolated SSH servers for authentication and file updates. Use Docker for
Linux destination fixtures. For Windows destinations, evaluate the
[dockur fixture](../tests/environments/windows/README.md) where KVM is available;
use Hyper-V virtual machines as the fallback. The current Docker Desktop WSL2
environment passed the KVM API and VM-creation checks. Compose and guest script
syntax are validated. The Windows 11 Enterprise Evaluation 25H2 guest passed
administrator SSH authentication, remote execution, stdin delivery, and an SFTP
session check. Initial setup required recovery; see the fixture's validation
record for the workaround, exact versions, and remaining scenarios.
The initialized standard-user profile also passed absent/installed/removed-key
authentication checks, mixed-newline stdin, remote exit-code preservation, and a
binary SFTP round trip. A copied fixture with Windows PowerShell as the sshd
default shell passed the same account tests; there sshd passes the command with
`powershell.exe -c`, and a native child command's nonzero exit code reaches the
client as 1. Forwarding the installation script's exit code without assuming the
default shell remains an open implementation question.
Track tested and pending scenarios in the
[validation matrix](validation.md); these are fixture tests, not Rust CLI conformance.
These are development/test tools, not product runtime dependencies.
For Windows
support, test standard users and administrators, as well as startup and ACL
handling with `cmd`, Windows PowerShell, and `pwsh` as the default shell.
Test SFTP-only servers separately.

## References

- [OpenSSH ssh-copy-id implementation](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id)
- [OpenSSH ssh-copy-id manual](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id.1)
- [SFTP v3 draft: opening files](https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-02#section-6.3)
- [OpenSSH sftp manual](https://man.openbsd.org/sftp.1)
- [Key management for Windows OpenSSH](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement)
- [Windows OpenSSH server configuration](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-server-configuration)

If upstream code is reused, retain its copyright notices and license terms.
