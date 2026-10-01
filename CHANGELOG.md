# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Entries name the [differences from upstream](docs/compatibility.md) they add or change.

## [Unreleased]

### Added

- Install one explicitly selected public key (`-i FILE`) on a Unix-like host, with
  `-p`, `-o`, and `-F` passed to `ssh`. The key is checked first and skipped when
  it already authenticates; a successful check with another configured identity
  is not taken as installed (D-01). The key is verified after writing (D-06).
- Reject private key material (D-04), CRLF line endings are normalized (D-05), a
  leading byte order mark is removed (D-07), and malformed lines are rejected
  (D-10) before anything is sent.
- The target path is passed to the remote shell as data (D-11).
- Warn when the `ssh` client version has not been tested (D-13).
- Without a console or `SSH_ASKPASS`, `ssh` runs with `BatchMode=yes` and fails
  at once instead of waiting for a password.
