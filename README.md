# ssh-copy-id for windows

This is `ssh-copy-id` for Windows.

> [!WARNING]
> This project is experimental until it reaches `v1.0.0`,
> and is subject to breaking changes in any release before then.
> It is not released yet, and the command does not install keys yet.
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

Not published yet. The first release, v0.0.1, will be published to crates.io
and winget; see the [delivery plan](docs/design.md#delivery-plan). The command
does not install keys yet. To build the current source, with Rust installed:

```powershell
cargo install --git https://github.com/ShortArrow/ssh-copy-id
```

## LICENSE

MIT, Apache 2.0
