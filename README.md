# ssh-copy-id for windows

This is `ssh-copy-id` for Windows.

> [!WARNING]
> This project is experimental until it reaches `v1.0.0`,
> and is subject to breaking changes in any release before then.
> Version 0.0.1 installs only one explicitly selected key on a Unix-like host.
> Do not rely on it for access to machines you cannot reach another way.

## Summary

This project focuses on installing existing SSH public keys on remote accounts,
including key selection, authentication checks, and the required file permissions.
It targets the role of `ssh-copy-id`, not the full OpenSSH suite. Key generation,
key lifecycle management, and SSH server or service administration are out of scope.
Windows is the primary client platform; Linux distribution is planned.

Design and implementation plan: [English](docs/design.md) | [日本語](docs/design.jp.md).

Documentation structure and update policy: [docs/README.md](docs/README.md).

Differences from upstream: [English](docs/compatibility.md) | [日本語](docs/compatibility.jp.md).

## Install

With Rust installed:

```powershell
cargo install ssh-copy-id
```

Or download `ssh-copy-id-x86_64-pc-windows-msvc.zip` from the
[latest release](https://github.com/ShortArrow/ssh-copy-id/releases/latest) and
put `ssh-copy-id.exe` on `PATH`. The system OpenSSH client (`ssh.exe`) is
required.

## Usage

Version 0.0.1 installs one explicitly selected key on a Unix-like host:

```powershell
ssh-copy-id -i ~/.ssh/id_ed25519.pub user@host
```

The remaining features follow the [delivery plan](docs/design.md#delivery-plan).

## License

MIT OR Apache-2.0; see [LICENSE](LICENSE).
