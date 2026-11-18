//! Datenmodell von Tephra.
//!
//! * [`hlc`] – Zeitstempel für geräteübergreifend geordnete Änderungen
//! * [`record`] – generischer, feldweise zusammenführbarer Datensatz
//! * [`entities`] – typisierte Objekte (Hosts, Schlüssel, VPN-Profile …)
//! * [`secret`] – Behälter für Passwörter und Schlüssel

pub mod entities;
pub mod hlc;
pub mod record;
pub mod secret;

pub use entities::*;
pub use hlc::{Hlc, HlcClock};
pub use record::{Entity, Field, ItemId, ItemKind, Record, RecordError};
pub use secret::Secret;
