# hayami

Keyboard-driven pull request review tool built on GPUI. See [SPEC.md](SPEC.md) for the MVP spec.

hayami runs on macOS only (GPUI renders with Metal).

## Layout

- `crates/hayami-core`: GPUI-independent logic (GitHub API, auth, git, diff, highlighting, display rows, key handling). Builds and tests on Linux too.
- `crates/hayami-app`: GPUI app (window, rendering, key event plumbing). macOS only.

## Requirements

- Rust stable (edition 2024)
- macOS with the Xcode Command Line Tools (`xcode-select --install`). Xcode itself is not required: GPUI is built with its `runtime_shaders` feature, which compiles the Metal shaders at startup instead of at build time with the `metal` compiler that only ships with Xcode
- [`just`](https://github.com/casey/just) as the task runner
- [`gh`](https://cli.github.com/) logged in with `gh auth login` (used to obtain a token)
- `cargo-audit` for `just audit`

## Development

Run the app:

```bash
just run
```

Run what CI runs on macOS (whole workspace):

```bash
just ci-macos
```

Run what CI runs on Linux (core only, plus formatting and audit):

```bash
just ci-linux
```

Other recipes are listed with `just --list`.

## GPUI version

GPUI is used from crates.io and pinned to an exact version because it has breaking changes between releases.

- Pinned: `gpui = "=0.2.2"` (crates.io)
- Upstream to follow: the `gpui` crate published from [zed-industries/zed](https://github.com/zed-industries/zed) (`crates/gpui`). When upgrading, bump the pinned version in `crates/hayami-app/Cargo.toml`, read the zed changelog for GPUI API changes, and run `just ci-macos`.
