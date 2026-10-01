# Open Questions

What is not decided yet. When an item is decided, it is removed here and the
result is stated in the [design](design.md).

| Question | Related to |
| --- | --- |
| `cargo install` on Linux installs a binary named `ssh-copy-id`, which can shadow OpenSSH's command in `PATH`, while Linux packages use `ssh-copy-id-rs`. Which name should the crate's binary use on Linux? | Crate publishing and Linux distribution |
| Package names for APT, pacman, and similar systems | Linux packages (stage 3) |
| Install channels beyond crates.io and GitHub releases, such as winget, which runex and gitreant use | Releases after v0.0.1 |
| Whether destinations with only `pwsh`, without Windows PowerShell, are supported | Validation row W06 |
| Whether an inherited profile ACL alone passes sshd's check, making the explicit ACL unnecessary under profile directories | Validation row W07, first check |
| The exact destination-detection probe command and its expected outputs | Validation row W09 |
| How `-s` tells authentication rejection apart from other failures when the SFTP session cannot be established (D-02) | `-s` (stage 3) |
| How a `-t` path containing `!`, CR, or LF reaches a csh or tcsh login shell, since the one-line installation command cannot quote them there | `-t` on Unix-like destinations (stage 1.5) |
| How much concurrency `-s` supports beyond the change check before upload (D-08) | `-s` (stage 3) |
| When to add the planned optimization that isolates the selected key, turning Inconclusive checks into conclusive ones | Selected-identity check (D-01) |
| Where the self-hosted runner for the Windows fixture runs, and whether a prepared guest disk may be distributed to hosted runners | Windows fixture in CI; the evaluation image's redistribution terms |
