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
[Documentation](#documentation).

## Checks

CI runs these on every pull request; run them before pushing:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The Windows destination fixture is run by hand; see
[tests/environments/windows](tests/environments/windows/README.md).

## Documentation

The roles of the documents under `docs/` are listed in
[docs/README.md](docs/README.md).

- Write primary documentation in English. Keep existing Japanese translations in sync when changing the corresponding English documents.
- A document without translations, such as `docs/README.md`, links a translated document by its English version only, and a translation links the versions in its own language. Each language version starts with one language line naming every version, the current one unlinked (`English | [日本語](design.jp.md)`), so adding a language changes only those lines. Links to other documents go on a separate line.
- Keep detailed decisions in the design document. The differences list contains summaries and links, not duplicated explanations of rationale, scope, implementation, or open questions.
- A decision in the design states the forces it weighs and the alternatives it refuses, each with the reason that still holds, so a later reader can tell whether that reason has expired before keeping or reversing the decision. When a reason stops holding, rewrite the decision.
- When the design defines a behavior the standards or the OpenSSH documentation leave open, add or update its `U-xx` row in both language versions of [definitions.md](docs/definitions.md), citing the source section.
- When a difference is accepted, update the design document and add or update its comparison row in both language versions of the differences list. Preserve its ID and link to the detailed decision.
- If a decision originated in the review, record its disposition there and link to the design. Keep unaccepted proposals distinct from accepted requirements.
- Describe upstream differences neutrally; do not label them as defects or bug fixes without an explicit decision to do so. Each difference states the apparent upstream intent and the reason to differ, taken from the design stance list.
- Distinguish accepted designs from implemented and verified behavior. Record verification narratives in the validation document; the design document names the validation matrix rows it depends on.
- The design and the differences list are living documents: when a decision changes, edit the section so it states the current design. Do not add decision dates, "superseded" notes, or former behavior; the history is in git.
- What is not decided goes in [open-questions.md](docs/open-questions.md), not in the design. When an item is decided, remove it there and state the result in the design.
- Living documents do not link to dated records such as validation result sections or the review; records may link to living documents. The validation matrix states current status and may be linked.
- Check relative links and section anchors when moving or renaming documents or headings.

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

7. Watch the run. "Publish to crates.io" waits in the `release` environment
   until the owner approves it on the run page; approve it after the GitHub
   release looks right.
8. Check the release page and `https://crates.io/crates/ssh-copy-id`.

Publishing uses crates.io trusted publishing: the crate's settings on crates.io
name the repository `ShortArrow/ssh-copy-id` and the workflow `release.yml`, so
no token is stored. The setting also names the environment `release`, so only an
approved run can publish. crates.io accepts that setting only for a crate that exists,
so version 0.0.0 was published once with `cargo login` and `cargo publish`
using a short-lived token, which was then revoked. A rerun after a partial failure skips the publish when the
version is already on crates.io.
