//! The credentials of one agent (issue #71), shown with the engine: sealed on disk, usable by their agent only, sent only where they
//! were given, gone the moment they are removed, absent from every log, export and backup.
//!
//! A stand-in server on loopback records what it receives (so that "the header reached it" and "nothing reached it" are facts, not
//! assumptions); `tests_isolation::allow_private_network_for` lets the runs of these engines reach it, which a build never does.

use crate::tests_isolation::{allow_private_network_for, apart, call, denied, engine_with, input_of, shared};
use crate::vault::{Keychain, MemoryKeyStore};
use crate::Engine;
use aiw_core::api::{AgentCredentialInfo, AgentCredentialInput};
use aiw_core::Error;
use aiw_runtimes::ScriptedBackend;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Recognisable fakes: nothing here is a real credential.
const SECRET_A: &str = "TESTONLY-ALFA-S3CR3T-CREDENTIAL-0001";
const SECRET_B: &str = "TESTONLY-BETA-S3CR3T-CREDENTIAL-0002";

/// A server on 127.0.0.1 that records every request it gets (the raw text, headers included) and answers with `respond(request)`.
struct Stand {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Stand {
    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn http(status: u16, extra: &str, body: &str) -> String {
    format!("HTTP/1.1 {status} X\r\n{extra}Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

async fn stand_in(respond: impl Fn(&str) -> String + Send + Sync + 'static) -> Stand {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (log, respond) = (seen.clone(), Arc::new(respond));
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            let (log, respond) = (log.clone(), respond.clone());
            tokio::spawn(async move {
                let (mut buf, mut n) = (vec![0u8; 8192], 0);
                loop {
                    let r = sock.read(&mut buf[n..]).await.unwrap_or(0);
                    n += r;
                    if r == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                log.lock().unwrap().push(req.clone());
                let _ = sock.write_all(respond(&req).as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Stand { port, seen }
}

/// A server that echoes the whole request (headers included) in its answer: what a careless API does.
async fn echo() -> Stand {
    stand_in(|req| http(200, "", &format!("ricevuto:\n{req}"))).await
}

fn cred(host: &str, value: &str) -> AgentCredentialInput {
    AgentCredentialInput { hosts: vec![host.into()], value: Some(value.into()), ..Default::default() }
}

fn lower(r: &str) -> String {
    r.to_ascii_lowercase()
}

/// The request carried this secret as a bearer token.
fn carried(request: &str, secret: &str) -> bool {
    lower(request).contains(&format!("authorization: bearer {}", lower(secret)))
}

fn get_args(url: &str, credential: &str) -> Value {
    json!({"url": url, "credential": credential})
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The files of the data folder a value could be in: the database and its log, the settings.
fn stored_files(data: &Path) -> Vec<(String, Vec<u8>)> {
    ["aiw.db", "aiw.db-wal", "config.json"].iter().filter_map(|n| Some((n.to_string(), std::fs::read(data.join(n)).ok()?))).collect()
}

struct Fx {
    e: Engine,
    data: tempfile::TempDir,
    ws: tempfile::TempDir,
    keys: Arc<MemoryKeyStore>,
}

/// An engine with a keychain (a fake one), able to reach a stand-in server on loopback, with the agent `alfa` holding `github`.
async fn fixture() -> Fx {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let keys = MemoryKeyStore::new();
    let e = engine_with(data.path(), ws.path(), Keychain::Custom(keys.clone())).await;
    allow_private_network_for(data.path());
    e.save_agent(shared("alfa", &["web_fetch_auth"])).unwrap();
    e.set_agent_credential("alfa", "github", cred("127.0.0.1", SECRET_A)).unwrap();
    Fx { e, data, ws, keys }
}

async fn run(e: &Engine, agent: &str, turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (String, String, ScriptedBackend) {
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let s = e.create_session(agent, None).unwrap();
    let run = e.start_run(&s.id, "leggi i dati".into()).await.unwrap();
    crate::tests::wait_finished(e, &run).await;
    (s.id, run, backend)
}

#[tokio::test]
async fn the_credential_travels_in_the_header_and_nothing_the_model_or_the_logs_hold_has_its_value() {
    let fx = fixture().await;
    let e = &fx.e;
    let stand = echo().await;
    let (session, run_id, backend) = run(
        e,
        "alfa",
        vec![
            ScriptedBackend::tool_turn("c1", "web_fetch_auth", get_args(&stand.url("/dati"), "github")),
            ScriptedBackend::text_turn("Ho letto i dati."),
        ],
    )
    .await;
    // The server got the header, with the real value: the engine put it there.
    let seen = stand.requests();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(carried(&seen[0], SECRET_A), "{}", seen[0]);
    // The tool answer is what the server sent, with the value stripped: the server echoed it and the model never saw it.
    let messages = e.messages(&session).unwrap();
    let tool = messages.iter().find(|m| m.role == "tool").expect("a tool result");
    assert!(
        tool.content.contains("ricevuto") && tool.content.contains("[credenziale]") && !tool.content.contains(SECRET_A),
        "{}",
        tool.content
    );
    // Everything the model received: the name and the hosts are there, the value is not.
    let model = format!("{:?}", backend.requests.lock().unwrap());
    assert!(model.contains("github") && model.contains("127.0.0.1") && !model.contains(SECRET_A), "the model must not hold the value");
    assert!(model.contains("# Credenziali di questo agente"), "the model is told which credentials it has");
    // And everything the app keeps or exports.
    let traces = |redact: bool| {
        let filter = crate::traces::TraceFilter { redact, ..Default::default() };
        serde_json::to_string(&e.export_traces(&filter).unwrap()).unwrap()
    };
    let places = [
        ("messages", serde_json::to_string(&messages).unwrap()),
        ("run events", serde_json::to_string(&e.run_events(&run_id)).unwrap()),
        ("audit", serde_json::to_string(&e.audit()).unwrap()),
        ("traces, as they are", traces(false)),
        ("traces, redacted", traces(true)),
        ("diagnostics", e.diagnostics().await.unwrap().to_string()),
        ("agent list", serde_json::to_string(&e.agents().unwrap()).unwrap()),
        ("credential list", serde_json::to_string(&e.agent_credentials("alfa").unwrap()).unwrap()),
        ("sessions", serde_json::to_string(&e.sessions().unwrap()).unwrap()),
    ];
    for (what, text) in &places {
        assert!(!text.contains(SECRET_A), "the credential is in {what}");
    }
    // The audit trail knows it was used, by whom and where: not the value.
    let used = e.audit().into_iter().find(|a| a.action == "agent.credential_used").expect("the use is noted");
    assert_eq!(used.detail, "alfa/github → 127.0.0.1");
    // On disk it is sealed: not in the database, not in its log.
    for (name, bytes) in stored_files(fx.data.path()) {
        assert!(!contains(&bytes, SECRET_A), "{SECRET_A} is in plain in {name}");
    }
    assert_eq!(fx.keys.len(), 1, "the key is in the keychain");
    // The absence above means something: the document is there, sealed.
    assert!(stored_files(fx.data.path()).iter().any(|(_, b)| contains(b, "aiwenc1:")), "a sealed value is in the database");
}

#[tokio::test]
async fn the_credential_survives_a_restart_sealed_and_still_works() {
    let fx = fixture().await;
    let Fx { e, data, ws, keys } = fx;
    drop(e);
    let e = engine_with(data.path(), ws.path(), Keychain::Custom(keys.clone())).await;
    allow_private_network_for(data.path());
    let list = e.agent_credentials("alfa").unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].set && list[0].name == "github" && list[0].hosts == ["127.0.0.1"] && list[0].header == "Authorization", "{list:?}");
    let stand = echo().await;
    let out = call(&e, &input_of(&e, "alfa"), "web_fetch_auth", get_args(&stand.url("/dopo"), "github")).await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    assert!(carried(&stand.requests()[0], SECRET_A), "the value survived the restart");
    drop(e);
    for (name, bytes) in stored_files(data.path()) {
        assert!(!contains(&bytes, SECRET_A), "{SECRET_A} is in plain in {name}");
    }
}

#[tokio::test]
async fn an_agent_never_uses_the_credential_of_another_not_even_one_with_the_same_name_or_a_longer_id() {
    let fx = fixture().await;
    let e = &fx.e;
    let stand = echo().await;
    for id in ["beta", "gamma", "a", "ab"] {
        e.save_agent(shared(id, &["web_fetch_auth"])).unwrap();
    }
    e.set_agent_credential("beta", "github", cred("127.0.0.1", SECRET_B)).unwrap();
    e.set_agent_credential("ab", "solo-ab", cred("127.0.0.1", "TESTONLY-AB-S3CR3T-CREDENTIAL-0003")).unwrap();
    let url = stand.url("/x");
    // Each one gets its own value under the same name.
    call(e, &input_of(e, "alfa"), "web_fetch_auth", get_args(&url, "github")).await.unwrap();
    call(e, &input_of(e, "beta"), "web_fetch_auth", get_args(&url, "github")).await.unwrap();
    let seen = stand.requests();
    assert!(carried(&seen[0], SECRET_A) && !carried(&seen[0], SECRET_B), "{}", seen[0]);
    assert!(carried(&seen[1], SECRET_B) && !carried(&seen[1], SECRET_A), "{}", seen[1]);
    // An agent without that credential gets nothing, and the server hears nothing.
    denied(call(e, &input_of(e, "gamma"), "web_fetch_auth", get_args(&url, "github")).await, "gamma has no credential");
    denied(call(e, &input_of(e, "a"), "web_fetch_auth", get_args(&url, "solo-ab")).await, "`a` does not own what `ab` holds");
    assert_eq!(stand.requests().len(), 2, "nothing else reached the server");
    // The lists are by owner, as a whole string: `a` has none.
    assert!(e.agent_credentials("a").unwrap().is_empty());
    assert_eq!(e.agent_credentials("ab").unwrap().len(), 1);
    assert_eq!(e.agent_credentials("beta").unwrap().len(), 1);
}

#[tokio::test]
async fn a_sub_agent_uses_the_credentials_of_its_parent_never_its_own_so_nothing_is_borrowed_by_delegating() {
    let fx = fixture().await;
    let e = &fx.e;
    let stand = echo().await;
    // `capo` holds no credential and delegates to `alfa`, who holds one: the child does not use its own.
    e.save_agent(shared("capo", &["agent", "web_fetch_auth"])).unwrap();
    let (session, _, backend) = run(
        e,
        "capo",
        vec![
            ScriptedBackend::tool_turn("d1", "agent", json!({"agent": "alfa", "task": "leggi"})),
            ScriptedBackend::tool_turn("w1", "web_fetch_auth", get_args(&stand.url("/preso"), "github")),
            ScriptedBackend::text_turn("niente"),
            ScriptedBackend::text_turn("fatto"),
        ],
    )
    .await;
    assert!(stand.requests().is_empty(), "the child borrowed the credential of alfa: {:?}", stand.requests());
    // The child did try, and was told no: the refusal is what its model read next (this is not an absence of the call).
    let next_turn = format!("{:?}", backend.requests.lock().unwrap()[2]);
    assert!(next_turn.contains("non ha una credenziale"), "{next_turn}");
    assert!(!serde_json::to_string(&e.messages(&session).unwrap()).unwrap().contains(SECRET_A));
    // `alfa` delegates to `gamma`, who has none: the child works with what its parent holds (the parent granted the tool).
    e.save_agent(shared("alfa", &["agent", "web_fetch_auth"])).unwrap();
    e.save_agent(shared("gamma", &["web_fetch_auth"])).unwrap();
    run(
        e,
        "alfa",
        vec![
            ScriptedBackend::tool_turn("d1", "agent", json!({"agent": "gamma", "task": "leggi"})),
            ScriptedBackend::tool_turn("w1", "web_fetch_auth", get_args(&stand.url("/dal-figlio"), "github")),
            ScriptedBackend::text_turn("letto"),
            ScriptedBackend::text_turn("fatto"),
        ],
    )
    .await;
    let seen = stand.requests();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(carried(&seen[0], SECRET_A));
}

#[tokio::test]
async fn the_credential_goes_only_to_its_hosts_only_in_https_and_not_after_a_redirect_elsewhere() {
    let fx = fixture().await;
    let e = &fx.e;
    let broker = e.credential_broker("alfa").unwrap();
    let url = |s: &str| reqwest::Url::parse(s).unwrap();
    // The scope is the host and its subdomains, in https.
    e.set_agent_credential("alfa", "api", cred("example.org", SECRET_A)).unwrap();
    assert!(broker.authorize("api", &url("https://example.org/x"), false).is_ok());
    assert!(broker.authorize("api", &url("https://api.example.org/x"), false).is_ok());
    for (bad, why) in [
        ("https://badexample.org/", "a name that merely ends like it"),
        ("https://example.org.evil.com/", "a name that merely starts like it"),
        ("http://example.org/", "plain http"),
        ("https://evil.com/?h=example.org", "the host in the query"),
        ("https://example.org@evil.com/", "the host in the user part"),
    ] {
        assert!(matches!(broker.authorize("api", &url(bad), false), Err(Error::Denied(_))), "{why}: {bad}");
    }
    // Plain http reaches a loopback address only when the build allows it (tests); a host that is not loopback never.
    assert!(broker.authorize("github", &url("http://127.0.0.1:9/"), true).is_ok());
    assert!(broker.authorize("github", &url("http://127.0.0.1:9/"), false).is_err());
    assert!(broker.authorize("api", &url("http://example.org/"), true).is_err());
    // Through the tool: a page on the host in scope that redirects to a host that is not: the credential is not sent there.
    let other = echo().await;
    let target = format!("http://localhost:{}/dopo", other.port);
    let stand = stand_in(move |_| http(302, &format!("Location: {target}\r\n"), "")).await;
    let out = call(e, &input_of(e, "alfa"), "web_fetch_auth", get_args(&stand.url("/"), "github")).await;
    assert!(matches!(&out, Err(Error::Denied(m)) if m.contains("non vale per localhost")), "{out:?}");
    assert_eq!(stand.requests().len(), 1, "the first hop went out (it is in scope)");
    assert!(other.requests().is_empty(), "the redirect target never got a request: {:?}", other.requests());
    // A host outside the scope is refused before any request.
    denied(call(e, &input_of(e, "alfa"), "web_fetch_auth", get_args("http://localhost:9/", "github")).await, "a host that is not in scope");
}

#[tokio::test]
async fn removing_a_credential_or_the_agent_makes_it_unusable_at_once_also_for_a_run_in_progress() {
    let fx = fixture().await;
    let e = &fx.e;
    let stand = echo().await;
    let input = input_of(e, "alfa"); // a run that is going on: it holds its broker since the start
    let url = stand.url("/x");
    call(e, &input, "web_fetch_auth", get_args(&url, "github")).await.unwrap();
    assert_eq!(stand.requests().len(), 1);
    // The credential is removed: the next call of the same run is refused and nothing reaches the server.
    e.remove_agent_credential("alfa", "github").unwrap();
    denied(call(e, &input, "web_fetch_auth", get_args(&url, "github")).await, "removed credential");
    assert_eq!(stand.requests().len(), 1);
    assert!(e.agent_credentials("alfa").unwrap().is_empty());
    assert!(matches!(e.remove_agent_credential("alfa", "github"), Err(Error::NotFound(_))));
    // The agent is removed: the run in progress cannot use what it had, and a new agent with the same id has nothing.
    e.set_agent_credential("alfa", "github", cred("127.0.0.1", SECRET_A)).unwrap();
    e.set_agent_credential("alfa", "altra", cred("127.0.0.1", SECRET_A)).unwrap();
    call(e, &input, "web_fetch_auth", get_args(&url, "github")).await.unwrap();
    assert_eq!(stand.requests().len(), 2);
    e.delete_agent("alfa").unwrap();
    let err = call(e, &input, "web_fetch_auth", get_args(&url, "github")).await;
    assert!(matches!(&err, Err(Error::Denied(m)) if m.contains("non esiste più")), "{err:?}");
    assert_eq!(stand.requests().len(), 2);
    assert!(
        e.0.store.list_docs::<Value>("agent_credential").unwrap().is_empty(),
        "no credential of the removed agent is left in the store"
    );
    e.save_agent(shared("alfa", &["web_fetch_auth"])).unwrap();
    assert!(e.agent_credentials("alfa").unwrap().is_empty(), "an agent created later with the same id inherits nothing");
    denied(call(e, &input_of(e, "alfa"), "web_fetch_auth", get_args(&url, "github")).await, "inherited credential");
    assert_eq!(stand.requests().len(), 2);
    assert!(e.audit().iter().any(|a| a.action == "agent.credential_removed" && !a.detail.contains(SECRET_A)));
}

#[tokio::test]
async fn a_backup_has_no_credential_value_unless_the_person_asks_for_the_secrets() {
    let fx = fixture().await;
    let e = &fx.e;
    let default = fx.data.path().join("default.tar.gz");
    let m = e.backup_create(&crate::backup::BackupOptions::default(), &default).unwrap();
    assert!(!m.sensitive);
    let restored = |archive: &Path| {
        let dir = tempfile::tempdir().unwrap();
        crate::backup::restore(&aiw_core::paths::DataDirs::at(dir.path().to_path_buf()), archive).unwrap();
        dir
    };
    let plain = restored(&default);
    for (name, bytes) in stored_files(plain.path()) {
        assert!(!contains(&bytes, SECRET_A), "{SECRET_A} travelled in {name} of a default backup");
        assert!(!contains(&bytes, "aiwenc1:"), "a sealed value travelled in {name}");
    }
    // What is not secret is kept: whose it is, its name, the hosts. The value is emptied.
    let db = rusqlite::Connection::open(plain.path().join("aiw.db")).unwrap();
    let doc: String = db.query_row("SELECT json FROM documents WHERE kind='agent_credential'", [], |r| r.get(0)).unwrap();
    let doc: Value = serde_json::from_str(&doc).unwrap();
    assert_eq!((doc["agent"].as_str(), doc["name"].as_str(), doc["secret"].as_str()), (Some("alfa"), Some("github"), Some("")), "{doc}");
    assert_eq!(doc["hosts"], json!(["127.0.0.1"]));
    // A restored credential without a value is «not set»: it is not usable and it says so.
    let e2 = engine_with(plain.path(), fx.ws.path(), Keychain::None).await;
    let info: Vec<AgentCredentialInfo> = e2.agent_credentials("alfa").unwrap();
    assert!(info.len() == 1 && !info[0].set, "{info:?}");
    denied(call(&e2, &input_of(&e2, "alfa"), "web_fetch_auth", get_args("http://127.0.0.1:9/", "github")).await, "no value");
    // On request the secrets go in the archive, in plain (the file is then as sensitive as the data folder), documented.
    let with = fx.data.path().join("con-segreti.tar.gz");
    assert!(e.backup_create(&crate::backup::BackupOptions { include_secrets: true, ..Default::default() }, &with).unwrap().sensitive);
    let full = restored(&with);
    assert!(
        stored_files(full.path()).iter().any(|(_, b)| contains(b, SECRET_A)),
        "with the secrets asked for, the value is in the archive"
    );
}

#[tokio::test]
async fn the_app_knows_the_value_as_a_secret_so_the_redaction_nets_hide_it_wherever_it_shows_up() {
    let fx = fixture().await;
    let e = &fx.e;
    assert!(e.known_secrets().iter().any(|s| s == SECRET_A));
    let stat = e.secrets_at_rest();
    assert_eq!((stat.state.as_str(), stat.plain, stat.unreadable), ("active", 0, 0), "{stat:?}");
    assert!(stat.encrypted >= 1);
    // A value somebody pasted in a conversation is hidden by its exact value in an export of the traces.
    let s = e.create_session("alfa", None).unwrap();
    e.0.store
        .append_messages(
            &s.id,
            &[aiw_core::api::ChatMessage {
                id: "m1".into(),
                role: "user".into(),
                content: format!("il mio token è {SECRET_A}"),
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                attachments: None,
                created_at: aiw_core::now_rfc3339(),
            }],
        )
        .unwrap();
    let out = serde_json::to_string(&e.export_traces(&crate::traces::TraceFilter { session: Some(s.id), ..Default::default() }).unwrap())
        .unwrap();
    assert!(!out.contains(SECRET_A), "{out}");
}

#[tokio::test]
async fn setting_a_credential_is_for_the_owner_and_is_checked_and_never_hands_the_value_back() {
    let fx = fixture().await;
    let e = &fx.e;
    let invalid = |r: aiw_core::Result<AgentCredentialInfo>, what: &str| match r {
        Err(Error::Invalid(_)) => {}
        other => panic!("{what}: expected Invalid, got {other:?}"),
    };
    invalid(
        e.set_agent_credential("alfa", "x", AgentCredentialInput { hosts: vec![], value: Some(SECRET_A.into()), ..Default::default() }),
        "no host",
    );
    for bad in ["com", "https://example.org", "example.org/x", "*.example.org", "example.org:8443"] {
        invalid(e.set_agent_credential("alfa", "x", cred(bad, SECRET_A)), bad);
    }
    invalid(e.set_agent_credential("alfa", "x", cred("example.org", "corto")), "too short");
    invalid(e.set_agent_credential("alfa", "x", cred("example.org", "TESTONLY-con\na-capo-S3CR3T")), "a newline in the value");
    invalid(e.set_agent_credential("alfa", "con spazio", cred("example.org", SECRET_A)), "a name with a space");
    invalid(
        e.set_agent_credential("alfa", "x", AgentCredentialInput { header: Some("Host".into()), ..cred("example.org", SECRET_A) }),
        "a header that routes",
    );
    invalid(
        e.set_agent_credential("alfa", "senza-valore", AgentCredentialInput { value: None, ..cred("example.org", SECRET_A) }),
        "no value for a new one",
    );
    assert!(matches!(e.set_agent_credential("nessuno", "x", cred("example.org", SECRET_A)), Err(Error::NotFound(_))));
    assert!(matches!(e.agent_credentials("nessuno"), Err(Error::NotFound(_))));
    // A header other than Authorization has no `Bearer ` by default; a change without a value keeps the stored one.
    let info = e
        .set_agent_credential(
            "alfa",
            "gitlab",
            AgentCredentialInput { header: Some("PRIVATE-TOKEN".into()), ..cred("gitlab.example.org", SECRET_B) },
        )
        .unwrap();
    assert_eq!((info.header.as_str(), info.prefix.as_str(), info.set), ("PRIVATE-TOKEN", "", true));
    let again = e
        .set_agent_credential(
            "alfa",
            "gitlab",
            AgentCredentialInput { hosts: vec!["gitlab.example.org".into(), "git.example.org".into()], ..Default::default() },
        )
        .unwrap();
    assert_eq!(again.hosts.len(), 2);
    assert_eq!(again.created_at, info.created_at);
    let broker = e.credential_broker("alfa").unwrap();
    let used = broker.authorize("gitlab", &reqwest::Url::parse("https://git.example.org/").unwrap(), false).unwrap();
    assert_eq!((used.header.as_str(), used.value.as_str()), ("PRIVATE-TOKEN", SECRET_B), "the value kept by a change without one");
    // What the API shows is the metadata and the state, never the value.
    let shown = serde_json::to_string(&e.agent_credentials("alfa").unwrap()).unwrap();
    assert!(!shown.contains(SECRET_A) && !shown.contains(SECRET_B), "{shown}");
    assert!(e.audit().iter().filter(|a| a.action == "agent.credential_set").all(|a| !a.detail.contains("S3CR3T")));
}

#[tokio::test]
async fn only_the_agent_talking_gets_its_credentials_not_a_background_a_channel_or_an_evaluation_run() {
    let fx = fixture().await;
    let e = &fx.e;
    assert!(input_of(e, "alfa").tool_ctx.credentials.is_some());
    for derived in [e.proactive_agent("alfa").unwrap(), e.channel_agent(crate::channels::RunKind::Ask, "alfa").unwrap()] {
        let id = derived.id.clone();
        let s = e.create_session(&id, None).unwrap();
        let input = e.build_input_at(
            "r",
            &s.id,
            derived,
            e.profile("default").unwrap(),
            vec![],
            String::new(),
            Default::default(),
            0,
            Default::default(),
            None,
        );
        assert!(input.tool_ctx.credentials.is_none(), "{id}");
    }
    // A run in a folder somebody chose (an evaluation) has none either.
    let s = e.create_session("alfa", None).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let input = e.build_input_at(
        "r",
        &s.id,
        e.agent("alfa").unwrap(),
        e.profile("default").unwrap(),
        vec![],
        String::new(),
        Default::default(),
        0,
        Default::default(),
        Some(folder.path().to_path_buf()),
    );
    assert!(input.tool_ctx.credentials.is_none());
    // And a conversation does not pass to or from an agent that holds credentials.
    e.save_agent(shared("altro", &["todo"])).unwrap();
    let host = crate::host::EngineHost::new(e.clone(), None, None);
    use aiw_agent::RunHost;
    let err = host.handoff_target(&input_of(e, "altro"), "alfa").unwrap_err();
    assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("isolato"), "{err}");
    assert!(host.handoff_target(&input_of(e, "alfa"), "altro").is_err());
}

#[tokio::test]
async fn the_chat_cannot_give_take_or_even_see_a_credential_and_cannot_edit_an_agent_that_holds_them() {
    let fx = fixture().await;
    let e = &fx.e;
    let owner = crate::RunOrigin { owner: true, questions: true, secrets: true };
    let ctx = crate::management::test_ctx(e, Some(owner));
    let save = e.tool_registry().get("aiw_agent_save").unwrap();
    // An agent that holds a credential is edited by its owner, not from the chat.
    let err = save.call(json!({"id": "alfa", "description": "ritoccato"}), &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("credenziali"), "{err}");
    assert_eq!(e.agent("alfa").unwrap().description, shared("alfa", &[]).description);
    // Nor one that is kept apart.
    e.save_agent(apart("beta", &["todo"])).unwrap();
    let err = save.call(json!({"id": "beta", "description": "ritoccato"}), &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("separato"), "{err}");
    assert!(e.agent("beta").unwrap().has_dedicated_workspace());
    // An agent made from the chat has no isolation, no credential and no web_fetch_auth (a tool that needs an explicit grant).
    let err = save.call(json!({"name": "Nuovo", "system_prompt": "ciao", "tools": ["web_fetch_auth"]}), &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Denied(_)), "{err}");
    // The inspection says whether an agent is apart or holds credentials, and nothing else about them.
    let inspect = e.tool_registry().get("aiw_inspect").unwrap();
    let out = inspect.call(json!({"topic": "agents", "id": "alfa"}), &ctx).await.unwrap().content;
    assert!(out.contains("has_credentials") && !out.contains(SECRET_A) && !out.contains("github"), "{out}");
}

#[tokio::test]
async fn an_agent_without_the_tool_is_not_offered_it_a_wildcard_does_not_cover_it_and_the_prompt_only_says_names_and_hosts() {
    let fx = fixture().await;
    let e = &fx.e;
    e.save_agent(shared("tutto", &["*"])).unwrap();
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("ok")]);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let s = e.create_session("tutto", None).unwrap();
    let run_id = e.start_run(&s.id, "ciao".into()).await.unwrap();
    crate::tests::wait_finished(e, &run_id).await;
    let offered: Vec<String> = backend.requests.lock().unwrap()[0].tools.iter().map(|t| t.function.name.clone()).collect();
    assert!(offered.iter().any(|t| t == "web_fetch") && !offered.iter().any(|t| t == "web_fetch_auth"), "{offered:?}");
    // The agent that lists it is told which credentials it has: names and hosts.
    let line = crate::Engine::credentials_prompt(&e.credential_broker("alfa").unwrap(), &["web_fetch_auth".to_string()]).unwrap();
    assert!(line.contains("`github`") && line.contains("127.0.0.1") && !line.contains(SECRET_A), "{line}");
    assert!(crate::Engine::credentials_prompt(&e.credential_broker("alfa").unwrap(), &["web_fetch".to_string()]).is_none());
}
