//! Speicher des Sync-Servers (SQLite).
//!
//! Der Server speichert pro Konto den Anmeldenachweis, den Kopf des Tresors
//! und die verschlüsselten Datensätze mit fortlaufender Revisionsnummer.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::Path;
use subtle::ConstantTimeEq;
use tephra_crypto::KdfParams;
use tephra_sync_proto::{Bytes, DeviceInfo, PushItem, PushOutcome, PushResult, RemoteItem};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
/// So lange gilt ein bereits eingelöstes Verlängerungstoken noch weiter.
const REFRESH_GRACE_MS: u64 = 2 * 60 * 1000;
/// Höchste Datenmenge einer Abholantwort; darüber hinaus wird geteilt.
const MAX_PULL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Benutzername ist bereits vergeben")]
    UsernameTaken,
    #[error("Einladungscode ist ungültig oder wurde bereits verwendet")]
    InvalidInvite,
    #[error("Konto nicht gefunden")]
    NoSuchAccount,
    #[error("Speicherkontingent des Kontos ist ausgeschöpft")]
    QuotaExceeded,
    #[error("Datenbank wurde mit einer neueren Programmversion angelegt (Schema {0})")]
    TooNew(i64),
    #[error("Datenbankfehler: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Daten sind beschädigt: {0}")]
    Corrupt(String),
    #[error("Zufallsquelle nicht verfügbar")]
    Random,
}

pub type Result<T> = std::result::Result<T, DbError>;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn random<const N: usize>() -> Result<[u8; N]> {
    tephra_crypto::random_bytes::<N>().map_err(|_| DbError::Random)
}

/// Erzeugt ein zufälliges Token in Hex-Schreibweise.
pub fn new_token() -> Result<String> {
    Ok(hex::encode(random::<32>()?))
}

/// Tokens und Einladungscodes werden nur als Hash gespeichert: Wer die
/// Datenbank liest, kann damit keine Sitzung übernehmen.
pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.trim().as_bytes()).to_vec()
}

/// Hash des Anmeldenachweises. Der Nachweis ist bereits das Ergebnis einer
/// aufwendigen Passwortableitung auf dem Client; hier genügt ein einfacher,
/// gesalzener Hash, damit der gespeicherte Wert nicht direkt verwendbar ist.
fn auth_hash(auth_salt: &[u8], auth_key: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"tephra/server-auth/v1");
    hasher.update(auth_salt);
    hasher.update(auth_key);
    hasher.finalize().to_vec()
}

pub struct Account {
    pub id: Uuid,
    pub kdf: KdfParams,
    pub salt: Vec<u8>,
    pub header: Vec<u8>,
    pub header_rev: i64,
    pub seq: i64,
}

pub struct AccountSummary {
    pub username: String,
    pub devices: u64,
    pub items: u64,
    pub bytes: u64,
    pub created_ms: u64,
}

pub struct NewAccount<'a> {
    pub username: &'a str,
    pub auth_key: &'a [u8],
    pub kdf: KdfParams,
    pub salt: &'a [u8],
    pub header: &'a [u8],
}

pub struct Tokens {
    pub device_id: Uuid,
    pub access: String,
    pub refresh: String,
}

pub struct Limits {
    pub max_items: u64,
    pub max_bytes: u64,
}

pub struct Db {
    conn: Connection,
}

fn uuid_from(bytes: Vec<u8>) -> rusqlite::Result<Uuid> {
    Uuid::from_slice(&bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "secure_delete", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(DbError::TooNew(version));
        }
        if version == 0 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL) WITHOUT ROWID;
                 CREATE TABLE accounts (
                     id          BLOB PRIMARY KEY,
                     username    TEXT NOT NULL UNIQUE COLLATE NOCASE,
                     auth_salt   BLOB NOT NULL,
                     auth_hash   BLOB NOT NULL,
                     kdf         TEXT NOT NULL,
                     salt        BLOB NOT NULL,
                     header      BLOB NOT NULL,
                     header_rev  INTEGER NOT NULL DEFAULT 1,
                     seq         INTEGER NOT NULL DEFAULT 0,
                     created_ms  INTEGER NOT NULL
                 ) WITHOUT ROWID;
                 CREATE TABLE devices (
                     id                 BLOB PRIMARY KEY,
                     account_id         BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     name               TEXT NOT NULL,
                     created_ms         INTEGER NOT NULL,
                     last_seen_ms       INTEGER NOT NULL,
                     access_hash        BLOB NOT NULL,
                     access_expires_ms  INTEGER NOT NULL,
                     refresh_hash       BLOB NOT NULL,
                     refresh_expires_ms INTEGER NOT NULL,
                     prev_refresh_hash  BLOB,
                     prev_refresh_until INTEGER NOT NULL DEFAULT 0
                 ) WITHOUT ROWID;
                 CREATE INDEX devices_by_account ON devices (account_id);
                 CREATE UNIQUE INDEX devices_by_access ON devices (access_hash);
                 CREATE UNIQUE INDEX devices_by_refresh ON devices (refresh_hash);
                 CREATE TABLE items (
                     account_id BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                     id         BLOB NOT NULL,
                     rev        INTEGER NOT NULL,
                     deleted    INTEGER NOT NULL,
                     blob       BLOB NOT NULL,
                     PRIMARY KEY (account_id, id)
                 ) WITHOUT ROWID;
                 CREATE UNIQUE INDEX items_by_rev ON items (account_id, rev);
                 CREATE TABLE invites (
                     code_hash  BLOB PRIMARY KEY,
                     created_ms INTEGER NOT NULL,
                     used_by    BLOB
                 ) WITHOUT ROWID;
                 COMMIT;",
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn })
    }

    /// Geheimnis des Servers, erzeugt beim ersten Start. Daraus entstehen
    /// u. a. die scheinbaren Passwortparameter unbekannter Benutzernamen.
    pub fn server_secret(&self) -> Result<[u8; 32]> {
        let existing: Option<Vec<u8>> =
            self.conn.query_row("SELECT value FROM meta WHERE key = 'secret'", [], |r| r.get(0)).optional()?;
        if let Some(bytes) = existing
            && let Ok(secret) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            return Ok(secret);
        }
        let secret = random::<32>()?;
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('secret', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![secret.as_slice()],
        )?;
        Ok(secret)
    }

    // ---------------------------------------------------------------- Konten

    pub fn create_account(&mut self, new: &NewAccount<'_>, invite: Option<&str>) -> Result<Uuid> {
        let tx = self.conn.transaction()?;
        let id = Uuid::new_v4();
        let auth_salt = random::<16>()?;
        let kdf = serde_json::to_string(&new.kdf).map_err(|e| DbError::Corrupt(e.to_string()))?;
        let inserted = tx.execute(
            "INSERT INTO accounts (id, username, auth_salt, auth_hash, kdf, salt, header, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(username) DO NOTHING",
            params![
                id.as_bytes().as_slice(),
                new.username,
                auth_salt.as_slice(),
                auth_hash(&auth_salt, new.auth_key),
                kdf,
                new.salt,
                new.header,
                now_ms() as i64
            ],
        )?;
        if inserted == 0 {
            return Err(DbError::UsernameTaken);
        }
        if let Some(code) = invite {
            // Prüfen und Verbrauchen in einem Schritt: Zwei gleichzeitige
            // Anfragen mit demselben Code können so nicht beide durchkommen.
            let used = tx.execute(
                "UPDATE invites SET used_by = ?2 WHERE code_hash = ?1 AND used_by IS NULL",
                params![token_hash(code), id.as_bytes().as_slice()],
            )?;
            if used != 1 {
                return Err(DbError::InvalidInvite);
            }
        }
        tx.commit()?;
        Ok(id)
    }

    fn account_by(&self, column: &str, value: &dyn rusqlite::ToSql) -> Result<Option<Account>> {
        let sql = format!("SELECT id, kdf, salt, header, header_rev, seq FROM accounts WHERE {column} = ?1");
        let row = self
            .conn
            .query_row(&sql, [value], |r| {
                Ok((
                    uuid_from(r.get(0)?)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .optional()?;
        let Some((id, kdf, salt, header, header_rev, seq)) = row else { return Ok(None) };
        let kdf = serde_json::from_str(&kdf).map_err(|e| DbError::Corrupt(e.to_string()))?;
        Ok(Some(Account { id, kdf, salt, header, header_rev, seq }))
    }

    pub fn account_by_username(&self, username: &str) -> Result<Option<Account>> {
        self.account_by("username", &username)
    }

    pub fn account(&self, id: Uuid) -> Result<Option<Account>> {
        self.account_by("id", &id.as_bytes().as_slice())
    }

    /// Prüft den Anmeldenachweis. Für unbekannte Konten wird dieselbe Arbeit
    /// verrichtet, damit die Antwortzeit nichts über deren Existenz verrät.
    pub fn verify_auth(&self, account: Option<Uuid>, auth_key: &[u8]) -> Result<bool> {
        let stored: Option<(Vec<u8>, Vec<u8>)> = match account {
            Some(id) => self
                .conn
                .query_row(
                    "SELECT auth_salt, auth_hash FROM accounts WHERE id = ?1",
                    params![id.as_bytes().as_slice()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?,
            None => None,
        };
        let (salt, expected) = stored.clone().unwrap_or_else(|| (vec![0u8; 16], vec![0u8; 32]));
        let matches: bool = auth_hash(&salt, auth_key).ct_eq(&expected).into();
        Ok(stored.is_some() && matches)
    }

    pub fn change_password(
        &mut self,
        account: Uuid,
        new_auth_key: &[u8],
        kdf: KdfParams,
        salt: &[u8],
        header: &[u8],
        keep_device: Uuid,
    ) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let auth_salt = random::<16>()?;
        let kdf = serde_json::to_string(&kdf).map_err(|e| DbError::Corrupt(e.to_string()))?;
        tx.execute(
            "UPDATE accounts SET auth_salt = ?2, auth_hash = ?3, kdf = ?4, salt = ?5, header = ?6,
                                 header_rev = header_rev + 1 WHERE id = ?1",
            params![
                account.as_bytes().as_slice(),
                auth_salt.as_slice(),
                auth_hash(&auth_salt, new_auth_key),
                kdf,
                salt,
                header
            ],
        )?;
        // Wer das alte Passwort kannte, verliert seine Sitzungen: Alle
        // anderen Geräte müssen sich mit dem neuen Passwort neu anmelden.
        tx.execute(
            "DELETE FROM devices WHERE account_id = ?1 AND id != ?2",
            params![account.as_bytes().as_slice(), keep_device.as_bytes().as_slice()],
        )?;
        let header_rev = tx.query_row(
            "SELECT header_rev FROM accounts WHERE id = ?1",
            params![account.as_bytes().as_slice()],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(header_rev)
    }

    pub fn delete_account(&mut self, account: Uuid) -> Result<()> {
        self.conn.execute("DELETE FROM accounts WHERE id = ?1", params![account.as_bytes().as_slice()])?;
        Ok(())
    }

    pub fn delete_account_by_username(&mut self, username: &str) -> Result<()> {
        match self.account_by_username(username)? {
            Some(account) => self.delete_account(account.id),
            None => Err(DbError::NoSuchAccount),
        }
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.username, a.created_ms,
                    (SELECT COUNT(*) FROM devices d WHERE d.account_id = a.id),
                    (SELECT COUNT(*) FROM items i WHERE i.account_id = a.id AND i.deleted = 0),
                    (SELECT COALESCE(SUM(LENGTH(i.blob)), 0) FROM items i WHERE i.account_id = a.id)
             FROM accounts a ORDER BY a.username",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountSummary {
                username: r.get(0)?,
                created_ms: r.get::<_, i64>(1)? as u64,
                devices: r.get::<_, i64>(2)? as u64,
                items: r.get::<_, i64>(3)? as u64,
                bytes: r.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---------------------------------------------------------------- Einladungen

    pub fn create_invite(&mut self) -> Result<String> {
        let code = hex::encode(random::<12>()?);
        self.conn.execute(
            "INSERT INTO invites (code_hash, created_ms) VALUES (?1, ?2)",
            params![token_hash(&code), now_ms() as i64],
        )?;
        Ok(code)
    }

    pub fn invite_is_open(&self, code: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM invites WHERE code_hash = ?1 AND used_by IS NULL",
                params![token_hash(code)],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    // ---------------------------------------------------------------- Geräte

    pub fn create_device(
        &mut self,
        account: Uuid,
        name: &str,
        access_ttl_ms: u64,
        refresh_ttl_ms: u64,
    ) -> Result<Tokens> {
        let device_id = Uuid::new_v4();
        let (access, refresh) = (new_token()?, new_token()?);
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO devices (id, account_id, name, created_ms, last_seen_ms, access_hash, access_expires_ms,
                                  refresh_hash, refresh_expires_ms)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8)",
            params![
                device_id.as_bytes().as_slice(),
                account.as_bytes().as_slice(),
                name,
                now as i64,
                token_hash(&access),
                (now + access_ttl_ms) as i64,
                token_hash(&refresh),
                (now + refresh_ttl_ms) as i64
            ],
        )?;
        Ok(Tokens { device_id, access, refresh })
    }

    /// Findet Konto und Gerät zu einem gültigen Zugriffstoken.
    pub fn authenticate(&self, access_token: &str) -> Result<Option<(Uuid, Uuid)>> {
        let now = now_ms() as i64;
        let found = self
            .conn
            .query_row(
                "SELECT account_id, id FROM devices WHERE access_hash = ?1 AND access_expires_ms > ?2",
                params![token_hash(access_token), now],
                |r| Ok((uuid_from(r.get(0)?)?, uuid_from(r.get(1)?)?)),
            )
            .optional()?;
        if let Some((_, device)) = found {
            self.conn.execute(
                "UPDATE devices SET last_seen_ms = ?2 WHERE id = ?1",
                params![device.as_bytes().as_slice(), now],
            )?;
        }
        Ok(found)
    }

    /// Tauscht ein Verlängerungstoken gegen ein neues Tokenpaar.
    ///
    /// Das alte Token bleibt noch kurz gültig. Geht die Antwort unterwegs
    /// verloren oder erneuern zwei Aufgaben desselben Geräts gleichzeitig,
    /// wäre das Gerät sonst ausgesperrt, obwohl nichts Unrechtes geschehen ist.
    pub fn refresh(
        &mut self,
        refresh_token: &str,
        access_ttl_ms: u64,
        refresh_ttl_ms: u64,
    ) -> Result<Option<(Uuid, Tokens)>> {
        let now = now_ms();
        let presented = token_hash(refresh_token);
        let found = self
            .conn
            .query_row(
                "SELECT account_id, id, refresh_hash = ?1 FROM devices
                 WHERE (refresh_hash = ?1 AND refresh_expires_ms > ?2)
                    OR (prev_refresh_hash = ?1 AND prev_refresh_until > ?2)",
                params![presented, now as i64],
                |r| Ok((uuid_from(r.get(0)?)?, uuid_from(r.get(1)?)?, r.get::<_, bool>(2)?)),
            )
            .optional()?;
        let Some((account, device_id, is_current)) = found else { return Ok(None) };
        let (access, refresh) = (new_token()?, new_token()?);
        // Nur das eben eingelöste Token wird zum „vorherigen"; wurde das
        // vorherige erneut vorgelegt, bleibt seine Frist unverändert.
        self.conn.execute(
            "UPDATE devices SET access_hash = ?2, access_expires_ms = ?3, refresh_hash = ?4, refresh_expires_ms = ?5,
                                last_seen_ms = ?6,
                                prev_refresh_hash = CASE WHEN ?7 THEN ?8 ELSE prev_refresh_hash END,
                                prev_refresh_until = CASE WHEN ?7 THEN ?9 ELSE prev_refresh_until END
             WHERE id = ?1",
            params![
                device_id.as_bytes().as_slice(),
                token_hash(&access),
                (now + access_ttl_ms) as i64,
                token_hash(&refresh),
                (now + refresh_ttl_ms) as i64,
                now as i64,
                is_current,
                presented,
                (now + REFRESH_GRACE_MS) as i64
            ],
        )?;
        Ok(Some((account, Tokens { device_id, access, refresh })))
    }

    pub fn list_devices(&self, account: Uuid, current: Uuid) -> Result<Vec<DeviceInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, created_ms, last_seen_ms FROM devices WHERE account_id = ?1 ORDER BY created_ms",
        )?;
        let rows = stmt.query_map(params![account.as_bytes().as_slice()], |r| {
            let id = uuid_from(r.get(0)?)?;
            Ok(DeviceInfo {
                id,
                name: r.get(1)?,
                created_ms: r.get::<_, i64>(2)? as u64,
                last_seen_ms: r.get::<_, i64>(3)? as u64,
                current: id == current,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Entfernt ein Gerät; seine Tokens sind sofort ungültig.
    pub fn delete_device(&mut self, account: Uuid, device: Uuid) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM devices WHERE account_id = ?1 AND id = ?2",
            params![account.as_bytes().as_slice(), device.as_bytes().as_slice()],
        )? > 0)
    }

    // ---------------------------------------------------------------- Datensätze

    /// Datensätze eines Kontos mit Revision größer als `since`, aufsteigend.
    /// Gibt zusätzlich an, ob es darüber hinaus noch weitere gibt.
    pub fn pull(&self, account: Uuid, since: i64, limit: usize) -> Result<(Vec<RemoteItem>, bool)> {
        let mut stmt = self.conn.prepare(
            "SELECT id, rev, deleted, blob FROM items WHERE account_id = ?1 AND rev > ?2 ORDER BY rev LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account.as_bytes().as_slice(), since, (limit + 1) as i64], |r| {
            Ok(RemoteItem { id: uuid_from(r.get(0)?)?, rev: r.get(1)?, deleted: r.get(2)?, blob: Bytes(r.get(3)?) })
        })?;
        let mut items = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        for row in rows {
            let item = row?;
            // Antworten bleiben überschaubar: Bei Erreichen der Stückzahl
            // oder der Datenmenge folgt der Rest in der nächsten Anfrage.
            if items.len() >= limit || (!items.is_empty() && bytes + item.blob.0.len() > MAX_PULL_BYTES) {
                more = true;
                break;
            }
            bytes += item.blob.0.len();
            items.push(item);
        }
        Ok((items, more))
    }

    /// Übernimmt hochgeladene Datensätze. Jeder wird einzeln beurteilt:
    /// angenommen, wenn er auf der aktuellen Fassung beruht, sonst Konflikt.
    pub fn push(&mut self, account: Uuid, items: &[PushItem], limits: &Limits) -> Result<Vec<PushResult>> {
        let tx = self.conn.transaction()?;
        let account_bytes = account.as_bytes().as_slice();
        let (mut count, mut bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(LENGTH(blob)), 0) FROM items WHERE account_id = ?1",
            params![account_bytes],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut seq: i64 = tx
            .query_row("SELECT seq FROM accounts WHERE id = ?1", params![account_bytes], |r| r.get(0))
            .optional()?
            .ok_or(DbError::NoSuchAccount)?;
        let mut results = Vec::with_capacity(items.len());

        for item in items {
            let id_bytes = item.id.as_bytes().as_slice();
            let current: Option<(i64, bool, Vec<u8>)> = tx
                .query_row(
                    "SELECT rev, deleted, blob FROM items WHERE account_id = ?1 AND id = ?2",
                    params![account_bytes, id_bytes],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let outcome = match current {
                Some((rev, deleted, blob)) if rev != item.base_rev => {
                    PushOutcome::Conflict { current: RemoteItem { id: item.id, rev, deleted, blob: Bytes(blob) } }
                }
                None if item.base_rev != 0 => PushOutcome::Rejected {
                    reason: "Der Datensatz ist dem Server unbekannt; base_rev muss 0 sein".into(),
                },
                current => {
                    let old_len = current.as_ref().map_or(0, |(_, _, blob)| blob.len() as i64);
                    let new_bytes = bytes - old_len + item.blob.0.len() as i64;
                    let new_count = count + i64::from(current.is_none());
                    if new_count as u64 > limits.max_items || new_bytes as u64 > limits.max_bytes {
                        PushOutcome::Rejected { reason: DbError::QuotaExceeded.to_string() }
                    } else {
                        seq += 1;
                        tx.execute(
                            "INSERT INTO items (account_id, id, rev, deleted, blob) VALUES (?1, ?2, ?3, ?4, ?5)
                             ON CONFLICT(account_id, id) DO UPDATE SET rev = excluded.rev, deleted = excluded.deleted,
                                                                      blob = excluded.blob",
                            params![account_bytes, id_bytes, seq, item.deleted, item.blob.0],
                        )?;
                        count = new_count;
                        bytes = new_bytes;
                        PushOutcome::Accepted { rev: seq }
                    }
                }
            };
            results.push(PushResult { id: item.id, outcome });
        }
        tx.execute("UPDATE accounts SET seq = ?2 WHERE id = ?1", params![account_bytes, seq])?;
        tx.commit()?;
        Ok(results)
    }
}
