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
  `-f` after `-i` still does and the login hint names it. Private key input, CRLF line endings, a leading
  byte order mark, malformed lines and a key file with several keys are still
  handled as without `-f` (D-04, D-05, D-07, D-10, D-21). As upstream's, the
  login hint after `-f -i` names `-i` without the private key.

### Changed

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
