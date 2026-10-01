# ssh-copy-id for Windows Design

[日本語](design.jp.md)

[Differences from upstream](compatibility.md)

Status: nothing is released, and the command does not install keys yet. The [delivery plan](#delivery-plan) orders
the remaining work, and [open questions](open-questions.md) lists what is not
decided. This document states the current design and the reasons that still
hold; the history of a change is in its commit message.

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

Every decision in this document follows four commitments.

1. Read the pinned upstream script and manual first, and state what its behavior
   appears to intend, before deciding whether to differ. No difference exists
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
   each row links to the section here that states it.

Upstream behavior is described neutrally. Stating a difference does not classify
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
| Unix-like remote hosts | First milestone, using the shell and file layout described below |
| Windows OpenSSH remote hosts | Second milestone: one standard user's key file. Administrator key files and their ACL requirements follow |
| SFTP-only remote access | In scope for `-s`, subject to accessible paths and permission capabilities |
| Other local operating systems or specialized SSH appliances | No support commitment in the initial scope; evaluate separately |

These are implementation targets, not current support claims. The first
milestone is one explicitly selected key on a Unix-like destination. The second
milestone is one explicitly selected key for one standard user on a Windows
destination, in normal mode. Broader CLI compatibility, SFTP, administrator
destinations, and Linux distribution are separate increments, and none is a
prerequisite for either milestone. The delivery plan orders them.

### Delivery Plan

Each stage ends when its checks pass; the IDs are rows of the
[validation matrix](validation.md#matrix).

| Stage | Release | Content | Done when |
| --- | --- | --- | --- |
| 0. Feasibility | none | Prototype of `ssh.exe` on Windows with authentication prompts while public keys go to stdin; a Linux sshd fixture for CLI integration tests | A01 passes for a password, an encrypted key's passphrase, agent confirmation, cancellation, and no terminal; L02 passes and runs in CI |
| 1. Unix destination, one key (first milestone) | v0.0.1 | `[user@]host`, `-i file`, `-p`, `-o`, `-F`; the installed-key check with three results; the `sh` installation script with result lines; post-installation verification; exit statuses | Initial requirements 1, 2, 3, 7, and 8 pass as tests against L02 |
| 1.5. CLI compatibility | v0.0.2 before stage 2; otherwise per the versioning rule | `-n`, `-f`, `-t` on Unix-like destinations, `-i` without a file, default key selection, agent keys, `-x`, upstream messages | Requirements 4 and 5 pass; the D-03 and D-11 tests pass; golden tests against the pinned script (P01) pass for the shared behavior |
| 2. Windows standard user (second milestone) | v0.1.0 | One explicitly selected key for one standard user in normal mode: destination detection, the PowerShell installation script, ACLs on new objects | W06, W09, and W07's first check (whether an inherited profile ACL alone passes) pass before the implementation; requirements 1, 2, 3, 7, and 8 pass against the dockur fixture |
| 3. Later increments | 0.1.x or later | Shared administrator file (the rest of W07), `-t` on Windows destinations (W08), `-s` (requirement 6), Linux packages | Decided per increment |

Stage 0 comes first because a failed prototype would invalidate the choice of
`ssh.exe` as the backend, on which stages 1 to 3 build. Stage 1.5 is not a
prerequisite for stage 2. Its release version follows the versioning rule when it
ships: 0.0.2 before stage 2, and after 0.1.0 a minor bump if it changes existing
command-line behavior, such as messages or default key selection, otherwise a
patch bump.

The Windows fixture runs Windows 11 Enterprise Evaluation. On 2026-09-30 its
license reported a grace period of 114,769 minutes, which ends on 2026-12-18; the
stage 2 fixture scenarios run before then or on a rebuilt guest.

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

- Count required executables, such as the system OpenSSH client, as runtime dependencies and document them; invoking an external command is not dependency-free.
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
Declare the OpenSSH client as a runtime dependency while it is the connection backend.

Users who want the familiar command name in their shell can explicitly opt in:

```sh
alias ssh-copy-id=ssh-copy-id-rs
```

Packages must not add this alias or modify the user's shell configuration
automatically. Package names need not match executable names. This policy does
not imply that an APT or pacman package or Linux client support is already
available.

### Versioning and Release

The model follows the owner's runex and gitreant repositories. A bullet that
follows one of them names it.

- Versions follow Semantic Versioning. `Cargo.toml` carries 0.0.0 until the first release, and every release, v0.0.1 included, raises it in its own bump commit, as runex does. Releases start at 0.0.1 with stage 1, and Windows destination support in stage 2 is 0.1.0; stage 1.5 takes the version the rule below gives when it ships, as the delivery plan describes. From 0.1.0 until 1.0.0, a breaking change to the command line bumps the minor version and anything else bumps the patch.
- The branching model is trunk-based, as in runex. `main` is always releasable, and changes reach it through pull requests from short-lived branches named after their issue when one exists (`feat/`, `fix/`, `docs/`, `ci/`). The commit that bumps the version to cut a release is the one commit pushed to `main` directly. The owner's general rule of GitFlow for versioned packages is not applied, for the reason runex dropped its `develop` branch: every change already lands as a self-contained pull request, so an integration branch only delays fixes.
- Releases are signed annotated tags `vX.Y.Z` on `main`, created with `git tag -s` and checked with `git tag -v`, as runex requires; runex's tags through v0.1.20 were annotated but unsigned. A tag containing `-`, such as `v0.0.1-pre`, is a prerelease, as in gitreant, and is signed as well. A prerelease rehearses the pipeline without publishing to crates.io: unlike gitreant, where only `[skip publish]` in the tagged commit stops publishing, the publish job here skips every tag containing `-`, because a crates.io version cannot be reused once published.
- `CHANGELOG.md` follows Keep a Changelog, as in runex; gitreant has no changelog. Every user-visible change adds an entry under `[Unreleased]`, which a release renames to the version and date. An entry names the difference IDs it adds or changes.
- `release.yml` is written before v0.0.1, modeled on runex and gitreant, which both do the following: tests on Linux, Windows, and macOS gate the release, then builds, a GitHub release, and publishing to crates.io through trusted publishing with OIDC. A release checklist in `CONTRIBUTING.md` comes with it, as in runex. The crate name is `ssh-copy-id`; it and `ssh-copy-id-rs` returned 404 from the crates.io API on 2026-09-30 and 2026-10-01.
- v0.0.1 is published to crates.io and as a GitHub release only. Until it is published, the README states that nothing is published and shows a build from source.

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

The following are compatibility targets, not a claim of full compatibility; the
compatibility suite is part of stage 1.5.

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
| `-x` | Print each client command and the remote script before running them (D-14) |
| `-h`, `-?` | Display help |
| `--target-os unix\|windows` | Override destination detection (D-12); not in upstream |

Normally, determine whether a key is already installed by checking whether it can
authenticate. Do not substitute a text comparison against `authorized_keys`.
Avoid false positives caused by authentication with another key or reuse of an
existing multiplexed connection. Do not interpret connection failures or host key
errors as evidence that a key is not installed.

### Recorded Difference: Identity Used for the Installed-Key Check

In the upstream source reviewed, a successful probe causes the selected key to
be skipped; the script does not independently verify which identity authenticated.
Another configured identity may remain a candidate. This project does not use
authentication with another key as evidence that the selected key is installed.
Using an existing key to authenticate the actual installation remains valid.
This is difference D-01. It is a behavioral difference, not a bug fix; the
original author's intent is not confirmed.

Apparent intent: the upstream probe reads as a cheap "can this key log in" test
that assumes `-i` names the only identity the client will offer;
`IdentitiesOnly=yes` in the probe supports that reading. The assumption breaks
because configured `IdentityFile` entries are additive. Reason to differ: a false
success report.

The check has three results.

| Result | Condition | Consequence |
| --- | --- | --- |
| Installed | The selected key was the only candidate the client could offer, and the probe succeeded | Skip the key |
| Not installed | The selected key was the only candidate, and the server answered `Permission denied` | Install the key |
| Inconclusive | Another identity or certificate was a candidate, the session failed after authentication, or the client reported another error | Install the key and print the reason; duplicates are possible, as with `-f` |

Whether other candidates exist is read from `ssh -G`. Generating a configuration that isolates the
selected key would turn Inconclusive into a conclusive result; it is not needed
for the first two milestones, and it has limits stated in
[selected-key isolation](#selected-identity-experiment-results).

### Recorded Difference: SFTP Installed-Key Check

Normal mode keeps the upstream remote `exit` check. In `-s` mode, the check
establishes an SFTP session with the selected key, without sending `exit` or any
other remote command. A successful SFTP protocol session, not merely a TCP
connection, is required.

The reviewed upstream implementation attempts `exit` even for `-s` and also
treats the message `allows sftp connections only` as a successful check.
This project does not use that message as a substitute for an SFTP session.
This is difference D-02, not a bug fix.

Apparent intent: the message shortcut reads as support for servers that permit
only SFTP, where `exit` cannot succeed. Reason to differ: a false success report,
since the message shows only that a shell was refused, not that the key opened
an SFTP session.

The selected-identity rule above applies to both modes. `-f` skips the check.
Failure to establish SFTP does not by itself mean that the key is absent. The SFTP
check does not replace normal-mode exit-status checking with direct inspection of
the authentication result.

### Recorded Difference: Existing Parent Directory of a Custom Target

When `-t` specifies a target, preserve the permissions of its existing parent
directory in both normal and SFTP modes. If permissions prevent installation,
report an error instead of changing the parent directory's mode or ACL.

In the reviewed upstream implementation, normal mode uses `umask 077` for
creation without an unconditional chmod of the existing parent. SFTP mode sets
the parent directory to mode 700. Preserving that directory in this project's
SFTP mode is difference D-03, not a bug fix.

Apparent intent: the SFTP chmod reads as a substitute for `umask 077`, which SFTP
cannot express, aimed at a freshly created `~/.ssh`. Reason to differ: damage to
data the user did not ask to change, when `-t` points into a shared directory.

Newly created directories and key files follow upstream: `umask 077` in normal
mode, and in `-s` mode `chmod 700` on a directory this tool created and
`chmod 600` on the key file. Existing files keep their modes in normal mode, and
`-s` keeps upstream's `chmod 600` on the key file it uploads. Links are followed
as upstream follows them, and no link or reparse-point check is added; the trust
boundary is the account's own home directory, and a concurrent link swap is
outside it. A relative `-t` path is relative to the home directory, as in
upstream, which runs `cd` before writing; on Windows destinations it is relative
to the profile directory. Concurrent installations in normal mode rely on
appending, as upstream's `cat >>` does, and no lock is added. None of these is a
difference.

The `-t` path is embedded in the Unix script with POSIX single-quote escaping and
in the Windows script as a PowerShell single-quoted literal, so quotes in the
path are data. Upstream embeds the path inside a single-quoted `sh -c` argument,
so a quote in the path ends the script text early and the rest of the path is
read as shell syntax. Apparent intent: `-t` names a plain path relative to the
home directory. Reason to differ: the run fails or executes text from the path in
the account, which can damage data the user did not ask to change. This is
difference D-11.

Test with a custom target in a shared directory and verify that its permissions
are unchanged in both modes, including when installation fails due to
insufficient access.

### Exit Statuses, Messages, and Special Destinations

Exit statuses are 0 and 1 as in upstream, including usage errors; the mapping is
in [installation outcome](#installation-outcome). Messages match upstream's
wording where the behavior is shared, and the golden tests compare them. Never
report a failed write as success.

`-x` prints each client command line and the remote script to stderr before
running them, the closest equivalent of upstream's `set -x`. Apparent intent of
upstream's `-x`: trace the shell script itself. Reason to differ: a compiled
program has no shell trace, so the platform cannot perform the upstream behavior.
This is difference D-14; the output format differs, the purpose does not.

The upstream special cases stay: OpenWrt as root installs into
`/etc/dropbear/authorized_keys`, Haiku uses `config/settings/ssh/authorized_keys`,
and NetScreen keys are installed one per command as upstream does.

### Dry-Run Behavior

`-n` performs installed-key checks and displays the keys that would be installed.
It does not create the destination directory or key file, append keys, or change
their permissions. Connections for the check still occur unless `-f` skips it;
`-n -f` makes no connection, as in upstream, where `-f` bypasses the probe loop.
Authentication logs, login hooks, host key handling, and user-configured
`AddKeysToAgent` behavior may still occur as part of those connections.

Describe `-n` in help as "Display the keys that would be installed without
performing installation operations," not as an offline or side-effect-free mode.
This clarifies the upstream dry-run boundary and adds no new behavioral difference;
the SFTP check difference D-02 still applies.

## Execution Flow

1. Parse arguments and select keys and connection settings.
2. Unless `-f` is set, check authentication for each key and exclude installed keys.
3. Exit successfully if all keys are installed. For `-n`, display the planned keys and exit.
4. In normal mode, detect the destination shell family unless `--target-os` is given; stop before writing when it is unknown.
5. Determine the destination and required permission handling.
6. Preserve existing contents, add a trailing newline if necessary, and append the public keys.
7. Read the result lines and report the outcome.
8. Unless `-f` is set, verify that each written key authenticates, and report keys that are installed but not verified.

### Installation Outcome

Each run reports one of four outcomes, mapped to upstream's two exit statuses.

| Outcome | Meaning | Exit status |
| --- | --- | --- |
| No change | Nothing was written: all keys were already installed, the dry run ended, or the run stopped before the first write | 0 for installed keys and dry runs; 1 when an error stopped the run |
| Installed | Every selected key was appended and the file was closed | 0 |
| Partial write | Some data was written, then the run failed at a known point | 1 |
| Unknown | The connection ended without a valid summary line | 1 |

The remote installation script prints result lines, and the CLI derives the
outcome from them alone. The remote exit status is not used, because a Windows
default shell can replace it: with Windows PowerShell as the default shell, a
native child command's nonzero exit status reaches the client as 1 (destination
difference [O-04](compatibility.md#destination-differences), validation row W05).
Usage errors exit 1, as in upstream.

The format is one line per key,
`ssh-copy-id: key=<n> result=<added|skipped|failed> path=<file>`, then one
summary line as the final output,
`ssh-copy-id: result=<unchanged|installed|partial> added=<n>`. `<n>` is the key's
position in the input, starting at 1, without leading zeros. In `<file>`, every
byte outside `0x21` to `0x7E`, and `%` and `=`, is written as `%XX` in uppercase
hexadecimal; encoding bytes from `0x80` keeps a console code page from altering a
non-ASCII path. Lines without the `ssh-copy-id: ` prefix are ignored. A prefixed
line that does not parse, including one that is not UTF-8, makes the outcome
Unknown, as does any prefixed line after the summary. Per-key lines show which
keys were written before a partial write. These lines are not shown to the user;
the user sees upstream's messages.

### Recorded Difference: Post-Installation Verification

After the keys are written, in both normal and `-s` modes, repeat the
installed-key check for each written key unless `-f` was given. Report each key
as installed and verified, or as installed but not verified with the reason. A
rejected verification does not change the exit status: the file was written, and
the message names what to check next, such as the shared administrator file or
the file's ACL. The verification uses the three results of the selected-identity
check; an inconclusive result is reported as not verified.

The reviewed upstream script does not verify after writing; it prints a suggested
login command. Apparent intent: leave verification to the user's next login,
which on Unix usually succeeds because `umask 077` satisfies `StrictModes`.
Reason to differ: a false success report. Windows destinations reject keys
silently when the file's ACL is wrong, when the account has no initialized
profile, or when an administrator's per-user file is ignored under the default
configuration. This is difference D-06, not a bug fix.

### Public Key Input

Preserve key options such as `restrict` and `command=`, and comments. Do not
reconstruct installation entries from only the key type and key blob. The
following differences apply to both installation transports, including `-f`.

#### Private Key Input Rejection

The reviewed upstream script has no explicit check rejecting private key content
in the installation input; with `-f`, that content may be transmitted. Reject
private key content supplied as public key installation input before transmitting
any of that input, and report an error. `-f` skips installed-key checks, not this
input check. This does not prohibit SSH from using a local private key for
authentication. This is difference D-04.

Apparent intent: upstream assumes the caller passes `.pub` files or `ssh-add -L`
output, and `-f` exists so that a public key alone suffices. Reason to differ:
transmission of private material.

Any input line containing both `-----BEGIN` and `PRIVATE KEY` rejects the whole
input; OpenSSH, PKCS#8, PKCS#1 RSA, EC, DSA, and encrypted variants share that
armor. Test representative OpenSSH and PEM private key inputs, including mixed
public/private content and forced mode, without sending their contents.

#### CRLF Normalization

The reviewed upstream script does not explicitly strip CR from CRLF input.
Normalize CRLF line endings in the public key installation input to LF for
transmission. Preserve options and comments; do not rewrite existing remote
file contents or modify the local source file. This is difference D-05. Test LF
and CRLF inputs with restricted entries and comments through both transports and
forced mode.

Apparent intent: the upstream script was written for inputs produced on the same
Unix host, where CRLF does not occur. Reason to differ: a false success report,
since a line ending in CR is not a usable key.

Strict public key parsing, beyond the checks in this section, is not adopted.
Input with no key entry is rejected, as upstream reports "No identities found".
A standalone CR, a NUL byte, or a line
that is neither empty, a `#` comment, nor a key entry is rejected before
transmission, with the line number; no size limit is imposed beyond memory.
Encoding is passed through as bytes; a leading byte order mark is handled below.
A key entry is, after leading spaces or tabs, `[options] keytype base64 [comment]`.
The keytype starts with `ssh-`, `ecdsa-sha2-`, `sk-ssh-`, or `sk-ecdsa-sha2-`,
which includes certificate types. The base64 field is one or more of
`A-Z a-z 0-9 + /` followed by at most two `=`. Options are present only when the
first field is not a keytype, and end at the first space or tab outside double
quotes, where `\"` escapes a quote. Apparent intent of upstream, which appends
such lines: input comes from `ssh-keygen` or `ssh-add` and is well formed. Reason
to differ: a false success report, since sshd ignores a malformed line that
upstream counts as added. This is difference D-10.

Certificate lines, whose type ends in `-cert-v01@openssh.com`, and
`cert-authority` lines are passed through as upstream does. sshd does not accept
a certificate line in an authorized-keys file for login, so the installed-key
check and the post-installation verification report such a line as Inconclusive
with that reason. This is not a difference.

#### Leading BOM Removal

Remove a UTF-8 byte order mark at the start of the installation input before
transmission. Only the first three bytes of the input are affected; existing
remote contents and the local source file are not modified. Apparent intent:
upstream expects input written by `ssh-keygen` or `ssh-add`, which never emit a
byte order mark. Reason to differ: a false success report, because a line that
begins with one is not recognized as a key. Windows editors and PowerShell 5
redirection produce such files. This is difference D-07, not a bug fix. Test a
prefixed `.pub` file through both transports and forced mode.

## Remote Operating Systems and Shells

Implement the basic path for Unix-like destinations first, then extend it to
Windows destinations. Do not require an OS option for normal use; assume a
Unix-like destination by default, as the Linux version does.

| Destination | Usual key file | Permission handling |
| --- | --- | --- |
| Unix-like system | `.ssh/authorized_keys` | Normal mode: new directory 700 and new file 600 through `umask 077`, existing permissions unchanged. `-s`: `chmod 700` on a directory it created and `chmod 600` on the key file it uploads |
| Standard Windows user | `.ssh/authorized_keys` in the user profile | Minimal ACL on new objects; existing ACLs unchanged |
| Windows user in the Administrators group | `%ProgramData%/ssh/administrators_authorized_keys` under the default configuration | Restrict access to SYSTEM and Administrators |

Server configuration can change the destination. The OS alone cannot establish
administrator group membership or the actual key file location; an explicit
`-t` path is always respected.

Distinguish the default shell that initially interprets the SSH command from the
shell that executes the installation script. For example:
`SSH → cmd.exe → powershell.exe → key installation`.

- Use POSIX `sh` for Unix-like installation scripts without requiring Bash-specific features.
- Use Windows PowerShell for Windows scripts, launched as described in [Windows remote command](#windows-remote-command), and verify that the scripts also run under `pwsh`.
- Test launching through `cmd`, Windows PowerShell, and `pwsh` as the default shell.
- Do not infer the OS from the shell name; `pwsh`, for example, also runs on Linux.

In normal mode, detect the destination shell family before writing with one
command whose output differs between POSIX `sh`, `cmd.exe`, and PowerShell, and
stop with an error before any write when the output matches none.
`--target-os unix|windows` overrides detection. `-s` cannot run the probe and
assumes a Unix-like destination unless `--target-os` says otherwise. The probe
command and its expected outputs come from measurements under `cmd.exe`, Windows
PowerShell, `pwsh`, `sh`, `bash`, and `dash` (validation row W09, not yet run);
`echo %OS%:$0` is the starting candidate. Apparent intent of upstream, which sends
its `sh` script without checking: destinations run a POSIX shell. Reason to
differ: Windows destinations cannot run that script. This is difference D-12:
normal mode makes one more connection, and `--target-os` is a new option.

### Windows Remote Command

The command sent to a Windows destination is exactly
`powershell.exe -NoProfile -NonInteractive -EncodedCommand <base64>`. It contains
only letters, digits, and base64, so `cmd.exe /c`, `powershell.exe -c`, and
`pwsh -c` interpret it identically. Every parameter, including the target path,
is inside the encoded script; public keys arrive on stdin. `powershell.exe` and,
when ACLs are set, `icacls.exe` are remote runtime requirements to document.
Under `cmd.exe` sshd runs `cmd.exe /c "<command>"`, and under Windows PowerShell
it runs `powershell.exe -c "<command>"`, where `$`, `;`, and quotes in a plain
command would be interpreted (destination difference O-03).

## Windows Permissions and Services

### Windows Administrator Scope

Follow Windows OpenSSH's key-file configuration. When installing into the shared
`administrators_authorized_keys` file, display the destination path and explain
that it is shared among administrator accounts before writing. Do not describe
the installation as granting access exclusively to the named account.

Do not modify server configuration to separate administrator accounts' key files.
This is a Windows destination specification, not a behavioral difference from
the Linux implementation. In normal mode the tool asks the destination whether
the account is in the Administrators group (by SID `S-1-5-32-544`) and selects
the shared file when it is; it never reads `sshd_config`, as upstream never does.
A custom `AuthorizedKeysFile` is honored only through `-t`, and a mismatch is
reported by the post-installation verification (D-06).

Validate that shared-file installations display the path and shared scope before
mutation, leave server configuration unchanged, and are distinguished from
installations into a standard user's key file.

### Permissions and Service Boundaries

Follow the upstream Unix rule, where `umask 077` restricts only newly created
objects. A directory or key file that this tool creates receives the minimal ACL
that Windows OpenSSH accepts: inheritance removed, owner set to the account, and
full control for the account, SYSTEM, and Administrators, applied with
`icacls.exe` by SID with its exit code checked. Existing directories and files
keep their ACLs, including extra entries. A key that the server then rejects is
reported by the post-installation verification (D-06) with the file's entries
listed, and the user decides what to remove. Whether an inherited profile ACL
alone passes the server's check, which would make the explicit ACL unnecessary
under profile directories, is an open question settled by validation row W07. Explain permission failures without automatically
elevating privileges or managing services.

`ssh-agent` is a helper service for private keys. Installing public keys on the
remote host does not require starting or restarting it. Do not automatically
start the local agent either; fall back to key file selection when it is
unavailable. Do not restart `sshd` solely for an `authorized_keys` update.

## Agent Use and User Configuration

The CLI does not invoke `ssh-add` to add keys. Listing public keys with
`ssh-add -L` remains in scope. Respect the user's `AddKeysToAgent` setting:
do not override it to prevent SSH from adding a key during authentication.
This preserves the reviewed upstream behavior and is not a difference. Agent
services are not started, stopped, or restarted by this tool.

Verify that the configured `AddKeysToAgent` behavior is preserved and that the
CLI itself issues no key-addition or service-management commands.

Option precedence has the same three phases as upstream, with upstream's
overrides and nothing more.

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
resumes a transfer; it does not append a file containing only the new data. `-s`
therefore downloads the file, edits it locally, and uploads it again, as upstream
does, because `sftp.exe` cannot append. Rewriting the whole file can lose
concurrent updates; direct appending would not guarantee atomicity across
duplicate checks and newline handling either.

- Run `ls -l` on the target before `get`. A "No such file" answer means the file is missing; any other error stops the run before writing. Upstream's `-get` ignores every error and cannot make that distinction. Apparent intent of the leading `-`: tolerate the missing file of a first installation. Reason to differ: when `get` fails for another reason, upstream continues and uploads a file that holds only the new keys, which can replace the existing file. That is damage to data the user did not ask to change. This is difference D-09.
- SFTP v3 Unix permission attributes alone cannot configure Windows ACLs. On Windows destinations, `-s` writes to `.ssh/authorized_keys` under the profile, the same default as Unix. It cannot set ACLs or learn whether the account is an administrator, so an existing directory with a correct ACL is a prerequisite (destination difference O-05). A key rejected afterwards is reported by the post-installation verification (D-06), with the suggestion to pass `-t` for the shared administrator file or to use normal mode.
- Do not automatically fall back to remote commands when `-s` cannot establish the required permissions.
- Test concurrent updates in `-s` mode to define the supported scope.

### Recorded Difference: Change Check Before Upload

In `-s` mode, run `ls -l` on the target again immediately before `put`, and stop
without writing, with the No change outcome and exit status 1, when the size or
modification time differs from the value read before `get`. Apparent intent:
upstream's download, edit, and upload sequence assumes a single writer. Reason to
differ: damage to data the user did not ask to change, since a concurrent append
between `get` and `put` is overwritten. This is a check, not a lock: a change
between the second `ls -l` and `put` is still overwritten. This is difference
D-08, not a bug fix. Test with an append injected between the two transfers.

## Implementation Boundaries

### Authentication Interaction

Delegate password and private-key passphrase prompts to the SSH client, and agent
confirmation requests to SSH and the agent. This CLI does not collect or store
passwords or passphrases. This follows upstream behavior and adds no compatibility
difference.

Whether sending public keys through stdin can coexist with authentication prompts
in the `ssh.exe` process path on Windows is the subject of stage 0. The prototype
covers password authentication, encrypted private keys, agent confirmation,
cancellation, and execution without an interactive terminal. Keep authentication
interaction separate from the public key data stream; do not solve prompt
handling by collecting credentials in this CLI or globally enabling
`BatchMode=yes`.

### Connection Backend Evaluation

Separate the CLI, key selection, installed-key checks, connections, destination
policy, and file updates.

The backend for the first two milestones is the system `ssh.exe`, and `sftp.exe`
for `-s`. It is expected to reuse the user's SSH configuration, authentication,
agent, jump hosts, and host key verification; whether interactive prompts coexist
with public key data on stdin is verified in stage 0. A Rust SSH/SFTP
library is reconsidered only if it can satisfy the authentication delegation
policy above without collecting credentials in this CLI, and handle SSH
configuration, agents, jump hosts, and host key verification. A Rust SFTP
implementation over the `ssh.exe` subsystem is reconsidered only for appending in
`-s` mode. Any reconsideration compares the full build and runtime dependency
footprint, including packaging costs, alongside compatibility and authentication
behavior. Implementing the SSH protocol from scratch is not the plan.

Authentication results are classified from the client's exit status and a short
list of stderr patterns: `Permission denied`, host key verification failure, and
connection failure. The tested clients are `OpenSSH_for_Windows_9.5p2` and
OpenSSH `9.6p1` as packaged in Ubuntu 24.04, which the fixtures exercise. Each
release lists its tested client versions, and a version is added after the
fixtures pass with it; every pattern is checked against every listed version. With any other
client version the tool prints a warning and continues. Output that matches no
known pattern makes the check Inconclusive, so the key is installed with a
warning and the post-installation verification reports the result. Apparent
intent of upstream, which runs with any client: it classifies only by exit status
and one `Permission denied` match, so it needs no version list. Reason to differ:
an untested pattern can misclassify, which is a false success or false failure
report. The warning and the Inconclusive result prevent that report without
refusing clients such as the one in Git for Windows. This is difference D-13.

### Selected-Identity Experiment Results

Isolating the selected key while keeping jump-host credentials and host key
verification is feasible for a fixed configuration: a temporary `-F`
configuration built from `ssh -G` output, with additive identity and certificate
lists removed, made an absent selected key fail with `Permission denied` and an
installed one succeed, both directly and through one jump host (validation row
L01). This is not a production implementation. Arbitrary configuration
serialization, `Match`, percent expansion, quoted paths, multiple jumps,
certificates, agent selection, and multiplexing are not covered. Isolation is a
later optimization that turns Inconclusive into a conclusive result; the first
two milestones do not require it.

## Initial Validation

The first milestone is to install one explicitly selected key on a Unix-like
destination without adding it again on a subsequent run. It covers requirements
1, 2, 3, 7, and 8 below; requirements 4 and 5 belong to stage 1.5 and
requirement 6 to stage 3 of the [delivery plan](#delivery-plan). Each
requirement becomes behavioral tests.

1. Do not append a key that can already authenticate, and do not mistake success with another key for success with the selected key.
2. Correctly append to an empty file, a missing file, and a file without a trailing newline.
3. Preserve existing contents and do not treat permission or communication errors as a missing file.
4. `-n` must not create the destination directory or key file, append keys, or change their permissions. Verify that installed-key checks still run unless `-f` skips them and that normal connection side effects are not described as absent.
5. `-f` must skip installed-key checks and must not require the corresponding private key file.
6. `-s` must not execute remote commands, including for discovery, authentication checks, or ACL configuration.
7. Treat paths and key comments containing spaces, Japanese characters, or quotes as data.
8. Do not report authentication failures, host key mismatches, write failures, or permission configuration failures as success.

Argument parsing, key input validation, the installation plan, and the outcome
states are pure functions with unit tests. The `ssh.exe` and `sftp.exe`
invocations sit behind a backend trait with a fake for CLI-level tests.
Integration tests run the real backend against the
[Linux Docker fixture](../tests/prototypes/identity-isolation/README.md) and the
[Windows dockur fixture](../tests/environments/windows/README.md). Compatibility
is a golden test: the pinned upstream script and this CLI run against the same
Linux fixture, and stdout and exit status are compared where behavior is shared.

Continuous integration (`.github/workflows/ci.yml`) runs `cargo fmt --check` and
`cargo clippy -D warnings` on Linux, `cargo test` on Linux and Windows, and the
Linux fixture, on GitHub-hosted runners for every push to `main` and every pull
request. The Windows
fixture needs KVM and a multi-gigabyte guest disk, so it runs on a self-hosted
runner from `workflow_dispatch` only; pull requests from forks never reach it.

Fixture tests demonstrate OpenSSH behavior, not Rust CLI conformance, and the
fixtures are development tools, not product runtime dependencies. Windows support
is tested with standard users and administrators under `cmd`, Windows PowerShell,
and `pwsh` default shells; SFTP-only servers are tested separately. The status of
each scenario is in the [validation matrix](validation.md#matrix).

## References

- [OpenSSH ssh-copy-id implementation](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id)
- [OpenSSH ssh-copy-id manual](https://github.com/openssh/openssh-portable/blob/master/contrib/ssh-copy-id.1)
- [SFTP v3 draft: opening files](https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-02#section-6.3)
- [OpenSSH sftp manual](https://man.openbsd.org/sftp.1)
- [Key management for Windows OpenSSH](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement)
- [Windows OpenSSH server configuration](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-server-configuration)

If upstream code is reused, retain its copyright notices and license terms.
