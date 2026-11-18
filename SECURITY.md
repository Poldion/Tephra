# Security

## Reporting

Please do **not** report security issues in public issues. Use GitHub's
[private vulnerability reporting](../../security/advisories/new) for Tephra instead.
You will get an answer within a few days.

Helpful in a report: the version (`tephra --version`), the operating system, the steps to reproduce and what an attacker
gains. Please leave out real credentials, keys and vault files.

The latest release is the supported version; fixes go into a new release.

## What Tephra protects

- **Your credentials at rest.** Passwords, SSH keys and VPN keys are stored encrypted in the vault. Without the master
  password or the recovery key the file is useless.
- **Your credentials in transit and on the sync server.** Records are encrypted on the device. The server stores
  ciphertext and cannot read, forge or swap records.
- **Your hosts from a language model.** Nothing is shared by default. A model reaches only the hosts you shared with
  its client, within the access level you chose, and never sees credentials.
- **Your connections.** Host keys are pinned on first use; a changed key stops the connection.

## What the sync server can see and do

It sees the user name, device names with the time they were last seen, the wrapped vault key with the parameters of
the password derivation, and per record an ID, a revision number, a deletion marker and a padded size – and of course
when and from which address a device syncs. It cannot read contents, and forged or altered records and a forged or
outdated vault header are detected by the devices. It can withhold changes and deletions, hand a new device an older
state or delete data without the devices noticing, and whoever controls it can try to guess a weak master password
offline.

## What is out of scope

- Malware or another program running under your own user account on a device with an unlocked vault.
- A device that was removed from the account keeps the data it has already fetched. The vault key is not replaced
  when a device is removed or the master password is changed.
- A language model on a host shared with level `auto`, or inside a confirmed interactive session, can do whatever the
  SSH user can do. The deny list prevents accidents, not misuse.
- Level `read-only` covers everything the SSH user is allowed to read. Refusing paths that look like credentials is a
  hurdle against accidents.
- Whatever a language model reads from a host is sent to the operator of that model.
- Weak master passwords.

The full picture, attacker by attacker: [docs/threat-model.md](docs/threat-model.md)
(German: [docs/bedrohungsmodell.md](docs/bedrohungsmodell.md)).

The cryptographic design has not been reviewed by a third party yet.

## Running the sync server

The server speaks plain HTTP. Put a reverse proxy with TLS in front of it (examples in [deploy/](deploy/)), start the
server with `--behind-proxy` so that failed logins are limited per sender, and keep registration on `invite` or
`closed` unless you really want anyone to be able to create accounts.
