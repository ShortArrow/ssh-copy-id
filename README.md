# ssh-copy-id for windows

This is `ssh-copy-id` for Windows.

> [!WARNING]
> This project is experimental until it reaches `v1.0.0`,
> and is subject to breaking changes.

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

winget

```powershell
winget install ShortArrow.ssh-copy-id
```

crate.io

```powershell
cargo install ssh-copy-id
```

## LICENSE

MIT, Apache 2.0
