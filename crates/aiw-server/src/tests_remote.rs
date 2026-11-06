//! The A2A client of one engine against the A2A server of another, over a real socket: delegation, errors,
//! cancellation, timeout, the registry and its masked token, and a team whose member is a remote agent.

use super::*;
use aiw_engine::EngineOptions;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

async fn device(name: &str, turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let mut dev = Engine::new(EngineOptions {
        data_dir: Some(data.path().join("probe")),
        workspace_root: Some(ws.path().into()),
        mode: "daemon".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap()
    .device();
    dev.id = format!("{name}-id");
    dev.name = name.into();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().into()),
        workspace_root: Some(ws.path().into()),
        mode: "daemon".into(),
        offline: true,
        device: Some(dev),
        ..Default::default()
    })
    .await
    .unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(turns)));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "T".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: Default::default(),
    })
    .unwrap();
    (e, data, ws)
}

/// Serves `e` on a local port and returns the address.
async fn serve(e: &Engine) -> String {
    let app = router(e.clone(), None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    format!("http://{addr}")
}

fn remote(id: &str, url: &str) -> RemoteAgent {
    RemoteAgent {
        id: id.into(),
        name: format!("Remoto {id}"),
        url: url.into(),
        token: String::new(),
        skill: None,
        description: String::new(),
        paired: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delegates_to_a_remote_agent_and_reports_failures() {
    let (server, _d1, _w1) = device("Server", vec![ScriptedBackend::text_turn("Risposta remota")]).await;
    let url = serve(&server).await;
    let (client, _d2, _w2) = device("Client", vec![]).await;
    let cancel = CancellationToken::new();

    client.save_remote_agent(remote("srv", &url)).unwrap();
    let out = client.delegate_remote("remote:srv", "Dimmi qualcosa", &cancel, None).await.unwrap();
    assert_eq!(out, "Risposta remota");
    // Every delegation is audited, with its outcome.
    let audit = client.audit();
    assert!(audit.iter().any(|a| a.action == "a2a.delegate" && a.detail.contains("Remoto srv")), "{audit:?}");
    assert!(audit.iter().any(|a| a.action == "a2a.result" && a.detail.contains("completato")));
    // The remote task is stored by the server, in SQLite.
    assert_eq!(server.sessions().unwrap().len(), 1);

    // An agent the remote does not have: its JSON-RPC error comes back as the error of the delegation.
    let mut ghost = remote("ghost", &url);
    ghost.skill = Some("nessuno".into());
    client.save_remote_agent(ghost).unwrap();
    let err = client.delegate_remote("remote:ghost", "x", &cancel, None).await.unwrap_err().to_string();
    assert!(err.contains("agente sconosciuto"), "{err}");

    // Nothing listens there; and an unknown id.
    client.save_remote_agent(remote("down", "http://127.0.0.1:9")).unwrap();
    assert!(client
        .delegate_remote("remote:down", "x", &cancel, None)
        .await
        .unwrap_err()
        .to_string()
        .contains("Agent Card non raggiungibile"));
    assert!(matches!(client.delegate_remote("remote:nope", "x", &cancel, None).await, Err(aiw_core::Error::NotFound(_))));
    assert!(client.audit().iter().any(|a| a.action == "a2a.result" && a.detail.contains("Agent Card non raggiungibile")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_agent_card_must_stay_on_the_registered_origin() {
    // A card that points to another host is not followed: the client only talks to registered URLs.
    let app = Router::new().route(
        "/.well-known/agent-card.json",
        get(|| async { axum::Json(json!({"supportedInterfaces": [{"url": "http://10.255.255.1:1/a2a", "protocolBinding": "JSONRPC"}]})) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (client, _d, _w) = device("Client", vec![]).await;
    client.save_remote_agent(remote("evil", &format!("http://{addr}"))).unwrap();
    let err = client.delegate_remote("remote:evil", "x", &CancellationToken::new(), None).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Denied(_)), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_and_timing_out_cancel_the_remote_task() {
    // The remote agent asks to write a file: the run waits for an approval nobody gives.
    let blocked = || {
        vec![ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "a.md", "content": "x"})), ScriptedBackend::text_turn("fatto")]
    };
    let (server, _d1, _w1) = device("Server", blocked()).await;
    let url = serve(&server).await;
    let (client, _d2, _w2) = device("Client", vec![]).await;
    client.save_remote_agent(remote("srv", &url)).unwrap();

    let cancel = CancellationToken::new();
    let c = client.clone();
    let t = cancel.clone();
    let call = tokio::spawn(async move { c.delegate_remote("remote:srv", "scrivi", &t, None).await });
    let started = wait_for(|| server.sessions().unwrap().iter().any(|s| server.active_run(&s.id).is_some())).await;
    assert!(started, "the remote run started");
    cancel.cancel();
    let err = call.await.unwrap().unwrap_err();
    assert!(matches!(err, aiw_core::Error::Cancelled), "{err}");
    assert!(
        wait_for(|| server.sessions().unwrap().iter().all(|s| server.active_run(&s.id).is_none())).await,
        "the remote run was cancelled"
    );

    // Timeout: same, with a one second budget.
    let (server, _d3, _w3) = device("Server2", blocked()).await;
    let url = serve(&server).await;
    client.save_remote_agent(remote("slow", &url)).unwrap();
    client.set_config("global", "a2a.timeout_seconds", json!(1)).unwrap();
    let err = client.delegate_remote("remote:slow", "scrivi", &CancellationToken::new(), None).await.unwrap_err().to_string();
    assert!(err.contains("tempo scaduto"), "{err}");
    assert!(
        wait_for(|| server.sessions().unwrap().iter().all(|s| server.active_run(&s.id).is_none())).await,
        "the remote run was cancelled"
    );
}

async fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paired_device_is_a_remote_agent_and_a_team_member() {
    let (server, _d1, _w1) = device("Server", vec![ScriptedBackend::text_turn("Dal dispositivo")]).await;
    let url = serve(&server).await;
    let (client, _d2, _w2) = device("Client", vec![ScriptedBackend::text_turn("Parte locale")]).await;
    let peer = client.pair_with_peer(&url, &server.pairing_code()).await.unwrap();

    let id = format!("peer-{}", peer.id);
    let listed = client.remote_agents();
    let p = listed.iter().find(|r| r.id == id).expect("the paired device is listed");
    assert!(p.paired);
    assert_eq!(p.token, aiw_core::api::SECRET_MASK, "its pairing token is never shown");
    assert!(client.save_remote_agent(remote(&id, &url)).is_err(), "the id is reserved");

    // A team with the paired device and a local agent as members: the task runs on both.
    let team = TeamDef {
        id: "fuori".into(),
        name: "Fuori".into(),
        description: String::new(),
        strategy: "parallel".into(),
        supervisor: None,
        members: vec![
            TeamMember { agent: format!("remote:{id}"), role: String::new() },
            TeamMember { agent: "planner".into(), role: String::new() },
        ],
        aggregator: None,
        budget: None,
        output_schema: None,
    };
    client.save_team(team).unwrap();
    let (text, _) = client.run_agent_task("team:fuori", "Fai il lavoro", None, "t").await.unwrap();
    assert!(text.contains("Dal dispositivo") && text.contains("Parte locale"), "{text}");
}

#[tokio::test]
async fn the_registry_masks_the_token_and_protects_the_agents_of_teams() {
    let (e, _d, _w) = device("Solo", vec![]).await;
    let mut r = remote("cloud", "http://example.test:9000/");
    r.token = "segreto".into();
    let saved = e.save_remote_agent(r.clone()).unwrap();
    assert_eq!(saved.token, aiw_core::api::SECRET_MASK);
    assert_eq!(saved.url, "http://example.test:9000/");
    assert_eq!(e.remote_agents()[0].token, aiw_core::api::SECRET_MASK);

    // Saving again with the mask keeps the stored token (the delegation would send it).
    let mut again = saved.clone();
    again.description = "nuova".into();
    e.save_remote_agent(again).unwrap();
    let stored: RemoteAgent = serde_json::from_value(serde_json::to_value(e.remote_agents().into_iter().next().unwrap()).unwrap()).unwrap();
    assert_eq!(stored.description, "nuova");

    for bad in [remote("", "http://x"), remote("a b", "http://x"), remote("ok", "ftp://x"), remote("ok", "non un url")] {
        assert!(e.save_remote_agent(bad).is_err());
    }
    let mut unnamed = remote("ok", "http://x");
    unnamed.name = " ".into();
    assert!(e.save_remote_agent(unnamed).is_err());

    // A team that uses it blocks the deletion.
    let team = TeamDef {
        id: "t".into(),
        name: "Squadra".into(),
        description: String::new(),
        strategy: "parallel".into(),
        supervisor: None,
        members: vec![TeamMember { agent: "remote:cloud".into(), role: String::new() }],
        aggregator: None,
        budget: None,
        output_schema: None,
    };
    e.save_team(team).unwrap();
    let err = e.delete_remote_agent("cloud").unwrap_err();
    assert!(matches!(err, aiw_core::Error::Conflict(_)) && err.to_string().contains("Squadra"), "{err}");
    e.delete_team("t").unwrap();
    e.delete_remote_agent("cloud").unwrap();
    assert!(e.remote_agents().is_empty());
    assert!(matches!(e.delete_remote_agent("cloud"), Err(aiw_core::Error::NotFound(_))));
}

#[tokio::test]
async fn the_remote_agent_routes_never_return_the_token() {
    let (e, _d, _w) = device("Solo", vec![]).await;
    let app = router(e, None);
    let body = json!({"id": "x", "name": "Cloud", "url": "http://example.test", "token": "segreto", "description": "d"});
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/remote-agents/cloud", Some(body)).await;
    assert_eq!(st, 200, "{saved}");
    assert_eq!(saved["id"], "cloud", "the path wins");
    assert_eq!(saved["token"], aiw_core::api::SECRET_MASK);
    let (_, list) = dispatch(app.clone(), "GET", "/api/remote-agents", None).await;
    assert!(!list.to_string().contains("segreto"), "{list}");
    let (st, err) = dispatch(app.clone(), "PUT", "/api/remote-agents/bad", Some(json!({"id": "", "name": "", "url": "x"}))).await;
    assert_eq!(st, 400, "{err}");
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/remote-agents/cloud", None).await;
    assert_eq!(st, 200);
    let (st, _) = dispatch(app, "DELETE", "/api/remote-agents/cloud", None).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn a2a_tasks_survive_in_the_database_and_expire() {
    let (e, _d, _w) = device("Solo", vec![ScriptedBackend::text_turn("ok")]).await;
    let app = router(e.clone(), None);
    let call = |body: Value| {
        let app = app.clone();
        async move {
            let req = Request::builder()
                .method("POST")
                .uri("/a2a")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let r = tower::ServiceExt::oneshot(app, req).await.unwrap();
            serde_json::from_slice::<Value>(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()).unwrap()
        }
    };
    let v = call(json!({"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": {"message": {"parts": [{"text": "ciao"}]}}})).await;
    let tid = v["result"]["task"]["id"].as_str().unwrap().to_string();
    let (task, session) = e.a2a_task(&tid).unwrap().expect("stored");
    assert_eq!(task["status"]["state"], "TASK_STATE_COMPLETED");
    assert!(session.is_some());
    assert_eq!(e.a2a_context_session(task["contextId"].as_str().unwrap()).unwrap(), session);
    // A finished task is not cancellable.
    let v = call(json!({"jsonrpc": "2.0", "id": 2, "method": "CancelTask", "params": {"id": tid}})).await;
    assert_eq!(v["error"]["code"], -32002);
}
