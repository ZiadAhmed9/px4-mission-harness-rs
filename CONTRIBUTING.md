# Contributing to PX4 Mission Resilience Test Harness

Thanks for your interest in contributing! This document covers the basics.

## Getting Started

1. Fork the repo and clone your fork
2. Run `cargo build` to verify everything compiles
3. Run `cargo test` to verify tests pass (no PX4 SITL required)
4. Create a branch for your changes

## Development

### Build & Test

```bash
cargo build                          # build all crates
cargo test                           # run unit tests
cargo clippy --workspace             # lint (must pass with no warnings)
cargo fmt --all -- --check           # format check
```

### Integration Tests

Integration tests require a running PX4 SITL instance and are gated behind a feature flag:

```bash
cargo test -p px4-harness-core --features sitl -- --nocapture
```

### Code Style

- Run `cargo fmt` before committing
- All `cargo clippy` warnings must be resolved
- Use `thiserror` for error types in the library crate, `anyhow` in the binary
- Prefer strongly-typed enums over stringly-typed data
- All async code uses Tokio — no `std::thread` unless required for blocking FFI
- Keep modules focused on a single concern
- Tests go in `#[cfg(test)]` submodules alongside the code they test

## Submitting Changes

1. Keep PRs focused — one concern per PR
2. Include tests for new functionality
3. Make sure CI passes (`cargo test`, `cargo clippy`, `cargo fmt`)
4. Write a clear PR description explaining what and why

## Reporting Issues

Open an issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- PX4 SITL version if relevant

## License

By contributing, you agree that your contributions will be licensed under the same dual license as the project: Apache-2.0 OR MIT.
