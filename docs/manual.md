# Manual

How to use the command line `tephra` and how to run the sync server `tephra-server`. German version:
[handbuch.md](handbuch.md).

The graphical app exists for Windows and Android so far, in a first version; this manual covers the command line. On
Windows both use the same vault. The messages and prompts of `tephra` are German at the moment – where a prompt matters,
this manual quotes it.

- [Installation](#installation)
- [The vault](#the-vault)
- [SSH keys](#ssh-keys)
- [Identities](#identities)
- [Hosts](#hosts)
- [Importing from OpenSSH](#importing-from-openssh)
- [Connecting](#connecting)
- [Port forwarding](#port-forwarding)
- [Known host keys](#known-host-keys)
- [VPN profiles](#vpn-profiles)
- [Snippets](#snippets)
- [Sync](#sync)
- [MCP: access for language models](#mcp-access-for-language-models)
- [Running the sync server](#running-the-sync-server)
- [Scripts and environment variables](#scripts-and-environment-variables)

## Installation

Prebuilt programs are on the [releases page](https://github.com/Poldion/Tephra/releases). On Windows the installer
(`…-setup.exe`) can put `tephra` on the `PATH`; on Ubuntu and Debian the package (`tephra_…_amd64.deb`) installs it to
`/usr/bin`. From the archives (`…-windows-x64.zip`, `…-linux-x64.tar.gz`) copy `tephra` to a folder on your `PATH`
yourself. Or build from source with Rust 1.90 or newer and a C compiler:

```bash
git clone https://github.com/Poldion/Tephra.git
cd Tephra
cargo build --release
```

This produces `target/release/tephra` and `target/release/tephra-server` (`tephra.exe` and `tephra-server.exe` on
Windows). Copy `tephra` to a folder on your `PATH`. `tephra --help` and `tephra <command> --help` list all commands and
options.

## The vault

Everything `tephra` knows is stored in one encrypted file, the vault.

```bash
tephra init
```

asks for a new master password (at least 8 characters, typed twice), creates the vault and shows the **recovery key**:

```
Wiederherstellungsschlüssel – bitte jetzt aufschreiben und sicher verwahren:

    7K2M-9QXA-…
```

Write it down now. It is stored nowhere and never shown again. Without master password and recovery key the data is
lost.

| Command | What it does |
|---|---|
| `tephra init` | Creates a new vault |
| `tephra passwd` | Changes the master password |
| `tephra recover` | Sets a new master password with the recovery key |
| `tephra export <file>` | Writes an encrypted backup of the vault |

**Where the vault is.** By default `vault.tephra` in the data folder of your user: `%APPDATA%\Tephra\data`
on Windows, `~/.local/share/tephra` on Linux. Another file is chosen with `--vault <path>` or the
environment variable `TEPHRA_VAULT`; this works with every command:

```bash
tephra --vault ~/work.tephra host list
```

**Unlocking.** Every command asks for the master password (`Master-Passwort:`) and locks the vault again when it
ends. Only `tephra daemon` keeps it unlocked.

**Unlocking without the master password (app, Windows and Android).** In the dialog "About Tephra", "Unlock without the
master password" switches this on for that device. The app then keeps the key of the vault protected on this device:
on Windows encrypted with your Windows account (data protection API, file `vault.tephra.device-unlock` next to the
vault), on Android in the phone's key store, usable only while the phone is unlocked. At start the app then opens the
vault by itself; after locking, the button "Unlock without the password" is enough. The master password stays valid,
and changing it on another device does not interfere, because the key of the vault stays the same. Every device decides
for itself; the setting is not synced. On this device the protection of the vault then depends on your Windows account
or on the lock of the phone: whoever gets past that gets to all credentials. If unlocking fails once – for example
because the vault was replaced or the lock of the phone was removed – the app switches it off on this device and asks
for the password again. Not available on Linux yet.

**With fingerprint (Android 9 and newer).** On these phones the switch is called "Unlock with fingerprint". The key in
the phone's key store is then bound to your fingerprint: Android releases it only after the system's fingerprint
dialog, every time, and holding the phone unlocked is not enough. There is deliberately no fallback to the PIN of the
phone; if the finger does not work, enter the master password. Cancelling or too many attempts change nothing. If a
fingerprint is added to or removed from the phone, Android makes the key unusable; the app then asks for the master
password once and switches the setting off. A setting from version 0.8.0 (without fingerprint) is converted after the
next unlock: the app asks for the fingerprint once; if you cancel, the setting is switched off.

**Backup.** `tephra export backup.tephra` writes a copy that is still encrypted with the same master password and
recovery key. It contains your hosts, identities, keys, VPN profiles, snippets and known host keys, but nothing that
belongs to one device: no keys marked `--local-only`, no sync session, no MCP clients or grants, no audit log. To
restore, use the file as a vault: copy it to the default location or pass it with `--vault`.

**Recovering.** `tephra recover` asks for the recovery key (upper and lower case, dashes and spaces do not matter) and
then for a new master password. The recovery key stays valid afterwards.

## SSH keys

```bash
tephra key generate laptop                       # new Ed25519 key pair, prints the public key
tephra key import old ~/.ssh/id_ed25519          # existing private key; asks for its passphrase if it has one
tephra key list
tephra key show laptop                           # public key for authorized_keys on the server
tephra key install laptop web                    # adds the public key to ~/.ssh/authorized_keys on the host "web"
tephra key rm old
```

Imported keys may be in OpenSSH, PKCS#8 or PuTTY format or in the old PEM format of OpenSSL (`RSA PRIVATE KEY`,
`EC PRIVATE KEY`), also when that one is protected by a passphrase with AES or Triple DES. `tephra` asks for the
passphrase once and stores the key in OpenSSH format without it – the vault encrypts it – so you are not asked again.
If the import fails, the message says whether the passphrase is missing or wrong or which format is not supported.

`tephra key install` signs in to the host with its current identity and appends the public key to
`~/.ssh/authorized_keys` of that user; entries that are already there stay untouched, and a key that is already listed
is not added twice. Afterwards you can give the host an identity that uses the key.

`--local-only` on `generate` or `import` keeps a key on this device: it is never sent to the sync server and never
reaches your other devices.

```bash
tephra key generate workstation --local-only
```

## Identities

An identity is a user name plus the way to log in. Several hosts can share one.

```bash
tephra identity add admin --user admin --key laptop      # log in with the SSH key "laptop"
tephra identity add deploy --user deploy                 # asks for a password and stores it in the vault
tephra identity add root --user root --ask               # stores no password, asks on every connection
tephra identity list
tephra identity rm deploy
```

## Hosts

```bash
tephra host add web 192.0.2.10 --identity admin
tephra host add db db.example.org --port 2222 --identity admin --tag prod --tag database
tephra host list
tephra host rm db
```

| Option | Meaning |
|---|---|
| `--port <port>` | SSH port, default 22 |
| `--identity <name>` | Identity used to log in. Without one, no login is possible |
| `--vpn <name>` | VPN profile the connection goes through |
| `--jump <name>` | Jump host; may be given several times, in connection order |
| `--tag <tag>` | Tag; may be given several times |

`tephra host list` shows label, user, address and port, and marks hosts that use a VPN profile (`[VPN]`) or jump hosts
(`[Jump-Host]`).

Everything is addressed by its label, ignoring case. Labels must be unique within their kind. There is no command to
edit an entry yet: remove it and add it again. A host that is added again is a new host – MCP grants for the old one
no longer apply.

**Jump hosts.** A host that is only reachable through another one:

```bash
tephra host add bastion bastion.example.org --identity admin
tephra host add inner 10.0.0.7 --identity admin --jump bastion
tephra ssh inner                                 # connects to bastion first, then through it to inner
```

A jump host can have jump hosts of its own. Each one in the chain logs in with its own identity and has its host key
checked.

## Importing from OpenSSH

```bash
tephra import-ssh-config                         # reads ~/.ssh/config
tephra import-ssh-config ./other-config
```

For every `Host` entry without wildcards, `tephra` creates a host with a matching identity:

| In the configuration | Becomes |
|---|---|
| `Host web` | Host with the label `web` |
| `HostName`, `Port` | Address and port; without `HostName` the label is the address |
| `User` | User name of the identity; without `User`, the name you are logged in with |
| `IdentityFile` | The private key is read into the vault and used by the identity |
| `ProxyJump` | Jump hosts, if they are named as entries of the same file or as hosts already in the vault; otherwise `tephra` reports it and sets none |

- Blocks with wildcards (`Host *`, `Host *.example.org`) supply defaults for the entries they match, as in OpenSSH, but
  do not create hosts themselves. `Match` blocks and `Include` are not evaluated; all other settings are ignored.
- A key protected by a passphrase is not imported here. `tephra` says so; read it in with `tephra key import` and
  create the identity yourself. Until then the identity of such a host asks for a password.
- Imported keys are ordinary keys in the vault and are synced. The files in `~/.ssh` stay untouched.
- Hosts whose label already exists are left alone and listed. Running the import twice does no harm.

## Connecting

```bash
tephra ssh web                                   # interactive session
tephra exec web -- df -h                         # one command; output and exit code are passed on
```

Put `--` before the command so that its options are not taken for options of `tephra`. `tephra exec` returns the exit
code of the remote command; `tephra ssh` returns the exit code of the shell.

**First connection.** If the host is not known yet, `tephra` shows the fingerprint of its key and asks:

```
Der Host 192.0.2.10:22 ist noch nicht bekannt.
  ssh-ed25519 SHA256:…
Diesem Schlüssel vertrauen? [j/N]
```

Compare the fingerprint with the one on the server (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`) and answer
`j` (or `y`). The key is stored in the vault and synced to your other devices.

**Changed key.** If a known host presents another key, `tephra` warns that this may be an attack and continues only
after you type the word `geändert`. Do that only if you know the server's key was renewed.

**One-time codes.** If the server asks questions during login (keyboard-interactive, for example a one-time code),
they appear in the terminal.

Not available yet: login through an SSH agent and file transfer from the command line.

## Port forwarding

`tephra forward` connects to a host and forwards ports through it until you stop it with Ctrl+C.

```bash
tephra forward db -L 5432:localhost:5432         # local port 5432 → port 5432 on the host itself
tephra forward bastion -L 8080:10.0.0.7:80       # local port 8080 → a web server behind the host
tephra forward bastion -D 1080                   # SOCKS5 proxy on port 1080; targets are reached from the host
tephra forward bastion -L 8080:10.0.0.7:80 -L 8443:10.0.0.7:443 -D 1080
```

| Option | Meaning |
|---|---|
| `-L PORT:TARGET:TARGETPORT` | Local port to a target as seen from the host. May be given several times |
| `-D PORT` | Local SOCKS5 proxy on this port. May be given several times |
| `--bind <address>` | Address to listen on, default `127.0.0.1` |

- The connection is built like any other: through the VPN profile and the jump hosts of the host.
- With `-D`, names are resolved by the host, not by your computer. The proxy supports `CONNECT` without
  authentication.
- **Everyone on your computer can use the forward** while it runs. With `--bind 0.0.0.0` or another address of your
  machine, everyone in your local network can as well – only do that on purpose.
- Remote forwarding (a port on the host leading back to you) is not available.

## Known host keys

```bash
tephra known-hosts list
tephra known-hosts forget 192.0.2.10             # the next connection asks again
```

Keys of hosts reached through a VPN profile are kept apart (marked `(über VPN)`), because the same private address can
belong to different machines in different networks.

## VPN profiles

A VPN profile describes a tunnel using the WireGuard protocol. The tunnel runs inside `tephra`: it creates no network
adapter, needs no administrator rights and carries only the connections of `tephra`. Other programs do not see it.

```bash
tephra vpn import office office.conf             # configuration in wg-quick format
tephra vpn list
tephra vpn test office                           # builds the tunnel and reports whether the handshake succeeds
tephra vpn export office                         # prints the profile in wg-quick format, including the private key
tephra vpn keygen                                # prints a new key pair, stores nothing
tephra vpn rm office
```

From the configuration file `tephra` reads `PrivateKey`, `Address`, `DNS` and `MTU` in `[Interface]` and `PublicKey`,
`PresharedKey`, `Endpoint`, `AllowedIPs` and `PersistentKeepalive` in each `[Peer]`. Entries that only make sense for a
system-wide VPN (`PostUp`, `Table`, `FwMark` …) are ignored; `tephra` never runs commands from a profile.

Assign the profile to a host:

```bash
tephra host add nas 10.8.0.5 --identity admin --vpn office
tephra ssh nas                                   # starts the tunnel, then connects through it
```

- The tunnel starts with the first connection that needs it. Several profiles can be in use at the same time.
- If the host is given by name, the name is resolved through the tunnel by the DNS servers of the profile. Without
  `DNS` in the profile, use an address.
- The address of the host must lie within the `AllowedIPs` of a peer.
- To set up a new peer: `tephra vpn keygen`, enter the public key on the other side, write the private key into a
  configuration file and import it.

## Snippets

A snippet is a stored command. Placeholders are written as `{{name}}`.

```bash
tephra snippet add restart "systemctl restart {{service}}"
tephra snippet list
tephra snippet rm restart
```

Snippets are synced and can be run by language models through MCP (tool `run_snippet`), where the values for the
placeholders are inserted as literal shell arguments. Running a snippet from the command line is not available yet.

## Sync

Sync is optional. The vault works without it; with it, all your devices have the same hosts, identities, keys, VPN
profiles, snippets and known host keys. You need a sync server – see [Running the sync
server](#running-the-sync-server).

**First device.** The vault already exists; create an account for it on the server:

```bash
tephra sync register https://sync.example.org alice --invite 4f7c…
```

The user name has 3 to 64 characters (letters, digits and `. - _ @ +`). The invite code comes from whoever runs the
server; on a server with open registration leave `--invite` out. The master password of the vault becomes the
password of the account. Everything in the vault is uploaded, encrypted.

**Further devices.** On a device without a vault:

```bash
tephra sync join https://sync.example.org alice
```

asks for the master password of the account, creates the vault and fetches everything.

| Command | What it does |
|---|---|
| `tephra sync now` | Syncs once: fetch, merge, upload |
| `tephra sync status` | Shows the server and user name this device is connected to |
| `tephra sync devices` | Lists the devices of the account with their IDs |
| `tephra sync remove-device <id>` | Removes a device from the account |
| `tephra sync reconnect` | Logs this device in again (expired session, changed password) |
| `tephra sync disconnect` | Disconnects this device from the server; the local vault stays |

`tephra sync now` syncs once. For continuous sync leave `tephra daemon` running: it uploads local changes within seconds
and is notified by the server when another device changes something.

**In the app.** The cloud symbol opens the same thing. On the first device enter the server address, a user name, the
invite code and the master password of the vault and choose "Create account". A further device must not have a vault
yet: on its first start choose "Fetch an existing vault" and sign in with the server address, the user name and the
master password. While the vault is open the app syncs continuously, like `tephra daemon`; its dialog lists the
devices of the account, removes one, signs this device in again and disconnects it. A phone that already has a vault
of its own cannot join an account – remove the app's data first (or reinstall it) and fetch the vault then. On
Android the app trusts the certificates of the system and those you installed yourself, so a server behind your own
certificate authority works once its root certificate is installed on the phone.

Good to know:

- **Addresses start with `https://`.** Plain `http://` is accepted only for `localhost` – and for a server in your own
  network if you allow it explicitly: with `--allow-http` for `tephra sync register` and `tephra sync join`, in the app
  with the checkbox "Connect without encryption" that appears below such an address. The permission holds for this
  connection until you disconnect it. What you give up is described under "Running the sync server".
- **Conflicts.** If two devices change the same entry, the changes are merged field by field; for the same field the
  later change wins. A deletion wins over a concurrent change.
- **Not synced:** keys marked `--local-only`, MCP clients, grants and the audit log, and the sync session itself.
- **Changing the master password** with `tephra passwd` changes it for the account too. Other devices are logged out and
  need `tephra sync reconnect` with the new password.
- **After `tephra recover`** the server still knows only the old password, and without it the password cannot be changed
  there. This device keeps syncing as long as its session is valid, but no device can log in any more. To get back to
  a working state: `tephra sync disconnect`, then create a new account with `tephra sync register`.
- **Records the server would refuse as too large** are skipped and reported by `tephra sync now`; everything else is
  synced. A record dated more than 24 hours in the future is not accepted from the server – check the clocks of your
  devices if records are reported as unreadable.
- **A device that has not synced for 60 days** is logged out; `tephra sync reconnect` logs it in again.
- **Removing a device** stops its sync at once. What it has already fetched stays readable there – if the device was
  lost, renew the passwords and keys of your hosts.
- **Device names** come from the environment variable `COMPUTERNAME` or `HOSTNAME`. If neither is set, the device is
  called `Kommandozeile`.
- **Limits of the server:** 1 MiB per record, by default 50,000 records and 256 MiB per account.

## MCP: access for language models

Through the Model Context Protocol a language model can run commands on your hosts – on the hosts you share with it,
within the limits you set. Read the section on language models in the [threat model](threat-model.md) first.

How the parts fit together:

```
MCP client (starts) ──► tephra mcp serve ──local channel──► tephra daemon ──SSH──► host
                        thin bridge,                    vault unlocked,
                        knows only its token            checks every call, asks you
```

### Step by step

**1. Create a client.** Each program that connects gets its own name and token.

```bash
tephra mcp add-client desktop
```

The token (`tephramcp_…`) is shown once. The vault keeps only a hash of it.

**2. Share hosts.** Nothing is shared at first.

```bash
tephra mcp grant desktop web --level read-only
tephra mcp grant desktop db                              # level confirm is the default
tephra mcp grant desktop build --level confirm --allow "docker ps*" --files-read
```

**3. Start the daemon** in a terminal you can see and leave it running:

```bash
tephra daemon
```

It asks for the master password, keeps the vault unlocked and waits. Requests for confirmation appear here. Stop it
with Ctrl+C; that locks the vault and closes all connections and tunnels.

Instead of `tephra daemon` the app can provide the service as well (see "In the app"). Only one of them runs for the
same vault; the second one reports on start that the service is already running.

**4. Enter the server in your MCP client.** The client must start the command `tephra mcp serve` with the environment
variable `TEPHRA_MCP_TOKEN`. In the common JSON format:

```json
{
  "mcpServers": {
    "tephra": {
      "command": "tephra",
      "args": ["mcp", "serve"],
      "env": { "TEPHRA_MCP_TOKEN": "tephramcp_…" }
    }
  }
}
```

If `tephra` is not on the `PATH` of the client, give the full path (`C:\\Tools\\tephra.exe`). If you use a vault other
than the default one, add `"TEPHRA_VAULT": "<path>"` to `env` – bridge and daemon find each other through the path of
the vault, so both must use the same one.

**5. Check.** The model should now see the tool `list_hosts` and the hosts you shared. `tephra mcp audit` shows what it
did.

### Access levels

| Level | Commands | Interactive sessions | Files |
|---|---|---|---|
| `read-only` | Only inspecting programs from a fixed list, with fixed options | No | Reading with `--files-read`; never writing |
| `confirm` | After your confirmation; patterns from `--allow` without asking | After confirmation when opened | With `--files-read` / `--files-write`, each access after confirmation |
| `auto` | Without asking, unless on the deny list | Yes; every line typed is checked against the deny list | With `--files-read` / `--files-write`, without asking |

Granting a host again replaces the previous grant.

#### read-only

A command is split into words and checked word by word. Allowed are these programs, each only with options that
change nothing, start nothing and do not run forever:

| Area | Programs |
|---|---|
| Files | `ls`, `cat`, `head`, `tail`, `wc`, `stat`, `file`, `readlink`, `realpath`, `tree`, `grep`, `du`, `df`, `sha256sum`, `md5sum`, `which`, `echo`, `pwd` |
| System | `uname`, `hostname`, `uptime`, `date`, `nproc`, `lscpu`, `free`, `lsblk`, `findmnt`, `mount` (without arguments), `dmesg`, `timedatectl`, `hostnamectl` |
| Users | `whoami`, `id`, `w`, `who`, `last`, `getent`, `crontab -l` |
| Processes and services | `ps`, `top -bn1`, `systemctl` with `status`, `is-active`, `is-enabled`, `is-failed`, `list-units`, `list-unit-files`, `list-timers`, `list-sockets`, `journalctl` |
| Network | `ip` (only showing `addr`, `route`, `link`, `neigh`, `rule`), `ss`, `netstat` |
| Software | `docker` with `ps`, `logs`, `images`; `dpkg -l/-L/-s/-S`, `rpm -q…`, `apt` with `list`, `policy`, `show`, `search`; `nginx -t/-T/-v/-V` |

Always refused:

- chaining, redirection and substitution: `; & | > < $ ( ) { }`, backticks, backslashes, line breaks
- quotes, the wildcards `*`, `?`, `[ ]` and `~` – the shell would form words from them that look different here than
  when they run
- options that are not on the list, for example `tail -f`, `journalctl --vacuum-size`, `date -s`
- input data (the `stdin` of `ssh_exec`) and interactive sessions
- paths that look like credentials, among them `.ssh`, `id_rsa`, `authorized_keys`, `shadow`, `sudoers`, `.env`,
  `.pem`, `.key`, `.aws`, `.kube`, `.gnupg`, `.netrc`, `/proc/`, and anything containing `passw`, `secret`, `token`,
  `credential` or `private`

"Read only" covers everything the SSH user is allowed to read. The path block is a hurdle against accidents; it does
not stop a model that searches on purpose. If that matters, share the host through a user with few rights.

With `--allow` you give your own patterns instead. **They replace the built-in check completely**: allowed is then
whatever matches a pattern, without the check of individual options and without the path block. Only chaining,
redirection and input data stay forbidden. `tephra` prints a warning when you do this.

```bash
tephra mcp grant desktop web --level read-only --allow "systemctl status *" --allow "df -h"
```

#### confirm

Every command needs your yes. Commands matching an `--allow` pattern run without asking, as long as they contain no
chaining or redirection.

#### auto

Commands run without asking unless they match the built-in deny list or one of your `--deny` patterns; your patterns
are added to the built-in ones. Built in are, among others, `rm -rf /`, `mkfs`, `dd … of=/dev/…`, `shutdown`, `reboot`,
`poweroff`, `passwd`, `userdel`, `visudo`, `iptables -F`, `ufw disable` and stopping or disabling SSH. `tephra` prints a
warning when you grant `auto`: the deny list prevents accidents, not deliberate misuse. In an interactive session it
is checked line by line and can be defeated simply by splitting an input.

**Patterns.** In `--allow` and `--deny`, `*` stands for any characters; the pattern must match the whole command. Both
options may be given several times.

### Confirming

With level `confirm`, each request appears in the terminal of `tephra daemon`:

```
── Anfrage von „desktop“ für Host „db“ ──
  Befehl ausführen:
    │ systemctl restart postgresql
  Erlauben? [j] einmal · [s] genau dies für diese Sitzung · [N] ablehnen:
```

Everything that comes from the model is shown in the lines marked with `│`:

- for a command, the command and – if any – its input data with their size
- for a file to be written, the path, the size, the beginning of the SHA-256 checksum and the beginning of the content
- for a file to be read, the path
- for an interactive session, a warning that all input of the model in this session is free afterwards and is no
  longer asked about one by one

Control characters, invisible characters and characters that change the reading direction appear as visible codes
such as `\x1b` or `\u{202e}`, so a request cannot overwrite itself on the screen. Text longer than 2,000 characters
(400 for file contents) is cut with an explicit note.

`j` (or `y`) allows it once. `s` allows exactly this action for the rest of this MCP connection: the same command with
the same input data, or the same file with the same content. Anything else, or no answer within two minutes, declines.
What you typed before the question appeared does not count. The same question appears before an unknown host key is
trusted; a changed host key is always refused for MCP.

The daemon needs a terminal to ask. If it runs without one, every request for confirmation is declined and noted in
the daemon's log – only `read-only` and `auto` hosts are usable then.

### In the app

On Windows and Linux the app has a section "MCP" of its own. Switch the service on there; it then runs while the vault
is open and starts again by itself after unlocking – also after the next start of the app. You do not need
`tephra daemon` for that.

- **Requests for confirmation** appear as a dialog on top of the app, whichever section is open. What is shown in the
  grey box comes from the model; control and invisible characters are made visible there as in the terminal. "Allow
  once" and "Exactly this for this connection" can only be pressed after a short moment, so that a click already on
  its way does not count as an answer; "Refuse" works at once. Without an answer within two minutes the request is
  refused. On Windows the app flashes in the taskbar while a question waits and the window is in the background.
- **Clients** are added there. The token is shown once, together with the ready-made entry for the configuration of
  the client.
- **Access** is given per client and host, with access level, patterns of your own and file access – as with
  `tephra mcp grant`. A client can be blocked without losing its hosts.
- The **kill switch** and the **log** are there as well.

The section and the service change the same permissions as `tephra mcp`; what you change in one place applies in the
other.

### On a Linux server without login

[`deploy/tephra-daemon.service`](../deploy/tephra-daemon.service) starts `tephra daemon` as a service of the user,
already at boot and without anyone logging in. MCP clients of the same user connect with `tephra mcp serve` as usual,
and the vault is kept in sync if the device is connected to a sync server.

```bash
mkdir -p ~/.config/tephra ~/.config/systemd/user
(umask 077; read -rs -p "Master password: " pw && printf '%s' "$pw" > ~/.config/tephra/master-password)
install -m 0644 deploy/tephra-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now tephra-daemon
sudo loginctl enable-linger "$USER"
```

The service reads the master password from the file named by `TEPHRA_PASSWORD_FILE`. Only the user may be able to read
it; otherwise `tephra` refuses it. Whoever can read it – that user and root – can open the vault, so give this vault only
the hosts and keys the server really needs. Without a terminal nobody can be asked: hosts with `read-only` and `auto`
are usable, refused requests are in the log (`journalctl --user -u tephra-daemon`). `systemctl --user stop
tephra-daemon` ends the service cleanly and locks the vault.

### What the model gets

| Tool | Purpose |
|---|---|
| `list_hosts` | Hosts shared with this client: ID, label, address, tags, access level. Never credentials |
| `ssh_exec` | Runs one command; returns exit code, stdout and stderr |
| `read_output` | Continues reading an output that was cut off |
| `list_snippets`, `run_snippet` | Lists snippets (label, description, placeholders) and runs one on a host, under the rules of that host |
| `ssh_session_open`, `ssh_session_send`, `ssh_session_read`, `ssh_session_close` | Interactive terminal session for programs that need one |
| `tunnel_status` | Whether a host is reached through a tunnel and the tunnel's state |
| `files_list`, `files_read`, `files_write` | Directory listing, reading and writing text files over SFTP |

Limits: 120 calls per minute and client, 4 open sessions per connection, 2 MiB of output per command and stream, 1 MiB
per file. Long output is delivered in pages of 20,000 characters.

**Run time of commands.** The model chooses for each command how long it may run (`timeout_secs`); without a choice a
command stops after 60 seconds. How long it may choose at most is up to you – in the app's section "MCP" under "Longest
run time of a command" or with `tephra mcp limits --max-timeout <seconds>`, between one minute and one day; without a
setting it is one hour. The model learns the limit from `list_hosts`, and a command that was stopped tells it how much
would have been possible.

While a command runs or waits for your confirmation, Tephra tells the client every 15 seconds that work is still going
on. Clients that restart their own time limit for tool calls with each such message therefore do not give up on long
commands early; whether yours does depends on the client. If the client gives up on a call, the command on the host ends
as well, and the log notes the cancellation.

**Redaction.** The credentials involved in reaching a host – its password, key and passphrase, those of its jump
hosts, its proxy password and the keys of its VPN profile – are replaced by `[redacted]` if they appear in the output
of that host or in a file read from it. If `tephra` cannot put this list together, the action is not run. Only literal
matches of six characters or more are found: not an encoded form, and not a secret that merely lives on the host.

### Managing access

```bash
tephra mcp clients                               # clients, their hosts and levels; whether MCP is switched on
tephra mcp revoke desktop db                     # withdraw one host
tephra mcp rm-client desktop                     # remove the client; its token becomes invalid
tephra mcp disable                               # kill switch
tephra mcp enable
tephra mcp audit                                 # the last 30 entries
tephra mcp audit -n 200
tephra mcp limits                                # how long a command may run
tephra mcp limits --max-timeout 7200             # the model may choose up to two hours
```

**Kill switch and revocation.** `tephra mcp disable` rejects every further call and, within a few seconds, ends all
interactive sessions and SSH connections of the daemon – commands that are still running end with them. Revoking a host
ends the sessions and the connection to that host; removing or disabling a client closes its sessions and the
connections it used; lowering a host to `read-only` closes the sessions on it. All of this works while the daemon is
running, from any other terminal.

**Audit log.** Every action is entered in the log before it runs. If the entry cannot be written, the action is not
run. When it has finished, a second entry follows. The log lists time (UTC), client, host, tool, result, exit code and
the command or path:

| Result | Meaning |
|---|---|
| `erlaubt` | Started, because the grant allows it without asking |
| `bestätigt` | Started after you confirmed it |
| `beendet` | A started action has finished; with exit code |
| `ABGELEHNT` | Refused by the rules – also an attempt on a host that is not shared |
| `VOM NUTZER ABGELEHNT` | You declined |
| `fehlgeschlagen` | Allowed, but failed, for example because the host could not be reached |

The log is stored encrypted in the vault of this device.

**Changes while the daemon runs.** The daemon notices what other `tephra` commands change in the same vault file – new
hosts, changed identities, grants – and uses it from the next call on. It does not need to be restarted.

**Bridge and daemon** prove to each other that they know the token of the client; the token itself never crosses the
channel. If something other than the daemon answers under the channel's name, `tephra mcp serve` breaks off.

Grants belong to one device. They are not synced: setting up MCP on one machine does not open your hosts on another.

## Running the sync server

`tephra-server` is a single program with a single SQLite file. It authenticates devices, stores encrypted records with
revision numbers and tells devices when there is something new. It cannot read what it stores.

```bash
tephra-server serve                              # listens on 127.0.0.1:8787, database tephra-server.sqlite in the current folder
tephra-server invite                             # prints an invite code for one new account
tephra-server accounts                           # accounts with number of devices, records and size
tephra-server delete-account alice               # deletes an account with all its data
```

| Option | Variable | Default | Meaning |
|---|---|---|---|
| `--db <file>` | `TEPHRA_SERVER_DB` | `tephra-server.sqlite` | Database file |
| `--bind <address>` | `TEPHRA_SERVER_BIND` | `127.0.0.1:8787` | Address the server listens on |
| `--registration <mode>` | `TEPHRA_SERVER_REGISTRATION` | `invite` | `open`: anyone may register; `invite`: only with an invite code; `closed`: nobody |
| `--max-items <n>` | `TEPHRA_SERVER_MAX_ITEMS` | `50000` | Records per account, deletion markers included |
| `--max-mib <n>` | `TEPHRA_SERVER_MAX_MIB` | `256` | Data per account in MiB |
| `--behind-proxy` | `TEPHRA_SERVER_BEHIND_PROXY` | off | The server is behind a reverse proxy that sets `X-Forwarded-For` |

The options apply with or without the command `serve`; `tephra-server` on its own starts the server.

**TLS.** The server speaks plain HTTP and is meant to listen on `127.0.0.1` behind a reverse proxy that provides TLS.
Clients refuse `http://` addresses other than `localhost` and check the certificate against the certificate store of
the operating system. Requests to `/v1/events` stay open for up to 55 seconds, so the proxy must not cut connections
off earlier. `GET /healthz` answers `ok`.

**In your own network without TLS.** If the server can only be reached in your own network – at home or through a
VPN – it can run without a proxy: with `--bind 0.0.0.0:8787` and without `--behind-proxy`. Clients then connect to
`http://<address>:8787` and need your explicit permission for that (`--allow-http`, in the app the checkbox "Connect
without encryption"). The content of the vault stays encrypted even so. Whoever listens in on that network does see
the login proof and the session tokens, though. With them they can sign in to the server as one of your devices, fetch
the encrypted records and try master passwords against them, remove devices and delete the account on the server.
Across the internet there always has to be TLS in front.

**Behind a proxy, set `--behind-proxy`.** Failed logins are limited per user name and sender: 10 within 5 minutes.
Without the option, every request seems to come from the proxy, and one person guessing passwords would slow down
everybody. With it, the sender is the last address in `X-Forwarded-For` – the one your proxy added. Do not set it if
the server is reachable directly; then anyone could put any address into that header. Correct logins are never slowed
down, so nobody can lock you out by trying wrong passwords for your name.

**Invite codes** are valid once. The server stores only their hash, like all session tokens.

**Stopping.** The server shuts down cleanly on Ctrl+C and on SIGTERM, as sent by systemd and Docker.

**Backup.** Stop the server and copy the database file together with the files ending in `-wal` and `-shm` next to
it. The backup contains ciphertext only, plus user names and device names.

### With Docker

The folder [`deploy/`](../deploy/) contains a `Dockerfile`, a Compose file with [Caddy](https://caddyserver.com) as TLS
proxy and a `Caddyfile`. The Compose file uses the published image `ghcr.io/poldion/tephra-server` (amd64 and arm64);
with `--build` it builds the image from the source instead.

1. Replace `sync.example.org` in `deploy/Caddyfile` with your own name. It must point to the machine, and ports 80 and
   443 must be reachable so that Caddy can obtain a certificate.
2. Start:

   ```bash
   cd deploy
   docker compose up -d
   ```

3. Create an invite code:

   ```bash
   docker compose exec sync tephra-server invite
   ```

The database lives in the volume `sync-data`. The server runs as an unprivileged user in a read-only container, with
`TEPHRA_SERVER_BEHIND_PROXY` set because Caddy is in front of it.
`docker compose exec sync tephra-server accounts` lists the accounts.

For a server in your own network only, without TLS (see above), drop the service `caddy`. The service `sync` then gets
`ports: ["8787:8787"]` instead of `expose`, and the line `TEPHRA_SERVER_BEHIND_PROXY` is removed.

### With systemd

[`deploy/tephra-server.service`](../deploy/tephra-server.service) runs the server as its own user with a hardened unit.

```bash
sudo install -m 0755 target/release/tephra-server /usr/local/bin/tephra-server
sudo useradd --system --user-group --home-dir /var/lib/tephra-server --shell /usr/sbin/nologin tephra-server
sudo install -m 0644 deploy/tephra-server.service /etc/systemd/system/tephra-server.service
sudo systemctl daemon-reload
sudo systemctl enable --now tephra-server
sudo -u tephra-server tephra-server --db /var/lib/tephra-server/tephra-server.sqlite invite
```

The database is `/var/lib/tephra-server/tephra-server.sqlite`. The server listens on `127.0.0.1:8787`; put Caddy, nginx
or another proxy with TLS in front of it. For Caddy, use the `Caddyfile` from `deploy/` with `127.0.0.1:8787` as the
target. The unit sets `TEPHRA_SERVER_BEHIND_PROXY`; your proxy must set `X-Forwarded-For` (Caddy does, for nginx add
`proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`).

## Scripts and environment variables

| Variable | Used by | Meaning |
|---|---|---|
| `TEPHRA_VAULT` | all commands | Path of the vault file |
| `TEPHRA_PASSWORD` | all commands that unlock | Master password instead of the prompt |
| `TEPHRA_PASSWORD_FILE` | all commands that unlock | File with the master password, for example for `tephra daemon` under systemd; on Linux readable by the user only |
| `TEPHRA_NEW_PASSWORD` | `tephra init`, `tephra passwd`, `tephra recover` | New master password instead of the prompt |
| `TEPHRA_IDENTITY_PASSWORD` | `tephra identity add` | Password to store instead of the prompt |
| `TEPHRA_KEY_PASSPHRASE` | `tephra key import` | Passphrase of the key instead of the prompt |
| `TEPHRA_MCP_TOKEN` | `tephra mcp serve` | Token of the MCP client |

```bash
export TEPHRA_PASSWORD='…'
tephra exec web -- uptime
tephra sync now
```

A password in an environment variable can be read by other programs of the same user and may end up in the history of
your shell. Use it for scripts and tests, not out of habit.

Prompts are written to standard error, so the output of a command can be redirected while you are still asked for the
password. Errors start with `Fehler:` and give exit code 1; `tephra exec` and `tephra ssh` return the exit code of the
remote side.
