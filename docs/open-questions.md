# Open Questions

What is not decided yet. Each item names what will decide it; when it is
decided, the item is removed here and the result is stated in the
[design](design.md).

| Question | Decided by |
| --- | --- |
| `cargo install` on Linux installs a binary named `ssh-copy-id`, which can shadow OpenSSH's command in `PATH`, while Linux packages use `ssh-copy-id-rs`. Which name should the crate's binary use on Linux? | Before the first crates.io release, v0.0.1 |
| Package names for APT, pacman, and similar systems | When Linux packaging starts (stage 3) |
| Install channels beyond crates.io and GitHub releases, such as winget, which runex and gitreant use | After v0.0.1 |
| Whether destinations with only `pwsh`, without Windows PowerShell, are supported | Validation row W06 |
| How `-s` tells authentication rejection apart from other failures when the SFTP session cannot be established (D-02) | Stage 3, with `-s` |
| How much concurrency `-s` supports beyond the change check before upload (D-08) | Stage 3, with `-s` |
| Whether to generate a configuration that isolates the selected key, turning Inconclusive checks into conclusive ones | After stage 2, if Inconclusive results prove common |
| Where the self-hosted runner for the Windows fixture runs, and whether a prepared guest disk may be distributed to hosted runners | Before stage 2; the second depends on the evaluation image's redistribution terms |
