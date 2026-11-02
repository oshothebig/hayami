# Task runner for hayami. Run `just --list` to see all recipes.

# Format all crates.
fmt:
    cargo fmt --all

# Check formatting without modifying files.
fmt-check:
    cargo fmt --all --check

# Lint the GPUI-independent core (works on Linux and macOS).
lint-core:
    cargo clippy -p hayami-core --all-targets --locked -- -D warnings

# Test the GPUI-independent core (works on Linux and macOS).
test-core:
    cargo test -p hayami-core --locked

# Lint the whole workspace (macOS only).
lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Test the whole workspace (macOS only).
test:
    cargo test --workspace --locked

# Build the release binary (macOS only).
build:
    cargo build --workspace --release --locked

# Run the app (macOS only).
run *ARGS:
    cargo run -p hayami-app --release -- {{ARGS}}

# Check dependencies for known vulnerabilities.
audit:
    cargo audit

# Everything CI runs on Linux.
ci-linux: fmt-check lint-core test-core audit

# Everything CI runs on macOS.
ci-macos: lint test build
