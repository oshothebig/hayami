# Rune review prototype

A standalone, in-memory review of the shared ripgrep PR #2610 fixture. Requires
Rust 1.99.1 (rustup reads `rust-toolchain.toml`). Rune and all six upstream patches
are pinned in `Cargo.toml`; `Cargo.lock` is committed.

```sh
# Ubuntu / Debian
sudo apt-get install -y libxkbcommon-dev libwayland-dev mesa-vulkan-drivers libxkbcommon-x11-0 fonts-noto-cjk
cd prototypes/rune
cargo build
cargo run -- [path/to/change.diff] [--repeat 3]
cargo test
cargo clippy
```

The default fixture path is relative to the compiled crate's manifest directory,
not the working directory. Repetition clones the parsed files and suffixes their
names before the extension, preserving syntax detection.

```sh
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
# Ubuntu 22.04's software Vulkan ICD (newer Mesa may call it lvp_icd.json):
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
target/debug/hayami-rune --scene composer --snap composer.png --tree composer.json --size 1400x900 --theme dark --scale 1 --at 1
RUNE_GPU_LOG=1 target/debug/hayami-rune --repeat 3 --scene scrolled --script benchmarks/scroll.json --frames 120 --size 1400x900 --scale 1
RUNE_GPU_LOG=1 target/debug/hayami-rune --repeat 3 --script benchmarks/file-switch.json --frames 90 --size 1400x900 --scale 1
```

Scenes: `default`, `file-selected`, `composer`, `commented`, `scrolled`, `folded`.
`--scale 1` makes the PNG exactly 1400×900; Rune otherwise defaults to 2×.
Headless output uses the same renderer with Mesa's CPU Vulkan adapter. It needs
no display server. Debug builds include Rune's headless tooling; release builds
need `--features rune/tooling`.

Select gutter lines (Shift/drag for ranges), then press `c`/Enter to compose
inline on the selection or keyboard-tracked hunk. The hover `+` opens a composer
directly. Reselecting keeps an existing composer and its draft at the original anchor.
Ctrl/Cmd+Enter posts; Escape cancels. Composer typing does not navigate the diff.
`j`/`k` navigate hunks; `]`/`[` navigate files. Comments are lost on exit.

The view uses Rune's virtual list, syntax colours, file collapse and unchanged
range folds. A context-limited patch cannot expand omitted source text; Rune
requires full old/new texts or a full-context patch for that. Rune has no public
per-hunk collapse API, so the `folded` scene demonstrates file collapse instead.
No GitHub network calls, posting, persistence, or arbitrary per-hunk folding are
implemented.

Rune exposes the visible file but not the visible hunk. The `c`/Enter fallback
tracks keyboard hunk navigation; after arbitrary wheel scrolling within a file,
select a gutter line to choose an exact comment location. No private Rune row-key
format is used to infer the hunk.

The benchmark scripts measure forced headless frames, not presentation latency
or a live window's idle behaviour. Debug CPU/Vulkan results are not optimized
hardware-GPU throughput.
