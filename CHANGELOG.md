# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project uses
[Semantic Versioning](https://semver.org/).

## [0.12.0] – 2026-11-26

Security release after a review of the app and of syncing in a home network.

### Added

- **TLS for the sync server without manual work.** On first start the server creates its own certificate and logs its
  fingerprint ("TLS-Fingerabdruck"). With `--tls auto` (default) it speaks HTTPS and HTTP on the same port; `--tls only`
  closes plain HTTP, `--tls off` is for running behind a proxy with TLS. Your own certificate: `--tls-cert`/`--tls-key`.
- **Pinned server certificate on the devices.** A device remembers the fingerprint on first contact, like SSH a host
  key, and afterwards only talks to exactly that server. Devices connected over `http://` switch to HTTPS by themselves
  as soon as the server offers it; entering an `http://` address of a server that can do HTTPS also ends up encrypted,
  without "Connect without encryption". A different certificate stops syncing until you accept it with
  `tephra sync trust` or "Check the fingerprint of the server" in the app. `tephra sync status` and the sync dialog
  show the fingerprint.
- **Destroyed server copies are repaired:** if the server holds a version of a record that cannot be decrypted (for
  example garbage uploaded with a stolen session token), a device with a valid version uploads it again.
- **After an update a device fetches everything once more**, so records of kinds an older version did not know (such as
  the MCP devices of 0.11) appear right away.

### Changed

- **Signing out another device needs the master password** (CLI, app and server). A captured session token is no
  longer enough to throw out your devices. Older apps can no longer sign out other devices against a 0.12 server.
- **MCP questions cannot hide a command any more:** runs of spaces and blank lines are shown as a note, commands with
  several lines are marked, and the app only allows once the text has been scrolled to its end. Commands or input
  longer than 20,000 characters are refused without asking instead of being shown cut off. The audit log keeps start,
  end, length and checksum of long commands.
- `deploy/docker-compose.yml` and the systemd unit set `TEPHRA_SERVER_TLS=off`, because Caddy provides TLS there.

## [0.11.0] – 2026-11-26

### Added

- **MCP remote management:** a device running `tephra daemon` – for example a Docker container without a screen – can
  agree once with `tephra mcp remote on` to have its MCP access managed from elsewhere. Its rules (kill switch,
  clients, hosts with access level and file rights, limits) then live in a synced entry of the vault, and any device
  with the vault edits them: in the app under **MCP → Other devices**, now also on the phone. New clients and their
  tokens are created there. Without consent on the device nothing changes: rules stay local and are never synced.
  `tephra mcp remote status` lists the devices, `tephra mcp remote off` withdraws the consent; removing a device in
  the app locks it until it agrees again.
- **Confirming on another device:** a remotely managed device sends its questions for the level "Confirm each" through
  the sync server to your other devices; the app shows them as the usual dialog with the name of the device, and the
  first answer counts (two minutes, then refused). Devices without a terminal can therefore use "Confirm each" now.
  On the phone questions arrive while the app is open or kept awake by open sessions.

### Changed

- The MCP section of the app is shown on the phone as well; with six sections the bottom bar labels only the selected
  one.

## [0.10.0] – 2026-11-26

### Added

- **Unlocking with your fingerprint (app on Android 9 and newer):** "Unlock with fingerprint" in the dialog "About
  Tephra". The key of the vault in the phone's key store is then bound to your fingerprint; Android releases it only
  after its fingerprint dialog, every time. At start the app asks for the finger by itself, after locking the button
  "Unlock with fingerprint" does. There is no fallback to the PIN of the phone: if the finger does not work, enter the
  master password; cancelling or too many attempts change nothing. If a fingerprint is added to or removed from the
  phone, the app asks for the master password once and switches the setting off.

### Changed

- A setting "Unlock without the master password" from 0.8.0 on Android is converted to the fingerprint after the next
  unlock; if you cancel the fingerprint dialog, it is switched off. Below Android 9 it works as before.

## [0.9.0] – 2026-11-25

### Added

- **Sessions that survive a dropped connection:** a new switch "Keep the session on the host (tmux)" in the host
  editor. The terminal then runs in tmux on the host (one tmux session `tephra-<n>` per tab). If the connection
  drops, everything keeps running there – Claude Code, builds, long commands – and the app reattaches to the same
  session. Swiping in the terminal scrolls through its history. Without tmux on the host the app says so and opens
  the normal shell. `tephra ssh` attaches to `tephra-1` of such a host.
- **Reconnecting by itself:** if an open session loses its connection, the app reconnects with growing pauses (from
  one second up to half a minute, for about eight minutes), and the status bar shows when it tries next and offers
  "Connect now". On Android a switch between Wi-Fi and mobile data, or the network coming back, starts the next try
  at once; when the app comes back to the foreground it checks its connections and replaces dead ones within a
  few seconds instead of waiting for the keepalive.
- **Command after login** in the host editor of the app (the vault field already existed for the command line).

### Changed

- SSH keepalives every 15 instead of 30 seconds, so a dead connection is noticed sooner.

## [0.8.0] – 2026-11-25

### Added

- **Unlocking without the master password (app on Windows and Android):** "Unlock without the master password" in the
  dialog "About Tephra" switches it on for that device. On Windows the key of the vault is then kept encrypted with
  your Windows account, on Android in the phone's key store, usable only while the phone is unlocked. At start the app
  opens the vault by itself; after locking, one button unlocks it again. The master password stays valid, and changing
  it on another device does not interfere. The setting belongs to the device and is not synced; if unlocking fails,
  the app switches it off on that device and asks for the password.

## [0.7.0] – 2026-11-25

### Added

- **Copying from the terminal on the phone:** touching and holding selects a word, holding and dragging extends the
  selection. A bar above the terminal then copies it to the clipboard or selects everything, including what has
  already scrolled out of view. A new key "Paste" in the row of extra keys inserts the clipboard into the terminal.

## [0.6.0] – 2026-11-24

### Added

- **MCP: the model chooses how long a command may run.** Up to a limit you set – in the app under "Longest run time of a
  command", on the command line with `tephra mcp limits --max-timeout <seconds>`, between one minute and one day. Without
  a setting the limit is one hour instead of the fixed ten minutes so far; without a choice of the model a command still
  stops after 60 seconds. `list_hosts` tells the model the limits, the result of every command says which run time
  applied, and a command that was stopped says how much would have been possible.
- **MCP: signs of life for long calls.** While a command runs or waits for your confirmation, Tephra tells the client
  every 15 seconds that it is still working. Clients that restart their own time limit on such a message no longer give
  up on long commands after a minute.

### Changed

- **MCP:** if the client gives up on a call, the command on the host ends as well and the log notes it, instead of the
  command running on unnoticed.

## [0.5.0] – 2026-11-24

### Added

- **MCP in the app (Windows, Linux):** a section "MCP" switches on the service for language models. It runs while the
  vault is open and starts again by itself after unlocking, also after the next start of the app – `tephra daemon` is
  no longer needed for that. Requests for confirmation appear as a dialog on top of the app with the same information
  as in the terminal; the buttons that allow something only react after a short moment, refusing works at once, and on
  Windows the app flashes in the taskbar while a question waits. The section also adds clients (the token is shown once
  together with the entry for the client's configuration), gives and withdraws access per host with access level,
  patterns and file access, blocks a client, holds the kill switch and shows the log.
- **`tephra daemon` on a Linux server without login:** `TEPHRA_PASSWORD_FILE` names a file with the master password; on
  Linux it must be readable by the user only, otherwise it is refused. Without a terminal, requests for confirmation are
  refused and noted in the log instead of waiting for an answer that cannot come, and SIGTERM ends the daemon as cleanly
  as Ctrl+C. `deploy/tephra-daemon.service` is a systemd user unit for it.

### Changed

- **Command line:** the local channel between `tephra mcp serve` and the service now lives in the library, so that the
  app and `tephra daemon` share it. Only one of them can serve the same vault at a time.

## [0.4.0] – 2026-11-24

### Added

- **Sync with a server in your own network without TLS:** an `http://` address that is not the own machine is now
  accepted if you allow it explicitly – in the app with the checkbox "Connect without encryption" that appears below
  such an address, on the command line with `--allow-http` for `tephra sync register` and `tephra sync join`. Without
  that permission nothing changes: such an address is refused before anything is sent. The content of the vault stays
  encrypted; the login proof and the session tokens are readable for whoever listens in on that network, so this is
  meant for a server that can only be reached at home or through a VPN. The app shows "Unencrypted connection" for
  such a connection. The manual describes how to run the server without a proxy.

### Security

- The library `url` is updated to 2.5.9, which fixes RUSTSEC-2026-0592 (a crash on deeply nested `blob:` addresses).

## [0.3.1] – 2026-11-19

### Fixed

- **Terminal on Windows and Linux:** Ctrl+Insert copies the selection; until now the key went to the host instead.
  Shift+Insert pastes, also with the Insert key of the number pad (Num Lock off). Ctrl+Shift+C and Ctrl+Shift+V work
  as before, and a right click copies the selection or pastes.

## [0.3.0] – 2026-11-19

### Added

- **Sync in the app:** the cloud symbol sets it up. On the first device the app creates the account on your sync
  server and uploads the vault; a further device chooses "Fetch an existing vault" on its first start, signs in with
  the master password and gets everything. While the vault is open the app keeps syncing: changes made here are
  uploaded within seconds, changes from other devices appear in the lists by themselves. The dialog shows the devices
  of the account, removes one, signs in again after the password was changed elsewhere, and disconnects. On Android
  the connection to the server trusts the certificates of the system and those you installed yourself.
- **File browser in the app (SFTP):** every session can switch between terminal and files. On the desktop this device
  is on the left and the host on the right; on the phone the system's own dialogs pick and save files. Create folders,
  rename, delete (folders with everything in them), upload and download single files with progress and cancelling. It
  uses the connection of the session, also through jump hosts and tunnels.
- **More keys on Android:** Alt, the function keys behind "Fn", further symbols, Del and Ins. Ctrl and Alt apply to
  the next key, a second tap locks them, a third releases them; they also combine with the keys of the bar. Arrows,
  paging and Del repeat while held.
- **The app updates itself:** once a day it asks GitHub whether there is a newer release (can be switched off under
  "About") and offers to install it – on Windows through the installer, on Android by handing the signed app to the
  system, as an AppImage by replacing its file. The zip and the Debian package only get a hint with a link. Every
  download is checked against the published SHA-256 checksum.
- **Install a key on a host:** "Install on a host" in the key list of the app and `tephra key install <key> <host>`
  add the public key to `~/.ssh/authorized_keys` of the user on that host. It is appended over SFTP; existing entries
  are never rewritten.
- **Key import:** keys in the old PEM format of OpenSSL that are protected by a passphrase (`RSA PRIVATE KEY` and
  `EC PRIVATE KEY` with AES-128, AES-192, AES-256 or Triple DES) can now be read. `TEPHRA_KEY_PASSPHRASE` supplies the
  passphrase to `tephra key import` in scripts.
- **Sessions stay connected in the background on Android:** while sessions are open, a service with a notification
  keeps the app in the foreground. Without it Android freezes an app a few seconds after you leave it – even for the
  system's own file dialog – and closes its connections. The first session asks for the permission to show the
  notification; the service works without it too.

### Changed

- **Terminal:** when the connection drops, the terminal says so instead of ending without a word.

- **Key import:** the error now says whether the passphrase is missing, whether it is wrong, or which format or
  cipher is not supported; a public key pasted by mistake is recognised. Imported keys are stored in OpenSSH format
  and without their passphrase – the vault encrypts them – so a later connection never fails on the original format.

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
