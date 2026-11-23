image := "hayami-validate-claude"

# List available recipes
default:
    @just --list

# Build the container image used to validate Claude Code settings
build-json-schema-validator:
    docker build -t {{image}} tools/claude

# Validate the Claude Code settings (.claude/settings.json) against its JSON schema inside a container
validate-claude: build-json-schema-validator
    docker run --rm -v "{{justfile_directory()}}:/workspace:ro" {{image}} .claude/settings.json

# Format Rust code
format:
    cargo fmt --all

# Run Clippy on all workspace targets, treating warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all workspace tests
test:
    cargo test --workspace --locked

# Build all workspace crates
build:
    cargo build --workspace --locked

# Run the hayami desktop app
[positional-arguments]
run *args:
    cargo run --locked -- "$@"
