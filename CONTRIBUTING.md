# Contributing

Describe the problem and the affected device, browser or platform. Follow the
[code of conduct](CODE_OF_CONDUCT.md). Keep tokens, private invite links and
other people's video out of issues, pull requests and commits.

## Browser TypeScript

Install Node.js 24 and pnpm, then run:

```sh
pnpm install --frozen-lockfile
pnpm format
pnpm check
pnpm test
pnpm build
```

`check` runs Biome and TypeScript. `check:all` also runs native Rust checks and
requires the native prerequisites below. The lockfile retains the configured
three-day minimum release age; update dependencies with `pnpm update --latest`
and review the manifest and lockfile together.

## Native Rust

Install Rust 1.92+. Linux builds need GLib/libclang headers and clang 21 or newer:

```sh
sudo apt-get install libglib2.0-dev libclang-21-dev clang-21
```

If your distribution does not provide clang 21, use LLVM's
[official package repository](https://apt.llvm.org/). Recording tests also need
the `ffmpeg` command with `libx264` available.

```sh
cargo fmt
CXX=clang++-21 cargo clippy --all-targets --all-features -- -D warnings
CXX=clang++-21 cargo test --workspace
```

`CXX` applies to Linux; use the platform compiler elsewhere. On Windows, set
`RUSTFLAGS=-C target-feature=+crt-static` to match LiveKit's WebRTC library.
CI builds and runs non-hardware tests on Linux, macOS and Windows. Report which
real devices and rooms you tested; CI does not validate hardware encoders or
physical cameras.

Run `cargo upgrade --dry-run --incompatible --pinned` to review newer releases.
Update the LiveKit SDK and signaling dependency together, and verify capture
backend changes on the OS matrix. Keep linker and sccache choices machine-local
so contributors and portable builds can use their platform defaults.

## CI scope and caching

Native source, Cargo configuration, lockfiles and workflow changes run the Rust
matrix on all three operating systems. Browser inputs run lint, tests and a
build that includes typechecking. Documentation-only changes run link checks and
Biome without compiling Rust or deploying the browser app. Manual workflow runs
always run every check.

Rust dependency artifacts are cached by platform, compiler/toolchain settings,
Cargo manifests and lockfile. Compiler variables are set before restoring the
cache, including Windows' static runtime flag. The browser job caches pnpm's
store. Only main-branch Rust jobs save caches; pull requests can restore them.
A cold run still compiles dependencies, and cached runs still execute tests and
build the native executables. Superseded pull-request runs are cancelled. Main
runs finish independently so a later documentation-only push cannot cancel an
outstanding Rust check. Pages deployment verifies the latest main revision.

## Code and documentation

Use descriptive names, explicit types and exhaustive variants where practical.
Test observable outcomes; avoid tests that only repeat third-party behavior.
Keep camera naming, freshness policy and credentials in the consuming application.

Follow the [Google developer documentation style guide](https://developers.google.com/style/highlights).
Keep setup instructions task-focused, use sentence-case headings and descriptive
links, and link to official upstream references for general SDK behavior. Keep
this project's defaults, contracts and limitations documented here.

Check local links with `lychee --offline --include-fragments=anchor-only README.md
CONTRIBUTING.md docs/*.md native/*/README.md`; use `lychee` without `--offline`
when validating external references. It checks links, not spelling.

Squash fixup commits before merging when useful. Preserve published revisions
that consumers pin. Contributions use the [MIT license](LICENSE).
