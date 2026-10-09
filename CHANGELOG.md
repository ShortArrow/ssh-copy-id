# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Entries name the [differences from upstream](docs/compatibility.md) they add or change.

## [Unreleased]

### Added

- Without `-i`, and with `-i` not followed by a file, the most recently
  modified `~/.ssh/id*.pub` other than a `*-cert.pub` is installed, as
  upstream selects it; its private key must exist. When there is none, the run
  stops with upstream's `No identities found` or `no ID file found`. The login
  hint names the key only when `-i` was given, as upstream's does.
- Without `-i`, the keys `ssh-add -L` lists are installed, also when
  `SSH_AUTH_SOCK` is unset, so the Windows agent is used (D-20); when it lists
  none or fails, the default key file is used. Each agent key is checked alone
  from a one-line public key file in the scratch directory, the keys not yet
  installed are appended in one run, and each written key is verified (D-06).
- A file-less `-i` followed only by a readable file containing `ssh` is
  reported as a missing hostname with upstream's `-i --` suggestion.
- `-f` installs every selected key without the installed-key check or the
  verification, so a key already installed is appended again. As upstream's,
  `-f` before `-i`, or without `-i`, does not need the private key file, while
  `-f` after `-i` still does and the login hint names it. Private key input, a
  leading byte order mark, malformed lines and a key file with several keys are
  still handled as without `-f` (D-04, D-07, D-10, D-21). As upstream's, the
  login hint after `-f -i` names `-i` without the private key.
- `-n` lists the keys that would be installed, in upstream's "Would have added
  the following key(s):" block, and exits 0 without the installation
  connection, writing nothing on the remote side. The installed-key check still
  runs, so keys already installed are skipped as without `-n`; with `-f` no
  connection is made and every selected key is listed. The list holds the lines
  that would be sent, comment and blank lines included (D-18).
- `-t target_path` installs into the given file on a Unix-like host, relative
  to the home directory unless absolute. Missing parent directories are created
  under `umask 077` and existing ones keep their modes, as upstream's. The path
  reaches the remote script as the first line of its input, never inside the
  command, so quotes, `!` and CR in it are data under any login shell, and a
  path containing LF is rejected before anything runs (D-11). The OpenWrt and
  Haiku targets apply only without `-t` (D-22). `-n` and `-f` work with it as
  without it. When the server still rejects the key afterwards, the warning
  asks to check that the server's `AuthorizedKeysFile` setting reads the file,
  as well as its permissions (D-06).
- `-x` prints each `ssh` and `ssh-add` command to stderr before running it, as
  a `+ ` line with the arguments quoted as `set -x` quotes them; standard input
  is not printed (D-14). As upstream's, the installation script starts with
  `set -x`, so the destination's shell traces it to stderr. `-n`, `-f` and
  `-t` work with it as without it.

### Changed

- Key lines are sent with their line endings as given: a CR before the LF, or
  at the end of the input, stays in the line, since every sshd tested accepts
  it. CRLF line endings are no longer normalized, and D-05 is removed. A CR
  elsewhere in a line is still rejected (D-10), and a single CR at the end of a
  line is not part of it when telling a comment or blank line from a key, here
  and in the remote script (D-18).
- On a destination whose BusyBox has no `od`, such as OpenWrt, the installation
  no longer inserts a blank line before the key: whether the target ends with
  a newline is read from `tail -c 1`, as upstream reads it. The check before
  removing a partial write compares the bytes with `hexdump` where `od` is
  absent; when neither exists, the partial write is left in place and reported
  `uncertain` (D-17).
- Under `-f` the key lines are sent as given, without removing spaces and
  tabs around them, as upstream reads them with `$(cat …)`.
- Messages follow the pinned upstream `ssh-copy-id` where the behavior is
  shared: the login hint leaves the `-i` and `-p` values unquoted, a key file
  that cannot be opened is reported with the reason, the skipped-keys warning
  has upstream's blank lines, a host key or connection failure relays `ssh`'s
  own messages, a missing destination prints only the usage, and an unknown
  option prints `illegal option -- <letter>`.
- A missing private key file and the warning that every key was skipped now
  end with upstream's hint to use `-f`.
- When `ssh` ends before the installation script reports anything, the extra
  line saying nothing was written if authentication failed is a recorded
  difference (D-19).
- A selected key file holding more than one key is still rejected before
  anything is sent, now with the rule it breaks: the file must hold the one
  public key of its private key (D-21).
- A key the server accepts with partial success, asking for a further method
  such as a password, counts as installed when no other identity could have
  been offered, in the check and in the verification; with other candidates
  the check is inconclusive and the key is installed (D-01).

## [0.0.1] - 2026-10-04

### Added

- Install one explicitly selected public key (`-i FILE`) on a Unix-like host, with
  `-p`, `-o`, and `-F` passed to `ssh`. A leading `~/` or `~\` in `-i` is expanded, the
  matching private key must exist, and a repeated `-i` is an error. The default
  target follows upstream, including the OpenWrt and Haiku locations.
- The key is checked first and skipped only when it alone authenticated with
  `publickey`, as recorded first in the client's own log, also when a
  `command=` option makes the check exit nonzero; a check that ends with status
  255 is never taken as installed, since a server's disconnect text reaches that
  log. Success with another configured identity, an identity named
  with a `%` token, or the `none` method is not taken as installed (D-01). The
  key is verified after writing (D-06).
- Reject private key material (D-04), CRLF line endings are normalized (D-05), a
  leading byte order mark is removed (D-07), and malformed lines are rejected
  (D-10) before anything is sent.
- The remote command is one line, so a csh or tcsh login shell runs it. A
  target that is not a regular file, or a non-empty one that cannot be read, is
  reported and left unchanged (D-16), and a failed write is removed again
  (D-17).
- Lines of the key file are trimmed and trailing blank lines dropped as upstream
  does; `#` comment lines and blank lines are appended, and only key lines are
  counted (D-18).
- Every connection uses `-a -x`, as upstream does, so a configured
  `ForwardAgent` or `ForwardX11` does not reach the destination.
- The installation runs without a terminal, so `RequestTTY=force` in the
  configuration cannot leave it waiting (D-15).
- Warn when the `ssh` client version has not been tested; an unknown check
  result is reported instead of guessed (D-13). Tested clients:
  `OpenSSH_for_Windows_9.5p2`, Git for Windows `OpenSSH_10.0p2`, and Ubuntu
  24.04's `OpenSSH_9.6p1`.
- Without a console or `SSH_ASKPASS`, `ssh` runs with `BatchMode=yes` and fails
  at once instead of waiting for a password. Ctrl-C ends `ssh` first; the CLI
  then reports what happened and stops with status 1.
- Messages name the target path the remote side reported.
