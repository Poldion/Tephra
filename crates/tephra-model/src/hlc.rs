//! Hybrid Logical Clock.
//!
//! Jede Feldänderung bekommt einen Zeitstempel, der Wanduhrzeit, einen Zähler
//! und die Geräte-ID kombiniert. Dadurch sind Änderungen geräteübergreifend
//! total geordnet, auch wenn Uhren leicht voneinander abweichen.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Zeitstempel einer Änderung. Die Ordnung ist `(wall_ms, counter, node)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct Hlc {
    /// Millisekunden seit der Unix-Epoche.
    pub wall_ms: u64,
    /// Zähler für Ereignisse innerhalb derselben Millisekunde.
    pub counter: u32,
    /// Kennung des erzeugenden Geräts, entscheidet bei Gleichstand.
    pub node: u64,
}

impl Hlc {
    pub const ZERO: Hlc = Hlc { wall_ms: 0, counter: 0, node: 0 };
}

/// Erzeugt monoton steigende Zeitstempel für ein Gerät.
#[derive(Debug, Clone)]
pub struct HlcClock {
    node: u64,
    last: Hlc,
}

impl HlcClock {
    pub fn new(node: u64) -> Self {
        Self { node, last: Hlc { wall_ms: 0, counter: 0, node } }
    }

    /// Setzt die Uhr auf einen zuvor gespeicherten Stand zurück.
    pub fn resume(node: u64, last: Hlc) -> Self {
        Self { node, last: Hlc { node, ..last } }
    }

    pub fn node(&self) -> u64 {
        self.node
    }

    pub fn last(&self) -> Hlc {
        self.last
    }

    /// Nächster Zeitstempel auf Basis der Systemzeit.
    pub fn now(&mut self) -> Hlc {
        self.tick(system_ms())
    }

    /// Nächster Zeitstempel für eine vorgegebene Wanduhrzeit (für Tests).
    pub fn tick(&mut self, physical_ms: u64) -> Hlc {
        if physical_ms > self.last.wall_ms {
            self.last = Hlc { wall_ms: physical_ms, counter: 0, node: self.node };
        } else if self.last.counter == u32::MAX {
            // Zähler erschöpft: in die nächste Millisekunde übertragen, damit
            // nie zwei gleiche Zeitstempel entstehen.
            self.last = Hlc { wall_ms: self.last.wall_ms + 1, counter: 0, node: self.node };
        } else {
            self.last.counter += 1;
        }
        self.last
    }

    /// Nimmt einen fremden Zeitstempel zur Kenntnis, damit spätere eigene
    /// Zeitstempel garantiert größer sind.
    pub fn observe(&mut self, remote: Hlc) {
        if (remote.wall_ms, remote.counter) > (self.last.wall_ms, self.last.counter) {
            self.last = Hlc { wall_ms: remote.wall_ms, counter: remote.counter, node: self.node };
        }
    }
}

fn system_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monoton_auch_bei_stehender_uhr() {
        let mut c = HlcClock::new(7);
        let a = c.tick(1000);
        let b = c.tick(1000);
        let d = c.tick(999);
        assert!(a < b && b < d);
    }

    #[test]
    fn erschoepfter_zaehler_wird_uebertragen() {
        let mut c = HlcClock::resume(1, Hlc { wall_ms: 1000, counter: u32::MAX, node: 1 });
        let next = c.tick(1000);
        assert_eq!((next.wall_ms, next.counter), (1001, 0));
        assert!(next > Hlc { wall_ms: 1000, counter: u32::MAX, node: 1 });
    }

    #[test]
    fn beobachtung_schiebt_die_uhr_vor() {
        let mut c = HlcClock::new(1);
        c.tick(1000);
        c.observe(Hlc { wall_ms: 5000, counter: 3, node: 2 });
        let next = c.tick(1001);
        assert!(next > Hlc { wall_ms: 5000, counter: 3, node: 2 });
        assert_eq!(next.node, 1);
    }
}
