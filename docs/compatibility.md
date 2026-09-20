# Differences from Upstream ssh-copy-id

[日本語](compatibility.jp.md) | [Design](design.md)

Updated: 2026-09-20. This is an index of accepted design differences, not a claim
that they have been implemented or tested. Differences are not classified as
upstream defects or bug fixes, and no claim is made about the original author's intent.

The comparison baseline is the [pinned upstream reference](design.md#pinned-upstream-reference).
Its revision and update policy are maintained in the design document.

## Accepted Behavioral Differences

This list contains concise comparisons and links only. The design document is
the authoritative location for rationale, scope, implementation details, open
questions, and implementation or verification status; do not duplicate those
details here.

| ID | Reviewed upstream behavior | This project's decision | Details |
| --- | --- | --- | --- |
| D-01 | A successful probe skips the selected key; another configured identity may have authenticated. | Authentication with another key does not establish that the selected key is installed. | [Selected identity](design.md#recorded-difference-identity-used-for-the-installed-key-check) |
| D-02 | In `-s` mode, attempts `exit` and accepts a specific SFTP-only error message as success. | Check by establishing an SFTP session, without remote commands or that message shortcut. | [SFTP check](design.md#recorded-difference-sftp-installed-key-check) |
| D-03 | SFTP mode sets the target's parent directory to 700; normal mode does not unconditionally chmod it. | Preserve the existing parent directory's permissions for `-t` in both modes; report insufficient access. | [Custom parent permissions](design.md#recorded-difference-existing-parent-directory-of-a-custom-target) |
| D-04 | No explicit private-key-content rejection in installation input; `-f` may transmit it. | Reject private key input before transmission, including with `-f`. | [Private key input](design.md#private-key-input-rejection) |
| D-05 | No explicit CR removal from CRLF installation input. | Normalize incoming CRLF line endings to LF for transmission; preserve existing remote contents. | [CRLF normalization](design.md#crlf-normalization) |

## Distribution Naming

Linux packages will install `ssh-copy-id-rs`, with matching manual pages and shell
completions, alongside the existing `ssh-copy-id`. Windows uses `ssh-copy-id.exe`.
Packages will not replace the OpenSSH command or automatically create an alias.
This is a distribution decision, separate from the behavioral differences above.

[Coexistence policy](design.md#coexistence-with-openssh-packages)

## Maintaining This List

Add accepted differences here and in the Japanese version with a stable ID and
a link to the detailed design decision. Keep unaccepted proposals in the
[design review](design-review.md). Update detailed implementation and verification
status in the design document; acceptance of a design is not evidence of implementation.
