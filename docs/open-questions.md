# Open Questions

What is not decided yet. When an item is decided, it is removed here and the
result is stated in the [design](design.md).

| Question | Related to |
| --- | --- |
| `cargo install` on Linux installs a binary named `ssh-copy-id`, which can shadow OpenSSH's command in `PATH`, while Linux packages use `ssh-copy-id-rs`. Which name should the crate's binary use on Linux? | Crate publishing and Linux distribution |
| Whether crates.io lets a trusted publisher be registered before the crate exists, or the first publish of v0.0.1 needs a short-lived API token | v0.0.1 publishing |
| Whether the repository is public by v0.0.1; while it is private, the crate's repository link and the GitHub release are not visible to its users, and build provenance is not attested | v0.0.1 publishing |
| When to ship an `aarch64-pc-windows-msvc` build; GitHub's Windows Arm runners are not used while the repository is private, and no fixture runs on Arm | Release targets |
| Package names for APT, pacman, and similar systems | Linux packages (stage 3) |
| Install channels beyond crates.io and GitHub releases, such as winget, which runex and gitreant use | Releases after v0.0.1 |
| Whether destinations with only `pwsh`, without Windows PowerShell, are supported | Validation row W06 |
| Whether an inherited profile ACL alone passes sshd's check, making the explicit ACL unnecessary under profile directories | Validation row W07, first check |
| The exact destination-detection probe command and its expected outputs | Validation row W09 |
| How `-s` tells authentication rejection apart from other failures when the SFTP session cannot be established (D-02) | `-s` (stage 3) |
| Whether the login hint after installation quotes the `-i` and `-p` values, which upstream prints unquoted so that a path with a space cannot be pasted as is, and how blank lines around messages and the usage-error wording follow upstream | Message parity, settled by the golden tests (stage 1.5) |
| How a `-t` path containing `!`, CR, or LF reaches a csh or tcsh login shell, since the one-line installation command cannot quote them there | `-t` on Unix-like destinations (stage 1.5) |
| How much concurrency `-s` supports beyond the change check before upload (D-08) | `-s` (stage 3) |
| How to report a private key that Windows OpenSSH ignores because its ACL is too open, which makes an installed key look not installed; upstream behaves the same | Installed-key check (D-01) |
| Whether upstream treats a remote startup file that reads stdin, and so truncates the keys, as a bug or as an accepted limit; upstream's repository, Debian and Red Hat trackers, and the OpenSSH manuals have no record, while mindrot Bugzilla (behind a login) and the openssh-unix-dev archive were not searched | Key transport on stdin; this tool behaves as upstream until settled |
| Whether `od -v -An -tx1`, which the rollback's content check uses, exists on BusyBox destinations such as OpenWrt; without it the check cannot compare and a failed write may be reported uncertain instead of rolled back | Remote script on minimal destinations (stage 1.5, with a BusyBox fixture) |
| When to add the planned optimization that isolates the selected key, turning Inconclusive checks into conclusive ones | Selected-identity check (D-01) |
| Where the self-hosted runner for the Windows fixture runs, and whether a prepared guest disk may be distributed to hosted runners | Windows fixture in CI; the evaluation image's redistribution terms |
