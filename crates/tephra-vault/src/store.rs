use crate::{Result, VaultError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tephra_crypto::{
    CryptoError, KdfParams, MasterKey, RecoveryKey, VaultKey, WrappedKey, authenticate, open_item, random_bytes,
    seal_item, verify,
};
use tephra_model::{Entity, Hlc, HlcClock, ItemId, ItemKind, Record};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
/// Kennung im SQLite-Kopf, an der sich eine Tresordatei erkennen lässt ("UTVT").
const APPLICATION_ID: i64 = 0x5554_5654;
/// Anzahl früherer Fassungen, die pro Datensatz aufbewahrt werden.
const HISTORY_DEPTH: i64 = 10;
/// Um so viel darf der Zeitstempel eines fremden Datensatzes der eigenen Uhr vorauseilen.
const MAX_CLOCK_SKEW_MS: u64 = 24 * 3600 * 1000;

/// Unverschlüsselter Kopf des Tresors: alles, was zum Entsperren nötig ist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    /// Kennung des Tresors; bindet jedes Chiffrat an genau diesen Tresor.
    pub vault_id: Uuid,
    pub kdf: KdfParams,
    pub salt: Vec<u8>,
    /// Vault Key, verpackt mit dem aus dem Passwort abgeleiteten Schlüssel.
    pub wrapped_key: WrappedKey,
    /// Vault Key, verpackt mit dem Wiederherstellungsschlüssel.
    pub recovery_wrapped_key: WrappedKey,
    /// Echtheitsnachweis über alle übrigen Felder, erzeugt mit dem Vault Key.
    /// Der Kopf liegt unverschlüsselt vor (auch beim Sync-Server); ohne
    /// Nachweis ließe er sich unbemerkt austauschen.
    #[serde(default)]
    pub auth_tag: Vec<u8>,
    /// Zähler, der mit jedem Passwortwechsel steigt und vom Echtheitsnachweis
    /// abgedeckt ist. Ein echter, aber älterer Kopf (mit einem früheren,
    /// womöglich bekannt gewordenen Passwort) lässt sich so nicht wieder
    /// unterschieben.
    #[serde(default)]
    pub generation: u64,
}

const HEADER_AUTH_CONTEXT: &[u8] = b"tephra/v1/header";
const KEY_CHECK_ITEM: [u8; 16] = *b"tephra-keycheck1";
const KEY_CHECK_VALUE: &[u8] = b"tephra";

impl VaultHeader {
    /// Kontext, an den die Passwort-Verpackung des Vault Keys gebunden ist.
    fn password_context(&self) -> Vec<u8> {
        let mut context = self.vault_id.as_bytes().to_vec();
        for value in [self.kdf.memory_kib, self.kdf.iterations, self.kdf.parallelism] {
            context.extend_from_slice(&value.to_le_bytes());
        }
        context.extend_from_slice(&self.salt);
        context
    }

    fn signed_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            &self.vault_id,
            &self.kdf,
            &self.salt,
            &self.wrapped_key,
            &self.recovery_wrapped_key,
            self.generation,
        ))?)
    }

    fn sign(&mut self, key: &VaultKey) -> Result<()> {
        self.auth_tag = authenticate(key, HEADER_AUTH_CONTEXT, &self.signed_bytes()?)?;
        Ok(())
    }

    /// Prüft, ob der Kopf mit diesem Vault Key erzeugt wurde.
    pub fn is_authentic(&self, key: &VaultKey) -> bool {
        self.signed_bytes().is_ok_and(|bytes| verify(key, HEADER_AUTH_CONTEXT, &bytes, &self.auth_tag))
    }

    /// Entpackt den Vault Key mit dem Master-Passwort. Dient auch einem
    /// neuen Gerät, das den Kopf vom Sync-Server erhalten hat.
    pub fn open_with_password(&self, password: &str) -> Result<VaultKey> {
        check_kdf(self.kdf)?;
        let master = MasterKey::derive(password, &self.salt, self.kdf)?;
        let key = match VaultKey::unwrap(&self.wrapped_key, &master.kek(), &self.password_context()) {
            Ok(key) => key,
            Err(CryptoError::Decrypt) => return Err(VaultError::WrongPassword),
            Err(e) => return Err(e.into()),
        };
        if !self.is_authentic(&key) {
            return Err(VaultError::HeaderTampered);
        }
        Ok(key)
    }

    /// Schlüssel, mit dem sich das Gerät am Sync-Server ausweist.
    pub fn auth_key(&self, password: &str) -> Result<tephra_crypto::AuthKey> {
        check_kdf(self.kdf)?;
        Ok(MasterKey::derive(password, &self.salt, self.kdf)?.auth_key())
    }
}

/// Lehnt Ableitungsparameter ab, die zu schwach oder absurd hoch sind. Die
/// bewusst schwachen Testparameter gelten nur in Test-Builds.
pub fn check_kdf(kdf: KdfParams) -> Result<()> {
    let test_params = cfg!(any(test, feature = "testing")) && kdf == KdfParams::INSECURE_FOR_TESTS;
    if kdf.is_acceptable() || test_params { Ok(()) } else { Err(VaultError::WeakKdf) }
}

/// Lokal geänderter Datensatz, der noch hochgeladen werden muss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirtyItem {
    pub id: ItemId,
    /// Server-Revision, auf der die lokale Fassung beruht (0 = noch nie hochgeladen).
    pub base_rev: i64,
    pub blob: Vec<u8>,
    pub deleted: bool,
}

/// Ergebnis der Übernahme eines Datensatzes vom Server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Datensatz war lokal unbekannt.
    Inserted,
    /// Lokale Fassung wurde durch die des Servers ersetzt.
    Updated,
    /// Beide Seiten hatten Änderungen; das Ergebnis muss wieder hochgeladen werden.
    Merged,
    /// Die lokale Fassung enthielt bereits alles.
    Unchanged,
}

/// Frühere Fassung eines Datensatzes.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// Millisekunden seit der Unix-Epoche, zu denen die Fassung abgelöst wurde.
    pub replaced_at_ms: u64,
    pub record: Record,
}

struct Unlocked {
    key: VaultKey,
    records: BTreeMap<ItemId, Record>,
    clock: HlcClock,
    /// Datensätze, die sich nicht entschlüsseln ließen.
    corrupt: Vec<ItemId>,
    /// Stand der Datenbank, zu dem `records` passt. Ändert ein anderer
    /// Prozess die Datei, weicht der Wert ab und es wird neu geladen.
    data_version: i64,
}

/// Lokaler Tresor. Nicht `Sync`; für nebenläufigen Zugriff in einen Mutex legen.
pub struct Vault {
    conn: Connection,
    header: VaultHeader,
    unlocked: Option<Unlocked>,
}

impl Vault {
    /// Legt einen neuen Tresor an und gibt den Wiederherstellungsschlüssel
    /// zurück. Dieser wird nirgends gespeichert und muss dem Nutzer jetzt
    /// angezeigt werden.
    pub fn create(path: &Path, password: &str, kdf: KdfParams) -> Result<(Self, RecoveryKey)> {
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        check_kdf(kdf)?;
        let salt = tephra_crypto::random_salt()?;
        let master = MasterKey::derive(password, &salt, kdf)?;
        let key = VaultKey::generate()?;
        let recovery = RecoveryKey::generate()?;
        let vault_id = Uuid::new_v4();
        let mut header = VaultHeader {
            vault_id,
            kdf,
            salt: salt.to_vec(),
            wrapped_key: WrappedKey(Vec::new()),
            recovery_wrapped_key: key.wrap_for_recovery(&recovery, vault_id.as_bytes())?,
            auth_tag: Vec::new(),
            generation: 1,
        };
        header.wrapped_key = key.wrap(&master.kek(), &header.password_context())?;
        header.sign(&key)?;
        let vault = Self::create_with_header(path, header, key)?;
        Ok((vault, recovery))
    }

    /// Legt einen leeren Tresor mit vorgegebenem Kopf und bereits bekanntem
    /// Vault Key an – für ein neues Gerät, das sich an einem bestehenden
    /// Konto anmeldet.
    pub fn create_with_header(path: &Path, header: VaultHeader, key: VaultKey) -> Result<Self> {
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        if !header.is_authentic(&key) {
            return Err(VaultError::HeaderTampered);
        }
        let conn = Connection::open(path)?;
        configure(&conn)?;
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL) WITHOUT ROWID;
             CREATE TABLE items (
                 id         BLOB PRIMARY KEY,
                 blob       BLOB NOT NULL,
                 rev        INTEGER NOT NULL DEFAULT 0,
                 dirty      INTEGER NOT NULL DEFAULT 0,
                 local_only INTEGER NOT NULL DEFAULT 0
             ) WITHOUT ROWID;
             CREATE TABLE history (
                 seq         INTEGER PRIMARY KEY AUTOINCREMENT,
                 id          BLOB NOT NULL,
                 replaced_at INTEGER NOT NULL,
                 blob        BLOB NOT NULL
             );
             CREATE INDEX history_by_item ON history (id, seq);
             CREATE TABLE secrets (name TEXT PRIMARY KEY, blob BLOB NOT NULL) WITHOUT ROWID;
             CREATE TABLE journal (seq INTEGER PRIMARY KEY AUTOINCREMENT, blob BLOB NOT NULL);
             COMMIT;",
        )?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let node = u64::from_le_bytes(random_bytes::<8>()?);
        let data_version: i64 = conn.pragma_query_value(None, "data_version", |r| r.get(0))?;
        put_meta(&conn, "header", &serde_json::to_vec(&header)?)?;
        put_meta(&conn, "node", &node.to_le_bytes())?;
        // Prüfwert, an dem sich später der richtige Schlüssel erkennen lässt –
        // auch solange der Tresor noch leer ist.
        put_meta(&conn, "key_check", &seal_item(&key, header.vault_id.as_bytes(), &KEY_CHECK_ITEM, KEY_CHECK_VALUE)?)?;
        Ok(Self {
            conn,
            header,
            unlocked: Some(Unlocked {
                key,
                records: BTreeMap::new(),
                clock: HlcClock::new(node),
                corrupt: Vec::new(),
                data_version,
            }),
        })
    }

    /// Öffnet einen bestehenden Tresor im gesperrten Zustand.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(VaultError::NotAVault);
        }
        let conn = Connection::open(path).map_err(|_| VaultError::NotAVault)?;
        let app_id: i64 =
            conn.pragma_query_value(None, "application_id", |r| r.get(0)).map_err(|_| VaultError::NotAVault)?;
        if app_id != APPLICATION_ID {
            return Err(VaultError::NotAVault);
        }
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(VaultError::TooNew(version));
        }
        configure(&conn)?;
        let header_bytes = get_meta(&conn, "header")?.ok_or(VaultError::NotAVault)?;
        let header: VaultHeader = serde_json::from_slice(&header_bytes).map_err(|_| VaultError::NotAVault)?;
        Ok(Self { conn, header, unlocked: None })
    }

    pub fn header(&self) -> &VaultHeader {
        &self.header
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    /// Entsperrt mit dem Master-Passwort.
    pub fn unlock(&mut self, password: &str) -> Result<()> {
        // Der Kopf ist unverschlüsselt und könnte manipuliert sein; unsinnige
        // Parameter werden abgelehnt, bevor sie Rechenzeit oder Speicher kosten.
        let key = self.header.open_with_password(password)?;
        self.unlock_with_key(key)
    }

    /// Entsperrt mit einem bereits bekannten Vault Key (z. B. aus dem
    /// Schlüsselspeicher des Betriebssystems nach biometrischer Freigabe).
    pub fn unlock_with_key(&mut self, key: VaultKey) -> Result<()> {
        let vault_id = *self.header.vault_id.as_bytes();
        // Zuerst sicherstellen, dass der Schlüssel zu diesem Tresor gehört.
        // Sonst würden neue Einträge mit einem fremden Schlüssel versiegelt
        // und wären mit dem richtigen Passwort später nicht mehr lesbar.
        let check = get_meta(&self.conn, "key_check")?.ok_or(VaultError::NotAVault)?;
        match open_item(&key, &vault_id, &KEY_CHECK_ITEM, &check) {
            Ok(value) if value == KEY_CHECK_VALUE => {}
            _ => return Err(VaultError::WrongPassword),
        }
        if !self.header.is_authentic(&key) {
            return Err(VaultError::HeaderTampered);
        }
        let data_version = self.data_version()?;
        let (records, corrupt) = self.load_records(&key)?;
        let node = match get_meta(&self.conn, "node")?.and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok()) {
            Some(bytes) => u64::from_le_bytes(bytes),
            // Eine eingespielte Sicherung bringt keine Gerätekennung mit.
            None => {
                let node = u64::from_le_bytes(random_bytes::<8>()?);
                put_meta(&self.conn, "node", &node.to_le_bytes())?;
                node
            }
        };
        let newest = records.values().map(Record::last_change).max().unwrap_or(Hlc::ZERO);
        let mut clock = HlcClock::new(node);
        clock.observe(newest);
        self.unlocked = Some(Unlocked { key, records, clock, corrupt, data_version });
        Ok(())
    }

    fn data_version(&self) -> Result<i64> {
        Ok(self.conn.pragma_query_value(None, "data_version", |r| r.get(0))?)
    }

    /// Entschlüsselt alle Datensätze der Datenbank.
    fn load_records(&self, key: &VaultKey) -> Result<(BTreeMap<ItemId, Record>, Vec<ItemId>)> {
        let vault_id = *self.header.vault_id.as_bytes();
        let mut records = BTreeMap::new();
        let mut corrupt = Vec::new();
        let mut stmt = self.conn.prepare("SELECT id, blob FROM items")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        for row in rows {
            let (id_bytes, blob) = row?;
            let Ok(id) = Uuid::from_slice(&id_bytes).map(ItemId) else { continue };
            match open_record(key, &vault_id, id, &blob) {
                Ok(record) => {
                    records.insert(id, record);
                }
                Err(_) => corrupt.push(id),
            }
        }
        Ok((records, corrupt))
    }

    /// Gleicht den Arbeitsspeicher mit der Datei ab, falls ein anderer
    /// Prozess sie inzwischen geändert hat (z. B. die Kommandozeile neben dem
    /// laufenden Dienst). Ohne das würde ein Prozess mit veraltetem Stand
    /// weiterarbeiten: Ein gelöschter Host bliebe nutzbar, und der nächste
    /// Schreibvorgang überschriebe fremde Änderungen.
    pub fn refresh(&mut self) -> Result<()> {
        let Some(state) = self.unlocked.as_ref() else { return Ok(()) };
        let current = self.data_version()?;
        if current == state.data_version {
            return Ok(());
        }
        let key = state.key.clone();
        // Auch der Kopf kann sich geändert haben (Passwortwechsel).
        if let Some(bytes) = get_meta(&self.conn, "header")?
            && let Ok(header) = serde_json::from_slice::<VaultHeader>(&bytes)
            && header != self.header
        {
            if !header.is_authentic(&key) || header.vault_id != self.header.vault_id {
                self.unlocked = None;
                return Err(VaultError::HeaderTampered);
            }
            self.header = header;
        }
        let (records, corrupt) = self.load_records(&key)?;
        if let Some(state) = self.unlocked.as_mut() {
            let newest = records.values().map(Record::last_change).max().unwrap_or(Hlc::ZERO);
            state.clock.observe(newest);
            state.records = records;
            state.corrupt = corrupt;
            state.data_version = current;
        }
        Ok(())
    }

    /// Sperrt den Tresor; Schlüssel und Klartext werden verworfen.
    pub fn lock(&mut self) {
        self.unlocked = None;
    }

    /// Kopie des Vault Keys, um ihn im Schlüsselspeicher des Betriebssystems
    /// abzulegen.
    pub fn vault_key(&self) -> Result<VaultKey> {
        Ok(self.state()?.key.clone())
    }

    /// Kennung für einen Datensatz, den es je Name nur einmal geben soll.
    /// Sie hängt vom Vault Key ab und verrät dem Server nichts über den Namen.
    pub fn derived_id(&self, name: &str) -> Result<ItemId> {
        Ok(ItemId(Uuid::from_bytes(self.state()?.key.derive_id(name.as_bytes()))))
    }

    /// Kennungen der Datensätze, die sich beim Entsperren nicht lesen ließen.
    pub fn corrupt_items(&self) -> Result<&[ItemId]> {
        Ok(&self.state()?.corrupt)
    }

    /// Ändert das Master-Passwort. Der Vault Key bleibt gleich, es wird nur
    /// seine Verpackung erneuert.
    pub fn change_password(&mut self, new_password: &str, kdf: KdfParams) -> Result<()> {
        let header = self.prepare_password_change(new_password, kdf)?;
        put_meta(&self.conn, "header", &serde_json::to_vec(&header)?)?;
        self.header = header;
        Ok(())
    }

    /// Berechnet den Kopf für ein neues Master-Passwort, ohne ihn zu
    /// speichern. Ist der Tresor mit einem Sync-Server verbunden, muss erst
    /// der Server den neuen Kopf annehmen; danach wird er mit
    /// [`Vault::replace_header`] übernommen.
    pub fn prepare_password_change(&self, new_password: &str, kdf: KdfParams) -> Result<VaultHeader> {
        let key = &self.state()?.key;
        check_kdf(kdf)?;
        let salt = tephra_crypto::random_salt()?;
        let master = MasterKey::derive(new_password, &salt, kdf)?;
        let mut header = self.header.clone();
        header.kdf = kdf;
        header.salt = salt.to_vec();
        header.generation = self.header.generation + 1;
        header.wrapped_key = key.wrap(&master.kek(), &header.password_context())?;
        header.sign(key)?;
        Ok(header)
    }

    /// Entsperrt mit dem Wiederherstellungsschlüssel und setzt ein neues Passwort.
    pub fn recover(&mut self, recovery: &RecoveryKey, new_password: &str, kdf: KdfParams) -> Result<()> {
        let context = *self.header.vault_id.as_bytes();
        let key = match VaultKey::unwrap_with_recovery(&self.header.recovery_wrapped_key, recovery, &context) {
            Ok(key) => key,
            Err(CryptoError::Decrypt) => return Err(VaultError::WrongPassword),
            Err(e) => return Err(e.into()),
        };
        self.unlock_with_key(key)?;
        self.change_password(new_password, kdf)
    }

    /// Ersetzt den Kopf, nachdem das Passwort auf einem anderen Gerät
    /// geändert wurde. Der Tresor muss entsperrt sein; der neue Kopf wird nur
    /// übernommen, wenn er nachweislich mit demselben Vault Key erzeugt
    /// wurde. Ein Kopf, den ein Dritter (etwa ein kompromittierter Server)
    /// untergeschoben hat, wird abgelehnt.
    pub fn replace_header(&mut self, header: VaultHeader) -> Result<()> {
        let key = &self.state()?.key;
        if header.vault_id != self.header.vault_id {
            return Err(VaultError::NotAVault);
        }
        if !header.is_authentic(key) {
            return Err(VaultError::HeaderTampered);
        }
        if header == self.header {
            return Ok(());
        }
        // Ein echter Kopf aus der Vergangenheit ist trotzdem der falsche.
        if header.generation <= self.header.generation {
            return Err(VaultError::HeaderOutdated);
        }
        check_kdf(header.kdf)?;
        put_meta(&self.conn, "header", &serde_json::to_vec(&header)?)?;
        self.header = header;
        Ok(())
    }

    // ---------------------------------------------------------------- Datensätze

    /// Alle nicht gelöschten Objekte eines Typs.
    pub fn list<T: Entity>(&self) -> Result<Vec<(ItemId, T)>> {
        let state = self.state()?;
        let mut out = Vec::new();
        for record in state.records.values() {
            if record.kind == T::KIND && !record.is_deleted() {
                out.push((record.id, record.entity::<T>()?));
            }
        }
        Ok(out)
    }

    pub fn get<T: Entity>(&self, id: ItemId) -> Result<Option<T>> {
        match self.state()?.records.get(&id) {
            Some(record) if record.kind == T::KIND && !record.is_deleted() => Ok(Some(record.entity::<T>()?)),
            _ => Ok(None),
        }
    }

    /// Art eines Datensatzes, falls er existiert und nicht gelöscht ist.
    pub fn kind_of(&self, id: ItemId) -> Result<Option<ItemKind>> {
        Ok(self.state()?.records.get(&id).filter(|r| !r.is_deleted()).map(|r| r.kind))
    }

    /// Legt ein neues Objekt an.
    pub fn insert<T: Entity>(&mut self, entity: &T) -> Result<ItemId> {
        let clock = self.tick()?;
        let record = Record::create(entity, clock)?;
        let id = record.id;
        self.write(record, None, true, None)?;
        Ok(id)
    }

    /// Legt ein neues Objekt an, das dieses Gerät nie verlässt. Anlegen und
    /// Kennzeichnen geschehen in einem Schritt, damit ein gleichzeitig
    /// laufender Abgleich den Datensatz nicht doch noch hochlädt.
    pub fn insert_local_only<T: Entity>(&mut self, entity: &T) -> Result<ItemId> {
        let clock = self.tick()?;
        let record = Record::create(entity, clock)?;
        let id = record.id;
        self.write(record, None, true, Some(true))?;
        Ok(id)
    }

    /// Legt ein Objekt unter einer festen Kennung an oder aktualisiert es.
    /// Für Dinge, die es nur einmal geben soll (z. B. die Einstellungen oder
    /// den bekannten Schlüssel eines Hosts): Zwei Geräte, die unabhängig
    /// voneinander dasselbe anlegen, landen so beim selben Datensatz. Gibt
    /// zurück, ob sich etwas geändert hat.
    pub fn upsert_with_id<T: Entity>(&mut self, id: ItemId, entity: &T) -> Result<bool> {
        let existing = self.state()?.records.get(&id).cloned();
        let clock = self.tick()?;
        let record = match existing {
            Some(mut record) if !record.is_deleted() => {
                if !record.update(entity, clock)? {
                    return Ok(false);
                }
                record
            }
            // Unbekannt oder gelöscht: eine neue Fassung beginnt.
            _ => Record::create_with_id(id, entity, clock)?,
        };
        self.write(record, None, true, None)?;
        Ok(true)
    }

    /// Aktualisiert ein bestehendes Objekt. Gibt zurück, ob sich etwas geändert hat.
    pub fn update<T: Entity>(&mut self, id: ItemId, entity: &T) -> Result<bool> {
        let mut record =
            self.state()?.records.get(&id).filter(|r| !r.is_deleted()).cloned().ok_or(VaultError::NotFound(id))?;
        let clock = self.tick()?;
        if !record.update(entity, clock)? {
            return Ok(false);
        }
        self.write(record, None, true, None)?;
        Ok(true)
    }

    /// Löscht ein Objekt mitsamt seinem Verlauf. Übrig bleibt ein leerer
    /// Löschmarker, damit die Löschung auch andere Geräte erreicht.
    pub fn delete(&mut self, id: ItemId) -> Result<()> {
        let mut record =
            self.state()?.records.get(&id).filter(|r| !r.is_deleted()).cloned().ok_or(VaultError::NotFound(id))?;
        let clock = self.tick()?;
        record.delete(clock);
        self.write(record, None, true, None)
    }

    /// Legt fest, ob ein Datensatz dieses Gerät nie verlässt. Ein bereits
    /// abgeglichener Datensatz lässt sich nicht nachträglich beschränken –
    /// er liegt schon beim Server und auf anderen Geräten.
    pub fn set_local_only(&mut self, id: ItemId, local_only: bool) -> Result<()> {
        self.state()?;
        if local_only {
            let rev: Option<i64> = self
                .conn
                .query_row("SELECT rev FROM items WHERE id = ?1", params![id.as_bytes().as_slice()], |r| r.get(0))
                .optional()?;
            if rev.is_some_and(|rev| rev > 0) {
                return Err(VaultError::AlreadySynced(id));
            }
        }
        let changed = self.conn.execute(
            "UPDATE items SET local_only = ?2, dirty = CASE WHEN ?2 = 0 THEN 1 ELSE dirty END WHERE id = ?1",
            params![id.as_bytes().as_slice(), local_only],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound(id));
        }
        Ok(())
    }

    pub fn is_local_only(&self, id: ItemId) -> Result<bool> {
        self.state()?;
        Ok(self
            .conn
            .query_row("SELECT local_only FROM items WHERE id = ?1", params![id.as_bytes().as_slice()], |r| {
                r.get::<_, bool>(0)
            })
            .optional()?
            .unwrap_or(false))
    }

    /// Frühere Fassungen eines Datensatzes, jüngste zuerst.
    pub fn history(&self, id: ItemId) -> Result<Vec<HistoryEntry>> {
        let state = self.state()?;
        let vault_id = *self.header.vault_id.as_bytes();
        let mut stmt = self.conn.prepare("SELECT replaced_at, blob FROM history WHERE id = ?1 ORDER BY seq DESC")?;
        let rows =
            stmt.query_map(params![id.as_bytes().as_slice()], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (replaced_at, blob) = row?;
            if let Ok(record) = open_record(&state.key, &vault_id, id, &blob) {
                out.push(HistoryEntry { replaced_at_ms: replaced_at.max(0) as u64, record });
            }
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- Abgleich

    /// Datensätze mit lokalen Änderungen, die noch nicht beim Server sind.
    pub fn dirty_items(&self) -> Result<Vec<DirtyItem>> {
        let state = self.state()?;
        let mut stmt =
            self.conn.prepare("SELECT id, rev, blob FROM items WHERE dirty = 1 AND local_only = 0 ORDER BY id")?;
        let rows =
            stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id_bytes, base_rev, blob) = row?;
            let Ok(id) = Uuid::from_slice(&id_bytes).map(ItemId) else { continue };
            // Was sich lokal nicht lesen lässt, wird auch nicht verteilt.
            let Some(record) = state.records.get(&id) else { continue };
            out.push(DirtyItem { id, base_rev, blob, deleted: record.is_deleted() });
        }
        Ok(out)
    }

    /// Vermerkt, dass der Server eine hochgeladene Fassung unter `rev`
    /// angenommen hat. Wurde der Datensatz während des Hochladens erneut
    /// geändert, bleibt er als geändert markiert.
    pub fn mark_pushed(&mut self, id: ItemId, rev: i64, pushed_blob: &[u8]) -> Result<()> {
        self.state()?;
        self.conn.execute(
            "UPDATE items SET rev = MAX(rev, ?2), dirty = CASE WHEN blob = ?3 THEN 0 ELSE 1 END WHERE id = ?1",
            params![id.as_bytes().as_slice(), rev, pushed_blob],
        )?;
        Ok(())
    }

    /// Übernimmt die Fassung eines Datensatzes vom Server und führt sie mit
    /// lokalen Änderungen zusammen.
    pub fn apply_remote(&mut self, id: ItemId, rev: i64, blob: &[u8]) -> Result<ApplyOutcome> {
        let vault_id = *self.header.vault_id.as_bytes();
        // Ein auf dieses Gerät beschränkter Datensatz nimmt nichts von außen an.
        if self.is_local_only(id)? {
            return Ok(ApplyOutcome::Unchanged);
        }
        let state = self.state_mut()?;
        let remote = open_record(&state.key, &vault_id, id, blob)?;
        // Ein Zeitstempel weit in der Zukunft würde alle Uhren dauerhaft
        // vorstellen; danach entschiede nicht mehr die Zeit über Änderungen.
        if remote.last_change().wall_ms > now_ms() + MAX_CLOCK_SKEW_MS {
            return Err(VaultError::ClockSkew);
        }
        state.clock.observe(remote.last_change());

        let Some(local) = state.records.get(&id).cloned() else {
            self.write_raw(id, remote, blob.to_vec(), Some(rev), false, None, None)?;
            return Ok(ApplyOutcome::Inserted);
        };

        let mut merged = local.clone();
        merged.merge(&remote)?;

        if merged == local && merged == remote {
            self.conn.execute(
                "UPDATE items SET rev = ?2, dirty = 0, blob = ?3 WHERE id = ?1",
                params![id.as_bytes().as_slice(), rev, blob],
            )?;
            return Ok(ApplyOutcome::Unchanged);
        }
        if merged == remote {
            // Der Server hat alles, was wir haben, und mehr.
            self.write_raw(id, remote, blob.to_vec(), Some(rev), false, Some(&local), None)?;
            return Ok(ApplyOutcome::Updated);
        }
        // Lokale Änderungen fehlen dem Server noch (reine lokale Führung oder
        // echter Konflikt): Ergebnis speichern und zum Hochladen vormerken.
        let outcome = if merged == local { ApplyOutcome::Unchanged } else { ApplyOutcome::Merged };
        self.write(merged, Some(rev), true, None)?;
        Ok(outcome)
    }

    /// Letzte vom Server vollständig abgeholte Revision.
    /// Der Server hält für `id` eine Fassung, die sich nicht entschlüsseln lässt (Revision `rev`) – etwa Datenmüll
    /// von jemandem mit einem gestohlenen Sitzungstoken. Die eigene, lesbare Fassung gilt weiter und wird beim
    /// nächsten Hochladen über die des Servers geschrieben. `false`, wenn es hier keine lesbare Fassung gibt.
    pub fn reclaim(&mut self, id: ItemId, rev: i64) -> Result<bool> {
        if !self.state()?.records.contains_key(&id) || self.is_local_only(id)? {
            return Ok(false);
        }
        self.conn
            .execute("UPDATE items SET rev = ?2, dirty = 1 WHERE id = ?1", params![id.as_bytes().as_slice(), rev])?;
        Ok(true)
    }

    pub fn sync_cursor(&self) -> Result<i64> {
        Ok(get_meta(&self.conn, "sync_cursor")?
            .and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok())
            .map(i64::from_le_bytes)
            .unwrap_or(0))
    }

    pub fn set_sync_cursor(&mut self, rev: i64) -> Result<()> {
        put_meta(&self.conn, "sync_cursor", &rev.to_le_bytes())
    }

    /// Markiert alle Datensätze als hochzuladen und setzt den Abholstand
    /// zurück – nötig beim erstmaligen Verbinden mit einem Server.
    pub fn reset_sync_state(&mut self) -> Result<()> {
        self.conn.execute("UPDATE items SET dirty = 1, rev = 0", [])?;
        self.set_sync_cursor(0)
    }

    // ---------------------------------------------------------------- Lokale Geheimnisse

    /// Speichert einen gerätegebundenen, verschlüsselten Wert (z. B.
    /// Sitzungstokens oder MCP-Freigaben). Solche Werte werden nie abgeglichen.
    pub fn put_secret(&mut self, name: &str, value: &[u8]) -> Result<()> {
        let state = self.state()?;
        let blob = seal_item(&state.key, self.header.vault_id.as_bytes(), &secret_aad(name), value)?;
        self.conn.execute(
            "INSERT INTO secrets (name, blob) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET blob = excluded.blob",
            params![name, blob],
        )?;
        Ok(())
    }

    pub fn get_secret(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let state = self.state()?;
        let blob: Option<Vec<u8>> =
            self.conn.query_row("SELECT blob FROM secrets WHERE name = ?1", params![name], |r| r.get(0)).optional()?;
        match blob {
            Some(blob) => Ok(Some(open_item(&state.key, self.header.vault_id.as_bytes(), &secret_aad(name), &blob)?)),
            None => Ok(None),
        }
    }

    pub fn delete_secret(&mut self, name: &str) -> Result<()> {
        self.state()?;
        self.conn.execute("DELETE FROM secrets WHERE name = ?1", params![name])?;
        Ok(())
    }

    // ---------------------------------------------------------------- Protokoll

    /// Hängt einen verschlüsselten Eintrag an das gerätegebundene Protokoll an
    /// (z. B. Zugriffe über MCP) und gibt seine laufende Nummer zurück.
    pub fn append_journal(&mut self, entry: &[u8]) -> Result<i64> {
        let state = self.state()?;
        // Die Nummer wird erst beim Einfügen vergeben; gebunden wird das
        // Chiffrat deshalb an eine zufällige Kennung, die mitgespeichert wird.
        let binding = random_bytes::<16>()?;
        let mut blob = binding.to_vec();
        blob.extend(seal_item(&state.key, self.header.vault_id.as_bytes(), &binding, entry)?);
        self.conn.execute("INSERT INTO journal (blob) VALUES (?1)", params![blob])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Liest die jüngsten Protokolleinträge, neueste zuerst. Mit `before`
    /// lässt sich seitenweise weiter zurückblättern.
    pub fn read_journal(&self, limit: usize, before: Option<i64>) -> Result<Vec<(i64, Vec<u8>)>> {
        let state = self.state()?;
        let mut stmt = self.conn.prepare("SELECT seq, blob FROM journal WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![before.unwrap_or(i64::MAX), limit as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, blob) = row?;
            let Some((binding, sealed)) = blob.split_at_checked(16) else { continue };
            let Ok(binding) = <[u8; 16]>::try_from(binding) else { continue };
            if let Ok(entry) = open_item(&state.key, self.header.vault_id.as_bytes(), &binding, sealed) {
                out.push((seq, entry));
            }
        }
        Ok(out)
    }

    /// Löscht das Protokoll bis einschließlich Eintrag `up_to`.
    pub fn trim_journal(&mut self, up_to: i64) -> Result<()> {
        self.state()?;
        self.conn.execute("DELETE FROM journal WHERE seq <= ?1", params![up_to])?;
        Ok(())
    }

    /// Schreibt eine konsistente Kopie des (weiterhin verschlüsselten)
    /// Tresors in eine neue Datei – als Sicherung oder zum Übertragen.
    pub fn export_to(&self, path: &Path) -> Result<()> {
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        self.conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
        // Die Kopie soll auf einem anderen Gerät als eigenständiger Tresor
        // dienen können: Gerätegebundenes (Sitzungstokens, MCP-Freigaben,
        // Protokoll, Gerätekennung, Abgleichstand) bleibt hier – ebenso
        // Datensätze, die als „nur dieses Gerät" gekennzeichnet sind.
        let copy = Connection::open(path)?;
        copy.execute_batch(
            "DELETE FROM secrets;
             DELETE FROM journal;
             DELETE FROM history WHERE id IN (SELECT id FROM items WHERE local_only = 1);
             DELETE FROM items WHERE local_only = 1;
             DELETE FROM meta WHERE key IN ('node', 'sync_cursor');
             UPDATE items SET rev = 0, dirty = 1;
             VACUUM;",
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- Intern

    fn state(&self) -> Result<&Unlocked> {
        self.unlocked.as_ref().ok_or(VaultError::Locked)
    }

    fn state_mut(&mut self) -> Result<&mut Unlocked> {
        self.unlocked.as_mut().ok_or(VaultError::Locked)
    }

    fn tick(&mut self) -> Result<Hlc> {
        Ok(self.state_mut()?.clock.now())
    }

    /// Verschlüsselt und speichert einen Datensatz.
    fn write(&mut self, record: Record, rev: Option<i64>, dirty: bool, local_only: Option<bool>) -> Result<()> {
        let state = self.state()?;
        let id = record.id;
        let blob = seal_item(&state.key, self.header.vault_id.as_bytes(), id.as_bytes(), &record.to_bytes()?)?;
        let previous = state.records.get(&id).cloned();
        self.write_raw(id, record, blob, rev, dirty, previous.as_ref(), local_only)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_raw(
        &mut self,
        id: ItemId,
        record: Record,
        blob: Vec<u8>,
        rev: Option<i64>,
        dirty: bool,
        previous: Option<&Record>,
        local_only: Option<bool>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        if record.is_deleted() {
            // Gelöscht heißt gelöscht: Auch frühere Fassungen verschwinden.
            tx.execute("DELETE FROM history WHERE id = ?1", params![id.as_bytes().as_slice()])?;
        } else if previous.is_some_and(|p| *p != record) {
            tx.execute(
                "INSERT INTO history (id, replaced_at, blob) SELECT id, ?2, blob FROM items WHERE id = ?1",
                params![id.as_bytes().as_slice(), now_ms() as i64],
            )?;
            tx.execute(
                "DELETE FROM history WHERE id = ?1 AND seq NOT IN
                   (SELECT seq FROM history WHERE id = ?1 ORDER BY seq DESC LIMIT ?2)",
                params![id.as_bytes().as_slice(), HISTORY_DEPTH],
            )?;
        }
        tx.execute(
            "INSERT INTO items (id, blob, rev, dirty, local_only) VALUES (?1, ?2, COALESCE(?3, 0), ?4, COALESCE(?5, 0))
             ON CONFLICT(id) DO UPDATE SET blob = excluded.blob, rev = COALESCE(?3, rev), dirty = excluded.dirty,
                                           local_only = COALESCE(?5, local_only)",
            params![id.as_bytes().as_slice(), blob, rev, dirty, local_only],
        )?;
        tx.commit()?;
        if let Some(state) = self.unlocked.as_mut() {
            state.records.insert(id, record);
        }
        Ok(())
    }
}

fn configure(conn: &Connection) -> Result<()> {
    // Greifen zwei Prozesse gleichzeitig zu, wartet der zweite kurz, statt zu scheitern.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // Gelöschte Inhalte werden überschrieben statt nur freigegeben.
    conn.pragma_update(None, "secure_delete", true)?;
    Ok(())
}

fn put_meta(conn: &Connection, key: &str, value: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn get_meta(conn: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional()?)
}

fn open_record(key: &VaultKey, vault_id: &[u8; 16], id: ItemId, blob: &[u8]) -> Result<Record> {
    let plain = open_item(key, vault_id, id.as_bytes(), blob)?;
    let record = Record::from_bytes(&plain)?;
    if record.id != id {
        return Err(VaultError::Crypto(CryptoError::Malformed));
    }
    Ok(record)
}

/// Gerätegebundene Geheimnisse bekommen eine aus dem Namen abgeleitete
/// Kennung, damit sich ihre Chiffrate nicht untereinander vertauschen lassen.
fn secret_aad(name: &str) -> [u8; 16] {
    *Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("tephra/secret/{name}").as_bytes()).as_bytes()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tephra_model::Hlc;
    use tephra_model::{Host, Identity, SshKey};

    const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;

    fn host(label: &str) -> Host {
        Host { label: label.into(), address: format!("{label}.example.org"), ..Host::default() }
    }

    fn new_vault(dir: &tempfile::TempDir, name: &str) -> (Vault, RecoveryKey) {
        Vault::create(&dir.path().join(name), "passwort", KDF).expect("anlegen")
    }

    /// Zweites Gerät desselben Kontos: gleicher Kopf, gleicher Schlüssel, leere Datenbank.
    fn second_device(dir: &tempfile::TempDir, first: &Vault, name: &str) -> Vault {
        Vault::create_with_header(&dir.path().join(name), first.header().clone(), first.vault_key().expect("key"))
            .expect("zweites gerät")
    }

    #[test]
    fn anlegen_sperren_entsperren() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let id = {
            let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
            vault.insert(&host("web")).expect("insert")
        };
        let mut vault = Vault::open(&path).expect("öffnen");
        assert!(!vault.is_unlocked());
        assert!(matches!(vault.list::<Host>(), Err(VaultError::Locked)));
        assert!(matches!(vault.unlock("falsch"), Err(VaultError::WrongPassword)));
        vault.unlock("passwort").expect("entsperren");
        assert_eq!(vault.get::<Host>(id).expect("get").expect("da").label, "web");
        assert!(vault.get::<Identity>(id).expect("get").is_none());
        vault.lock();
        assert!(matches!(vault.get::<Host>(id), Err(VaultError::Locked)));
    }

    #[test]
    fn datei_enthaelt_keinen_klartext() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        {
            let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
            vault
                .insert(&SshKey {
                    label: "EINDEUTIGES-ETIKETT".into(),
                    private_key: "STRENG-GEHEIM-123".into(),
                    ..SshKey::default()
                })
                .expect("insert");
            vault.put_secret("token", b"LOKALES-GEHEIMNIS-456").expect("secret");
        }
        let mut raw = std::fs::read(&path).expect("lesen");
        if let Ok(wal) = std::fs::read(dir.path().join("v.tephra-wal")) {
            raw.extend(wal);
        }
        for needle in [&b"EINDEUTIGES-ETIKETT"[..], b"STRENG-GEHEIM-123", b"LOKALES-GEHEIMNIS-456"] {
            assert!(!raw.windows(needle.len()).any(|w| w == needle), "Klartext in der Tresordatei gefunden");
        }
    }

    #[test]
    fn aendern_loeschen_verlauf() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut vault, _) = new_vault(&dir, "v.tephra");
        let id = vault.insert(&host("web")).expect("insert");
        let mut h = vault.get::<Host>(id).expect("get").expect("da");
        h.port = 2222;
        assert!(vault.update(id, &h).expect("update"));
        assert!(!vault.update(id, &h).expect("update"));
        let history = vault.history(id).expect("verlauf");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].record.entity::<Host>().expect("host").port, 22);

        vault.delete(id).expect("delete");
        assert!(vault.get::<Host>(id).expect("get").is_none());
        assert!(vault.list::<Host>().expect("list").is_empty());
        assert!(matches!(vault.update(ItemId::new(), &h), Err(VaultError::NotFound(_))));
    }

    #[test]
    fn verlauf_ist_begrenzt() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut vault, _) = new_vault(&dir, "v.tephra");
        let id = vault.insert(&host("web")).expect("insert");
        for port in 1000..1030u16 {
            let mut h = vault.get::<Host>(id).expect("get").expect("da");
            h.port = port;
            vault.update(id, &h).expect("update");
        }
        assert_eq!(vault.history(id).expect("verlauf").len(), HISTORY_DEPTH as usize);
    }

    #[test]
    fn passwort_aendern_und_wiederherstellen() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (mut vault, recovery) = Vault::create(&path, "alt", KDF).expect("anlegen");
        let id = vault.insert(&host("web")).expect("insert");
        vault.change_password("neu", KDF).expect("ändern");
        drop(vault);

        let mut vault = Vault::open(&path).expect("öffnen");
        assert!(matches!(vault.unlock("alt"), Err(VaultError::WrongPassword)));
        vault.unlock("neu").expect("entsperren");
        vault.lock();

        let wrong = RecoveryKey::generate().expect("zufall");
        assert!(matches!(vault.recover(&wrong, "x", KDF), Err(VaultError::WrongPassword)));
        vault.recover(&recovery, "ganz neu", KDF).expect("wiederherstellen");
        assert!(vault.get::<Host>(id).expect("get").is_some());
        vault.lock();
        vault.unlock("ganz neu").expect("entsperren");
    }

    #[test]
    fn abgleich_zwischen_zwei_geraeten() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");

        // A legt an und lädt hoch.
        let id = a.insert(&host("web")).expect("insert");
        let dirty = a.dirty_items().expect("dirty");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].base_rev, 0);
        a.mark_pushed(id, 1, &dirty[0].blob).expect("pushed");
        assert!(a.dirty_items().expect("dirty").is_empty());

        // B holt ab.
        assert_eq!(b.apply_remote(id, 1, &dirty[0].blob).expect("apply"), ApplyOutcome::Inserted);
        assert!(b.dirty_items().expect("dirty").is_empty());

        // Beide ändern unterschiedliche Felder.
        let mut ha = a.get::<Host>(id).expect("get").expect("da");
        ha.label = "webserver".into();
        a.update(id, &ha).expect("update");
        let mut hb = b.get::<Host>(id).expect("get").expect("da");
        hb.port = 2222;
        b.update(id, &hb).expect("update");

        // A ist zuerst beim Server (rev 2); B bekommt einen Konflikt, holt ab, führt zusammen.
        let a_push = a.dirty_items().expect("dirty").remove(0);
        a.mark_pushed(id, 2, &a_push.blob).expect("pushed");
        assert_eq!(b.apply_remote(id, 2, &a_push.blob).expect("apply"), ApplyOutcome::Merged);
        let b_push = b.dirty_items().expect("dirty").remove(0);
        assert_eq!(b_push.base_rev, 2);
        b.mark_pushed(id, 3, &b_push.blob).expect("pushed");

        // A holt das Ergebnis ab.
        assert_eq!(a.apply_remote(id, 3, &b_push.blob).expect("apply"), ApplyOutcome::Updated);
        let fa = a.get::<Host>(id).expect("get").expect("da");
        let fb = b.get::<Host>(id).expect("get").expect("da");
        assert_eq!(fa, fb);
        assert_eq!(fa.label, "webserver");
        assert_eq!(fa.port, 2222);
        assert!(a.dirty_items().expect("dirty").is_empty());
        assert!(b.dirty_items().expect("dirty").is_empty());
        // Die verdrängte Fassung liegt im Verlauf.
        assert!(!a.history(id).expect("verlauf").is_empty());
    }

    #[test]
    fn aenderung_waehrend_des_hochladens_bleibt_vorgemerkt() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let id = a.insert(&host("web")).expect("insert");
        let in_flight = a.dirty_items().expect("dirty").remove(0);
        let mut h = a.get::<Host>(id).expect("get").expect("da");
        h.notes = "zwischendurch geändert".into();
        a.update(id, &h).expect("update");
        a.mark_pushed(id, 1, &in_flight.blob).expect("pushed");
        let dirty = a.dirty_items().expect("dirty");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].base_rev, 1);
    }

    #[test]
    fn loeschung_wird_abgeglichen() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");
        let id = a.insert(&host("web")).expect("insert");
        let first = a.dirty_items().expect("dirty").remove(0);
        a.mark_pushed(id, 1, &first.blob).expect("pushed");
        b.apply_remote(id, 1, &first.blob).expect("apply");

        a.delete(id).expect("delete");
        let tombstone = a.dirty_items().expect("dirty").remove(0);
        assert!(tombstone.deleted);
        assert_eq!(b.apply_remote(id, 2, &tombstone.blob).expect("apply"), ApplyOutcome::Updated);
        assert!(b.get::<Host>(id).expect("get").is_none());
    }

    #[test]
    fn nur_lokale_datensaetze_werden_nicht_hochgeladen() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let id = a.insert(&SshKey { label: "nur hier".into(), ..SshKey::default() }).expect("insert");
        a.set_local_only(id, true).expect("local");
        assert!(a.is_local_only(id).expect("abfrage"));
        assert!(a.dirty_items().expect("dirty").is_empty());
        a.set_local_only(id, false).expect("local");
        assert_eq!(a.dirty_items().expect("dirty").len(), 1);
    }

    #[test]
    fn fremdes_chiffrat_wird_abgelehnt() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let (mut fremd, _) = new_vault(&dir, "fremd.tephra");
        let id = fremd.insert(&host("böse")).expect("insert");
        let blob = fremd.dirty_items().expect("dirty").remove(0).blob;
        assert!(a.apply_remote(id, 1, &blob).is_err());

        // Auch ein Chiffrat aus dem eigenen Tresor lässt sich keinem anderen Datensatz unterschieben.
        let own = a.insert(&host("eigen")).expect("insert");
        let own_blob = a.dirty_items().expect("dirty").remove(0).blob;
        assert!(a.apply_remote(ItemId::new(), 1, &own_blob).is_err());
        assert!(a.get::<Host>(own).expect("get").is_some());
    }

    #[test]
    fn lokale_geheimnisse() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
        assert!(vault.get_secret("token").expect("get").is_none());
        vault.put_secret("token", b"abc").expect("put");
        vault.put_secret("token", b"def").expect("put");
        assert_eq!(vault.get_secret("token").expect("get").as_deref(), Some(&b"def"[..]));
        vault.delete_secret("token").expect("delete");
        assert!(vault.get_secret("token").expect("get").is_none());
        vault.lock();
        assert!(matches!(vault.get_secret("token"), Err(VaultError::Locked)));
    }

    #[test]
    fn protokoll() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
        for i in 0..5u8 {
            vault.append_journal(format!("EINTRAG-NUMMER-{i}").as_bytes()).expect("append");
        }
        let newest = vault.read_journal(2, None).expect("lesen");
        assert_eq!(newest.len(), 2);
        assert_eq!(newest[0].1, b"EINTRAG-NUMMER-4");
        let older = vault.read_journal(10, Some(newest[1].0)).expect("lesen");
        assert_eq!(older.len(), 3);
        vault.trim_journal(older[0].0).expect("kürzen");
        assert_eq!(vault.read_journal(10, None).expect("lesen").len(), 2);

        drop(vault);
        let raw = std::fs::read(&path).expect("lesen");
        assert!(!raw.windows(15).any(|w| w == b"EINTRAG-NUMMER-"), "Protokoll liegt im Klartext vor");
    }

    #[test]
    fn export_ist_ein_vollstaendiger_tresor() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut vault, _) = new_vault(&dir, "v.tephra");
        let id = vault.insert(&host("web")).expect("insert");
        let backup = dir.path().join("sicherung.tephra");
        vault.export_to(&backup).expect("export");
        assert!(matches!(vault.export_to(&backup), Err(VaultError::AlreadyExists)));
        let mut copy = Vault::open(&backup).expect("öffnen");
        copy.unlock("passwort").expect("entsperren");
        assert!(copy.get::<Host>(id).expect("get").is_some());
    }

    #[test]
    fn falscher_schluessel_entsperrt_auch_einen_leeren_tresor_nicht() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        drop(Vault::create(&path, "passwort", KDF).expect("anlegen"));
        let mut vault = Vault::open(&path).expect("öffnen");
        let foreign = VaultKey::generate().expect("zufall");
        assert!(matches!(vault.unlock_with_key(foreign), Err(VaultError::WrongPassword)));
        assert!(!vault.is_unlocked());
        vault.unlock("passwort").expect("entsperren");
    }

    #[test]
    fn untergeschobener_kopf_wird_abgelehnt() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");

        // Passwortwechsel auf Gerät A ergibt einen Kopf, den B annimmt.
        a.change_password("neues passwort", KDF).expect("ändern");
        b.replace_header(a.header().clone()).expect("echter kopf");
        b.lock();
        assert!(matches!(b.unlock("passwort"), Err(VaultError::WrongPassword)));
        b.unlock("neues passwort").expect("entsperren");

        // Ein Kopf mit unbrauchbarer Verpackung (etwa von einem bösartigen
        // Server) würde den Nutzer nach dem nächsten Sperren aussperren.
        let mut garbage = a.header().clone();
        garbage.wrapped_key = WrappedKey(vec![1; 73]);
        garbage.recovery_wrapped_key = WrappedKey(vec![1; 73]);
        assert!(matches!(b.replace_header(garbage), Err(VaultError::HeaderTampered)));

        // Auch abgeschwächte Passwortparameter fallen auf.
        let mut weak = a.header().clone();
        weak.kdf = KdfParams { memory_kib: 8, iterations: 1, parallelism: 1 };
        assert!(matches!(b.replace_header(weak), Err(VaultError::HeaderTampered)));

        // Ein fremder Tresor wird nicht als zweites Gerät akzeptiert.
        let (other, _) = new_vault(&dir, "fremd.tephra");
        let result = Vault::create_with_header(
            &dir.path().join("c.tephra"),
            other.header().clone(),
            a.vault_key().expect("key"),
        );
        assert!(matches!(result, Err(VaultError::HeaderTampered)));
    }

    #[test]
    fn veraenderter_kopf_in_der_datei_faellt_beim_entsperren_auf() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let key = {
            let (vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
            vault.vault_key().expect("key")
        };
        // Jemand mit Dateizugriff tauscht die Wiederherstellungs-Verpackung aus.
        {
            let conn = Connection::open(&path).expect("db");
            let mut header: VaultHeader =
                serde_json::from_slice(&get_meta(&conn, "header").expect("meta").expect("kopf")).expect("json");
            header.recovery_wrapped_key = WrappedKey(vec![7; 73]);
            put_meta(&conn, "header", &serde_json::to_vec(&header).expect("json")).expect("schreiben");
        }
        let mut vault = Vault::open(&path).expect("öffnen");
        assert!(matches!(vault.unlock("passwort"), Err(VaultError::HeaderTampered)));
        assert!(matches!(vault.unlock_with_key(key), Err(VaultError::HeaderTampered)));
    }

    #[test]
    fn schwache_passwortparameter_werden_abgelehnt() {
        let dir = tempfile::tempdir().expect("tmp");
        let weak = KdfParams { memory_kib: 8, iterations: 1, parallelism: 1 };
        assert!(matches!(Vault::create(&dir.path().join("w.tephra"), "passwort", weak), Err(VaultError::WeakKdf)));
        let (mut vault, _) = new_vault(&dir, "v.tephra");
        assert!(matches!(vault.change_password("neu", weak), Err(VaultError::WeakKdf)));
    }

    #[test]
    fn nur_dieses_geraet_gilt_von_anfang_an() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");

        let id = a.insert_local_only(&SshKey { label: "nur hier".into(), ..SshKey::default() }).expect("insert");
        assert!(a.is_local_only(id).expect("abfrage"));
        assert!(a.dirty_items().expect("dirty").is_empty(), "darf nie zum Hochladen vorgemerkt sein");

        // Änderungen ändern daran nichts.
        let mut key = a.get::<SshKey>(id).expect("get").expect("da");
        key.label = "umbenannt".into();
        a.update(id, &key).expect("update");
        assert!(a.dirty_items().expect("dirty").is_empty());

        // Ein Löschmarker von außen entfernt den lokalen Schlüssel nicht.
        let mut tombstone =
            Record::create_with_id(id, &SshKey::default(), Hlc { wall_ms: u64::MAX / 2, counter: 0, node: 9 })
                .expect("record");
        tombstone.delete(Hlc { wall_ms: u64::MAX / 2 + 1, counter: 0, node: 9 });
        let blob = seal_item(
            &a.vault_key().expect("key"),
            a.header().vault_id.as_bytes(),
            id.as_bytes(),
            &tombstone.to_bytes().expect("bytes"),
        )
        .expect("seal");
        assert_eq!(a.apply_remote(id, 5, &blob).expect("apply"), ApplyOutcome::Unchanged);
        assert_eq!(a.get::<SshKey>(id).expect("get").expect("noch da").label, "umbenannt");

        // Was schon abgeglichen wurde, lässt sich nicht nachträglich beschränken.
        let synced = a.insert(&host("web")).expect("insert");
        let pushed = a.dirty_items().expect("dirty").remove(0);
        a.mark_pushed(synced, 1, &pushed.blob).expect("pushed");
        assert!(matches!(a.set_local_only(synced, true), Err(VaultError::AlreadySynced(_))));
        b.apply_remote(synced, 1, &pushed.blob).expect("apply");
    }

    #[test]
    fn loeschen_entfernt_inhalt_und_verlauf() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
        let id = vault
            .insert(&SshKey { label: "alt".into(), private_key: "GEHEIMER-SCHLUESSEL".into(), ..SshKey::default() })
            .expect("insert");
        let mut key = vault.get::<SshKey>(id).expect("get").expect("da");
        key.label = "neu".into();
        vault.update(id, &key).expect("update");
        assert_eq!(vault.history(id).expect("verlauf").len(), 1);

        vault.delete(id).expect("delete");
        assert!(vault.history(id).expect("verlauf").is_empty());
        assert!(matches!(vault.delete(id), Err(VaultError::NotFound(_))));
        assert!(matches!(vault.update(id, &key), Err(VaultError::NotFound(_))));

        // Der Löschmarker, der zum Server geht, enthält nichts mehr vom Schlüssel.
        let tombstone = vault.dirty_items().expect("dirty").remove(0);
        assert!(tombstone.deleted);
        let record =
            open_record(&vault.vault_key().expect("key"), vault.header().vault_id.as_bytes(), id, &tombstone.blob)
                .expect("öffnen");
        assert!(record.fields.is_empty());
    }

    #[test]
    fn feste_kennung_anlegen_aendern_wiederbeleben() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");
        let id = ItemId(Uuid::new_v5(&Uuid::NAMESPACE_OID, b"einstellungen"));

        assert!(a.upsert_with_id(id, &host("eins")).expect("upsert"));
        assert!(!a.upsert_with_id(id, &host("eins")).expect("upsert"));
        assert!(a.upsert_with_id(id, &host("zwei")).expect("upsert"));
        assert_eq!(a.list::<Host>().expect("liste").len(), 1);

        // Nach dem Löschen lässt sich dieselbe Kennung neu belegen …
        a.delete(id).expect("delete");
        assert!(a.get::<Host>(id).expect("get").is_none());
        assert!(a.upsert_with_id(id, &host("drei")).expect("upsert"));
        assert_eq!(a.get::<Host>(id).expect("get").expect("da").label, "drei");

        // … und zwei Geräte, die unabhängig dasselbe anlegen, landen beim selben Datensatz.
        b.upsert_with_id(id, &host("von b")).expect("upsert");
        let from_a = a.dirty_items().expect("dirty").remove(0);
        let from_b = b.dirty_items().expect("dirty").remove(0);
        a.apply_remote(id, 1, &from_b.blob).expect("apply");
        b.apply_remote(id, 1, &from_a.blob).expect("apply");
        assert_eq!(a.get::<Host>(id).expect("get"), b.get::<Host>(id).expect("get"));
        assert_eq!(a.list::<Host>().expect("liste").len(), 1);
    }

    #[test]
    fn verspaetete_bestaetigung_setzt_die_revision_nicht_zurueck() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");
        let id = a.insert(&host("web")).expect("insert");
        let first = a.dirty_items().expect("dirty").remove(0);
        b.apply_remote(id, 1, &first.blob).expect("apply");
        let mut h = b.get::<Host>(id).expect("get").expect("da");
        h.port = 2222;
        b.update(id, &h).expect("update");
        let newer = b.dirty_items().expect("dirty").remove(0);

        // Bei A trifft erst die neuere Fassung ein (rev 2), dann die
        // verspätete Bestätigung des eigenen Hochladens (rev 1).
        a.apply_remote(id, 2, &newer.blob).expect("apply");
        a.mark_pushed(id, 1, &first.blob).expect("pushed");
        let mut h = a.get::<Host>(id).expect("get").expect("da");
        h.notes = "weiter".into();
        a.update(id, &h).expect("update");
        assert_eq!(a.dirty_items().expect("dirty")[0].base_rev, 2);
    }

    #[test]
    fn sicherung_enthaelt_nichts_geraetegebundenes() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut vault, _) = new_vault(&dir, "v.tephra");
        let id = vault.insert(&host("web")).expect("insert");
        let pushed = vault.dirty_items().expect("dirty").remove(0);
        vault.mark_pushed(id, 7, &pushed.blob).expect("pushed");
        vault.set_sync_cursor(7).expect("cursor");
        vault.put_secret("sitzung", b"token").expect("secret");
        vault.append_journal(b"eintrag").expect("journal");
        let local = vault.insert_local_only(&SshKey { label: "nur hier".into(), ..SshKey::default() }).expect("insert");

        let backup = dir.path().join("sicherung.tephra");
        vault.export_to(&backup).expect("export");
        let mut copy = Vault::open(&backup).expect("öffnen");
        copy.unlock("passwort").expect("entsperren");
        assert!(copy.get::<Host>(id).expect("get").is_some());
        assert!(copy.get_secret("sitzung").expect("secret").is_none());
        assert!(
            copy.get::<SshKey>(local).expect("get").is_none(),
            "nur lokale Datensätze dürfen nicht in die Sicherung"
        );
        assert!(copy.read_journal(10, None).expect("journal").is_empty());
        assert_eq!(copy.sync_cursor().expect("cursor"), 0);
        // Auf einem neuen Gerät muss alles neu hochgeladen werden können.
        assert_eq!(copy.dirty_items().expect("dirty")[0].base_rev, 0);
    }

    #[test]
    fn aelterer_echter_kopf_wird_nicht_wieder_angenommen() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let mut b = second_device(&dir, &a, "b.tephra");
        let old = a.header().clone();
        a.change_password("neues passwort", KDF).expect("ändern");
        let new = a.header().clone();
        assert!(new.generation > old.generation);

        b.replace_header(new.clone()).expect("neuer kopf");
        // Derselbe Kopf noch einmal ist in Ordnung, der alte nicht mehr.
        b.replace_header(new).expect("unverändert");
        assert!(matches!(b.replace_header(old), Err(VaultError::HeaderOutdated)));
        b.lock();
        assert!(matches!(b.unlock("passwort"), Err(VaultError::WrongPassword)));
        b.unlock("neues passwort").expect("entsperren");
    }

    #[test]
    fn aenderungen_eines_anderen_prozesses_werden_bemerkt() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (mut daemon, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
        let kept = daemon.insert(&host("bleibt")).expect("insert");
        let removed = daemon.insert(&host("wird gelöscht")).expect("insert");

        // Ein zweiter Prozess (hier: zweites Handle auf dieselbe Datei)
        // legt einen Host an und löscht einen anderen.
        let mut cli = Vault::open(&path).expect("öffnen");
        cli.unlock("passwort").expect("entsperren");
        let added = cli.insert(&host("neu")).expect("insert");
        cli.delete(removed).expect("delete");

        // Ohne Nachladen sähe der erste Prozess den alten Stand.
        daemon.refresh().expect("nachladen");
        assert!(daemon.get::<Host>(added).expect("get").is_some());
        assert!(daemon.get::<Host>(removed).expect("get").is_none());
        assert!(daemon.get::<Host>(kept).expect("get").is_some());

        // Schreibt der erste Prozess danach, bleibt die Änderung des zweiten erhalten.
        let mut h = daemon.get::<Host>(kept).expect("get").expect("da");
        h.port = 2222;
        daemon.update(kept, &h).expect("update");
        cli.refresh().expect("nachladen");
        assert_eq!(cli.get::<Host>(kept).expect("get").expect("da").port, 2222);
        assert!(cli.get::<Host>(added).expect("get").is_some());

        // Ein Passwortwechsel im anderen Prozess kommt ebenfalls an.
        cli.change_password("neues passwort", KDF).expect("ändern");
        daemon.refresh().expect("nachladen");
        assert_eq!(daemon.header(), cli.header());
    }

    #[test]
    fn zeitstempel_aus_ferner_zukunft_werden_abgelehnt() {
        let dir = tempfile::tempdir().expect("tmp");
        let (mut a, _) = new_vault(&dir, "a.tephra");
        let far = Hlc { wall_ms: now_ms() + 365 * 24 * 3600 * 1000, counter: 0, node: 9 };
        let record = Record::create(&host("aus der zukunft"), far).expect("record");
        let blob = seal_item(
            &a.vault_key().expect("key"),
            a.header().vault_id.as_bytes(),
            record.id.as_bytes(),
            &record.to_bytes().expect("bytes"),
        )
        .expect("seal");
        assert!(matches!(a.apply_remote(record.id, 1, &blob), Err(VaultError::ClockSkew)));
        // Die eigene Uhr wurde nicht vorgestellt.
        let id = a.insert(&host("jetzt")).expect("insert");
        let dirty = a.dirty_items().expect("dirty");
        let own = open_record(&a.vault_key().expect("key"), a.header().vault_id.as_bytes(), id, &dirty[0].blob)
            .expect("öffnen");
        assert!(own.last_change().wall_ms < far.wall_ms);
    }

    #[test]
    fn beschaedigter_datensatz_sperrt_nicht_aus_und_wird_repariert() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("v.tephra");
        let (good, broken, intact_blob) = {
            let (mut vault, _) = Vault::create(&path, "passwort", KDF).expect("anlegen");
            let good = vault.insert(&host("heil")).expect("insert");
            let broken = vault.insert(&host("kaputt")).expect("insert");
            let blob = vault.dirty_items().expect("dirty").into_iter().find(|d| d.id == broken).expect("blob").blob;
            (good, broken, blob)
        };
        // Ein Bit im gespeicherten Chiffrat kippt (Plattenfehler, abgebrochenes Schreiben).
        {
            let conn = Connection::open(&path).expect("db");
            let mut damaged = intact_blob.clone();
            let last = damaged.len() - 1;
            damaged[last] ^= 1;
            conn.execute("UPDATE items SET blob = ?2 WHERE id = ?1", params![broken.as_bytes().as_slice(), damaged])
                .expect("beschädigen");
        }
        let mut vault = Vault::open(&path).expect("öffnen");
        vault.unlock("passwort").expect("ein beschädigter Datensatz darf nicht aussperren");
        assert_eq!(vault.corrupt_items().expect("liste"), &[broken]);
        assert!(vault.get::<Host>(good).expect("get").is_some());
        assert!(vault.get::<Host>(broken).expect("get").is_none());
        // Das Unlesbare wird nicht an andere Geräte verteilt …
        assert!(vault.dirty_items().expect("dirty").iter().all(|d| d.id != broken));
        // … und die heile Fassung vom Server stellt den Datensatz wieder her.
        assert_eq!(vault.apply_remote(broken, 1, &intact_blob).expect("apply"), ApplyOutcome::Inserted);
        assert_eq!(vault.get::<Host>(broken).expect("get").expect("repariert").label, "kaputt");
    }

    #[test]
    fn beliebige_datei_ist_kein_tresor() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("x.tephra");
        std::fs::write(&path, b"das ist kein tresor").expect("schreiben");
        assert!(matches!(Vault::open(&path), Err(VaultError::NotAVault)));
        assert!(matches!(Vault::open(&dir.path().join("gibtsnicht")), Err(VaultError::NotAVault)));
    }
}
