# Contributing

Issues and pull requests are welcome. Follow our [code of conduct](CODE_OF_CONDUCT.md).
Describe the problem and the device,
browser or platform involved; small, focused changes are easiest to review.
Do not include LiveKit tokens, API keys, private camera links, or video from
other people in an issue or pull request.

For browser changes, install Node.js 24 and pnpm, then run:

```sh
pnpm install --frozen-lockfile
pnpm format
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

For native changes, install the dependencies in the [README](README.md#native-camera-publisher),
then run:

```sh
cargo fmt --all
CXX=clang++-21 cargo clippy --all-targets --all-features -- -D warnings
CXX=clang++-21 cargo test --workspace
```

The `CXX` setting applies to Linux with clang 21; use the platform's supported
compiler elsewhere. Explain which devices and LiveKit server you tested, and
identify hardware or platforms you could not test. Contributions are accepted
under this repository's [MIT license](LICENSE).

On Windows, LiveKit's prebuilt WebRTC library uses the static MSVC runtime.
Set `RUSTFLAGS=-C target-feature=+crt-static` for native builds, as CI does.
The matrix compiles, links and runs non-hardware tests on Linux, macOS and
Windows. Camera devices and hardware encoders still require device testing.
