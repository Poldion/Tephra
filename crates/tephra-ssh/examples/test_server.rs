//! Startet den SSH-Test-Server als eigenständiges Programm, um Clients von
//! Hand auszuprobieren:
//!
//! ```text
//! cargo run -p tephra-ssh --features testing --example test_server
//! ```
//!
//! Der Server führt nichts auf dem Rechner aus; er kennt nur die in
//! `tephra_ssh::testing` beschriebenen Befehle.

use tephra_ssh::testing::{PASSWORD, TestServer, USER};

#[tokio::main]
async fn main() {
    let server = TestServer::start().await;
    println!("port={}", server.addr.port());
    println!("user={USER}");
    println!("password={PASSWORD}");
    println!("fingerprint={}", server.host_key_fingerprint);
    server.files.lock().expect("lock").insert("/etc/motd".into(), b"Willkommen auf dem Test-Server\n".to_vec());
    std::future::pending::<()>().await;
}
