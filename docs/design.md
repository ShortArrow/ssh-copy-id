# ssh-copy-id for Windows Design

[日本語](design.jp.md)

[Differences from upstream](compatibility.md) | [Behavior defined where the standards are silent](definitions.md)

Status: v0.0.1 is released. The command installs one explicitly selected key on
a Unix-like destination (stage 1). The [delivery plan](#delivery-plan) orders
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
   without that reading. Whether a behavior is intended or a flaw is settled
   from evidence, not from how it looks: the script and its comments, its
   commit history, the manual, and upstream's issue tracker. The evidence is
   cited next to the apparent intent; an intent found in none of them is marked
   as inferred, and what would settle it goes to the
   [open questions](open-questions.md).
2. Differ from upstream only for a reason on this list: a false success or false
   failure report, damage to data the user did not ask to change, transmission of
   private material, an operation that a supported platform cannot perform, or
   a run that never ends without reporting an outcome. The reason is written
   next to the difference.
3. Give Unix-like and Windows destinations the same observable behavior wherever
   the platform allows: the same options, outcome states, exit statuses, and
   messages. Where the platform forbids parity, the difference is a destination
   difference, not a compatibility difference.
4. Index every accepted difference. Differences from upstream are `D-xx` rows and
   destination differences are `O-xx` rows in [compatibility.md](compatibility.md);
   each row links to the section here that states it. Behavior this tool defines
   where the SSH and SFTP standards or the OpenSSH documentation are silent is
   indexed the same way, as `U-xx` rows in [definitions.md](definitions.md).

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
| 0. Feasibility | none | Prototype of `ssh.exe` on Windows with authentication prompts while public keys go to stdin; a Linux sshd fixture for CLI integration tests | A01 passes for a password, an encrypted key's passphrase, agent confirmation (or the agent's refusal of it), cancellation, and no terminal; L02 passes and runs in CI |
| 1. Unix destination, one key (first milestone) | v0.0.1 | `[user@]host`, `-i file`, `-p`, `-o`, `-F`; the installed-key check with three results; the `sh` installation script with result lines; post-installation verification; exit statuses | Initial requirements 1, 2, 3, 7, and 8 pass as tests against L02 |
| 1.5. CLI compatibility | v0.0.2 before stage 2; otherwise per the versioning rule | `-n`, `-f`, `-t` on Unix-like destinations, `-i` without a file, key files with several keys, default key selection, agent keys, `-x`, upstream messages, NetScreen destinations | Requirements 4 and 5 pass; the D-03 and D-11 tests pass; golden tests against the pinned script (P01) pass for the shared behavior |
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
- Releases are signed annotated tags `vX.Y.Z` on `main`, created with `git tag -s` and checked with `git tag -v`, as runex requires; runex's tags through v0.1.20 were annotated but unsigned. A tag containing `-`, such as `v0.0.1-pre`, is a prerelease, as in gitreant, and is signed as well. A prerelease rehearses the pipeline without publishing to crates.io: unlike gitreant, where only `[skip publish]` in the tagged commit stops publishing, the publish job here skips every tag containing `-`, because a crates.io version cannot be reused once published. A prerelease is tagged at the version `main` carries, without a bump commit; the release workflow checks that every tag is on `main` and that a full release tag names the version in `Cargo.toml`.
- `CHANGELOG.md` follows Keep a Changelog, as in runex; gitreant has no changelog. Every user-visible change adds an entry under `[Unreleased]`, which a release renames to the version and date. An entry names the difference IDs it adds or changes.
- `release.yml` is modeled on runex and gitreant: tests on Linux, Windows, and macOS, and here the Linux fixture tests as well, gate the release, then a build, a GitHub release, and publishing to crates.io through trusted publishing with OIDC. The GitHub release carries one archive, `ssh-copy-id-x86_64-pc-windows-msvc.zip` with `ssh-copy-id.exe`, `LICENSE`, and `README.md`; other targets are added when they are supported. Build provenance is attested while the repository is public, as gitreant does. The release checklist is in `CONTRIBUTING.md`, as in runex. The crate is dual-licensed under MIT or Apache-2.0, as runex is. The crate name is `ssh-copy-id`; it and `ssh-copy-id-rs` returned 404 from the crates.io API on 2026-09-30 and 2026-10-01.
- v0.0.1 is published to crates.io and as a GitHub release only. Before it, version 0.0.0 is published once from a local `cargo publish` with a short-lived token, only to reserve the crate name: crates.io accepts a trusted publisher only for a crate that already exists. 0.0.0 has no tag and no GitHub release. Until v0.0.1, the README states that there is no release and shows a build from source.

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
| `-i [identity_file]` | Select the specified public key file, adding `.pub` if absent. The private key file, the same path without `.pub`, must exist unless `-f` came before that `-i`, as upstream requires. A leading `~/` or `~\` is expanded with the home directory, because the Windows shells do not expand it. A second `-i` is an error, as upstream. As upstream, when only one argument follows `-i`, that argument is the destination and `-i` has no file, and a readable file containing `ssh` there is reported as a missing hostname with upstream's suggestion of `-i --`. `-i` without a file selects the default key file of the next row, without asking the agent; otherwise `-i` takes the next argument unless it looks like one of the options `-[iopFtfnsxh?-]` |
| No `-i` | Prefer every public key `ssh-add -L` lists, also when `SSH_AUTH_SOCK` is unset (D-20); otherwise select the most recently modified `~/.ssh/id*.pub`, excluding `*-cert.pub`, as upstream. `ls -d` order is followed: a directory can be the newest entry, equal times are ordered by name in byte order, and an unreadable default counts as none. The login hint after installation names `-i` only when `-i` was given |
| `-p port` | Set the destination port |
| `-o option`, `-F config` | Specify SSH options or a configuration file. Allow repeated `-o` arguments |
| `-f` | Skip the installed-key check, the post-installation verification, and the untested-client warning wherever `-f` appears; duplicates may result. As upstream, the private key is not required when `-f` comes before the `-i` that selects the file, or when there is no `-i`; `-f` after `-i` still requires it, and the login hint then names it. After `-f -i`, the login hint shows `-i` without a value, as upstream prints it |
| `-n` | Display the keys that would be installed without performing installation operations. Connect for installed-key checks unless skipped with `-f` |
| `-s` | Install keys using SFTP |
| `-t target_path` | Specify the destination file. The path reaches the remote script as data, not inside the command (D-11); a path containing LF is rejected before connecting. The OpenWrt and Haiku targets apply only without `-t` (D-22) |
| `-x` | Print each client command and the remote script before running them (D-14) |
| `-h`, `-?` | Display help |
| `--target-os unix\|windows` | Override destination detection (D-12); not in upstream |

Normally, determine whether a key is already installed by checking whether it can
authenticate. Do not substitute a text comparison against `authorized_keys`.
Avoid false positives caused by authentication with another key or reuse of an
existing multiplexed connection. Do not interpret connection failures or host key
errors as evidence that a key is not installed.

Each key is checked alone, as upstream does. A key read from a file whose
private key exists is checked with that private key. A key from the agent is
written as a one-line public key file in the scratch directory and passed with
`-i`; `ssh` then authenticates with the agent's matching private key, and the
other rules of the check are unchanged. The files are `agent-key-<n>.pub`, and
the probe logs `check-<n>.log` and `verify-<n>.log`, all in the scratch
directory; with several keys, each warning and verification line starts with
`key <n> from ssh-add -L:`.

### Recorded Difference: Agent Without `SSH_AUTH_SOCK`

Without `-i`, upstream asks the agent for keys only when `SSH_AUTH_SOCK` is set.
This tool runs `ssh-add -L` whatever the variable holds and uses the keys it
lists; when it fails or lists none, the default key file is used, as upstream.
Apparent intent: use the keys of an agent that is available, and
`SSH_AUTH_SOCK` is how a Unix agent announces itself. Reason to differ: the
Windows OpenSSH agent listens on a fixed named pipe and leaves `SSH_AUTH_SOCK`
unset, so upstream's condition would never use it on a supported platform. On
Unix, `ssh-add -L` fails without the variable and the result is upstream's. This
is difference D-20.

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
| Installed | The selected key was the only candidate the client could offer, and `ssh`'s log records authentication with the `publickey` method, whatever the exit status | Skip the key |
| Not installed | The server answered `Permission denied (…)`, so no candidate authenticated | Install the key |
| Inconclusive | Another identity or certificate was a candidate, authentication used another method such as `none`, or the client reported an error that matches no known pattern | Install the key and print the reason; duplicates are possible, as with `-f` |

The probe runs with `LogLevel=VERBOSE` and `-E` naming a file in a fresh
directory under the local `~/.ssh`, created with mode 700 like upstream's scratch
directory and removed when the run ends; when it cannot be created, the run stops
before connecting with upstream's message. `ssh` then writes `Authenticated to …
using "publickey"` to that file (`sshconnect2.c`); every tested client does.
Banners and remote output stay on stderr, and text on stderr is never evidence
of an installed key, while the `Permission denied` and connection patterns are
matched on both. The log is not free of server text either: `ssh` logs a
received disconnect message with its text, newlines kept (`packet.c`,
`log.c`), so a peer can plant a line there before the host key is checked. A
disconnect always ends `ssh` with status 255, so a probe that exits 255 is never
Installed, and only the first `Authenticated to` line in the log counts. A log
that holds a `Received disconnect from` line is no evidence at all. When the
first authentication line is `Authenticated using "publickey" with partial
success.`, the server accepted the selected key and asks for a further method,
as `AuthenticationMethods` can require; the key is then Installed if it was the
only candidate and Inconclusive otherwise, although the probe ends with
`Permission denied`, which upstream reads as not installed.
A log that cannot be read makes the check Inconclusive. The line decides even
when the probe exits with another nonzero status, as it does for a key
restricted by `command=`;
upstream stops there with an error on every run after the first installation. A
probe that exits 0 without any `Authenticated to` line has not authenticated, and
the run stops as a failure. Without the `publickey` line a successful probe is
not evidence: a server that accepts the `none` method, such as an account with
an empty password and `PermitEmptyPasswords yes`, lets the probe exit 0 with no
key at all, and upstream then skips the key without installing it.

Whether other candidates exist is read from `ssh -G`: every `identityfile` other
than the selected key whose file or `.pub` file exists (OpenSSH loads the public
half and pairs a matching agent key with it even under `IdentitiesOnly=yes`),
every `certificatefile` whose file
exists, the selected key's own `-cert.pub` or `-cert` file, and a `pkcs11provider`
other than `none`. An `identityfile` that names the same file as the selected key
by another path, such as `~/.ssh/id_ed25519` against an absolute `-i`, is the
selected key and not a candidate. A value containing `%` tokens or `${…}`, which
`ssh -G` prints unexpanded, is counted as a candidate. Generating a configuration that isolates the
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

The `-t` path reaches the Unix script as the first line of its standard input,
before the keys, and the Windows script as a PowerShell single-quoted literal, so
quotes, `!`, and CR in the path are data whatever the login shell; csh and tcsh
would expand `!` even inside single quotes. The second line is the path in the
`%XX` form of the result lines, for `path=`. A path containing LF cannot be one
line and is rejected before connecting. Every command in the script takes the
path after `--` or as `of=`, so a path starting with `-` is a file name. A
repeated `-t` keeps the last value, as upstream's option loop does. Upstream embeds the path inside a single-quoted `sh -c` argument,
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
report a failed write as success. The login hint after installation prints the
`-i` and `-p` values unquoted and the other options quoted, as upstream does. A
missing destination prints only the usage; an unknown option prints
`ssh-copy-id: illegal option -- <letter>` before it, the form bash and macOS's
`sh` give upstream's `getopts`. dash words that line differently, so the golden
tests treat it as dependent on the shell. The same holds for the reason after a
key file that cannot be opened: upstream takes it from the shell's own error,
dash's "No such file" where bash and this tool print the system's "No such file
or directory".

`-x` prints each client command line and the remote script to stderr before
running them, the closest equivalent of upstream's `set -x`. Apparent intent of
upstream's `-x`: trace the shell script itself. Reason to differ: a compiled
program has no shell trace, so the platform cannot perform the upstream behavior.
This is difference D-14; the output format differs, the purpose does not.

The upstream special cases stay. For the default target, OpenWrt as root
installs into `/etc/dropbear/authorized_keys` and Haiku uses
`config/settings/ssh/authorized_keys`. NetScreen keys are installed one per
command as upstream does; that case belongs to stage 1.5.

### Recorded Difference: Explicit Target on OpenWrt and Haiku

With `-t`, the given path is written on every destination. Upstream sets its
OpenWrt-root and Haiku targets after reading `-t` and writes there instead. Its
history adds `-t` two weeks after the OpenWrt case without touching it, and no
record says the override is meant; that intent is inferred. Reason to differ: a
change the user did not ask for, since the file the user named stays as it was
and another file gains the key. This is difference D-22.

The Unix installation command is one line, as upstream's is, so that csh and
tcsh login shells can run `exec sh -c '…'`: they reject a newline inside single
quotes and expand `!` even there.

### Recorded Difference: Targets That Cannot Be Appended Safely

A target that exists but is not a regular file, such as a FIFO or a directory,
a non-empty target that cannot be read, and any target when the home directory
cannot be entered are not written; every key is reported failed. Upstream's `cd`
failing leaves it in the server's working directory, where the relative target
is not the account's file. Upstream appends to an unreadable file without knowing whether
its last line ends with a newline, which can join the new key to that line, and
opening a FIFO waits for a reader indefinitely. Apparent intent: upstream expects
the target to be the account's own readable file. Reason to differ: damage to
existing data, and a run that never ends without reporting. This is difference
D-16.

### Recorded Difference: Removing a Partial Write

When appending a line fails, for example on a full file system, the script
truncates the target back to its size before that key's group: the key line and
the comment and blank lines written since the previous key, including a newline
it added before the first line. A file the group created is removed, and later
lines are not written. Before truncating, the script checks that the bytes after
the group's starting size are a prefix of the text it tried to write; anything
else means another writer appended meanwhile, and nothing is truncated. Upstream leaves
the fragment, so the file no longer ends with a newline and the next append joins
onto it, while the run reports that the key was not written. Apparent intent:
upstream reports a failed append as a failed run and expects nothing to be left.
Reason to differ: damage to existing data and a false report that nothing was
written. When the size afterwards cannot be confirmed, or another writer
prevented the truncation, the key is reported `uncertain`, the summary is
`uncertain`, and the user is told to check the file. This is difference D-17.

### Dry-Run Behavior

`-n` performs installed-key checks and displays the keys that would be installed.
It does not create the destination directory or key file, append keys, or change
their permissions. Connections for the check still occur unless `-f` skips it;
`-n -f` makes no authenticated connection, as in upstream, where `-f` bypasses
the probe loop. Upstream still opens one connection without authentication to
read the server's version for NetScreen detection; this tool makes that
connection from stage 1.5, when NetScreen destinations are handled.
Authentication logs, login hooks, host key handling, and user-configured
`AddKeysToAgent` behavior may still occur as part of those connections.

The usage describes `-n` with upstream's line, `dry run -- no keys are actually
copied`, which does not present it as offline or free of side effects. The keys
are listed on stdout in upstream's `Would have added the following key(s):`
block, with the lines that would be appended, comment and blank lines included,
and the run exits 0. `-n -f` prints no `BatchMode` notice, since no `ssh` runs.
This adds no behavioral difference; the SFTP check difference D-02 still
applies.

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
`ssh-copy-id: key=<n> result=<added|skipped|failed|uncertain> path=<file>`, then one
summary line as the final output,
`ssh-copy-id: result=<unchanged|installed|partial|uncertain> added=<n>`. `<n>` is
the key's position among the key lines of the input, starting at 1, without
leading zeros. `uncertain` marks a failed write whose removal could not be
confirmed (D-17). In `<file>`, every
byte outside `0x21` to `0x7E`, and `%` and `=`, is written as `%XX` in uppercase
hexadecimal; encoding bytes from `0x80` keeps a console code page from altering a
non-ASCII path. Lines without the `ssh-copy-id: ` prefix are ignored. A prefixed
line that does not parse, including one that is not UTF-8, makes the outcome
Unknown, as does any prefixed line after the summary. Per-key lines show which
keys were written before a partial write. These lines are not shown to the user;
the user sees upstream's messages.

### Recorded Difference: Exit Before Any Result Line

When `ssh` ends the installation with status 255 before the script printed any
result line, the CLI prints, after `ssh`'s own messages, that the installation
script reported nothing and that nothing was written if authentication failed.
Upstream exits 1 with only `ssh`'s messages. Apparent intent: any `ssh` failure
is the run's failure. Reason to differ: a false failure report, since the same
status also follows a connection that drops after the script wrote the key, and
then the file may have changed. This is difference D-19.

### Recorded Difference: Post-Installation Verification

After the keys are written, in both normal and `-s` modes, repeat the
installed-key check for each written key unless `-f` was given. Report each key
as installed and verified, or as installed but not verified with the reason. A
rejected verification does not change the exit status: the file was written, and
the message names what to check next, such as the shared administrator file or
the file's ACL; with `-t` it also asks whether the server's `AuthorizedKeysFile`
setting reads the target, since sshd reads no other file. The verification uses the three results of the selected-identity
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
Unix host, where CRLF does not occur. Reason to differ: not settled. The reason
first recorded, that a line ending in CR is not a usable key, does not hold for
OpenSSH 9.6p1, whose sshd accepts such a line (validation row K01). Whether
Dropbear, Windows OpenSSH and older OpenSSH releases accept it decides whether
the normalization stays; until then it stays, and the
[open questions](open-questions.md) track it.

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

#### Recorded Difference: One Key per Selected File

A selected public key file, given with `-i` or chosen as the default, holds one
key line, the public half of the private key that the same path without `.pub`
names. A file with more key lines
is rejected before anything is sent. Upstream accepts it and checks every line
with that one private key, because its probe passes only the private key and
never the line; when that key is not installed yet, every line is appended, and
keys the user did not select get login access to the account. Apparent intent:
one key per `-i` file, as the comment above upstream's probe assumes. Reason to
differ: a change the user did not ask for. Several keys are installed only when
they come from the agent, and each of them is checked alone. This is difference
D-21.

Certificate lines, whose type ends in `-cert-v01@openssh.com`, and
`cert-authority` lines are passed through as upstream does. sshd does not accept
a certificate line in an authorized-keys file for login, so for such a selected
key the installed-key check and the post-installation verification make no
connection and report Inconclusive with that reason. This is not a difference.

#### Leading BOM Removal

Remove a UTF-8 byte order mark at the start of the installation input before
transmission. Only the first three bytes of the input are affected; existing
remote contents and the local source file are not modified. Apparent intent:
upstream expects input written by `ssh-keygen` or `ssh-add`, which never emit a
byte order mark. Reason to differ: a false success report, because a line that
begins with one is not recognized as a key. Windows editors and PowerShell 5
redirection produce such files. This is difference D-07, not a bug fix. Test a
prefixed `.pub` file through both transports and forced mode.

#### Recorded Difference: Counting Keys

Each line of the key file loses its leading and trailing spaces and tabs, and
blank lines at the end are dropped, as upstream's line reading does. Under `-f`
upstream reads the keys with `$(cat …)` or `$(ssh-add -L)` instead, so the
lines are sent as given and only empty lines at the end are dropped; this tool
does the same. `#` comment
lines and the remaining blank lines are appended as upstream appends them. A
comment line is one whose first character other than a space or
tab is `#`; a blank line holds only spaces and tabs. The local check and the
remote script use the same rule, and a run whose reported key count differs
from the keys sent ends with an unknown outcome. Upstream also counts them in "Number of key(s) added" and in the
keys that remain to be installed. Here only key lines are numbered, reported,
and counted. Apparent intent: upstream expects one key per line. Reason to
differ: a false report of how many keys were installed. This is difference D-18.

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
overrides and nothing more. Every `ssh` connection passes `-a -x` ahead of the
user's options, as upstream's `ssh -a -x` does, so a configured `ForwardAgent`
or `ForwardX11` does not reach a host that is only being set up.

| Phase | Options this tool sets | User options |
| --- | --- | --- |
| Installed-key check | `-a`, `-x`, `-E` with a log file in the scratch directory, `ControlPath=none`, `LogLevel=VERBOSE`, `PreferredAuthentications=publickey`, `IdentitiesOnly=yes`, and `-i` with the selected key | `-o`, `-F`, `-p`, and the user's configuration are passed through after the overrides |
| Installation | `-a`, `-x`, `RequestTTY=no` (D-15) | passed through after the override |
| `-s` transfer | a control master shared by the `sftp` sessions, as upstream | passed through unchanged |

Upstream sets `LogLevel=INFO` for the probe; `VERBOSE` adds the authentication
method to the captured output, which the user does not see, and is not a
difference.

### Recorded Difference: No Terminal for the Installation

The installation connection sets `RequestTTY=no` ahead of the user's options, so
a configured `RequestTTY force` or `yes` does not allocate a terminal for it.
Upstream passes the user's setting through; with a forced terminal, end of input
never reaches the remote script through the pseudo terminal, and the run waits
indefinitely after writing the key. Apparent intent: upstream relies on the
default, which allocates no terminal for a command with redirected input. Reason
to differ: a run that never ends without reporting an outcome. This is
difference D-15.

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

Public keys travel on the stdin pipe of `ssh`, and both tested Windows clients
read password and passphrase prompts from the console, not from stdin, so the
two coexist (validation row A01). Keep authentication interaction separate from
the public key data stream; do not solve prompt handling by collecting
credentials in this CLI or globally enabling `BatchMode=yes`.

The remote script reads the keys from the same stdin after the login shell has
run its startup files, as upstream's `cat` does ("the cat adds the keys we're
getting via STDIN"). A startup file that reads stdin, such as a `~/.bashrc` that
Debian's bash sources for `sshd` commands, consumes part of the keys: the rest
may be appended as a fragment, and the post-installation verification then
reports the key as rejected. This tool behaves as upstream here. Upstream has
fixed other startup-file interference as bugs (`cd` in `~/.bashrc`, Debian
#404134, and tcsh and fish syntax, behind its one-line `exec sh -c` command), but
no upstream record found treats stdin consumption as intended or as a bug, so
that intent is inferred; the [open questions](open-questions.md) say what would
settle it.

The prompt mode follows the console, as Sudo for Windows offers inline and
input-closed modes:

| Situation | Behavior |
| --- | --- |
| The CLI has a console | Prompts appear inline on that console, as upstream's do on a terminal |
| No console (`CONIN$` cannot be opened) and `SSH_ASKPASS` is set | `ssh` uses the user's askpass program |
| No console and no `SSH_ASKPASS` | Pass `BatchMode=yes` ahead of the user's options, so a configured `BatchMode no` cannot reopen the wait: key and agent authentication still work, and a password or passphrase request fails at once with a message naming the reason |

Without the last rule, Windows OpenSSH waits indefinitely at a password prompt
when there is no console, while upstream on Unix fails at once when it cannot
open a terminal. The rule keeps that upstream behavior on Windows and is not a
difference.

Once the arguments are parsed, the CLI does not end on a console Ctrl-C: while
`ssh` runs, it waits for `ssh` to exit and reports the outcome from what `ssh`
returned. The interrupt then ends the run with status 1, as it ends upstream's
script: before the installation nothing is written, and after it the outcome
is reported and the verification is skipped. The exit status of an interrupted
`ssh` is not evidence; Windows OpenSSH can exit 0. As upstream, the CLI sets no
timeout of its own: a server that accepts the connection and never answers, or
a key whose `command=` never ends, keeps `ssh` waiting until Ctrl-C or until a
`ConnectTimeout` the user passes with `-o` or sets in the configuration. With Git for Windows'
client, Ctrl-C reaches the CLI as well as `ssh`; ending first would leave the
outcome unreported. The Windows OpenSSH agent refuses keys added with
confirmation (`ssh-add -c` answers `agent refused operation`), so no
confirmation request reaches `ssh` from it.

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
connection failure. The tested clients are `OpenSSH_for_Windows_9.5p2`,
`OpenSSH_10.0p2` from Git for Windows, and OpenSSH `9.6p1` as packaged in Ubuntu
24.04 (`OpenSSH_9.6p1 Ubuntu-3ubuntu13`), which the fixtures exercise. Each
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
[Linux Docker fixture](../tests/environments/linux/) and the
[Windows dockur fixture](../tests/environments/windows/README.md). Compatibility
is a golden test: the pinned upstream script and this CLI run against the same
Linux fixture, and stdout and exit status are compared where behavior is shared.

Continuous integration (`.github/workflows/ci.yml`) runs `cargo fmt --check` and
`cargo clippy -D warnings` on Linux, `cargo test` on Linux, Windows, and macOS,
and the Linux fixture, on GitHub-hosted runners for every push to `main` and
every pull request. The Windows fixture needs KVM and a multi-gigabyte guest disk
and runs by hand. It needs a self-hosted runner to run in CI, and that runner is
not registered to this public repository: a pull request from a fork can add a
workflow that targets any runner the repository has, so the runner is attached
to a separate private repository, or created for one dispatched run and removed
afterwards.

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
