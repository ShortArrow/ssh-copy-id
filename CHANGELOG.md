# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Entries name the [differences from upstream](docs/compatibility.md) they add or change.

## [Unreleased]

### Added

- Install one explicitly selected public key (`-i FILE`) on a Unix-like host, with
  `-p`, `-o`, and `-F` passed to `ssh`. A leading `~/` or `~\` in `-i` is expanded, the
  matching private key must exist, and a repeated `-i` is an error. The default
  target follows upstream, including the OpenWrt and Haiku locations.
- The key is checked first and skipped only when it alone authenticated with
  `publickey`, as recorded in the client's own log rather than in output a
  server can write, also when a `command=` option makes the check exit nonzero. Success with another configured identity, an identity named
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
