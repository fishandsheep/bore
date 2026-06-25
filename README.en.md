# bore

[![Build status](https://img.shields.io/github/actions/workflow/status/fishandsheep/bore/ci.yml)](https://github.com/fishandsheep/bore/actions)
[![Crates.io](https://img.shields.io/crates/v/bore-cli.svg)](https://crates.io/crates/bore-cli)

[中文文档](README.md)

`bore` is a small TCP tunnel written in async Rust. It exposes a local TCP port through a remote server, which is useful when NAT or firewall rules prevent direct inbound connections.

## Recent Updates

- npm distribution is available through `@qinshower/bore`, with platform-specific optional packages.
- Release scripts now prepare and publish npm packages from local package paths.
- End-to-end tests now wait for the control port to be released and stop the server after each case.
- The server defaults `--bind-tunnels` to `--bind-addr`, and both options use IP address validation.
- TCP proxying uses `copy_bidirectional` and handles half-closed streams.

## Installation

### npm-compatible runners

```sh
npx @qinshower/bore local 8000 --to bore.pub
bunx @qinshower/bore local 8000 --to bore.pub
```

Pinned versions are supported:

```sh
npx @qinshower/bore@0.6.0 --help
```

### Cargo

```sh
cargo install bore-cli
```

### Binary releases

Download prebuilt binaries from the [releases page](https://github.com/fishandsheep/bore/releases), unzip the archive, and put the `bore` executable on your `PATH`.

## Usage

Start a server:

```sh
bore server
```

Expose local port `8000` through a remote server:

```sh
bore local 8000 --to bore.pub
```

Choose a remote port:

```sh
bore local 8000 --to bore.pub --port 9000
```

Expose a different local host:

```sh
bore local 8080 --local-host 192.168.1.10 --to bore.pub
```

## Self-hosting

Run a server on your own machine:

```sh
bore server --bind-addr 0.0.0.0
```

Then connect clients to that server:

```sh
bore local 8000 --to <SERVER_ADDRESS>
```

The control port is `7835`. Tunnel ports are selected from `--min-port` to `--max-port`, defaulting to `1024..=65535`.

## Authentication

Use a shared secret to restrict access to a custom server:

```sh
# server
bore server --secret my_secret_string

# client
bore local 8000 --to <SERVER_ADDRESS> --secret my_secret_string
```

`BORE_SECRET` can also provide the secret. The secret protects the handshake only; tunnel traffic is not encrypted by `bore` itself.

## Development

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features
npm run npm:check
npm run npm:pack:dry-run
```

## License

MIT. This fork is based on the original `bore` project by Eric Zhang.
