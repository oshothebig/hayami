# Native Shell prototype

A one-screen pull request review prototype on [Native Shell](https://github.com/longbridge/nsh) 0.6.1
for issue #22. It reads a local unified diff (no GitHub API) and shows:

- a file list on the left (click or `J`/`K` to switch files),
- the patch in Native Shell's `DiffView` (unified layout, virtualized rows, syntax highlighting,
  per-file folding),
- inline comments: select lines on the line numbers (press, Shift-press or drag) or use the
  `+` in the gutter, or press `c` to comment on the current hunk; `Enter` posts, `Esc` cancels.

| Key | Action |
| --- | --- |
| `j` / `k` | Next / previous hunk |
| `J` / `K` | Next / previous file |
| `o` / `O` | Fold the current file / fold or unfold all files |
| `c` | Comment on the selected lines (else the current hunk) |
| `Enter` / `Cmd+Enter` (`Ctrl+Enter`) | Post the comment |
| `Esc` | Cancel the comment |

The patch is `fixtures/ripgrep-2610.diff` unless `HAYAMI_DIFF` names another file.

## Run

macOS (Xcode command line tools and Rust 1.97+):

```sh
cargo run --release
```

Linux needs GTK 4.14+ (Ubuntu 24.04 or later), GStreamer and clang. On older distributions use
the bundled image:

```sh
docker build -t hayami-nsh-dev .
docker run --rm -it --net=host -e DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v "$HOME/.cargo:/root/.cargo" -v "$HOME/.rustup:/root/.rustup" -v "$PWD/../..:/work" \
  -w /work/prototypes/nsh -e PATH=/root/.cargo/bin:/usr/bin:/bin hayami-nsh-dev cargo run --release
```

## Test

```sh
cargo test
```

The tests drive the screen headless (`nsh-testing`): file list, `c`/`Enter`/`Esc`, `J`, and
that `j`/`k` typed into a comment stay text.
