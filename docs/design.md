# ssh-copy-id for Windows Design

[日本語](design.jp.md)

[Differences from upstream](compatibility.md)

Status: pre-implementation design. Records decisions and open questions as of 2026-09-18;
decisions through 2026-09-24 are indexed in the [decision log](#decision-log).

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

### Design Stance

Decisions in this document follow four commitments, recorded on 2026-09-24.

1. Read the pinned upstream script and manual first, and record what its behavior
   appears to intend, before deciding whether to differ. No difference is recorded
   without that reading.
2. Differ from upstream only for a reason on this list: a false success or false
   failure report, damage to data the user did not ask to change, transmission of
   private material, or an operation that a supported platform cannot perform.
   The reason is written next to the difference.
3. Give Unix-like and Windows destinations the same observable behavior wherever
   the platform allows: the same options, outcome states, exit statuses, and
   messages. Where the platform forbids parity, the difference is a destination
   difference, not a compatibility difference.
4. Index every accepted difference. Differences from upstream are `D-xx` rows and
   destination differences are `O-xx` rows in [compatibility.md](compatibility.md);
   each row links to the decision here.

Upstream behavior is described neutrally. Recording a difference does not classify
upstream as defective.

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
| Windows OpenSSH remote hosts | Second milestone: one standard user's key file. Administrator key files and their ACL requirements follow |
| SFTP-only remote access | In scope for `-s`, subject to accessible paths and permission capabilities |
| Other local operating systems or specialized SSH appliances | No support commitment in the initial scope; evaluate separately |

These are implementation targets, not current support claims. The first
milestone remains Windows to a Unix-like host with one explicitly selected key.
Decision recorded on 2026-09-24: the second milestone is one explicitly selected
key for one standard user on a Windows destination, in normal mode. Broader CLI
compatibility, SFTP, administrator destinations, and Linux distribution follow as
separate increments; none is a prerequisite for the first two milestones.

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
bug fix. The original author's intent is not confirmed.

Apparent intent (recorded 2026-09-24): the upstream probe reads as a cheap "can
this key log in" test that assumes `-i` names the only identity the client will
offer; `IdentitiesOnly=yes` in the probe supports that reading. The assumption
breaks because configured `IdentityFile` entries are additive. Reason to differ:
a false success report.

In the upstream source reviewed, a successful probe causes the selected key to
be skipped; the script does not independently verify which identity authenticated.
Another configured identity may remain a candidate. This project will not use
authentication with another key as evidence that the selected key is installed.
Using an existing key to authenticate the actual installation remains valid.

Decision recorded on 2026-09-24: the check has three results.

| Result | Condition | Consequence |
| --- | --- | --- |
| Installed | The selected key was the only candidate the client could offer, and the probe succeeded | Skip the key |
| Not installed | The selected key was the only candidate, and the server answered `Permission denied` | Install the key |
| Inconclusive | Another identity or certificate was a candidate, the session failed after authentication, or the client reported another error | Install the key and print the reason; duplicates are possible, as with `-f` |

Whether other candidates exist is read from `ssh -G`. Generating a configuration
that isolates the selected key, as the Linux experiment did, is a later
optimization that turns Inconclusive into a conclusive result; the first two
milestones do not require it. The scenario has been reproduced in Docker; see the
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

Apparent intent (recorded 2026-09-24): the message shortcut reads as support
for servers that permit only SFTP, where `exit` cannot succeed. Reason to
differ: a false success report, since the message shows only that a shell was
refused, not that the key opened an SFTP session.

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

Apparent intent (recorded 2026-09-24): the SFTP chmod reads as a substitute for
`umask 077`, which SFTP cannot express, aimed at a freshly created `~/.ssh`.
Reason to differ: damage to data the user did not ask to change, when `-t`
points into a shared directory.

This decision covers an existing parent directory selected through `-t`.
Decision recorded on 2026-09-24 for the rest: newly created directories and key
files follow upstream, `umask 077` in normal mode and `chmod 700` on a directory
this tool created and `chmod 600` on the key file in `-s` mode; existing files
keep their modes in normal mode, and `-s` keeps upstream's `chmod 600` on the
key file it uploads. Links are followed as upstream follows them, and no link or
reparse-point check is added; the trust boundary is the account's own home
directory, and a concurrent link swap is outside it. The `-t` path is embedded in
the Unix script with POSIX single-quote escaping and in the Windows script as a
PowerShell single-quoted literal, so quotes in the path are data; upstream's
script breaks on a quote. Test with a custom target in a shared
directory and verify that its permissions are unchanged in both modes, including
when installation fails due to insufficient access.

### Remaining Compatibility Questions

Decision recorded on 2026-09-24: exit statuses are 0 and 1 as in upstream,
including usage errors; the mapping is in [installation outcome](#installation-outcome).
Decision recorded on 2026-09-24 on the rest: messages match upstream's wording
where the behavior is shared, and the golden tests compare them. `-x` prints
each client command line and the remote script to stderr before running them,
the closest equivalent of upstream's `set -x`. The upstream special cases stay:
OpenWrt as root installs into `/etc/dropbear/authorized_keys`, Haiku uses
`config/settings/ssh/authorized_keys`, and NetScreen keys are installed one per
command as upstream does. Never report a failed write as success.

### Dry-Run Behavior

`-n` performs installed-key checks and displays the keys that would be installed.
It does not create the destination directory or key file, append keys, or change
their permissions. Connections for the check still occur unless `-f` skips it.
Authentication logs, login hooks, host key handling, and user-configured
`AddKeysToAgent` behavior may still occur as part of those connections.

Describe `-n` in help as "Display the keys that would be installed without
performing installation operations," not as an offline or side-effect-free mode.
This clarifies the upstream dry-run boundary and adds no new behavioral difference;
the separately recorded SFTP check difference still applies. Decision recorded
on 2026-09-24: `-n -f` makes no connection, as in upstream, where `-f` bypasses
the probe loop.

## Execution Flow

1. Parse arguments and select keys and connection settings.
2. Unless `-f` is set, check authentication for each key and exclude installed keys.
3. Exit successfully if all keys are installed. For `-n`, display the planned keys and exit.
4. In normal mode, detect the destination shell family unless `--target-os` is given; stop before writing when it is unknown.
5. Determine the destination and required permission handling.
6. Preserve existing contents, add a trailing newline if necessary, and append the public keys.
7. Read the result line and report the outcome.
8. Unless `-f` is set, verify that each written key authenticates, and report keys that are installed but not verified.

### Installation Outcome

Decision recorded on 2026-09-24: each run reports one of four outcomes, mapped to
upstream's two exit statuses.

| Outcome | Meaning | Exit status |
| --- | --- | --- |
| No change | Nothing was written: all keys were already installed, the dry run ended, or the run stopped before the first write | 0 for installed keys and dry runs; 1 when an error stopped the run |
| Installed | Every selected key was appended and the file was closed | 0 |
| Partial write | Some data was written, then the run failed at a known point | 1 |
| Unknown | The connection ended without a result line | 1 |

The remote installation script prints one result line as its final output,
`ssh-copy-id: result=<outcome> path=<file>`, and the CLI derives the outcome from
that line alone. The remote exit status is not used: a Windows default shell can
replace it, as recorded in destination difference
[O-04](compatibility.md#destination-differences) and the
[W05 measurement](validation.md#command-delivery-and-exit-codes). A missing
result line is Unknown. Usage errors exit 1, as in upstream.

### Recorded Difference: Post-Installation Verification

Decision recorded on 2026-09-24: after the keys are written, in both normal and
`-s` modes, repeat the installed-key check for each written key unless `-f` was
given. Report each key as installed and verified, or as installed but not
verified with the reason. A rejected verification does not change the exit
status: the file was written, and the message names what to check next, such as
the shared administrator file or the file's ACL.

The reviewed upstream script does not verify after writing; it prints a suggested
login command. Apparent intent: leave verification to the user's next login,
which on Unix usually succeeds because `umask 077` satisfies `StrictModes`.
Reason to differ: a false success report. Windows destinations reject keys silently when the file's ACL is
wrong, when the account has no initialized profile, or when an administrator's
per-user file is ignored under the default configuration, so this project
verifies. Recorded as difference D-06, not as a bug fix. The verification uses
the three results of the selected-identity check; an inconclusive result is
reported as not verified.

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

Apparent intent (recorded 2026-09-24): upstream assumes the caller passes `.pub`
files or `ssh-add -L` output, and `-f` exists so that a public key alone
suffices. Reason to differ: transmission of private material.

Decision recorded on 2026-09-24 on detection: any input line matching
`-----BEGIN` followed by `PRIVATE KEY` (OpenSSH, PKCS#8, PKCS#1 RSA, EC, DSA,
and encrypted variants share that armor) rejects the whole input. Test
representative OpenSSH and PEM private key inputs, including mixed
public/private content and forced mode, without sending their contents.

#### CRLF Normalization

The reviewed upstream script does not explicitly strip CR from CRLF input.
Normalize CRLF line endings in the public key installation input to LF for
transmission. Preserve options and comments; do not rewrite existing remote
file contents or modify the local source file. Test LF and CRLF inputs with
restricted entries and comments through both transports and forced mode.

Apparent intent (recorded 2026-09-24): the upstream script was written for
inputs produced on the same Unix host, where CRLF does not occur. Reason to
differ: a false success report, since a line ending in CR is not a usable key.

Strict public key parsing is not adopted by this decision. Decision recorded on
2026-09-24 on malformed input: a standalone CR, a NUL byte, or a line that
is neither empty, a `#` comment, nor a key entry is rejected before
transmission, with the line number; no size limit is imposed beyond memory.
Encoding is passed through as bytes; a leading byte order mark is handled below.

#### Leading BOM Removal

Decision recorded on 2026-09-24: remove a UTF-8 byte order mark at the start of
the installation input before transmission. Only the first three bytes of the
input are affected; existing remote contents and the local source file are not
modified. Apparent intent: upstream expects input written by `ssh-keygen` or
`ssh-add`, which never emit a byte order mark. Reason to differ: a false success
report, because a line that begins with one is not recognized as a key. Windows
editors and PowerShell 5 redirection produce such files. Recorded as difference
D-07, not as a bug fix. Test a prefixed `.pub` file through both transports and
forced mode.

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
- Decision recorded on 2026-09-24: in normal mode, detect the destination shell family before writing with one command whose output differs between POSIX `sh`, `cmd.exe`, and PowerShell, and stop with an error before any write when the output matches none. `--target-os` overrides detection. `-s` cannot run the probe and assumes a Unix-like destination unless `--target-os` says otherwise. `--remote-shell` is not adopted.

### Windows Remote Command

Decision recorded on 2026-09-24: the command sent to a Windows destination is
exactly `powershell.exe -NoProfile -NonInteractive -EncodedCommand <base64>`. It
contains only letters, digits, and base64, so `cmd.exe /c`, `powershell.exe -c`,
and `pwsh -c` interpret it identically. Every parameter, including the target
path, is inside the encoded script; public keys arrive on stdin. `powershell.exe`
and, when ACLs are set, `icacls.exe` are remote runtime requirements to document.
Whether `pwsh`-only destinations are supported is decided by the W06 result.
The measured command line for each default shell is in the
[validation record](validation.md#command-delivery-and-exit-codes).

## Windows Permissions and Services

### Windows Administrator Scope

Follow Windows OpenSSH's key-file configuration. When installing into the shared
`administrators_authorized_keys` file, display the destination path and explain
that it is shared among administrator accounts before writing. Do not describe
the installation as granting access exclusively to the named account.

Do not modify server configuration to separate administrator accounts' key files.
This is a Windows destination specification, not a behavioral difference from
the Linux implementation. Decision recorded on 2026-09-24: in normal mode the
tool asks the destination whether the account is in the Administrators group
(by SID `S-1-5-32-544`) and selects the shared file when it is; it never reads
`sshd_config`, as upstream never does. A custom `AuthorizedKeysFile` is honored
only through `-t`, and a mismatch is reported by the post-installation
verification (D-06). The ACL procedure is in
[permissions](#permissions-and-service-boundaries).

Validate that shared-file installations display the path and shared scope before
mutation, leave server configuration unchanged, and are distinguished from
installations into a standard user's key file.

### Permissions and Service Boundaries

Decision recorded on 2026-09-24: follow the upstream Unix rule, where `umask 077`
restricts only newly created objects. A directory or key file that this tool
creates receives the minimal ACL that Windows OpenSSH accepts: inheritance
removed, owner set to the account, and full control for the account, SYSTEM, and
Administrators, applied with `icacls.exe` by SID with its exit code checked.
Existing directories and files keep their ACLs, including extra entries. A key
that the server then rejects is reported by the post-installation verification
(D-06) with the file's entries listed, and the user decides what to remove. W07
first checks whether inherited ACLs alone pass the server's check, which would
make the explicit ACL unnecessary under profile directories.
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
CLI itself issues no key-addition or service-management commands.

Decision recorded on 2026-09-24 on option precedence: the same three phases as
upstream, with upstream's overrides and nothing more.

| Phase | Options this tool sets | User options |
| --- | --- | --- |
| Installed-key check | `ControlPath=none`, `LogLevel=INFO`, `PreferredAuthentications=publickey`, `IdentitiesOnly=yes`, and `-i` with the selected key | `-o`, `-F`, `-p`, and the user's configuration are passed through after the overrides |
| Installation | none | passed through unchanged |
| `-s` transfer | a control master shared by the `sftp` sessions, as upstream | passed through unchanged |

The agent is whichever one the SSH client selects through `SSH_AUTH_SOCK` and
`IdentityAgent`; `ssh-add -L` reads `SSH_AUTH_SOCK` only, so a host-specific
`IdentityAgent` can list different keys. This matches upstream and is documented,
not corrected.

## SFTP Mode

SFTP v3 provides `SSH_FXF_APPEND`. When using an API that supports appending, do
not specify `TRUNC`, which discards existing contents. OpenSSH `sftp`'s `put -a`
resumes a transfer; it does not append a file containing only the new data.
Upstream `-s` downloads the file, edits it locally, and uploads it again.

- Distinguish a missing file from permission and communication failures. Decision recorded on 2026-09-24: run `ls -l` on the target before `get`; upstream's `-get` ignores every error and cannot make that distinction. A "No such file" answer means missing; any other error stops the run before writing.
- Decision recorded on 2026-09-24: `-s` downloads, edits locally, and uploads the whole file, as upstream does, because `sftp.exe` cannot append (DL-12).
- Rewriting the entire file can lose concurrent updates. Direct appending also does not guarantee atomicity across duplicate checks and newline handling. Test and define the scope of concurrency support.
- SFTP v3 Unix permission attributes alone cannot configure Windows ACLs adequately. Limit Windows SFTP support and document prerequisites such as existing ACLs.
- Do not automatically fall back to remote commands when `-s` cannot establish the required permissions.

### Recorded Difference: Change Check Before Upload

Decision recorded on 2026-09-24: in `-s` mode, run `ls -l` on the target again
immediately before `put`, and stop without writing, with the No change outcome
and exit status 1, when the size or modification time differs from the value
read before `get`. Apparent intent: upstream's download, edit, and upload
sequence assumes a single writer. Reason to differ: damage to data the user did
not ask to change, since a concurrent append between `get` and `put` is
overwritten. This is a check, not a lock: a change between the second `ls -l`
and `put` is still overwritten. Recorded as difference D-08, not as a bug fix.
Test with an append injected between the two transfers.
- Decision recorded on 2026-09-24: on Windows destinations, `-s` writes to `.ssh/authorized_keys` under the profile, the same default as Unix. It cannot set ACLs or learn whether the account is an administrator, so an existing directory with a correct ACL is a prerequisite. A key rejected afterwards is reported by the post-installation verification (D-06), with the suggestion to pass `-t` for the shared administrator file or to use normal mode.

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

Decision recorded on 2026-09-24: the first two milestones invoke the system
`ssh.exe`, and `sftp.exe` for `-s`. Authentication results are classified from
the client's exit status and a short list of stderr patterns. Each pattern is
tested against every supported OpenSSH client version, and the supported versions
are listed with each release; a client outside that list gets a stated error, not
a guess. A Rust SFTP implementation is re-evaluated only for appending in `-s`
mode. Implementing the SSH protocol from scratch is not the plan.
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

Decision recorded on 2026-09-24 on test structure: argument parsing, key input
validation (D-04, D-05), the installation plan, and the outcome states are pure
functions with unit tests. The `ssh.exe` and `sftp.exe` invocations sit behind a
backend trait with a fake for CLI-level tests. Integration tests run the real
backend against the [Linux Docker fixture](../tests/prototypes/identity-isolation/README.md)
and the [Windows dockur fixture](../tests/environments/windows/README.md).
Compatibility is a golden test: the pinned upstream script and this CLI run
against the same Linux fixture, and stdout and exit status are compared where
behavior is shared.

Continuous integration runs the unit tests and the Linux fixture on GitHub-hosted
runners for every change. The Windows fixture needs KVM and a multi-gigabyte
guest disk, so it runs on a self-hosted runner, to be provisioned later, from
`workflow_dispatch` only; pull requests from forks never reach it. Distributing
a prepared guest disk to hosted runners is deferred until the evaluation image's
redistribution terms are checked.

Fixture results and pending scenarios are in the [validation record](validation.md).
Fixture tests demonstrate OpenSSH behavior, not Rust CLI conformance, and the
fixtures are development tools, not product runtime dependencies. Windows support
is tested with standard users and administrators under `cmd`, Windows PowerShell,
and `pwsh` default shells; SFTP-only servers are tested separately.

## Decision Log

Each decision is recorded once, in the section it affects; this table is the
index. Commit messages name decisions by ID.

| ID | Date | Decision | Section |
| --- | --- | --- | --- |
| DL-01 | 2026-09-20 | Pin upstream commit `eabf198` as the compatibility baseline | [Pinned reference](#pinned-upstream-reference) |
| DL-02 | 2026-09-20 | D-01: another key's success does not show that the selected key is installed | [Selected identity](#recorded-difference-identity-used-for-the-installed-key-check) |
| DL-03 | 2026-09-20 | D-02: `-s` checks by SFTP session, without remote commands | [SFTP check](#recorded-difference-sftp-installed-key-check) |
| DL-04 | 2026-09-20 | D-03: preserve the existing parent directory of a `-t` target | [Custom parent](#recorded-difference-existing-parent-directory-of-a-custom-target) |
| DL-05 | 2026-09-20 | D-04: reject private key input before transmission | [Private key input](#private-key-input-rejection) |
| DL-06 | 2026-09-20 | D-05: normalize CRLF input to LF | [CRLF normalization](#crlf-normalization) |
| DL-07 | 2026-09-20 | Dry run connects for checks and writes nothing | [Dry-run behavior](#dry-run-behavior) |
| DL-08 | 2026-09-20 | Shared administrator file: display path and scope; never change server configuration | [Windows administrator scope](#windows-administrator-scope) |
| DL-09 | 2026-09-20 | Respect `AddKeysToAgent`; never invoke `ssh-add` to add keys | [Agent use](#agent-use-and-user-configuration) |
| DL-10 | 2026-09-20 | Delegate password and passphrase prompts to the SSH client | [Authentication interaction](#authentication-interaction) |
| DL-11 | 2026-09-24 | Design stance: four commitments and the difference procedure | [Design stance](#design-stance) |
| DL-12 | 2026-09-24 | Backend: `ssh.exe` and `sftp.exe`; classification by exit status and tested stderr patterns | [Connection backend](#connection-backend-evaluation) |
| DL-13 | 2026-09-24 | Four outcomes on a result line; exit statuses 0 and 1 | [Installation outcome](#installation-outcome) |
| DL-14 | 2026-09-24 | D-01 has three results; an inconclusive check installs with a warning | [Selected identity](#recorded-difference-identity-used-for-the-installed-key-check) |
| DL-15 | 2026-09-24 | D-06: verify each written key after installation | [Post-installation verification](#recorded-difference-post-installation-verification) |
| DL-16 | 2026-09-24 | Second milestone: one standard user on a Windows destination | [Platform boundaries](#platform-and-delivery-boundaries) |
| DL-17 | 2026-09-24 | Windows remote command is an `EncodedCommand` line only | [Windows remote command](#windows-remote-command) |
| DL-18 | 2026-09-24 | Detect the destination shell family; `--target-os` overrides | [Remote operating systems](#remote-operating-systems-and-shells) |
| DL-19 | 2026-09-24 | ACLs: minimal ACL on new objects only; existing objects untouched | [Permissions](#permissions-and-service-boundaries) |
| DL-20 | 2026-09-24 | `-s` on Windows writes under the profile; D-06 reports rejection | [SFTP mode](#sftp-mode) |
| DL-21 | 2026-09-24 | Test structure and CI placement | [Initial validation](#initial-validation) |
| DL-22 | 2026-09-24 | `-n -f` makes no connection; exit statuses match upstream | [Dry-run behavior](#dry-run-behavior), [Remaining questions](#remaining-compatibility-questions) |
| DL-23 | 2026-09-24 | Verification narratives live in the validation record; this document keeps the index | [Documentation roles](README.md) |
| DL-24 | 2026-09-24 | Every recorded difference states the apparent upstream intent and its reason from the design stance | [Design stance](#design-stance) |
| DL-25 | 2026-09-24 | Option precedence per phase matches upstream's overrides; agent selection as upstream | [Agent use](#agent-use-and-user-configuration) |
| DL-26 | 2026-09-24 | Messages match upstream where shared; `-x` prints commands; OpenWrt, Haiku, and NetScreen cases kept | [Remaining questions](#remaining-compatibility-questions) |
| DL-27 | 2026-09-24 | Private key detection by PEM armor; malformed lines rejected before transmission | [Private key input](#private-key-input-rejection), [CRLF normalization](#crlf-normalization) |
| DL-28 | 2026-09-24 | New-object modes as upstream; links followed; target paths quoted as data | [Custom parent](#recorded-difference-existing-parent-directory-of-a-custom-target) |
| DL-29 | 2026-09-24 | Administrators detected by SID in normal mode; `sshd_config` never read | [Windows administrator scope](#windows-administrator-scope) |
| DL-30 | 2026-09-24 | `-s` checks the target with `ls -l` first and rewrites the whole file | [SFTP mode](#sftp-mode) |
| DL-31 | 2026-09-24 | D-07: remove a leading byte order mark from the input | [Leading BOM removal](#leading-bom-removal) |
| DL-32 | 2026-09-24 | D-08: `-s` stops before `put` when the target changed since `get` | [Change check before upload](#recorded-difference-change-check-before-upload) |

## References

- [OpenSSH ssh-copy-id implementation](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id)
- [OpenSSH ssh-copy-id manual](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id.1)
- [SFTP v3 draft: opening files](https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-02#section-6.3)
- [OpenSSH sftp manual](https://man.openbsd.org/sftp.1)
- [Key management for Windows OpenSSH](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement)
- [Windows OpenSSH server configuration](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-server-configuration)

If upstream code is reused, retain its copyright notices and license terms.
