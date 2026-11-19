# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project uses
[Semantic Versioning](https://semver.org/).

## [0.2.0] – 2026-11-19

### Added

- **Installer for Windows:** `Tephra-<version>-windows-x64-setup.exe` installs the app, the command line and the sync
  server for the current user, without administrator rights: Start menu entry, uninstalling through Windows and, if
  wanted, `tephra` on the `PATH`. The zip stays as a portable version.
- **App for Linux:** as a Debian package (`tephra_<version>_amd64.deb`, for Ubuntu 22.04 and Debian 12 or newer; it
  also contains the command line and the sync server) and as an AppImage. The automated app test now runs on Linux as
  well.
- **Sync server as a container image:** `ghcr.io/poldion/tephra-server` for amd64 and arm64, published with every
  release. The Compose file in `deploy/` uses it.

### Fixed

- **App on Linux:** the first error from the core, for example a wrong master password, crashed the app. Collecting a
  stack trace for the error message collided with the stack unwinder that comes with Flutter on Linux; the app no
  longer collects stack traces.

## [0.1.0] – 2026-11-18

First release, a preview: the command line `tephra` and the sync server `tephra-server` for Windows and Linux, and a
first version of the graphical app for Windows and Android.

### Added

- **Vault:** encrypted local vault in a single file (SQLite, ciphertext only). The master password is stretched with
  Argon2id, every record is encrypted with XChaCha20-Poly1305 and padded to fixed size steps. A recovery key is shown
  once when the vault is created. The vault header is authenticated and carries a counter, so that an older genuine
  header (with a previous password) is not accepted again. `tephra init`, `tephra passwd`, `tephra recover`, `tephra
  export`. Backups do not contain keys marked as "this device only".
- **Hosts, identities and keys:** `tephra host`, `tephra identity`, `tephra key`. Identities with a stored password, a
  password asked on every connection or an SSH key. Ed25519 key generation, import of existing keys (OpenSSH, PKCS#8,
  PEM) and keys that stay on one device (`--local-only`).
- **Import:** `tephra import-ssh-config` takes over hosts, users, unencrypted keys and `ProxyJump` from an OpenSSH
  configuration.
- **SSH:** interactive sessions (`tephra ssh`) and single commands (`tephra exec`), server prompts such as one-time
  codes, chains of jump hosts (`--jump`), SFTP in the core.
- **Port forwarding:** `tephra forward` with local forwards (`-L`) and a SOCKS5 proxy (`-D`); listens on 127.0.0.1
  unless `--bind` says otherwise.
- **Host keys:** trust on first use, stored in the vault and synced; a changed key stops the connection until it is
  confirmed explicitly. The record IDs of known host keys are derived with the vault key and reveal nothing about the
  address. `tephra known-hosts list`, `tephra known-hosts forget`.
- **VPN:** per-host tunnel using the WireGuard protocol that runs inside the program, without a network adapter or
  administrator rights. Import and export in `wg-quick` format, key generation, handshake test, name resolution
  through the tunnel. `tephra vpn`.
- **Snippets:** stored commands with `{{placeholders}}`. `tephra snippet`.
- **Sync:** end-to-end encrypted sync with a self-hosted server. Concurrent changes are merged field by field; a
  deletion wins. Second devices join with user name and master password. Device list and removal, password change
  across devices. Records with a timestamp more than 24 hours in the future are refused; records too large for the
  server are skipped and reported instead of blocking the others. `tephra sync register`, `join`, `reconnect`, `now`,
  `status`, `devices`, `remove-device`, `disconnect`.
- **Sync server:** `tephra-server` with an SQLite database, registration open, by invite code or closed, quota per
  account, long polling for change notifications. Failed logins are counted per user name and sender (`--behind-proxy`
  for use behind a reverse proxy); correct logins are never slowed down. Invite codes are consumed atomically, small
  size limits on all routes except upload, clean shutdown on SIGINT and SIGTERM. `tephra-server serve`, `invite`,
  `accounts`, `delete-account`.
- **MCP:** local MCP server for language models. Clients with their own token, hosts shared per client, three access
  levels (`read-only`, `confirm`, `auto`), separate permissions for reading and writing files, rate and size limits.
  `tephra mcp`, `tephra daemon`.
  - `read-only` allows a fixed set of inspecting programs, each with a fixed set of options, and refuses chaining,
    redirection, quotes, wildcards, input data and paths that look like credentials.
  - Confirmation in the terminal shows the command, its input data and, for files, path, size, checksum and the
    beginning of the content. Control and invisible characters are made visible. "For this session" covers exactly
    the same action only.
  - Every action is logged before it runs and again when it has finished; without a log entry nothing runs. Attempts
    on hosts that are not shared are logged as well.
  - Kill switch and revocation end running sessions and connections within seconds.
  - Output is cleaned of the credentials used to reach the host; if that cannot be set up, nothing runs.
  - Bridge (`tephra mcp serve`) and daemon prove knowledge of the client token to each other without transmitting it.
- **App (Windows, Android):** graphical app built with Flutter on top of the same core. Create and unlock the vault with
  the recovery key shown once, manage hosts (identity, VPN profile, jump hosts, tags, favourites), identities, SSH keys
  (generate, import, copy the public key) and WireGuard profiles (import, handshake test). Terminal sessions in tabs
  that keep running in the background; questions about host keys, passwords and server prompts appear inside the
  session. Locking the vault ends all sessions. Dark design by default; a light one and following the system can be
  chosen. German and English, following the system language. On Windows it uses
  the same vault file as `tephra`. On Android the vault lives in the private data folder of the app and is excluded from
  cloud backup and device transfer; a row of extra keys (Esc, Tab, Ctrl, arrows and common symbols) sits above the
  on-screen keyboard.
- **Downloads:** every release carries the app, command line and sync server for Windows, statically linked command
  line and sync server for Linux, and the signed Android app, each with a SHA-256 checksum.
- **Background service:** `tephra daemon` picks up changes that other `tephra` commands make to the same vault file.
- **Deployment:** Dockerfile, Compose file with Caddy and a systemd unit for the sync server in `deploy/`.
- **Project:** CI on Linux and Windows (format, Clippy, tests, license and advisory check), manual, threat model,
  license assessment of all dependencies.
