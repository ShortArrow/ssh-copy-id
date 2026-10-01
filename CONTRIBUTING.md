# Contributing

## Branches and pull requests

`main` is the trunk and is always releasable. Every change reaches it through a
pull request from a short-lived branch, named after its issue when one exists,
such as `feat/issue-12-result-lines`; the prefixes are `feat/`, `fix/`, `docs/`,
and `ci/`. The one commit pushed to `main` directly is the version bump that
cuts a release. The reasons are recorded in the
[versioning decision](docs/design.md#versioning-and-release).

Commits are signed. A commit message states why the change was made in one or
two sentences and names any difference (`D-xx`, `O-xx`) it adds or changes. The
design documents state only the current design; see
[docs/README.md](docs/README.md).

## Checks

CI runs these on every pull request; run them before pushing:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The Windows destination fixture is run by hand; see
[tests/environments/windows](tests/environments/windows/README.md).

## Changelog

A user-visible change adds an entry under `[Unreleased]` in
[CHANGELOG.md](CHANGELOG.md), in the Keep a Changelog sections (Added, Changed,
Deprecated, Removed, Fixed, Security). A release renames `[Unreleased]` to the
version and date.

## Releases

Releases are signed tags `vX.Y.Z` on `main`, starting at v0.0.1, created with
`git tag -s` and checked with `git tag -v`. A tag containing `-` is a prerelease
and never publishes to crates.io. The release workflow and its checklist are
written before v0.0.1.
