# Security Policy

Report a vulnerability privately through
[GitHub's private vulnerability reporting](https://github.com/ShortArrow/ssh-copy-id/security/advisories/new),
not in a public issue. Include the version or commit, the destination OS and
shell, the `ssh -V` output, and the steps that reproduce it.

Fixes go into the latest release only; there are no maintained older versions
before v1.0.0.

The password in `tests/environments/windows` belongs to disposable test guests
that listen on 127.0.0.1 only; it is not a credential for any real system.
