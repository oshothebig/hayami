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
one unified `gpui_component::diff::Diff` containing every file. `DiffFile::parse`
is the only patch parser, and `DiffState` owns the virtualized variable-height
list, source-line selection, navigation and folding. Only the custom sidebar
uses `uniform_list`. Its highlight follows the last explicitly jumped-to,
selected, or header-toggled file, **not passive scrolling or `j/k` navigation**.

The component supplies syntax highlighting, old/new gutters, addition/deletion
backgrounds, inline word-change emphasis and hunk separators. Its native
highlighter prepares each side of each hunk on a background executor. The
existing Rust, TypeScript, TOML, YAML, Bash and implicit JSON grammar features
are still used by that highlighter; no replacement highlighter or extra
dependency is installed. Hunk fragments can lack surrounding syntax;
unsupported languages are plain text.
The application registers `gpui_kit::assets::Assets` for the native header,
fold and add-comment icons.

`src/diff.rs` and the custom diff-row renderer/model are deleted. `main.rs`
contains sidebar, action and benchmark glue; `review.rs` only holds in-memory
comments, a code-click range origin, range labels, annotation targeting and
tests. The component itself is unmodified: no fork, vendored copy or registry
patch. Source size decreased from about 1,140 to 652 lines, including tests.

## Review controls

| Control | Action |
| --- | --- |
| Click sidebar file | Scroll to its header |
| `n` / `p` | Next / previous file |
| `j` / `k` | Native next / previous changed group, with native wrapping |
| File header chevron, `o`, or Space | Collapse / expand file (`o` uses sidebar-highlighted file) |
| Compact context / Expand context | Native global unchanged-context folding / expansion |
| Native context-fold controls | Reveal hidden unchanged lines |
| Click source text or gutter | Select a line |
| Shift+click, Shift+J / Shift+K | Extend line selection within a file |
| `c` or Comment button | Open a multiline gpui-component Textarea |
| Enter / Ctrl+Enter, Post comment | Submit a nonempty comment |
| Shift+Enter | Insert a newline |
| Escape, Cancel | Discard draft |

Select a line first, then use `c`, Comment, or the native gutter add-comment
button. Review shortcuts are disabled while a draft is open, so typing `j`, `k`,
`n`, `p`, or `c` does not navigate. Comment labels show the old/new side and
line range, including both sides for a cross-side range. Comments use component
path/side/source-line anchors, not row indices. The application owns one
`Entity<TextareaState>` for the lifetime of the review, as before; it now renders
the Textarea **inside `Diff::render_annotation`**, instead of a custom list row.
The component owns placement and row measurement, not the input or comment data.
Enter handling remains the Textarea's `InputEvent::PressEnter`, so XIM commits
are not intercepted by a review-level Enter binding. Shift+J/K dispatch the
component's registered `diff::ExtendDown` / `diff::ExtendUp` actions through
GPUI's public action builder (their Rust types are private).
Code clicks use `on_line_click` and `set_selected_lines`, retaining a small
application-side origin so Shift+click survives the component's intervening
character-selection clearing. Gutter drag/range selection remains native.

When a file is collapsed, application glue temporarily targets its comments
at a component file annotation beneath the header, keeping the stored ranges
unchanged. Expanding restores line annotations. Native unchanged-context folds
can hide a line annotation until the context is expanded; the comment remains
stored. Comments are in-memory only; there is no GitHub authentication or
persistence.

## Component gaps and evaluation changes

- There is no public per-hunk collapse API. `o`/Space now toggles **whole-file**
  collapse; hunk headers do not collapse changed lines. Native unchanged-context
  folding is available separately. This intentionally differs from the previous
  prototype rather than replacing the component's renderer.
  We configure one native context line and initially expand all supplied lines.
  Compact context restores that one-line setting; Expand context reveals all
  supplied context. The component's default three lines would not meaningfully
  shrink this fixture, whose unchanged runs are at most six lines.
- `j/k` uses native changed-group navigation, not our former hunk navigation.
  It is viewport-based and may wrap or stop at a different location than an
  explicit selected line's hunk.
- `DiffState` exposes no public current-visible-file getter, scroll handle or
  scroll event. Sidebar highlighting therefore tracks explicit file jumps and
  selection/header events only. After passive scrolling or `j/k`, select a line
  before using `o` to target the newly visible file.
- The same missing scroll handle changes the benchmark to public line/file
  jumps (details below); its results are **not directly comparable** with the
  old visual-row scroll benchmark.
- File identities are paths. A deletion of `a.rs` and rename `b.rs` to `a.rs`
  parse as two files with the same `path()`. `DiffState` indexes the **first**
  file for path-based scroll/selection APIs; sidebar clicks to either entry
  resolve to that first file. A modified-side selection may clear if that first
  file is a deletion with no modified lines. Collapse uses a shared path set,
  so both entries collapse together. Line annotations use path/side/line keys
  and can appear in both files when those positions exist in both. They cannot
  identify just the renamed file. We retain this component behavior; the prior
  custom file-index collision fix is deliberately not reproduced.
  A direct UI check with the deletion first confirmed that clicking the second
  sidebar entry highlights/jumps to the first, original-line-1 drafts and posted
  comments render under both files, and whole-file collapse affects both entries.
  Both draft widgets render the same application-owned Textarea entity.

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
fixture parsing and benchmark-target setup, to GPUI's callback directly after the first
rendered frame. This is a framework frame-completion timestamp, not an external
display scanout or GPU-fence measurement. Syntax preparation is asynchronous
and need not have finished at that timestamp.

`--bench` samples both original and modified sides of every file, in patch file
order, then jumps to all 32 file headers and exits. For each side it asks
`set_selected_lines` / `selected_lines` to clip source-number windows of 20
lines to supplied patch lines, skipping empty windows, and jumps to the first
supplied line of each using `scroll_to_line`. This uses the component's parser
and clipping rather than parsing hunk metadata ourselves. Selection is cleared
before rendering. One public line/file jump is scheduled per frame; context
lines can be visited twice, once per side. This replaces the old 20-**visual-row**
scroll sweep because `DiffState`'s list/viewport APIs are private. It logs each
`BENCH_FRAME` interval and a `BENCH_SUMMARY` with average/p95/maximum frame
interval in milliseconds and Linux peak RSS (`/proc/self/status` `VmHWM`).
These are end-to-end frame intervals, including scheduling/vsync, rather than
isolated CPU render timings. Run without competing builds for comparable results.

These software-render results must not be interpreted as hardware GPU
performance or as a controlled renderer-only comparison with the old benchmark.

On the 2-core Xvnc VM at 1600×1000 using llvmpipe OpenGL, one isolated release
run completed 455 line/file-jump frames: first-frame completion 92.42 ms,
average interval 443.457 ms, p95 533.407 ms, maximum 683.287 ms, peak RSS
196,436 kB. The release executable was 64,656,104 bytes. In particular, the
first-frame figure does not promise that asynchronous syntax highlighting has
finished; frame intervals remain slow under this CPU-rendered workload.

## Japanese input on X11

```sh
sudo apt-get install -y fcitx5 fcitx5-mozc fonts-noto-cjk
DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 fcitx5 -d --replace
```

Use a desktop shell with its session bus loaded (`DBUS_SESSION_BUS_ADDRESS`).
If this VM's desktop session has restarted, recreate the user-owned mode-0700
`/run/user/1000` runtime directory and start a session bus with `dbus-launch`
before starting fcitx5. Run `fcitx5-remote` from that same session.

Add Mozc alongside Keyboard-English in the fcitx5 default input-method group.
Start the app with the software-render variables above plus
`XMODIFIERS=@im=fcitx GTK_IM_MODULE=fcitx QT_IM_MODULE=fcitx`.
In the focused comment box, activate Mozc using Ctrl+Space or
`DISPLAY=:0 fcitx5-remote -s mozc` (and `fcitx5-remote -o`).
Type `nihongo`, Space to convert, then Enter to commit the Japanese composition;
Enter again (or Ctrl+Enter) posts the comment. The first Enter must commit the
IME composition without submitting the review. This uses GPUI's XIM path, not
GTK's input widget.

`fonts-noto-cjk` supplies the glyphs. The annotation placement changes, but the
same owner-held Textarea and GPUI XIM input path remain in use.

Verified on the Xvnc VM with the component annotation: underlined `にほんご`
preedit appeared in the Textarea, Space converted it to `日本語`, Enter committed
without posting or losing the draft, and Ctrl+Enter posted the Japanese comment
inline. No IME-specific interception or alternative input widget was added.

## Parser scope

`DiffFile::parse` uses the component's default strict options, with parse errors
returned before window creation. Tests cover the complete shared fixture,
empty input, malformed hunk metadata, comment validation/folding, and duplicate
path anchors. Binary/metadata handling, quoted paths, hunk validation and all
other parser behavior are delegated to the released component, not a local
parser or preprocessing pass.

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
