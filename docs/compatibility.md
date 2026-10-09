# Differences from Upstream ssh-copy-id

[日本語](compatibility.jp.md) | [Design](design.md)

This is an index of accepted design differences, not a claim
that they have been implemented or tested. Differences are not classified as
upstream defects or bug fixes, and no claim is made about the original author's intent.

The comparison baseline is the [pinned upstream reference](design.md#pinned-upstream-reference).
Its revision and update policy are maintained in the design document.

## Accepted Behavioral Differences

This list contains concise comparisons and links only. The design document
states each difference and its reason; do not duplicate them here. Open
questions are in [open-questions.md](open-questions.md) and verification status
is in the [validation matrix](validation.md#matrix).

| ID | Reviewed upstream behavior | This project's decision | Details |
| --- | --- | --- | --- |
| D-01 | A successful probe skips the selected key; another configured identity may have authenticated. | Authentication with another key does not establish that the selected key is installed. When other candidates make the check inconclusive, install with a warning; duplicates are possible. | [Selected identity](design.md#recorded-difference-identity-used-for-the-installed-key-check) |
| D-02 | In `-s` mode, attempts `exit` and accepts a specific SFTP-only error message as success. | Check by establishing an SFTP session, without remote commands or that message shortcut. | [SFTP check](design.md#recorded-difference-sftp-installed-key-check) |
| D-03 | SFTP mode sets the target's parent directory to 700; normal mode does not unconditionally chmod it. | Preserve the existing parent directory's permissions for `-t` in both modes; report insufficient access. | [Custom parent permissions](design.md#recorded-difference-existing-parent-directory-of-a-custom-target) |
| D-04 | No explicit private-key-content rejection in installation input; `-f` may transmit it. | Reject private key input before transmission, including with `-f`. | [Private key input](design.md#private-key-input-rejection) |
| D-05 | No explicit CR removal from CRLF installation input. | Normalize incoming CRLF line endings to LF for transmission; preserve existing remote contents. | [CRLF normalization](design.md#crlf-normalization) |
| D-06 | After writing, prints a suggested login command and does not verify. | Repeat the installed-key check for each written key unless `-f`; report installed-but-unverified keys with the reason. | [Post-installation verification](design.md#recorded-difference-post-installation-verification) |
| D-07 | Writes a leading UTF-8 byte order mark as part of the first key line. | Remove a leading byte order mark from the installation input before transmission. | [Leading BOM removal](design.md#leading-bom-removal) |
| D-08 | `-s` uploads the edited file without checking for changes since the download. | Repeat `ls -l` before `put` and stop without writing when the size or modification time changed. | [Change check before upload](design.md#recorded-difference-change-check-before-upload) |
| D-09 | `-s` ignores every `get` error and continues; a failed read can lead to uploading a file with only the new keys. | Check the target with `ls -l` first; stop before writing on any error except a missing file. | [SFTP mode](design.md#sftp-mode) |
| D-10 | Appends malformed lines, a standalone CR, or a NUL byte as given. | Reject such input before transmission, with the line number. | [CRLF normalization](design.md#crlf-normalization) |
| D-11 | A quote in the `-t` path breaks the remote `sh -c` script, and `!` breaks it under csh and tcsh. | Pass the path as data: on stdin on Unix destinations, as a quoted literal on Windows destinations; reject a path containing LF. | [Custom parent](design.md#recorded-difference-existing-parent-directory-of-a-custom-target) |
| D-12 | Sends the `sh` script without checking the destination; has no OS option. | In normal mode, detect the shell family with one more connection before writing; `--target-os` overrides. | [Remote operating systems](design.md#remote-operating-systems-and-shells) |
| D-13 | Runs with any OpenSSH client. | Warns on client versions whose stderr patterns were not tested; output it cannot classify makes the check inconclusive. | [Connection backend](design.md#connection-backend-evaluation) |
| D-14 | `-x` enables the shell's `set -x` trace. | `-x` prints each client command and the remote script before running them. | [Exit statuses and messages](design.md#exit-statuses-messages-and-special-destinations) |
| D-15 | Passes the user's `RequestTTY` to the installation connection; with a forced terminal the run never ends. | Sets `RequestTTY=no` for the installation connection ahead of the user's options. | [No terminal for the installation](design.md#recorded-difference-no-terminal-for-the-installation) |
| D-16 | Appends to an unreadable target without checking its final newline; a FIFO target waits indefinitely; a failed `cd` leaves the target relative to the server's working directory. | Does not write a non-empty target it cannot read, a target that is not a regular file, or anything when `cd` fails; reports the keys failed. | [Unsafe targets](design.md#recorded-difference-targets-that-cannot-be-appended-safely) |
| D-17 | A failed append can leave a fragment of the key line, and the run reports that the key was not written. | Truncates the target back to its size before the failed key's group of lines, unless another writer appended meanwhile; reports `uncertain` when that is not confirmed. | [Partial write](design.md#recorded-difference-removing-a-partial-write) |
| D-18 | Counts `#` comment lines and blank lines of the key file as keys added. | Appends those lines too, but numbers, reports, and counts key lines only. | [Counting keys](design.md#recorded-difference-counting-keys) |
| D-19 | Exits 1 with only `ssh`'s messages when `ssh` fails before the script reports anything, also after a connection drop that follows the write. | Adds a line saying the script reported nothing and nothing was written if authentication failed. | [Exit before any result line](design.md#recorded-difference-exit-before-any-result-line) |
| D-20 | Without `-i`, asks the agent only when `SSH_AUTH_SOCK` is set. | Runs `ssh-add -L` whatever `SSH_AUTH_SOCK` holds, so the Windows agent's keys are used. | [Agent without `SSH_AUTH_SOCK`](design.md#recorded-difference-agent-without-ssh_auth_sock) |
| D-21 | Accepts a `-i` file with several key lines, checks them all with its one private key, and appends them all when that key is not installed. | Rejects a `-i` file with more than one key line before anything is sent. | [One key per selected file](design.md#recorded-difference-one-key-per-selected-file) |
| D-22 | Writes OpenWrt root's and Haiku's own targets even when `-t` names another path. | Writes the `-t` path on every destination; the special targets apply only without `-t`. | [Explicit target](design.md#recorded-difference-explicit-target-on-openwrt-and-haiku) |

## Destination Differences

Unix-like and Windows destinations behave the same wherever the platform allows
([design stance](design.md#design-stance)). This table lists where they cannot,
with links to the decisions.

| ID | Unix-like destination | Windows destination | Details |
| --- | --- | --- | --- |
| O-01 | Every account's keys go to `.ssh/authorized_keys`. | Administrators' keys go to the shared `administrators_authorized_keys` under the default server configuration. | [Administrator scope](design.md#windows-administrator-scope) |
| O-02 | New directories and files get modes 700 and 600 through `umask`. | New directories and files get the minimal ACL for the account, SYSTEM, and Administrators through `icacls.exe`. Existing objects are untouched on both. | [Permissions](design.md#permissions-and-service-boundaries) |
| O-03 | The account's POSIX shell runs an `sh` script. | The default shell (`cmd.exe`, Windows PowerShell, or `pwsh`) wraps the command; only an `EncodedCommand` line is sent. | [Windows remote command](design.md#windows-remote-command) |
| O-04 | The remote exit status reaches the client. | A PowerShell default shell reports a child's nonzero status as 1. Both destinations therefore report the outcome on a result line. | [Installation outcome](design.md#installation-outcome) |
| O-05 | `-s` sets file mode 600; the parent is unchanged (D-03). | `-s` cannot set ACLs; a directory with a correct ACL is a prerequisite. | [SFTP mode](design.md#sftp-mode) |
| O-06 | Remote runtime requirement: POSIX `sh`. | Remote runtime requirements: `powershell.exe`, and `icacls.exe` when ACLs are set. | [Windows remote command](design.md#windows-remote-command) |

## Distribution Naming

Linux packages will install `ssh-copy-id-rs`, with matching manual pages and shell
completions, alongside the existing `ssh-copy-id`. Windows uses `ssh-copy-id.exe`.
Packages will not replace the OpenSSH command or automatically create an alias.
This is a distribution decision, separate from the behavioral differences above.

[Coexistence policy](design.md#coexistence-with-openssh-packages)

## Maintaining This List

Add accepted differences here and in the Japanese version with a stable ID and
a link to the design section that states them: `D-xx` for differences from
upstream and `O-xx` for destination differences. When a difference changes,
edit its row so it describes the current design; the change history is in git.
Acceptance of a design is not evidence of implementation.
