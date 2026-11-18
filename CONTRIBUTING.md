# Contributing

Thanks for your interest! Bug reports, ideas and code are all welcome. Issues and pull requests may be written in
English or German.

When reporting a bug, please include the version (`tephra --version`, `tephra-server --version`), the operating system,
what you did, what you expected and what happened. Please **don't paste real host names, addresses, keys, tokens, VPN
configurations or vault files** into issues – replace them with invented values. Security issues go through
[SECURITY.md](SECURITY.md), not public issues.

## Development setup

Requirements: Rust 1.90 or newer (stable, with `clippy` and `rustfmt`) and a C compiler for the bundled SQLite. On
Windows that is the MSVC build tools, on Linux `build-essential` or its equivalent.

```bash
cargo build                       # target/debug/tephra and target/debug/tephra-server
cargo run -p tephra-cli -- --help
cargo run -p tephra-server -- --help
```

To try things without touching your own vault, point `tephra` at a throwaway file:

```bash
export TEPHRA_VAULT=/tmp/dev.tephra
cargo run -p tephra-cli -- init
cargo run -p tephra-ssh --features testing --example test_server    # SSH server for manual tests, runs nothing on your machine
```

Layout:

| Folder | Contents |
|---|---|
| `crates/tephra-model` | Data model: hosts, identities, keys, VPN profiles, snippets as mergeable records with a timestamp per field |
| `crates/tephra-crypto` | Key hierarchy (Argon2id, HKDF) and record encryption (XChaCha20-Poly1305) |
| `crates/tephra-vault` | Encrypted vault on SQLite, history, sync state, device-bound secrets and journal |
| `crates/tephra-transport` | How a host is reached: direct, SOCKS5/HTTP proxy, in-process tunnel using the WireGuard protocol |
| `crates/tephra-ssh` | SSH client: login, commands, interactive shell, jump hosts, port forwarding, SFTP, key handling |
| `crates/tephra-core` | Facade that ties vault, tunnels and connections together for command line and MCP; import of OpenSSH configurations |
| `crates/tephra-mcp` | MCP server, access rules, confirmation, audit log |
| `crates/tephra-sync-proto` | Types shared by sync client and server |
| `crates/tephra-sync` | Sync client |
| `crates/tephra-server` | Sync server (program `tephra-server`) |
| `crates/tephra-cli` | Command line (program `tephra`) |
| `deploy/` | Dockerfile, Compose file, Caddy example and systemd unit for the sync server |
| `docs/` | Manual, threat model, license assessment |

## Tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny --locked check         # needs cargo-deny
```

The tests need no external services and no network: an SSH server, both ends of a tunnel and the sync server run
inside the test process on the loopback interface. The CI (GitHub Actions) runs format check, Clippy and the tests on
Linux and Windows and the license and advisory check on every push and pull request.

Development happens on Windows. The Linux-specific parts – above all the Unix socket of `tephra daemon` and its
directory with mode 0700 – have so far only been compiled and tested by the CI. Reports and fixes from people who use
Linux every day are especially welcome.

New behaviour gets a test next to the code. Anything that parses untrusted input (configuration files, server
responses, data from an SSH host) gets tests for malformed input as well.

## Guidelines

- **Nothing security-relevant outside the core.** Keys and credentials are handled in the Rust crates; interfaces show
  them only as long as necessary and never log them. Use `Secret` for secret strings.
- **No cryptography of our own.** Only established primitives through the existing functions in `tephra-crypto`. Changes
  to the key hierarchy or the record format need a note in the [threat model](docs/threat-model.md).
- **The server stays dumb.** It authenticates devices, stores ciphertext with revision numbers and announces changes.
  It never needs to understand a record.
- **Nothing is allowed by default.** MCP access, new hosts for a client and file access are granted explicitly; when in
  doubt, a call is rejected.
- **No `unsafe`.** It is forbidden for the whole workspace. `unwrap` is only fine in tests.
- **Old data stays readable.** New fields get default values; never change the meaning of an existing field.
- **Style:** `rustfmt` with the settings in `rustfmt.toml` (lines up to 120 characters), no Clippy warnings. Code
  comments and messages of the command line are German; match the surrounding code. Tool descriptions for MCP are
  English because language models read them.
- **Dependencies:** only with a good reason, only from crates.io and only under a license already allowed in
  `deny.toml`. Add direct dependencies to `THIRD_PARTY_NOTICES.md` and `docs/lizenzen.md`.
- **Commit messages:** a short summary line, then what changed and why. Write in English or German.
- **No private data in code, tests or examples:** use `example.org`, addresses from `192.0.2.0/24` or `10.8.0.0/24` and
  invented keys.
- Commands and options that change go into the manual (`docs/manual.md` and `docs/handbuch.md`) and the
  `CHANGELOG.md`.
- Pull requests: short description of what, why and how it was tested.

## License

By contributing you agree that your contribution is published under the project's [MIT license](LICENSE).

## Deutsch

Beiträge auf Deutsch sind willkommen – Issues, Pull Requests und Diskussionen dürfen deutsch sein. Die Kommentare im
Code und die Meldungen der Kommandozeile sind deutsch. Handbuch und Bedrohungsmodell gibt es auf Deutsch
([`docs/handbuch.md`](docs/handbuch.md), [`docs/bedrohungsmodell.md`](docs/bedrohungsmodell.md)) und Englisch; die
Lizenzübersicht unter `docs/lizenzen.md` ist nur deutsch.
