//! WireGuard-Tunnel innerhalb des Prozesses.
//!
//! Der Tunnel legt keinen Netzwerkadapter an und braucht keine besonderen
//! Rechte: Das WireGuard-Protokoll läuft über einen gewöhnlichen UDP-Socket,
//! darüber liegt ein eigener TCP/IP-Stack. Nach außen entsteht ein normaler
//! Byte-Strom ([`TunnelStream`]), den nur diese Anwendung benutzt.
//!
//! ```text
//! SSH ──► TunnelStream ──► TCP/IP-Stack ──► WireGuard ──► UDP ──► Gegenstelle ──► Ziel
//! ```

mod config;
mod tunnel;

pub use config::{DEFAULT_MTU, IpNet, WgConfig, WgPeer, generate_keypair, parse_conf, public_key_of, to_conf};
pub use tunnel::{PeerStatus, TunnelListener, TunnelStream, WgTunnel};
