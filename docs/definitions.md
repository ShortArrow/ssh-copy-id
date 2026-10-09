# Behavior Defined Where the Standards Are Silent

[日本語](definitions.jp.md) | [Design](design.md) | [Differences from upstream](compatibility.md)

This is an index of places where the SSH and SFTP standards and the OpenSSH or
Windows OpenSSH documentation leave a behavior undefined, unspecified, or
implementation-specific, and this tool defines it. The design states each
definition; this index only names the gap, its source, and the definition in
short. A behavior that differs from upstream `ssh-copy-id` is indexed in
[compatibility.md](compatibility.md) as well; this index is about the standards,
not about upstream.

Sources are cited as published: RFC 4251 to RFC 4254, `draft-ietf-secsh-filexfer-02`
(SFTP version 3, which OpenSSH implements), and the OpenSSH manuals at the
[pinned upstream reference](design.md#pinned-upstream-reference). Where a manual
is silent and the OpenSSH source decides, the row says so; that behavior can
change between releases.

| ID | What the sources leave open | This tool's definition | Details |
| --- | --- | --- | --- |
| U-01 | The content of an `exec` command's output: RFC 4254 §6.5 and §6.6 carry it as data and define nothing in it, and §6.10 makes the exit status only RECOMMENDED. | The installation script prints prefixed result lines with `%XX`-encoded paths, and the outcome is derived from them alone. | [Installation outcome](design.md#installation-outcome) |
| U-02 | Framing of an `exec` command's standard input: RFC 4254 defines none. | With `-t`, two header lines precede the key lines: the path, then its `%XX` form; a path containing LF is rejected. | [Custom parent](design.md#recorded-difference-existing-parent-directory-of-a-custom-target) |
| U-03 | Whether a line with spaces before `#` is a comment: sshd(8) says only "lines starting with a `#`"; the source skips leading spaces and tabs. | Follows the source: the first character other than a space or tab decides. | [Counting keys](design.md#recorded-difference-counting-keys) |
| U-04 | Line endings in authorized_keys: sshd(8) is silent; OpenSSH 9.6p1 accepts a line ending in CR. | CRLF is normalized to LF before sending (D-05); whether that stays is open. | [CRLF normalization](design.md#crlf-normalization) |
| U-05 | Text encoding of authorized_keys: sshd(8) is silent; OpenSSH 9.6p1 rejects a line starting with a UTF-8 BOM, and Windows OpenSSH strips one at the start of the file. | A leading BOM is removed from the input before sending (D-07). | [Leading BOM removal](design.md#leading-bom-removal) |
| U-06 | Malformed lines in authorized_keys: sshd(8) is silent; the source skips a line it cannot parse. | Input with a malformed line, a standalone CR, or a NUL is rejected before sending (D-10). | [CRLF normalization](design.md#crlf-normalization) |
| U-07 | How many keys a `.pub` file holds: ssh-keygen(1) and ssh-add(1) are silent; the tools write one key per line. | A key file selected with `-i` or as the default holds one key (D-21). | [One key per selected file](design.md#recorded-difference-one-key-per-selected-file) |
| U-08 | Which identity authenticated: no OpenSSH manual documents the client's messages; RFC 4252 §5.1 defines partial success. | The first `Authenticated to` line of the client's own `-E` log is the evidence; exit 255 and a log with a received disconnect are not; partial success with `publickey` counts as accepted. | [Identity used for the check](design.md#recorded-difference-identity-used-for-the-installed-key-check) |
| U-09 | What exit status 255 means: ssh(1) says only "255 if an error occurred", which a remote command can also return. | 255 before any result line is reported as possibly written (D-19); 255 is never evidence of an installed key. | [Exit before any result line](design.md#recorded-difference-exit-before-any-result-line) |
| U-10 | The `ls -l` listing of SFTP: draft-02 §7 leaves the `longname` format unspecified and says clients SHOULD NOT parse it; OpenSSH `sftp` prints times to the minute and the same text for a missing and an unreadable file. | Planned for `-s` (stage 3): the change check and the missing-file test still rely on that listing and are open. | [SFTP mode](design.md#sftp-mode) |
| U-11 | Which ACL Windows OpenSSH requires on authorized-keys files: Microsoft's documentation and the Win32 source differ, and the documentation lists `StrictModes` as unavailable while the source checks under it. | Planned for Windows destinations (stage 2): settled by validation rows W07 and W08. | [Windows permissions](design.md#permissions-and-service-boundaries) |
