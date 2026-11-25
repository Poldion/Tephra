# Threat model

What Tephra protects, against whom, and where the protection ends. This document describes the code as it is today – the
command line `tephra` and the sync server `tephra-server`. German version: [bedrohungsmodell.md](bedrohungsmodell.md).

The design has not been reviewed by a third party yet. It uses established primitives only (Argon2id, HKDF-SHA256,
XChaCha20-Poly1305) and no cryptography of its own, but that is no substitute for a review.

## What is being protected

| Asset | Where it lives |
|---|---|
| Passwords of identities, private SSH keys and their passphrases, VPN private and preshared keys, proxy passwords | In the vault, encrypted |
| Host list: names, addresses, user names, tags, snippets, known host keys | In the vault, encrypted |
| Master password and recovery key | In your head and on paper; never stored |
| Vault key | In memory while the vault is unlocked; on disk and on the server only wrapped |
| Sync session tokens, MCP client list and grants, MCP audit log | In the vault of one device, encrypted, never synced |
| MCP client token | Shown once; the vault keeps only its SHA-256. The clear text sits in the configuration of the MCP client |
| The hosts themselves | Reachable for whoever holds the credentials – or for a language model within its grants |

## How the vault is built

```
master password ──Argon2id (64 MiB, 3 passes, per-vault salt)──► master key
   master key ──HKDF-SHA256──► auth key   (proves knowledge of the password to the sync server)
   master key ──HKDF-SHA256──► KEK        (wraps the vault key, never leaves the device)
   vault key (random, 256 bit)             encrypts every record
   recovery key (random, 256 bit)          wraps the vault key a second time
```

- Every record is encrypted with XChaCha20-Poly1305 under the vault key with a random nonce. The vault ID and the
  record ID are authenticated along with it, so a ciphertext cannot be moved to another record or another vault.
- Before encryption the plain text is padded: to 256 bytes at least, then to the next power of two up to 64 KiB, then
  in steps of 16 KiB. The type of a record is inside the ciphertext.
- The vault header (salt, derivation parameters, the two wrapped copies of the vault key) is not encrypted, but
  authenticated with the vault key. A device rejects a header that was not produced with its vault key, and it
  rejects derivation parameters below a fixed minimum (19 MiB, 2 passes) or above a maximum.
- The header carries a counter that rises with every change of the master password and is covered by the
  authentication. A genuine but older header – one that still opens with a previous, possibly leaked password – is not
  accepted again.
- Record IDs are random. Records that must exist only once per name, such as the known key of a host, get an ID
  derived from that name with the vault key. Without the vault key nobody can tell which name is behind an ID.
- A record whose timestamp lies more than 24 hours in the future is refused. Otherwise one device with a wrong clock
  could decide every later conflict in its favour.
- The vault file contains exactly the ciphertexts the sync server gets. Unlocking decrypts all records into memory;
  locking discards them. Keys and secret strings are overwritten when they are dropped.
- Each command of `tephra` unlocks the vault for its own duration. `tephra daemon` keeps it unlocked until it is
  stopped.
- A backup written with `tephra export` contains no keys marked as "this device only", no MCP grants and no sync
  session.

**The vault key is never replaced.** Changing the master password or removing a device wraps the same key anew.
Whoever once had the vault key – a removed device, someone who knew an old password and kept a copy of the vault –
can still decrypt every ciphertext they get hold of.

## Attackers

### A compromised sync server

Someone controls the server, its database or its backups.

**They see:**

- the user name of the account, the names of the devices, when each was created and last seen, and – together with
  the reverse proxy – the network addresses the devices connect from
- the vault header: salt, derivation parameters and the wrapped vault key
- a hash of the auth key
- per record: its ID, a revision number, whether it is a deletion marker, and its padded size – and therefore how
  many records there are and when each one changes

**They cannot:**

- read any record, or learn its type, a host address, a user name or a key
- forge or modify a record or move a ciphertext to another record: that fails authentication on the device
- pass off a header of their own, or an older genuine one: the first fails authentication, the second the counter
- overwrite a newer value on a device that already has it: merging is done per field by timestamp on the device, so
  an old version sent again changes nothing there
- take over sessions from a database dump: session tokens and invite codes are stored as hashes

**They can, without any device noticing:**

- **withhold changes.** That includes deletion markers: a host or key you deleted on one device then stays on the
  others.
- **hand a new device an older state.** Older ciphertexts are genuine; a device that joins has nothing to compare
  them with.
- **pretend to have accepted an upload** and discard it. The uploading device believes the change is stored.
- deliver changes to some devices only, delay them, or delete everything.

There is no signed record of the vault's overall state that would reveal this. Keep a backup (`tephra export`).

**They can also:**

- **guess the master password offline.** The header contains everything needed to test a guess; each guess costs one
  Argon2id run. A long, unique master password is the only protection here. The same holds for the stored hash of the
  auth key.
- hand weak derivation parameters to a new device: the device refuses them.
- observe usage: when you sync, how many records you have, which sizes change.

The server limits failed logins per user name and sender; correct logins are never slowed down, so a third party
cannot lock you out with wrong passwords. Behind a reverse proxy this needs `--behind-proxy`, otherwise all requests
appear to come from the proxy.

### A stolen or lost device

**Vault locked** (no `tephra daemon` running): the thief has the vault file and can guess the master password offline,
as above. Without the master password or the recovery key the contents are not readable. Keys marked `--local-only`
exist only in this file.

**Vault unlocked** (`tephra daemon` running, session not locked): the keys are in memory and the daemon answers MCP
clients. Treat this like an unlocked password manager.

What to do: remove the device with `tephra sync remove-device`. Its session tokens stop working at once and it gets no
further changes. **What it has already fetched stays on it, and the vault key is not replaced.** If the master
password was weak or might be known, change the passwords and keys of the hosts themselves – changing the master
password does not help.

**Unlocking without the master password** (switched on in the app): on Windows the vault key, encrypted with the
Windows account, lies next to the vault. Whoever removes the disk additionally needs the password of that account;
whoever sits at the signed-in account or runs programs in it gets to the vault key. On Android it lies encrypted in the
app's private folder; the key for that sits in the phone's key store, never leaves it and only works while the phone
is unlocked. On these devices the sign-in to the device then protects the vault, no longer the master password.

Not there yet: biometrics or Windows Hello when unlocking, and automatic locking after a period of inactivity.

### A malicious or compromised SSH host

- **Impersonation.** Host keys are trusted on first use and stored in the vault. If a host presents a different key
  later, the connection stops; `tephra` continues only after the word `geändert` has been typed. Through MCP a changed
  key is always refused, and an unknown key needs your confirmation. The first connection to a host is as trustworthy as
  the network it is made over – compare the fingerprint.
- **Known keys are shared between your devices.** A key accepted on one device is trusted on all of them. Two devices
  that trust the same host independently end up with the same record; if their keys differ, the one stored later
  wins, and the other device sees "key changed" the next time it connects.
- **What the host gets.** With password login the host receives the password in clear text; that is how SSH password
  authentication works. Use keys for hosts you do not fully trust. Agent forwarding does not exist, so a host cannot
  use your keys for onward connections.
- **Terminal output.** `tephra ssh` passes the output of the host to your terminal unchanged, including control
  sequences, exactly like any SSH client.
- **Instructions for a language model.** A host can put text into command output or files that tries to steer a
  model. Tool descriptions mark all output as untrusted data, control sequences are removed from session output, and
  the credentials used to reach the host are blacked out. None of this makes a model immune. The access level decides
  what a misled model can do.

### A misguided language model

The model is treated as untrusted: it may be wrong, or it may follow instructions it found in the output of a host.

**In place:**

- Nothing is shared by default. Grants are per MCP client and per host, stored on this device only and never synced.
  A host that is not shared with a client behaves exactly like a host that does not exist; the attempt is logged.
- Credentials are never returned by any tool.
- `read-only`: a fixed list of inspecting programs, each with a fixed list of options that change nothing, start
  nothing and do not run forever. A command is split into words and checked word by word. Refused are chaining,
  redirection and substitution (`; & | > < $ ( ) { }`, backticks, backslashes, line breaks), quotes, wildcards and
  `~`, input data, interactive sessions, file writes, and paths that look like credentials (`.ssh`, `shadow`, `.env`,
  `.key`, `/proc/` and the like).
- `confirm`: every command, every session and every file access is shown in the terminal of `tephra daemon` and needs a
  yes, unless the command matches a pattern you allowed in advance. The request shows the command and its input
  data; for a file its path, size, checksum and the beginning of the content. Control characters, invisible
  characters and characters that change the reading direction are shown as visible codes, so that the display cannot
  be made to show something other than what will run. What was typed before the question appeared does not count as
  an answer; no answer within two minutes counts as no.
- `auto`: commands run unless they match the built-in deny list or one of your own patterns.
- Reading and writing files are separate permissions.
- Limits: calls per minute and client, parallel sessions, run time, output and file size.
- **Audit log.** Every action is logged before it runs; if the entry cannot be written, the action does not run. A
  second entry records its end. Refused calls are logged as well.
- **Kill switch and revocation.** `tephra mcp disable`, removing a client, revoking a host or locking the vault end
  running sessions and connections within a few seconds, not just at the next call.
- **Redaction.** Output is cleaned of the credentials involved in reaching that host – its own, those of its jump
  hosts, its proxy and its VPN profile. If the list cannot be put together, nothing is run.
- **Bridge and daemon** prove to each other that they know the client's token; the token itself is not sent. A
  program that merely occupies the name of the channel learns nothing it could use and gets no MCP data.

**Not protected:**

- **`auto` is not a sandbox.** A deny list catches `rm -rf /` typed in the obvious way and nothing that is meant to
  get past it. In an interactive session it is checked line by line and is defeated by simply splitting an input in
  two. It prevents accidents, not misuse.
- **`read-only` means "changes nothing", not "reveals nothing".** It covers everything the SSH user is allowed to
  read. Refusing paths that look like credentials is a hurdle against accidents, not protection from a model that
  searches on purpose. Share hosts through an SSH user with few rights.
- **Your own `--allow` patterns replace the built-in check of `read-only` completely** – the check of individual
  options and the path block included. Only the bans on chaining, redirection and input data remain. `*` matches
  anything.
- **A confirmed interactive session is open-ended.** It is confirmed once when it is opened; what is typed into it
  afterwards is not shown for confirmation. The request says so.
- **You confirm what you see.** Long commands and input data are shown up to 2,000 characters, file contents up to
  400; the rest is announced, not displayed. "For this session" then covers exactly the same action – the same command
  with the same input, or the same file with the same content.
- **Redaction finds literal matches only**, of six characters or more: not encoded forms (Base64, hex), not a secret
  that arrives split over two reads, and not secrets that merely live on the host.
- **Everything the model reads leaves your machine** and goes to whoever operates the model.

### Another user on the same machine

- The channel between `tephra mcp serve` and `tephra daemon` is a named pipe that refuses remote clients (Windows) or a
  Unix socket inside a directory that only its owner can enter (Linux). A connection is served only after the mutual
  proof described above.
- The vault file is encrypted regardless of file permissions.
- **Port forwards are open to the whole machine.** While `tephra forward` runs, every program and every user on the
  computer can use the forwarded port or the SOCKS5 proxy; there is no authentication. They listen on 127.0.0.1 by
  default; `--bind` with another address opens them to the local network as well.
- Not protected: an administrator or root, who can read the memory of the running daemon. A token passed in
  `TEPHRA_MCP_TOKEN` or a password passed in `TEPHRA_PASSWORD` is visible to whoever can read the environment of that
  process or the file it is configured in.

## Explicitly out of scope

- **Whoever controls the releases on GitHub.** The app looks for updates there once a day (it can be switched off) and
  installs one only when you confirm. The checksum it verifies comes from the same place as the file: it protects
  against a damaged download, not against a forged release. On Android the system additionally refuses an app that is
  not signed with the same key; the Windows programs and the AppImage are not signed.
- **Malware on the device**, or any program running under your own account while the vault is unlocked. It can read
  memory, log the master password as you type it, read the MCP token from the client's configuration and talk to the
  daemon.
- **A removed device keeps what it has.** Removal is not remote wipe, and the vault key is not rotated.
- **Completeness and freshness of what the sync server delivers.** It can lose, withhold or replay data. The local
  vault keeps working without it.
- **Metadata on the sync server:** sizes in steps, times, deletion markers, number of records, device names.
- **Weak master passwords.** The minimum length is eight characters; that is a lower bound, not a recommendation.
- **The transport to the sync server** is whatever TLS your reverse proxy provides; the client checks the certificate
  against the certificate store of the operating system. Clients refuse plain `http://` except for `localhost` – and
  unless you allow it explicitly for a server in your own network (`--allow-http`, in the app "Connect without
  encryption"). Without TLS the contents of the vault stay encrypted, but session tokens and the auth key do not:
  whoever listens in can sign in as a device, fetch the ciphertexts and try master passwords against them, remove
  devices and delete the account. That choice is therefore meant only for a network you trust.
- **Side channels and forensic traces** in memory, swap or crash dumps of the operating system.
