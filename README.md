# hayami

A GitHub pull request review tool.

## Status

This project is at a very early stage. Very little works yet, and the design and structure may change a lot.

## Concept

hayami aims to provide a fast, native environment for reviewing GitHub pull requests.

## Motivation

- To build a tool I actually want and can use in my own software development
- To gain agent-native development skills by building it

## Scope

- Target service: GitHub
- Target platforms: macOS and Linux

## Non-goals

- Support for services other than GitHub (such as GitLab or Bitbucket) is out of scope for now.

## License

See [LICENSE](LICENSE).

## Development

Build and run the empty desktop app with the Rust toolchain and `just` versions
in [mise.toml](mise.toml):

```sh
just build
cargo run --locked
```

Use `just format`, `just lint`, and `just test` to check changes.

Validate the Claude Code settings (`.claude/settings.json`) against their JSON schema in a container (requires [just](https://github.com/casey/just) and Docker):

```sh
just validate-claude
```
