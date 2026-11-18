//! Generischer, zusammenführbarer Datensatz.
//!
//! Alle Objekte (Hosts, Schlüssel, VPN-Profile …) werden als [`Record`]
//! gespeichert und synchronisiert: eine Feldtabelle, in der jedes Feld seinen
//! eigenen Änderungszeitstempel trägt. Zwei Fassungen desselben Datensatzes
//! lassen sich damit feldweise zusammenführen, ohne dass eine Seite verliert.

use crate::hlc::Hlc;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Eindeutige Kennung eines Datensatzes (UUIDv7, zeitlich sortierbar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub Uuid);

impl ItemId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Art eines Datensatzes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Host,
    Group,
    Identity,
    SshKey,
    VpnProfile,
    KnownHost,
    Snippet,
    Settings,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Host => "host",
            ItemKind::Group => "group",
            ItemKind::Identity => "identity",
            ItemKind::SshKey => "ssh_key",
            ItemKind::VpnProfile => "vpn_profile",
            ItemKind::KnownHost => "known_host",
            ItemKind::Snippet => "snippet",
            ItemKind::Settings => "settings",
        }
    }
}

/// Typisiertes Objekt, das sich in einen [`Record`] abbilden lässt.
pub trait Entity: Serialize + DeserializeOwned {
    const KIND: ItemKind;
    /// Aktuelle Schemaversion; wird beim Schreiben im Datensatz vermerkt.
    const SCHEMA_VERSION: u32 = 1;
}

/// Ein Feldwert mit dem Zeitstempel seiner letzten Änderung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    #[serde(rename = "v")]
    pub value: Value,
    #[serde(rename = "t")]
    pub clock: Hlc,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("Datensatz ist vom Typ {found:?}, erwartet wurde {expected:?}")]
    WrongKind { expected: ItemKind, found: ItemKind },
    #[error("Objekt lässt sich nicht als Feldtabelle darstellen")]
    NotAnObject,
    #[error("Datensätze mit unterschiedlicher Kennung oder Art lassen sich nicht zusammenführen")]
    Mismatch,
    #[error("Datensatz wurde gelöscht")]
    Deleted,
    #[error("Serialisierung fehlgeschlagen: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Zusammenführbarer Datensatz.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: ItemId,
    pub kind: ItemKind,
    pub schema_version: u32,
    /// Zeitpunkt, zu dem dieser Datensatz (neu) angelegt wurde. Wird eine
    /// Kennung nach dem Löschen wiederverwendet, beginnt eine neue Fassung
    /// mit späterem Zeitpunkt; sie ersetzt die ältere vollständig.
    #[serde(default)]
    pub epoch: Hlc,
    pub fields: BTreeMap<String, Field>,
    /// Zeitpunkt der Löschung. Ein gelöschter Datensatz enthält keine Felder
    /// mehr und bleibt nur als Marker erhalten, damit die Löschung auch
    /// andere Geräte erreicht.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Hlc>,
}

impl Record {
    /// Legt einen neuen Datensatz aus einem typisierten Objekt an.
    pub fn create<T: Entity>(entity: &T, clock: Hlc) -> Result<Self, RecordError> {
        Self::create_with_id(ItemId::new(), entity, clock)
    }

    pub fn create_with_id<T: Entity>(id: ItemId, entity: &T, clock: Hlc) -> Result<Self, RecordError> {
        let mut record = Record {
            id,
            kind: T::KIND,
            schema_version: T::SCHEMA_VERSION,
            epoch: clock,
            fields: BTreeMap::new(),
            deleted: None,
        };
        record.update(entity, clock)?;
        Ok(record)
    }

    /// Übernimmt den Zustand von `entity`. Nur tatsächlich geänderte Felder
    /// erhalten den neuen Zeitstempel; unbekannte Felder (aus neueren
    /// Programmversionen) bleiben unangetastet. Gibt zurück, ob sich etwas
    /// geändert hat. Gelöschte Datensätze lassen sich nicht ändern.
    pub fn update<T: Entity>(&mut self, entity: &T, clock: Hlc) -> Result<bool, RecordError> {
        if self.kind != T::KIND {
            return Err(RecordError::WrongKind { expected: T::KIND, found: self.kind });
        }
        if self.is_deleted() {
            return Err(RecordError::Deleted);
        }
        let Value::Object(map) = serde_json::to_value(entity)? else {
            return Err(RecordError::NotAnObject);
        };
        let mut changed = false;
        for (name, value) in map {
            match self.fields.get_mut(&name) {
                Some(field) if field.value == value => {}
                Some(field) => {
                    field.value = value;
                    field.clock = clock;
                    changed = true;
                }
                None => {
                    self.fields.insert(name, Field { value, clock });
                    changed = true;
                }
            }
        }
        if changed {
            self.schema_version = self.schema_version.max(T::SCHEMA_VERSION);
        }
        Ok(changed)
    }

    /// Liest den Datensatz als typisiertes Objekt.
    pub fn entity<T: Entity>(&self) -> Result<T, RecordError> {
        if self.kind != T::KIND {
            return Err(RecordError::WrongKind { expected: T::KIND, found: self.kind });
        }
        let map: serde_json::Map<String, Value> =
            self.fields.iter().map(|(k, f)| (k.clone(), f.value.clone())).collect();
        Ok(serde_json::from_value(Value::Object(map))?)
    }

    /// Löscht den Datensatz. Seine Felder – und damit alle Geheimnisse –
    /// werden sofort entfernt; übrig bleibt nur der Löschmarker.
    pub fn delete(&mut self, clock: Hlc) {
        self.deleted = Some(self.deleted.map_or(clock, |d| d.max(clock)));
        self.fields.clear();
    }

    /// Jüngster Zeitstempel überhaupt (Anlage, Felder und Löschung).
    pub fn last_change(&self) -> Hlc {
        let fields = self.fields.values().map(|f| f.clock).max().unwrap_or(Hlc::ZERO);
        fields.max(self.deleted.unwrap_or(Hlc::ZERO)).max(self.epoch)
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted.is_some()
    }

    /// Führt `other` in diesen Datensatz ein.
    ///
    /// * Die später angelegte Fassung ersetzt die frühere vollständig.
    /// * Innerhalb derselben Fassung gewinnt pro Feld der jüngere Zeitstempel.
    /// * Eine Löschung gilt endgültig: Gleichzeitige Änderungen auf anderen
    ///   Geräten verlieren gegen sie.
    ///
    /// Die Operation ist kommutativ, assoziativ und idempotent; die
    /// Reihenfolge der Zusammenführung spielt also keine Rolle.
    pub fn merge(&mut self, other: &Record) -> Result<(), RecordError> {
        if self.id != other.id || self.kind != other.kind {
            return Err(RecordError::Mismatch);
        }
        let schema_version = self.schema_version.max(other.schema_version);
        match self.epoch.cmp(&other.epoch) {
            std::cmp::Ordering::Greater => {}
            std::cmp::Ordering::Less => *self = other.clone(),
            std::cmp::Ordering::Equal => {
                for (name, theirs) in &other.fields {
                    match self.fields.get_mut(name) {
                        Some(ours) => {
                            if field_wins(theirs, ours) {
                                *ours = theirs.clone();
                            }
                        }
                        None => {
                            self.fields.insert(name.clone(), theirs.clone());
                        }
                    }
                }
                self.deleted = match (self.deleted, other.deleted) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                };
                if self.deleted.is_some() {
                    self.fields.clear();
                }
            }
        }
        self.schema_version = schema_version;
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, RecordError> {
        Ok(serde_json::to_vec(self)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecordError> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

/// Entscheidet, ob `candidate` das Feld `current` ersetzt. Bei identischem
/// Zeitstempel (praktisch nur bei Uhrenfehlern) entscheidet die kanonische
/// Textform des Werts, damit beide Seiten zum selben Ergebnis kommen.
// JSON-Werte haben keine eigene Ordnung; verglichen wird ihre Textform.
#[allow(clippy::cmp_owned)]
fn field_wins(candidate: &Field, current: &Field) -> bool {
    match candidate.clock.cmp(&current.clock) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => candidate.value.to_string() > current.value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Host;
    use crate::hlc::HlcClock;
    use proptest::prelude::*;

    fn host(label: &str, address: &str) -> Host {
        Host { label: label.into(), address: address.into(), ..Host::default() }
    }

    #[test]
    fn hin_und_zurueck() {
        let mut clock = HlcClock::new(1);
        let original = host("web", "10.0.0.5");
        let record = Record::create(&original, clock.tick(10)).expect("anlegen");
        let back: Host = record.entity().expect("lesen");
        assert_eq!(back, original);
        let again = Record::from_bytes(&record.to_bytes().expect("bytes")).expect("parse");
        assert_eq!(again, record);
    }

    #[test]
    fn nur_geaenderte_felder_bekommen_neuen_zeitstempel() {
        let mut clock = HlcClock::new(1);
        let mut h = host("web", "10.0.0.5");
        let mut record = Record::create(&h, clock.tick(10)).expect("anlegen");
        h.port = 2222;
        let t2 = clock.tick(20);
        assert!(record.update(&h, t2).expect("update"));
        assert_eq!(record.fields["port"].clock, t2);
        assert!(record.fields["label"].clock < t2);
        assert!(!record.update(&h, clock.tick(30)).expect("update"));
    }

    #[test]
    fn gleichzeitige_aenderungen_verschiedener_felder_bleiben_erhalten() {
        let mut a_clock = HlcClock::new(1);
        let mut b_clock = HlcClock::new(2);
        let base = Record::create(&host("web", "10.0.0.5"), a_clock.tick(10)).expect("anlegen");

        let mut a = base.clone();
        let mut ha: Host = a.entity().expect("a");
        ha.label = "webserver".into();
        a.update(&ha, a_clock.tick(20)).expect("a update");

        let mut b = base.clone();
        let mut hb: Host = b.entity().expect("b");
        hb.address = "10.0.0.6".into();
        b.update(&hb, b_clock.tick(25)).expect("b update");

        let mut ab = a.clone();
        ab.merge(&b).expect("merge");
        let mut ba = b.clone();
        ba.merge(&a).expect("merge");
        assert_eq!(ab, ba);
        let merged: Host = ab.entity().expect("merged");
        assert_eq!(merged.label, "webserver");
        assert_eq!(merged.address, "10.0.0.6");
    }

    #[test]
    fn loeschung_entfernt_die_felder_und_ist_endgueltig() {
        let mut a_clock = HlcClock::new(1);
        let mut b_clock = HlcClock::new(2);
        let mut h = host("web", "10.0.0.5");
        let base = Record::create(&h, a_clock.tick(10)).expect("anlegen");

        let mut deleted = base.clone();
        deleted.delete(a_clock.tick(20));
        assert!(deleted.is_deleted());
        assert!(deleted.fields.is_empty(), "Löschmarker darf keine Inhalte behalten");
        assert!(matches!(deleted.update(&h, a_clock.tick(25)), Err(RecordError::Deleted)));

        // Eine spätere Änderung auf einem anderen Gerät verliert gegen die Löschung.
        let mut edited = base.clone();
        h.label = "doch behalten".into();
        edited.update(&h, b_clock.tick(30)).expect("update");
        let mut merged = edited.clone();
        merged.merge(&deleted).expect("merge");
        assert!(merged.is_deleted());
        assert!(merged.fields.is_empty());
        let mut other_way = deleted.clone();
        other_way.merge(&edited).expect("merge");
        assert_eq!(merged, other_way);
    }

    #[test]
    fn neu_angelegte_fassung_ersetzt_den_loeschmarker() {
        let mut clock = HlcClock::new(1);
        let id = ItemId::new();
        let mut old = Record::create_with_id(id, &host("alt", "10.0.0.5"), clock.tick(10)).expect("anlegen");
        old.delete(clock.tick(20));
        let fresh = Record::create_with_id(id, &host("neu", "10.0.0.6"), clock.tick(30)).expect("anlegen");

        let mut a = old.clone();
        a.merge(&fresh).expect("merge");
        let mut b = fresh.clone();
        b.merge(&old).expect("merge");
        assert_eq!(a, b);
        assert!(!a.is_deleted());
        assert_eq!(a.entity::<Host>().expect("host").label, "neu");
    }

    #[test]
    fn unbekannte_felder_ueberleben_ein_update() {
        let mut clock = HlcClock::new(1);
        let h = host("web", "10.0.0.5");
        let mut record = Record::create(&h, clock.tick(10)).expect("anlegen");
        record.fields.insert("feld_aus_der_zukunft".into(), Field { value: Value::from(42), clock: clock.tick(11) });
        record.update(&h, clock.tick(20)).expect("update");
        assert_eq!(record.fields["feld_aus_der_zukunft"].value, Value::from(42));
    }

    proptest! {
        #[test]
        fn beliebige_bytes_als_datensatz_stuerzen_nicht_ab(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            let _ = Record::from_bytes(&bytes);
        }
    }

    fn arb_record(id: ItemId) -> impl Strategy<Value = Record> {
        let field = (0u64..50, 0u32..3, 0u64..3, 0i64..5).prop_map(|(wall_ms, counter, node, v)| Field {
            value: Value::from(v),
            clock: Hlc { wall_ms, counter, node },
        });
        let deleted =
            proptest::option::of((0u64..50, 0u64..3).prop_map(|(wall_ms, node)| Hlc { wall_ms, counter: 0, node }));
        (proptest::collection::btree_map("[a-d]", field, 0..4), deleted, 0u64..3).prop_map(
            move |(mut fields, deleted, epoch)| {
                // Wie in echten Datensätzen: Ein Löschmarker hat keine Felder.
                if deleted.is_some() {
                    fields.clear();
                }
                Record {
                    id,
                    kind: ItemKind::Host,
                    schema_version: 1,
                    epoch: Hlc { wall_ms: epoch, counter: 0, node: 0 },
                    fields,
                    deleted,
                }
            },
        )
    }

    proptest! {
        #[test]
        fn merge_ist_kommutativ_assoziativ_idempotent(
            a in arb_record(ItemId(Uuid::nil())),
            b in arb_record(ItemId(Uuid::nil())),
            c in arb_record(ItemId(Uuid::nil())),
        ) {
            let mut ab = a.clone(); ab.merge(&b).unwrap();
            let mut ba = b.clone(); ba.merge(&a).unwrap();
            prop_assert_eq!(&ab, &ba);

            let mut ab_c = ab.clone(); ab_c.merge(&c).unwrap();
            let mut bc = b.clone(); bc.merge(&c).unwrap();
            let mut a_bc = a.clone(); a_bc.merge(&bc).unwrap();
            prop_assert_eq!(&ab_c, &a_bc);

            let mut aa = a.clone(); aa.merge(&a).unwrap();
            prop_assert_eq!(&aa, &a);

            // Innerhalb derselben Fassung geht ohne Löschung kein Feld verloren.
            if a.epoch == b.epoch && !ab.is_deleted() {
                for name in a.fields.keys().chain(b.fields.keys()) {
                    prop_assert!(ab.fields.contains_key(name));
                }
            }
            // Ein Löschmarker enthält nie Inhalte.
            prop_assert!(!ab.is_deleted() || ab.fields.is_empty());
        }
    }
}
