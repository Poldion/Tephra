//! Ende-zu-Ende-Tests: ein echter MCP-Client spricht über einen Byte-Strom
//! mit dem Server, der Befehle auf einem SSH-Test-Server ausführt.

use crate::audit::{self, Outcome};
use crate::policy::{AccessLevel, HostGrant, McpPolicy};
use crate::{Action, ApprovalAnswer, ApprovalRequest, Approver, McpServer};
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tephra_core::model::{AuthMethod, Host, Identity, ItemId, KnownHost, Snippet};
use tephra_core::{Core, KdfParams};
use tephra_ssh::testing::{PASSWORD, TestServer, USER};

#[derive(Clone, Default)]
struct Client;

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

/// Beantwortet Rückfragen fest und merkt sich, was gefragt wurde.
struct Answers {
    answer: Mutex<ApprovalAnswer>,
    asked: Mutex<Vec<ApprovalRequest>>,
}

#[async_trait]
impl Approver for Answers {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalAnswer {
        self.asked.lock().expect("lock").push(request.clone());
        *self.answer.lock().expect("lock")
    }
}

struct Harness {
    core: Core,
    server: TestServer,
    host: ItemId,
    client_id: String,
    answers: Arc<Answers>,
    mcp: RunningService<RoleClient, Client>,
    _dir: tempfile::TempDir,
}

impl Harness {
    /// Baut Tresor, SSH-Test-Server und MCP-Verbindung auf. Der Host
    /// „web1" ist mit `grant` freigegeben; ein zweiter Host „geheim" nicht.
    async fn new(grant: HostGrant, answer: ApprovalAnswer) -> Self {
        let dir = tempfile::tempdir().expect("tmp");
        let (core, _) =
            Core::create(&dir.path().join("t.tephra"), "passwort", KdfParams::INSECURE_FOR_TESTS).expect("tresor");
        let server = TestServer::start().await;
        let identity = core
            .with_vault(|v| {
                v.insert(&Identity {
                    label: "Tester".into(),
                    username: USER.into(),
                    auth: AuthMethod::Password { password: PASSWORD.into() },
                })
            })
            .expect("identität");
        let insert_host = |label: &str| {
            core.with_vault(|v| {
                v.insert(&Host {
                    label: label.into(),
                    address: "127.0.0.1".into(),
                    port: server.addr.port(),
                    identity_id: Some(identity),
                    tags: vec!["test".into()],
                    ..Host::default()
                })
            })
            .expect("host")
        };
        let host = insert_host("web1");
        insert_host("geheim");
        // Der Hostschlüssel ist bereits bekannt, wie nach einer ersten Verbindung in der App.
        core.with_vault(|v| {
            v.insert(&KnownHost {
                host: "127.0.0.1".into(),
                port: server.addr.port(),
                key_type: "ssh-ed25519".into(),
                fingerprint: server.host_key_fingerprint.clone(),
                ..KnownHost::default()
            })
        })
        .expect("bekannt");

        let mut policy = McpPolicy::default();
        let (client_id, _token) = policy.add_client("Testmodell").expect("client");
        policy.client_mut(&client_id).expect("client").hosts.insert(host, grant);
        policy.save(&core).expect("speichern");

        let answers = Arc::new(Answers { answer: Mutex::new(answer), asked: Mutex::new(Vec::new()) });
        let (server_side, client_side) = tokio::io::duplex(64 * 1024);
        let mcp_server = McpServer::new(core.clone(), client_id.clone(), answers.clone());
        tokio::spawn(async move {
            let _ = mcp_server.serve_stream(server_side).await;
        });
        let mcp = Client.serve(client_side).await.expect("mcp-client");
        Self { core, server, host, client_id, answers, mcp, _dir: dir }
    }

    /// Ruft ein Werkzeug auf: `Ok` mit dem strukturierten Ergebnis oder `Err`
    /// mit der Fehlermeldung, die das Modell zu sehen bekäme.
    async fn call(&self, tool: &str, arguments: Value) -> Result<Value, String> {
        let params =
            CallToolRequestParams::new(tool.to_string()).with_arguments(arguments.as_object().expect("objekt").clone());
        let result = self.mcp.call_tool(params).await.map_err(|e| format!("Protokollfehler: {e}"))?;
        let text = result.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default();
        if result.is_error == Some(true) {
            return Err(text);
        }
        Ok(result.structured_content.unwrap_or_else(|| serde_json::from_str(&text).unwrap_or(Value::String(text))))
    }

    fn set_policy(&self, edit: impl FnOnce(&mut McpPolicy)) {
        let mut policy = McpPolicy::load(&self.core).expect("laden");
        edit(&mut policy);
        policy.save(&self.core).expect("speichern");
    }

    fn asked(&self) -> Vec<ApprovalRequest> {
        self.answers.asked.lock().expect("lock").clone()
    }

    fn last_audit(&self) -> audit::AuditEntry {
        self.audits(1).remove(0)
    }

    /// Die jüngsten Protokolleinträge, neueste zuerst.
    fn audits(&self, count: usize) -> Vec<audit::AuditEntry> {
        audit::read(&self.core, count, None).expect("protokoll").into_iter().map(|(_, entry)| entry).collect()
    }
}

fn auto() -> HostGrant {
    HostGrant { level: AccessLevel::Auto, ..HostGrant::default() }
}

#[tokio::test]
async fn werkzeuge_sind_beschrieben() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let tools = h.mcp.list_all_tools().await.expect("tools");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "list_hosts",
        "ssh_exec",
        "read_output",
        "list_snippets",
        "run_snippet",
        "ssh_session_open",
        "ssh_session_send",
        "ssh_session_read",
        "ssh_session_close",
        "tunnel_status",
        "files_list",
        "files_read",
        "files_write",
    ] {
        assert!(names.contains(&expected), "Werkzeug {expected} fehlt in {names:?}");
    }
    assert!(tools.iter().all(|t| t.description.as_ref().is_some_and(|d| !d.is_empty())));
}

#[tokio::test]
async fn nur_freigegebene_hosts_sind_sichtbar_und_ohne_zugangsdaten() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let listed = h.call("list_hosts", json!({})).await.expect("liste");
    let hosts = listed["hosts"].as_array().expect("array");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["label"], "web1");
    assert_eq!(hosts[0]["access"], "automatic");
    let text = listed.to_string();
    assert!(!text.contains(PASSWORD) && !text.contains("geheim"), "Liste verrät zu viel: {text}");

    assert!(
        h.call("list_hosts", json!({"query": "gibtsnicht"})).await.expect("liste")["hosts"]
            .as_array()
            .expect("array")
            .is_empty()
    );

    // Ein nicht freigegebener Host verhält sich wie ein nicht vorhandener.
    let hidden = h.call("ssh_exec", json!({"host": "geheim", "command": "whoami"})).await.expect_err("abgelehnt");
    let missing = h.call("ssh_exec", json!({"host": "erfunden", "command": "whoami"})).await.expect_err("abgelehnt");
    assert_eq!(hidden.replace("geheim", "X"), missing.replace("erfunden", "X"));
}

#[tokio::test]
async fn befehl_ausfuehren_per_label_und_per_id() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let out = h.call("ssh_exec", json!({"host": "WEB1", "command": "echo hallo"})).await.expect("exec");
    assert_eq!(out["stdout"], "hallo\n");
    assert_eq!(out["exit_code"], 0);
    assert_eq!(out["timed_out"], false);

    let out = h.call("ssh_exec", json!({"host": h.host.to_string(), "command": "fail"})).await.expect("exec");
    assert_eq!(out["exit_code"], 3);
    assert_eq!(out["stderr"], "kaputt\n");

    let out = h.call("ssh_exec", json!({"host": "web1", "command": "cat", "stdin": "eingabe"})).await.expect("exec");
    assert_eq!(out["stdout"], "eingabe");

    let out =
        h.call("ssh_exec", json!({"host": "web1", "command": "sleep 5000", "timeout_secs": 1})).await.expect("exec");
    assert_eq!(out["timed_out"], true);

    // Jeder Befehl hinterlässt zwei Einträge: vor der Ausführung und nach dem Ende.
    let entries = h.audits(2);
    let (finished, begun) = (&entries[0], &entries[1]);
    assert_eq!(begun.outcome, Outcome::Allowed);
    assert_eq!(finished.outcome, Outcome::Completed);
    assert_eq!(finished.note, "timed out");
    for entry in [begun, finished] {
        assert_eq!(entry.tool, "ssh_exec");
        assert_eq!(entry.detail, "sleep 5000");
        assert_eq!(entry.client_name, "Testmodell");
        assert_eq!(entry.host_label, "web1");
    }
    // Eingabedaten sind im Protokoll vermerkt.
    assert!(h.audits(6).iter().any(|e| e.detail == "cat" && e.note == "stdin: 7 bytes"));
}

#[tokio::test]
async fn nur_lesen_laesst_nur_harmloses_durch() {
    let h =
        Harness::new(HostGrant { level: AccessLevel::ReadOnly, ..HostGrant::default() }, ApprovalAnswer::Session).await;
    assert_eq!(
        h.call("ssh_exec", json!({"host": "web1", "command": "echo ok"})).await.expect("exec")["stdout"],
        "ok\n"
    );

    for command in ["fail", "echo x; fail", "echo $(fail)", "echo x > /etc/passwd"] {
        let err = h.call("ssh_exec", json!({"host": "web1", "command": command})).await.expect_err("abgelehnt");
        assert!(err.starts_with("Rejected"), "{command}: {err}");
    }
    assert_eq!(h.last_audit().outcome, Outcome::Denied);
    assert!(h.call("ssh_session_open", json!({"host": "web1"})).await.is_err());
    // Im Modus „Nur lesen" wird der Nutzer nie gefragt.
    assert!(h.asked().is_empty());
}

#[tokio::test]
async fn bestaetigung_ablehnen_einmal_erlauben_fuer_die_sitzung_erlauben() {
    let h = Harness::new(HostGrant::default(), ApprovalAnswer::Deny).await;
    let exec = json!({"host": "web1", "command": "echo x"});

    let err = h.call("ssh_exec", exec.clone()).await.expect_err("abgelehnt");
    assert!(err.contains("declined"), "{err}");
    assert_eq!(h.last_audit().outcome, Outcome::Rejected);
    let asked = h.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].action, Action::Command { command: "echo x".into(), stdin: None });
    assert_eq!(asked[0].client_name, "Testmodell");
    assert_eq!(asked[0].host_label, "web1");

    // „Einmal": Jeder weitere Aufruf fragt erneut.
    *h.answers.answer.lock().expect("lock") = ApprovalAnswer::Once;
    assert_eq!(h.call("ssh_exec", exec.clone()).await.expect("exec")["stdout"], "x\n");
    assert_eq!(h.audits(2)[1].outcome, Outcome::Approved);
    h.call("ssh_exec", exec.clone()).await.expect("exec");
    assert_eq!(h.asked().len(), 3);

    // „Für diese Sitzung": Derselbe Befehl läuft danach ohne Rückfrage, ein anderer nicht.
    *h.answers.answer.lock().expect("lock") = ApprovalAnswer::Session;
    h.call("ssh_exec", exec.clone()).await.expect("exec");
    *h.answers.answer.lock().expect("lock") = ApprovalAnswer::Deny;
    h.call("ssh_exec", exec.clone()).await.expect("ohne rückfrage");
    assert_eq!(h.asked().len(), 4);
    assert!(h.call("ssh_exec", json!({"host": "web1", "command": "echo y"})).await.is_err());
    assert_eq!(h.asked().len(), 5);
}

#[tokio::test]
async fn not_aus_widerruf_und_gesperrter_tresor() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let exec = json!({"host": "web1", "command": "echo x"});
    h.call("ssh_exec", exec.clone()).await.expect("exec");

    h.set_policy(|p| p.enabled = false);
    assert!(h.call("ssh_exec", exec.clone()).await.expect_err("aus").contains("switched off"));
    assert!(h.call("list_hosts", json!({})).await.is_err());
    h.set_policy(|p| p.enabled = true);

    // Freigabe eines Hosts entziehen wirkt sofort, auch bei bestehender Verbindung.
    h.set_policy(|p| p.client_mut(&h.client_id).expect("client").hosts.clear());
    assert!(h.call("ssh_exec", exec.clone()).await.expect_err("entzogen").contains("not found"));

    h.set_policy(|p| p.client_mut(&h.client_id).expect("client").disabled = true);
    assert!(h.call("list_hosts", json!({})).await.expect_err("widerrufen").contains("revoked"));

    h.core.lock().await;
    assert!(h.call("list_hosts", json!({})).await.expect_err("gesperrt").contains("locked"));
}

#[tokio::test]
async fn aufrufrate_ist_begrenzt() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    h.set_policy(|p| p.limits.calls_per_minute = 3);
    for _ in 0..3 {
        h.call("list_hosts", json!({})).await.expect("erlaubt");
    }
    assert!(h.call("list_hosts", json!({})).await.expect_err("gebremst").contains("Rate limit"));
}

#[tokio::test]
async fn gespeicherte_geheimnisse_werden_geschwaerzt() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let out = h
        .call("ssh_exec", json!({"host": "web1", "command": format!("echo passwort ist {PASSWORD} ok")}))
        .await
        .expect("exec");
    assert_eq!(out["stdout"], "passwort ist [redacted] ok\n");
}

#[tokio::test]
async fn lange_ausgabe_wird_seitenweise_geliefert() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    h.set_policy(|p| p.limits.page_chars = 1000);
    let out = h.call("ssh_exec", json!({"host": "web1", "command": "big 2500"})).await.expect("exec");
    assert_eq!(out["stdout"].as_str().expect("text").len(), 1000);
    let id = out["more"]["output_id"].as_str().expect("id").to_string();
    assert_eq!(out["more"]["total_chars"], 2500);

    let mut collected = out["stdout"].as_str().expect("text").to_string();
    let mut offset = out["more"]["next_offset"].as_u64();
    while let Some(next) = offset {
        let page = h.call("read_output", json!({"output_id": id, "offset": next})).await.expect("seite");
        collected.push_str(page["text"].as_str().expect("text"));
        offset = page["next_offset"].as_u64();
    }
    assert_eq!(collected, "x".repeat(2500));
    assert!(h.call("read_output", json!({"output_id": "unbekannt", "offset": 0})).await.is_err());
}

#[tokio::test]
async fn snippets_setzen_werte_als_ein_argument_ein() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    h.core
        .with_vault(|v| {
            v.insert(&Snippet { label: "Gruß".into(), command: "echo {{text}}".into(), ..Snippet::default() })
        })
        .expect("snippet");
    let listed = h.call("list_snippets", json!({})).await.expect("liste");
    assert_eq!(listed["snippets"][0]["variables"], json!(["text"]));

    assert!(
        h.call("run_snippet", json!({"host": "web1", "snippet": "Gruß"})).await.expect_err("fehlt").contains("text")
    );
    h.call("run_snippet", json!({"host": "web1", "snippet": "Gruß", "variables": {"text": "a; fail"}}))
        .await
        .expect("snippet");
    // Der Wert kommt in Anführungszeichen an und kann keinen Befehl anhängen.
    assert_eq!(h.last_audit().detail, "echo 'a; fail'");
    assert!(h.call("run_snippet", json!({"host": "web1", "snippet": "gibtsnicht"})).await.is_err());
}

#[tokio::test]
async fn interaktive_sitzung() {
    let h = Harness::new(HostGrant::default(), ApprovalAnswer::Once).await;
    let opened = h.call("ssh_session_open", json!({"host": "web1"})).await.expect("öffnen");
    assert_eq!(h.asked()[0].action, Action::Session);
    assert!(opened["output"].as_str().expect("text").contains("bereit>"));
    let id = opened["session_id"].as_str().expect("id").to_string();

    let sent = h.call("ssh_session_send", json!({"session_id": id, "input": "uptime\n"})).await.expect("senden");
    assert!(sent["output"].as_str().expect("text").contains("uptime"));
    assert_eq!(sent["closed"], false);
    // Einzelne Eingaben einer bestätigten Sitzung fragen nicht erneut.
    assert_eq!(h.asked().len(), 1);

    let quiet = h.call("ssh_session_read", json!({"session_id": id, "wait_ms": 100})).await.expect("lesen");
    assert_eq!(quiet["output"], "");

    let ended = h.call("ssh_session_send", json!({"session_id": id, "input": "exit\n"})).await.expect("senden");
    assert_eq!(ended["closed"], true);
    assert_eq!(ended["exit_code"], 0);
    assert!(h.call("ssh_session_send", json!({"session_id": id, "input": "x"})).await.is_err());
    h.call("ssh_session_close", json!({"session_id": id})).await.expect("schließen");
    assert!(h.call("ssh_session_read", json!({"session_id": id})).await.is_err());
}

#[tokio::test]
async fn sitzungen_sind_begrenzt_und_sperrliste_gilt_auch_dort() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    h.set_policy(|p| p.limits.max_sessions = 1);
    let id = h.call("ssh_session_open", json!({"host": "web1"})).await.expect("öffnen")["session_id"]
        .as_str()
        .expect("id")
        .to_string();
    assert!(h.call("ssh_session_open", json!({"host": "web1"})).await.expect_err("limit").contains("Too many"));
    let err =
        h.call("ssh_session_send", json!({"session_id": id, "input": "sudo reboot\n"})).await.expect_err("sperrliste");
    assert!(err.starts_with("Rejected"), "{err}");
}

#[tokio::test]
async fn dateien_lesen_und_schreiben_nach_freigabe() {
    let grant = HostGrant { level: AccessLevel::Auto, files_read: true, files_write: false, ..HostGrant::default() };
    let h = Harness::new(grant, ApprovalAnswer::Deny).await;
    {
        let mut files = h.server.files.lock().expect("lock");
        files.insert("/etc/motd".into(), b"willkommen".to_vec());
        files.insert("/etc/nginx/nginx.conf".into(), b"worker_processes 1;".to_vec());
    }
    let listing = h.call("files_list", json!({"host": "web1", "path": "/etc"})).await.expect("liste");
    let names: Vec<(&str, &str)> = listing["entries"]
        .as_array()
        .expect("array")
        .iter()
        .map(|e| (e["name"].as_str().expect("name"), e["type"].as_str().expect("typ")))
        .collect();
    assert_eq!(names, vec![("motd", "file"), ("nginx", "dir")]);

    let read = h.call("files_read", json!({"host": "web1", "path": "/etc/motd"})).await.expect("lesen");
    assert_eq!(read["content"], "willkommen");
    assert_eq!(read["truncated"], false);
    assert!(h.call("files_read", json!({"host": "web1", "path": "/gibts/nicht"})).await.is_err());
    assert_eq!(h.last_audit().outcome, Outcome::Failed);

    // Schreiben ist nicht freigegeben.
    let write = json!({"host": "web1", "path": "/etc/motd", "content": "gekapert"});
    assert!(h.call("files_write", write.clone()).await.expect_err("abgelehnt").starts_with("Rejected"));
    assert_eq!(h.server.files.lock().expect("lock")["/etc/motd"], b"willkommen");

    h.set_policy(|p| {
        p.client_mut(&h.client_id).expect("client").hosts.get_mut(&h.host).expect("host").files_write = true
    });
    let done = h.call("files_write", write).await.expect("schreiben");
    assert_eq!(done["bytes_written"], 8);
    assert_eq!(h.server.files.lock().expect("lock")["/etc/motd"], b"gekapert");

    // Größenlimit beim Lesen.
    h.set_policy(|p| p.limits.max_file_bytes = 4);
    let read = h.call("files_read", json!({"host": "web1", "path": "/etc/motd"})).await.expect("lesen");
    assert_eq!(read["content"], "geka");
    assert_eq!(read["truncated"], true);
}

#[tokio::test]
async fn dateizugriff_mit_bestaetigung_fragt_nach() {
    let grant = HostGrant { level: AccessLevel::Confirm, files_read: true, files_write: true, ..HostGrant::default() };
    let h = Harness::new(grant, ApprovalAnswer::Deny).await;
    h.server.files.lock().expect("lock").insert("/a".into(), b"x".to_vec());
    assert!(h.call("files_read", json!({"host": "web1", "path": "/a"})).await.is_err());
    assert_eq!(h.asked()[0].action, Action::FileRead { path: "/a".into() });
    *h.answers.answer.lock().expect("lock") = ApprovalAnswer::Once;
    h.call("files_write", json!({"host": "web1", "path": "/b", "content": "neu"})).await.expect("schreiben");
    match &h.asked()[1].action {
        Action::FileWrite { path, bytes, preview, sha256 } => {
            assert_eq!((path.as_str(), *bytes, preview.as_str()), ("/b", 3, "neu"));
            assert_eq!(sha256.len(), 64);
        }
        other => panic!("unerwartet: {other:?}"),
    }
}

#[tokio::test]
async fn unbekannter_hostschluessel_geht_an_den_nutzer_geaenderter_nie() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    // Bekannten Schlüssel entfernen: Der Host ist jetzt „neu".
    let forget = |core: &Core| {
        core.with_vault(|v| {
            for (id, _) in v.list::<KnownHost>()? {
                v.delete(id)?;
            }
            Ok(())
        })
        .expect("vergessen");
    };
    forget(&h.core);
    let exec = json!({"host": "web1", "command": "echo x"});
    assert!(h.call("ssh_exec", exec.clone()).await.is_err());
    assert!(matches!(h.asked()[0].action, Action::TrustHostKey { .. }));

    *h.answers.answer.lock().expect("lock") = ApprovalAnswer::Once;
    h.call("ssh_exec", exec.clone()).await.expect("nach zustimmung");
    assert_eq!(h.core.with_vault(|v| v.list::<KnownHost>()).expect("liste").len(), 1);

    // Ein geänderter Schlüssel wird über MCP nie akzeptiert, egal was der Nutzer antworten würde.
    h.core.disconnect_all().await;
    forget(&h.core);
    h.core
        .with_vault(|v| {
            v.insert(&KnownHost {
                host: "127.0.0.1".into(),
                port: h.server.addr.port(),
                key_type: "ssh-ed25519".into(),
                fingerprint: "SHA256:anderer".into(),
                ..KnownHost::default()
            })
        })
        .expect("bekannt");
    let before = h.asked().len();
    assert!(h.call("ssh_exec", exec).await.is_err());
    assert_eq!(h.asked().len(), before, "bei geändertem Schlüssel darf gar nicht erst gefragt werden");
}

#[tokio::test]
async fn zustimmung_umfasst_eingabedaten_und_dateiinhalt() {
    let grant = HostGrant { level: AccessLevel::Confirm, files_write: true, ..HostGrant::default() };
    let h = Harness::new(grant, ApprovalAnswer::Session).await;

    // Die Eingabedaten gehören zu dem, was der Nutzer sieht und bestätigt.
    h.call("ssh_exec", json!({"host": "web1", "command": "cat", "stdin": "harmlos"})).await.expect("exec");
    assert_eq!(h.asked()[0].action, Action::Command { command: "cat".into(), stdin: Some("harmlos".into()) });
    // „Für diese Sitzung" gilt nur für genau dieselbe Eingabe …
    h.call("ssh_exec", json!({"host": "web1", "command": "cat", "stdin": "harmlos"})).await.expect("exec");
    assert_eq!(h.asked().len(), 1);
    // … andere Eingabedaten zum selben Befehl werden erneut vorgelegt.
    h.call("ssh_exec", json!({"host": "web1", "command": "cat", "stdin": "rm -rf /"})).await.expect("exec");
    assert_eq!(h.asked().len(), 2);

    // Dasselbe beim Schreiben: gleicher Pfad, gleiche Länge, anderer Inhalt.
    let write = |content: &str| json!({"host": "web1", "path": "/etc/motd", "content": content});
    h.call("files_write", write("aaaa")).await.expect("schreiben");
    h.call("files_write", write("aaaa")).await.expect("schreiben");
    assert_eq!(h.asked().len(), 3);
    h.call("files_write", write("bbbb")).await.expect("schreiben");
    assert_eq!(h.asked().len(), 4);
    assert!(h.audits(2).iter().any(|e| e.tool == "files_write" && e.note.starts_with("4 bytes, sha256 ")));
}

#[tokio::test]
async fn nur_lesen_nimmt_keine_eingabedaten() {
    let h =
        Harness::new(HostGrant { level: AccessLevel::ReadOnly, ..HostGrant::default() }, ApprovalAnswer::Deny).await;
    let err = h
        .call("ssh_exec", json!({"host": "web1", "command": "cat /etc/motd", "stdin": "x"}))
        .await
        .expect_err("abgelehnt");
    assert!(err.starts_with("Rejected"), "{err}");
}

#[tokio::test]
async fn versuche_auf_fremde_hosts_stehen_im_protokoll() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    assert!(h.call("ssh_exec", json!({"host": "geheim", "command": "whoami"})).await.is_err());
    let entry = h.last_audit();
    assert_eq!(entry.outcome, Outcome::Denied);
    assert_eq!(entry.host_label, "geheim");
    assert_eq!(entry.host_id, None);
}

#[tokio::test]
async fn entzug_beendet_sitzungen_und_verbindungen_von_selbst() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let first = h.call("ssh_session_open", json!({"host": "web1"})).await.expect("öffnen")["session_id"]
        .as_str()
        .expect("id")
        .to_string();
    assert_eq!(h.core.pooled_hosts().await, vec![h.host]);

    // Freigabe entziehen und gleich wieder erteilen: Die alte Sitzung ist
    // trotzdem beendet, ohne dass das Modell noch etwas aufgerufen hätte.
    let grant = McpPolicy::load(&h.core).expect("laden").client(&h.client_id).expect("client").hosts[&h.host].clone();
    h.set_policy(|p| p.client_mut(&h.client_id).expect("client").hosts.clear());
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert!(h.core.pooled_hosts().await.is_empty(), "Verbindung zum entzogenen Host blieb offen");
    h.set_policy(|p| {
        p.client_mut(&h.client_id).expect("client").hosts.insert(h.host, grant);
    });
    let err = h.call("ssh_session_read", json!({"session_id": first})).await.expect_err("beendet");
    assert!(err.contains("Unknown session_id"), "{err}");

    // Not-Aus: Sitzungen und alle Verbindungen enden.
    let second = h.call("ssh_session_open", json!({"host": "web1"})).await.expect("öffnen")["session_id"]
        .as_str()
        .expect("id")
        .to_string();
    h.set_policy(|p| p.enabled = false);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert!(h.core.pooled_hosts().await.is_empty(), "Not-Aus hat die Verbindung nicht getrennt");
    h.set_policy(|p| p.enabled = true);
    assert!(h.call("ssh_session_read", json!({"session_id": second})).await.is_err());

    // Wird der Client gesperrt, enden auch die Verbindungen, die er benutzt hat.
    h.call("ssh_exec", json!({"host": "web1", "command": "echo x"})).await.expect("exec");
    assert_eq!(h.core.pooled_hosts().await, vec![h.host]);
    h.set_policy(|p| p.client_mut(&h.client_id).expect("client").disabled = true);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert!(h.core.pooled_hosts().await.is_empty(), "Verbindung des gesperrten Clients blieb offen");
}

#[tokio::test]
async fn stufenwechsel_beendet_offene_sitzungen() {
    let h = Harness::new(auto(), ApprovalAnswer::Deny).await;
    let id = h.call("ssh_session_open", json!({"host": "web1"})).await.expect("öffnen")["session_id"]
        .as_str()
        .expect("id")
        .to_string();
    // Von „automatisch" auf „mit Bestätigung": Die nie bestätigte Sitzung darf nicht weiterlaufen.
    h.set_policy(|p| {
        p.client_mut(&h.client_id).expect("client").hosts.get_mut(&h.host).expect("host").level = AccessLevel::Confirm;
    });
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let err = h.call("ssh_session_send", json!({"session_id": id, "input": "ls\n"})).await.expect_err("beendet");
    assert!(err.contains("Unknown session_id"), "{err}");
    // Eine neue Sitzung braucht jetzt die Zustimmung des Nutzers – die hier verweigert wird.
    assert!(h.call("ssh_session_open", json!({"host": "web1"})).await.is_err());
    assert_eq!(h.asked()[0].action, Action::Session);
}

#[tokio::test]
async fn nur_lesen_schreibt_auch_mit_dateifreigabe_nicht() {
    let grant = HostGrant { level: AccessLevel::ReadOnly, files_read: true, files_write: true, ..HostGrant::default() };
    let h = Harness::new(grant, ApprovalAnswer::Session).await;
    h.server.files.lock().expect("lock").insert("/etc/motd".into(), b"alt".to_vec());
    let err = h
        .call("files_write", json!({"host": "web1", "path": "/etc/motd", "content": "neu"}))
        .await
        .expect_err("abgelehnt");
    assert!(err.contains("read-only"), "{err}");
    assert_eq!(h.server.files.lock().expect("lock")["/etc/motd"], b"alt");
    assert_eq!(
        h.call("files_read", json!({"host": "web1", "path": "/etc/motd"})).await.expect("lesen")["content"],
        "alt"
    );
    assert!(h.asked().is_empty());
}
