# ssh-copy-id for windows

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/ShortArrow/ssh-copy-id)

This is `ssh-copy-id` for Windows.

> [!WARNING]
> This project is experimental until it reaches `v1.0.0`,
> and is subject to breaking changes in any release before then.
> It installs keys on Unix-like hosts and for Windows users, administrators
> included; `-s` is not supported yet.
> Do not rely on it for access to machines you cannot reach another way.

## Summary

This project focuses on installing existing SSH public keys on remote accounts,
including key selection, authentication checks, and the required file permissions.
It targets the role of `ssh-copy-id`, not the full OpenSSH suite. Key generation,
key lifecycle management, and SSH server or service administration are out of scope.
Windows is the primary client platform; Linux distribution is planned.

[Design and implementation plan](docs/design.md).

Documentation structure and update policy: [docs/README.md](docs/README.md).

[Differences from upstream](docs/compatibility.md).

[Behavior defined where the standards are silent](docs/definitions.md).

## Install

With Rust installed:

```powershell
cargo install ssh-copy-id
```

Or download `ssh-copy-id-x86_64-pc-windows-msvc.zip` from the
[latest release](https://github.com/ShortArrow/ssh-copy-id/releases/latest) and
put `ssh-copy-id.exe` on `PATH`. The system OpenSSH client (`ssh.exe`) is
required.
The executable `cargo install` builds needs the Visual C++ runtime, while the
one in the release archive does not, so copy the latter to a machine without
that runtime.

## Usage

Install the keys of the running `ssh-agent`, or else the newest `~/.ssh/id*.pub`
with its private key, on a Unix-like host or for a Windows user:

```powershell
ssh-copy-id user@host
```

For a member of a Windows host's Administrators group, the keys go to the
shared `administrators_authorized_keys` under `%ProgramData%\ssh`, which sshd reads
for every administrator account of that host under its default configuration;
the command says so before writing. A host whose `sshd_config` has
administrators use their own profile's `authorized_keys` instead does not read
that file, and the run reports the key rejected; there, name the profile's file
with `-t .ssh\authorized_keys`. On a Windows destination a relative `-t` path is
relative to the profile, and a key file the run creates allows only the
account, SYSTEM, and Administrators.

The client's shell can change a `-t` path before the command sees it. Git Bash
drops the backslashes of an unquoted Windows path, so put it in single quotes or
write it with `/`. It also rewrites a path starting with `/`, quoted or not, and
an unquoted `~/`, into a path on the client, such as `C:/Program
Files/Git/etc/ssh/keys` for `/etc/ssh/keys`; set `MSYS_NO_PATHCONV=1` (or
`MSYS2_ARG_CONV_EXCL='*'` under MSYS2) to pass it as written. PowerShell 7.6
expands an unquoted leading `~` to the client's home, so quote it there.

Install one key file, or list what would be installed without writing anything:

```powershell
ssh-copy-id -i ~/.ssh/id_ed25519.pub user@host
ssh-copy-id -n user@host
```

Before writing, one more connection tells a Windows host from any other;
`--target-os unix` or `--target-os windows` names it instead.
`ssh-copy-id -h` lists the options.

The remaining features follow the [delivery plan](docs/design.md#delivery-plan).

## License

MIT OR Apache-2.0; see [LICENSE](LICENSE).
