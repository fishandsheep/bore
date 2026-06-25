# Repository Guidelines

## Project Structure & Module Organization

This repository contains `bore-cli`, a Rust TCP tunneling CLI. Core Rust code lives in `src/`: `main.rs` defines the CLI, `client.rs` and `server.rs` implement tunnel behavior, `auth.rs` handles secret authentication, and `shared.rs` contains protocol utilities. Integration tests live in `tests/`, currently `auth_test.rs` and `e2e_test.rs`. Release and cross-compilation helpers are in `ci/`. npm distribution metadata and wrapper scripts live under `npm/`, with `npm/bore` as the main package and platform packages in sibling directories.

## Build, Test, and Development Commands

- `cargo build`: builds the `bore` binary for the host target.
- `cargo run -- server`: runs a local bore server.
- `cargo run -- local 8000 --to localhost`: runs a local client against a server.
- `cargo test`: runs all Rust tests.
- `cargo fmt --check`: verifies standard Rust formatting.
- `cargo clippy --all-targets --all-features`: checks Rust lints before review.
- `./ci/build.bash cargo <target-triple>`: CI-style target build, for example `./ci/build.bash cargo x86_64-unknown-linux-gnu`.
- `npm run npm:check` and `npm run npm:pack:dry-run`: validate npm release package metadata and packaging.

## Coding Style & Naming Conventions

Use Rust 2021 idioms and rustfmt defaults: four-space indentation, snake_case modules/functions, PascalCase types, and SCREAMING_SNAKE_CASE constants. Prefer `anyhow::Result` for fallible command and async flows already using it. Keep async network code on Tokio primitives. CLI flags should use Clap derives and environment variables should follow the existing `BORE_*` naming pattern.

## Testing Guidelines

Use Rust integration tests in `tests/` for protocol and end-to-end behavior. Name test files by feature, such as `auth_test.rs`, and test functions by expected behavior, such as `auth_handshake_fail`. Add async tests with `#[tokio::test]` when exercising client/server or stream behavior. Run `cargo test` before submitting changes; run targeted tests with `cargo test auth_handshake` while iterating.

## Commit & Pull Request Guidelines

Recent history uses concise Conventional Commit-style subjects, especially `ci: ...`, `fix(test): ...`, and `feat(npm): ...`; keep subjects imperative and scoped when useful. Pull requests should describe the behavior change, note test coverage, and link issues when applicable. Include CLI examples or npm packaging notes when user-visible commands, release assets, or platform packages change.

## Security & Configuration Tips

Do not log secret values. Authentication uses `BORE_SECRET`; CLI definitions should keep secret environment values hidden. Treat tunnel traffic as unencrypted unless callers add their own transport security.
