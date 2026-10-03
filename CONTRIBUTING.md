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

Releases are signed tags `vX.Y.Z` on `main`, starting at v0.0.1. Pushing a tag
runs [release.yml](.github/workflows/release.yml): it checks the tag, runs the
tests on Linux, Windows, and macOS and against the Linux fixture, builds the
Windows archive, creates the GitHub release, and publishes to crates.io. A tag
containing `-`, such as `v0.0.1-pre`, is a prerelease: it runs the same
pipeline at the version `main` already carries and never publishes to crates.io.

### Rehearse with a prerelease

1. Update `main`:

   ```sh
   git switch main
   git pull --ff-only
   ```

2. Tag, check the signature, and push:

   ```sh
   git tag -s vX.Y.Z-pre -m "Prerelease vX.Y.Z-pre"
   git tag -v vX.Y.Z-pre
   git push origin vX.Y.Z-pre
   ```

   `git tag -v` must print a good signature.

3. Watch the run with `gh run watch` until every job but "Publish to crates.io"
   succeeds; that one is skipped.
4. Check the release page: it is marked as a prerelease and carries
   `ssh-copy-id-x86_64-pc-windows-msvc.zip`. Download it, unpack it, and run
   `ssh-copy-id.exe -h`, which prints the usage and exits 1.

### Cut the release

1. Update `main` as above.
2. In `Cargo.toml`, set `version` to `X.Y.Z`, then run `cargo check` to update
   `Cargo.lock`.
3. In `CHANGELOG.md`, rename `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD` and add an
   empty `[Unreleased]` above it.
4. Commit and push the bump, the one commit that reaches `main` without a pull
   request:

   ```sh
   git add Cargo.toml Cargo.lock CHANGELOG.md
   git commit -S -m "Release vX.Y.Z"
   git push origin main
   ```

5. Wait for CI on `main` to pass.
6. Tag, check the signature, and push:

   ```sh
   git tag -s vX.Y.Z -m "Release vX.Y.Z"
   git tag -v vX.Y.Z
   git push origin vX.Y.Z
   ```

7. Watch the run until every job succeeds, then check the release page and
   `https://crates.io/crates/ssh-copy-id`.

Publishing uses crates.io trusted publishing: the crate's settings on crates.io
name the repository `ShortArrow/ssh-copy-id` and the workflow `release.yml`, so
no token is stored. A rerun after a partial failure skips the publish when the
version is already on crates.io.
