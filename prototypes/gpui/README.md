# GPUI review prototype

Standalone Cargo project; run commands from this directory:

```sh
cargo run --release -- [path/to/change.diff]
cargo test --locked
```

The default is `../../fixtures/ripgrep-2610.diff`, resolved relative to this
crate, so launching the binary from another directory also works. The shared
fixture is BurntSushi/ripgrep PR [#2610](https://github.com/BurntSushi/ripgrep/pull/2610)
(MIT/Unlicense), 4,468 lines across 32 files. The sidebar jumps to a file in
one virtualized, variable-height GPUI `list` containing all files, headers,
hunks, lines, and inline comments. Its highlight follows the visible file.
Only the sidebar uses `uniform_list`.

Syntax highlighting uses gpui-component's tree-sitter `SyntaxHighlighter`,
with Rust, TypeScript, TOML, YAML, Bash, and JSON grammars. Each hunk is parsed
separately on its old and new sides, so deleted lines do not corrupt the
new-side parse. Hunk fragments can lack surrounding syntax; unsupported
languages are plain text. Highlight spans are precomputed, not parsed while
scrolling. Both old/new gutters and addition/deletion backgrounds are shown.

## Review controls

| Control | Action |
| --- | --- |
| Click sidebar file | Scroll to its header |
| `n` / `p` | Next / previous file |
| `j` / `k` | Next / previous hunk |
| Click hunk header, `o`, or Space | Fold / unfold current hunk |
| Click line | Select a line |
| Shift+click, Shift+J / Shift+K | Extend selection on the same file and side |
| `c` or Comment button | Open a multiline gpui-component Textarea |
| Enter / Ctrl+Enter, Post comment | Submit a nonempty comment |
| Shift+Enter | Insert a newline |
| Escape, Cancel | Discard draft |

Review shortcuts are disabled while a draft is open, so typing `j`, `k`,
`n`, `p`, or `c` does not navigate. Comment labels show the old/new side and
line range. Anchors store file identity, side, and source line numbers in the
UI-independent `review` module, not virtual-list row indices. Folding keeps
comments visible beneath the containing hunk header. Comments are in-memory
only; this prototype does not authenticate to GitHub or persist reviews.

## Dependencies

`gpui-component = "=0.10.0"` is the latest crates.io release at setup time.
Its exact engine dependency is `gpui-pre = "=0.3.12"` (imported as `gpui`).
`gpui-kit = "=0.10.0"` supplies the matching platform backend and application
entry point, following the component README's basic example. All are registry
dependencies, with no Git sources or `[patch]`. No pinned toolchain was needed;
the build uses the installed Rust 1.99.1, edition 2024. The lockfile records the
complete dependency resolution. No Zed editor or git UI crates are used.

Linux packages explicitly requested during setup:

```sh
sudo apt-get update
sudo apt-get install -y pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev \
  libx11-xcb-dev libxcb1-dev libssl-dev clang cmake libxcb-shape0-dev \
  libxcb-xfixes0-dev libasound2-dev mesa-vulkan-drivers imagemagick
```

`mesa-vulkan-drivers` provides lavapipe for software rendering on a VM.
To force the X11 software path:

```sh
DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY= \
  VK_ICD_FILENAMES=/nonexistent LIBGL_ALWAYS_SOFTWARE=1 \
  cargo run --release --locked
```

On this Xvnc display, Vulkan selected lavapipe successfully but produced an
invisible client area. Setting `VK_ICD_FILENAMES=/nonexistent` disables Vulkan
discovery and lets GPUI select Mesa's software OpenGL backend, which rendered
the window successfully. Use `RUST_LOG=gpui_wgpu=info` to inspect backend selection.
Both lavapipe (Vulkan) and llvmpipe (OpenGL) are CPU renderers, not hardware GPU
measurements. To try Vulkan instead, set
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`.

## Instrumentation

```sh
cargo build --release --locked -j 2
DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY= \
  VK_ICD_FILENAMES=/nonexistent LIBGL_ALWAYS_SOFTWARE=1 \
  RUST_LOG=gpui_wgpu=info \
  ./target/release/hayami-gpui-prototype --bench 2>bench.log
```

The app logs `FIRST_FRAME_PAINTED_MS` from the start of `main`, including
fixture parsing and highlighting, to GPUI's callback directly after the first
rendered frame. This is a framework frame-completion timestamp, not an external
display scanout or GPU-fence measurement. `--bench` scrolls in 20-row increments
through the whole diff, then jumps to every file header and exits. It logs each
`BENCH_FRAME` interval and a `BENCH_SUMMARY` with average/p95/maximum frame
interval in milliseconds and Linux peak RSS (`/proc/self/status` `VmHWM`).
These are end-to-end frame intervals, including scheduling/vsync, rather than
isolated CPU render timings. Run without competing builds for comparable results.

On the 2-core Xvnc VM at 1600×1000 using llvmpipe OpenGL (CPU rendering),
the release build measured first-frame completion at 5,385.72 ms. The 251-frame
scroll/file-jump run averaged 321.900 ms, p95 416.640 ms, maximum 500.010 ms,
with peak RSS 190,552 kB (about 186 MiB). These software-render results are slow
and must not be interpreted as hardware GPU performance. The release binary
with the enabled grammars is 63,754,136 bytes.

## Japanese input on X11

```sh
sudo apt-get install -y fcitx5 fcitx5-mozc fonts-noto-cjk
DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 fcitx5 -d --replace
```

Add Mozc alongside Keyboard-English in the fcitx5 default input-method group.
Start the app with the software-render variables above plus
`XMODIFIERS=@im=fcitx GTK_IM_MODULE=fcitx QT_IM_MODULE=fcitx`.
In the focused comment box, activate Mozc using Ctrl+Space or
`DISPLAY=:0 fcitx5-remote -s mozc` (and `fcitx5-remote -o`).
Type `nihongo`, Space to convert, then Enter to commit the Japanese composition;
Enter again (or Ctrl+Enter) posts the comment. The first Enter must commit the
IME composition without submitting the review. This uses GPUI's XIM path, not
GTK's input widget.

Verified on the setup VM: underlined `にほんご` preedit appeared in the Textarea,
Space converted it to `日本語`, the first Enter committed without posting, and
Ctrl+Enter posted the Japanese text inline. `fonts-noto-cjk` supplies the glyphs.

## Parser scope

The UI-independent `diff` module supports ordinary unified Git patches,
multiple hunks/files, added/deleted files, and metadata-only file entries.
It validates hunk counts and reports malformed input rather than silently
displaying truncated changes. Combined diffs and Git's C-quoted paths are not
supported. Binary patches are represented as file entries without text hunks.

## Initial scaffold measurements

On the 2-core Ubuntu VM, clean builds used `cargo build --locked -j 2` and
`cargo build --release --locked -j 2`, with registry downloads completed first.
The debug build took 201.23 seconds (3m 21s), and the release build took
516.63 seconds (8m 36s); the unstripped release binary
is 53,910,704 bytes (51.4 MiB). Build logs and timing files are in
`/home/ubuntu/gpui-*-build.log` and `/home/ubuntu/gpui-*-time.txt` on the setup VM.
These sizes and clean-build times are for the initial plain-text scaffold,
before adding tree-sitter grammars and review controls. Its screenshot is
`/home/ubuntu/shots/step1.png`.
