# Prior Art

Other tools that install public keys from or onto Windows, as read on the date
of each section. This is a record: a section is not rewritten when a tool
changes; a later survey adds a new dated section. Statements come from the
pinned source named in each entry; anything not shown by that source is marked
as inferred. The comparison points are the ones this project's
[design](design.md) had to decide:

- whether a Windows destination is supported;
- how `administrators_authorized_keys` is handled;
- what ACL a written key file gets;
- how a key already installed is recognized;
- how the keys travel to the destination.

## Survey: 2026-10-24

Included: the `ssh-copy-id` Git for Windows ships, the winget package with the
moniker `ssh-copy-id`, and the PowerShell Gallery modules found by searching for
`ssh-copy-id`. GitHub repositories with no release in a package manager were
not read. No npm package named `ssh-copy-id` exists. None of the tools was run;
each entry is from reading its source.

### Upstream `ssh-copy-id` in Git for Windows

Git for Windows 2.50.1 ships `/usr/bin/ssh-copy-id` from the MSYS2 `openssh`
10.0p1-1 package: the upstream shell script (copyright 1999-2024), run by Git
Bash.

- Windows destination: none. The script sends a POSIX `sh` command; nothing in
  it names Windows, `cmd.exe` or PowerShell.
- `administrators_authorized_keys` and ACLs: not handled.
- Installed keys: a login with each key, as this project's check follows with
  its own rules ([D-01](design.md#recorded-difference-identity-used-for-the-installed-key-check)).
- Keys travel on standard input.

This project compares itself with upstream at the
[pinned upstream reference](design.md#pinned-upstream-reference) rather than
with this copy.

### `axeprpr.SSHCopyID` 1.1.1 (winget)

"SSH-Copy-ID for Windows", a Go program, at tag `v1.1.1`, commit `5e536bb` of
<https://github.com/axeprpr/ssh-copy-id-windows> (`main.go`, 235 lines). In
winget it holds the moniker `ssh-copy-id` and installs a portable
`ssh-copy-id.exe`.

- It connects with `golang.org/x/crypto/ssh` instead of the system `ssh`, with
  `ssh.InsecureIgnoreHostKey()`, so the host key is not checked. This project
  leaves host keys to `ssh`.
- Authentication is by password only, read as one line from standard input
  with echo on. Options are `-i` and `-p`; `-o`, `-F` and `~/.ssh/config` are
  not read.
- Windows destination: none. The remote command is
  `umask 077 && mkdir -p ~/.ssh && touch … && cat >> … && sort -u … -o … && chmod …`,
  which needs a POSIX shell.
- `administrators_authorized_keys` and ACLs: not handled.
- Installed keys: not checked by login. After appending, `sort -u` rewrites the
  whole file, which reorders existing lines and drops repeated comment and
  blank lines. This project only appends.
- Keys travel on standard input.
- The default key is the first of `id_rsa.pub`, `id_ed25519.pub`,
  `id_ecdsa.pub` and `C:\ProgramData\ssh\ssh_host_rsa_key.pub` that exists. The
  last is the Windows sshd's host key; the source does not say why it is a
  candidate.

### `SSH-Copy-ID` 1.1.0 (PowerShell Gallery)

A PowerShell module by n8tg, linking <https://github.com/n8tg/ssh-copy-id>,
read from the 1.1.0 package (`SSH-Copy-ID.psm1`, 98 lines).

- It pipes the key file to the system `ssh` running
  `cd; umask 077; mkdir -p ".ssh/" && … && cat >> .ssh/authorized_keys`, a
  POSIX shell command, so a Windows destination is not supported.
- `administrators_authorized_keys` and ACLs: not handled.
- Installed keys: not checked; its help says it appends keys that already
  exist.

### `Copy-SshId` 2.0.0 (PowerShell Gallery)

A PowerShell module by 埃博拉酱, linking
<https://github.com/Ebola-Chan-bot/Copy-SshId>, read from the 2.0.0 package
(`Copy-SshId.psm1` and `Install-SshKey.ps1`). Its help text matches
`SSH-Copy-ID`'s; that it started from that module is inferred.

- Windows destination: supported. One connection with `-v -o BatchMode=yes`
  runs `cmd /c echo __SSH_COPY_ID_WINDOWS__`; the marker or
  `OpenSSH_for_Windows` in the output means Windows, a failed authentication
  means Unix-like, and anything else runs the Unix command and, if it fails,
  the Windows script. This project's probe reads the shell's own expansion
  instead of the server version
  ([D-12](design.md#remote-operating-systems-and-shells)).
- The Windows script is sent base64-encoded on standard input to
  `powershell -NoProfile -Command`, which reads it to the end and runs it with
  `iex`; the keys are a base64 literal inside it.
- Target file: the script reads `%ProgramData%\ssh\sshd_config`. An
  administrator, detected as an enabled `S-1-5-32-544` in `whoami /groups`,
  takes the first `AuthorizedKeysFile` under a `Match` line naming `all` or a
  group containing `administrators`; anyone else takes the first global one.
  `__PROGRAMDATA__`, `%h` and `%u` are expanded. `Include` and other `Match`
  criteria are not read. This project decided against reading `sshd_config`
  ([Windows administrator scope](design.md#windows-administrator-scope)).
- ACLs: when the target is the shared administrator file, the script replaces
  its ACL with full control for SYSTEM and Administrators, inheritance removed,
  owner Administrators, also when the file existed. Other files keep what they
  inherit. This project keeps an existing file's ACL.
- Installed keys: not checked by login. A key is skipped when its type and
  base64 fields match a line already in the file.

### `SshImportId` 0.1.0 (PowerShell Gallery)

A PowerShell module by Phoenix Chen,
<https://github.com/yggdrasill-dev/SshImportId>, read from the 0.1.0 package.
It runs on the destination itself, as `ssh-import-id` does: it fetches the keys
an account publishes on GitHub, GitLab or Launchpad and appends them locally. It
does not connect to another host, so it is not an `ssh-copy-id`.

- `administrators_authorized_keys`: written only with `-ForAdministrators` or
  that path given, and only from an elevated session; `%ProgramData%\ssh` must
  exist.
- ACLs: after keys are added, `icacls /inheritance:r` and full control for the
  account, SYSTEM and Administrators on the key file and its directory; on the
  shared file, SYSTEM and Administrators only. Existing ACLs are replaced.
- Installed keys: a key is skipped when its type and base64 fields match a line
  already in the file. Each added line gets a `# ssh-import-id <source>`
  comment.
