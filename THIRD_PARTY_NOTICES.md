# Third-party software

The code of this repository is licensed under the [MIT license](LICENSE). Dependencies are fetched from crates.io by
Cargo and are not part of the repository. They are compiled into the programs `tephra` and `tephra-server`. The
graphical app in `app/` additionally uses packages from pub.dev, listed [below](#graphical-app).

Tephra ships no third-party data. No dependency is under the GPL, LGPL or AGPL. The licenses that occur are MIT,
Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib and Unicode-3.0. Where a crate offers a choice (`MIT OR
Apache-2.0`), it is used under MIT.

## Direct dependencies

| Component | License | Purpose |
|---|---|---|
| [argon2](https://github.com/RustCrypto/password-hashes) | MIT OR Apache-2.0 | Key derivation from the master password |
| [chacha20poly1305](https://github.com/RustCrypto/AEADs) | MIT OR Apache-2.0 | Authenticated encryption of vault records |
| [hkdf](https://github.com/RustCrypto/KDFs), [sha2](https://github.com/RustCrypto/hashes) | MIT OR Apache-2.0 | Key derivation, hashes |
| [subtle](https://github.com/dalek-cryptography/subtle) | BSD-3-Clause | Constant-time comparisons |
| [zeroize](https://github.com/RustCrypto/utils) | MIT OR Apache-2.0 | Wiping secrets from memory |
| [getrandom](https://github.com/rust-random/getrandom), [rand](https://github.com/rust-random/rand) | MIT OR Apache-2.0 | Random numbers |
| [rusqlite](https://github.com/rusqlite/rusqlite) with bundled [SQLite](https://www.sqlite.org) | MIT (SQLite: public domain) | Vault file and server database |
| [russh](https://github.com/warp-tech/russh) | Apache-2.0 | SSH protocol |
| [russh-sftp](https://github.com/AspectUnk/russh-sftp) | Apache-2.0 | SFTP |
| [boringtun](https://github.com/cloudflare/boringtun) | BSD-3-Clause | WireGuard protocol in user space |
| [smoltcp](https://github.com/smoltcp-rs/smoltcp) | 0BSD | TCP/IP stack inside the tunnel |
| [socket2](https://github.com/rust-lang/socket2) | MIT OR Apache-2.0 | Socket options |
| [rmcp](https://github.com/modelcontextprotocol/rust-sdk) | Apache-2.0 (parts MIT) | MCP server |
| [axum](https://github.com/tokio-rs/axum) | MIT | HTTP interface of the sync server |
| [reqwest](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 | HTTP client for sync |
| [rustls](https://github.com/rustls/rustls) | Apache-2.0 OR ISC OR MIT | TLS for sync |
| [tokio](https://github.com/tokio-rs/tokio) | MIT | Asynchronous runtime |
| [async-trait](https://github.com/dtolnay/async-trait) | MIT OR Apache-2.0 | Asynchronous trait methods |
| [serde](https://github.com/serde-rs/serde), [serde_json](https://github.com/serde-rs/json) | MIT OR Apache-2.0 | Serialisation |
| [uuid](https://github.com/uuid-rs/uuid) | MIT OR Apache-2.0 | Identifiers |
| [base64](https://github.com/marshallpierce/rust-base64), [hex](https://github.com/KokaKiwi/rust-hex) | MIT OR Apache-2.0 | Encodings |
| [thiserror](https://github.com/dtolnay/thiserror) | MIT OR Apache-2.0 | Error types |
| [tracing](https://github.com/tokio-rs/tracing), tracing-subscriber | MIT | Logging |
| [clap](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | Command line parsing |
| [crossterm](https://github.com/crossterm-rs/crossterm) | MIT | Terminal handling for `tephra ssh` |
| [rpassword](https://github.com/conradkleinespel/rpassword) | Apache-2.0 | Password prompt |

Used for tests only and not part of the programs: [proptest](https://github.com/proptest-rs/proptest) and
[tempfile](https://github.com/Stebalien/tempfile) (both MIT OR Apache-2.0).

## Graphical app

The app is built with [Flutter](https://github.com/flutter/flutter) (BSD-3-Clause); the Flutter engine and the Dart
runtime are shipped with it. Its Dart packages are under MIT, BSD-3-Clause or Apache-2.0.

| Component | License | Purpose |
|---|---|---|
| [material_ui](https://pub.dev/packages/material_ui), [cupertino_icons](https://pub.dev/packages/cupertino_icons) | BSD-3-Clause, MIT | Widgets and icons |
| [xterm](https://github.com/TerminalStudio/xterm.dart) | MIT | Terminal emulation and rendering |
| [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge) (Dart package and Rust crate) | MIT | Bridge between the app and the Rust core |
| [freezed_annotation](https://github.com/rrousselGit/freezed) | MIT | Data classes for events from the core |

The complete list of Dart packages is in `app/pubspec.lock`; `flutter pub deps` prints it as a tree. The app shows the
licenses of all Dart packages and of the engine under "About".

## Worth knowing

- **ring** (Apache-2.0 AND ISC) comes in through `russh`, `rustls` and `boringtun`. It contains C and assembly code
  from BoringSSL that is linked statically into the programs.
- **rmcp** names Apache-2.0 in its manifest; its license file explains that contributions not yet relicensed remain
  under MIT. Both apply.
- **boringtun** is Copyright (c) 2019 Cloudflare, Inc. The BSD-3-Clause license does not allow using that name to
  promote this project.
- The sync client verifies the server's certificate with the certificate store of the operating system
  (`rustls-platform-verifier`); no list of root certificates is compiled in.

## Complete list

The table above lists direct dependencies only. The complete list of all packages with their licenses is printed by

```bash
cargo deny --locked list
```

and `cargo deny --locked check` verifies that nothing outside the allowed licenses has crept in (configuration:
[deny.toml](deny.toml)). The assessment of every license and the obligations when distributing binaries are written
down in [docs/lizenzen.md](docs/lizenzen.md) (German).

## Trademark

“WireGuard” is a registered trademark of Jason A. Donenfeld. Tephra is not affiliated with the WireGuard
project and is neither supported nor endorsed by it. The term is used only to name the protocol.
