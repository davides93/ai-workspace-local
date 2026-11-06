//! The HTTP error contract: every failure is `{"error": "<Italian message>"}` with the status
//! codes the UI knows, whoever produced it (a handler, the router, an extractor); validation of
//! what the settings screens save; masked MCP secrets; 404 for unknown sessions and servers.

use super::*;
use aiw_engine::EngineOptions;
use axum::http::{header, HeaderMap, Method};

async fn fixture(ui: Option<PathBuf>) -> (Router, Engine, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().into()),
        workspace_root: Some(data.path().join("ws")),
        mode: "daemon".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    (router(e.clone(), ui), e, data)
}

/// One request with any method, headers and raw body.
async fn raw(app: &Router, method: Method, path: &str, content_type: Option<&str>, body: &str) -> (StatusCode, HeaderMap, String) {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(ct) = content_type {
        b = b.header(header::CONTENT_TYPE, ct);
    }
    let res = tower::ServiceExt::oneshot(app.clone(), b.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, headers, String::from_utf8_lossy(&bytes).into_owned())
}

fn is_json(h: &HeaderMap) -> bool {
    h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"))
}

/// The status and the `error` text of a JSON error response.
async fn failure(app: &Router, method: &str, path: &str, body: Option<Value>) -> (u16, String) {
    let (st, v) = dispatch(app.clone(), method, path, body).await;
    let msg = v["error"].as_str().unwrap_or_else(|| panic!("{method} {path}: not an error body: {v}")).to_string();
    (st, msg)
}

fn ui_dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("index.html"), "<!doctype html><title>AIW</title>").unwrap();
    std::fs::create_dir_all(d.path().join("assets")).unwrap();
    std::fs::write(d.path().join("assets/app.js"), "console.log(1)").unwrap();
    d
}

// ------------------------------------------------------------------ unknown paths

#[tokio::test]
async fn unknown_backend_paths_are_a_404_json_with_or_without_the_ui() {
    let ui = ui_dir();
    let (with_ui, _e, _d) = fixture(Some(ui.path().into())).await;
    let (without_ui, _e2, _d2) = fixture(None).await;
    for app in [&with_ui, &without_ui] {
        for path in [
            "/api/non-esiste",
            "/api/sessions/ses_1/non-esiste",
            "/api",
            "/api/",
            "/v1/non-esiste",
            "/v1/chat/completions/x",
            "/mcp/x",
            "/mcp2",
            "/a2a/x",
            "/hooks",
            "/hooks/x/y",
            "/.well-known/non-esiste",
        ] {
            for method in [Method::GET, Method::POST] {
                let (st, h, body) = raw(app, method.clone(), path, None, "").await;
                assert_eq!(st, StatusCode::NOT_FOUND, "{method} {path}");
                assert!(is_json(&h), "{method} {path}: {h:?}");
                let v: Value = serde_json::from_str(&body).unwrap_or_else(|_| panic!("{method} {path}: {body}"));
                assert_eq!(v["error"], "percorso non trovato", "{method} {path}");
            }
        }
    }
    // Without a UI nothing else exists either.
    let (st, h, _) = raw(&without_ui, Method::GET, "/", None, "").await;
    assert_eq!((st, is_json(&h)), (StatusCode::NOT_FOUND, true));
}

#[tokio::test]
async fn the_ui_answers_the_paths_that_are_not_the_servers() {
    let ui = ui_dir();
    let (app, _e, _d) = fixture(Some(ui.path().into())).await;
    for path in ["/", "/index.html", "/models/discover", "/chat/ses_1", "/assets/mancante.js", "/apiary", "/v10", "/hooksy"] {
        let (st, h, body) = raw(&app, Method::GET, path, None, "").await;
        assert_eq!(st, StatusCode::OK, "{path}");
        assert!(h.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("text/html"), "{path}: {h:?}");
        assert!(body.contains("<title>AIW</title>"), "{path}");
    }
    let (st, h, body) = raw(&app, Method::GET, "/assets/app.js", None, "").await;
    assert_eq!((st, body.as_str()), (StatusCode::OK, "console.log(1)"));
    assert!(!h.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("text/html"));
    // The real routes still work next to the fallback.
    let (st, _, body) = raw(&app, Method::GET, "/api/system", None, "").await;
    assert_eq!(st, StatusCode::OK);
    assert!(serde_json::from_str::<Value>(&body).unwrap()["version"].is_string());
    let (st, _, body) = raw(&app, Method::GET, "/.well-known/agent-card.json", None, "").await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("skills"));
}

#[tokio::test]
async fn a_wrong_method_on_a_real_path_is_a_405_json() {
    let (app, _e, _d) = fixture(None).await;
    for (method, path) in [(Method::DELETE, "/api/system"), (Method::PUT, "/api/sessions"), (Method::GET, "/api/kill")] {
        let (st, h, body) = raw(&app, method.clone(), path, None, "").await;
        assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        assert!(is_json(&h), "{method} {path}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"], "metodo non consentito per questo percorso");
    }
}

// ------------------------------------------------------------------ extractor rejections

#[tokio::test]
async fn malformed_requests_get_the_api_error_shape_with_a_400() {
    let (app, _e, _d) = fixture(None).await;
    let json = Some("application/json");
    for (method, path, ct, body, why) in [
        // Not JSON at all, truncated JSON, JSON of the wrong shape, a missing field.
        (Method::POST, "/api/sessions", json, "non json", "JSON non valido: "),
        (Method::POST, "/api/sessions", json, "{\"agent_id\":", "JSON non valido: "),
        (Method::POST, "/api/sessions", json, "", "JSON non valido: "),
        (Method::POST, "/api/sessions", json, "{}", "dati non validi: campo mancante `agent_id`"),
        (Method::POST, "/api/sessions", json, "{\"agent_id\": 5}", "dati non validi: agent_id: tipo non valido: numero intero `5`"),
        (Method::PUT, "/api/policy", json, "{}", "campo mancante `rules`"),
        (Method::PUT, "/api/hooks", json, "{\"rules\": []}", "campo mancante `hooks`"),
        (Method::POST, "/api/pair", json, "{\"code\": 123456}", "dati non validi: "),
        // The body is JSON but nobody said so.
        (Method::POST, "/api/sessions", None, "{\"agent_id\": \"operator\"}", "Content-Type: application/json"),
        (Method::POST, "/api/sessions", Some("text/plain"), "{\"agent_id\": \"operator\"}", "Content-Type: application/json"),
        // Query strings.
        (Method::GET, "/api/models/search?provider=hf&limit=abc", None, "", "parametri non validi: "),
        (Method::GET, "/api/runs/run_1/events?after=-1", None, "", "parametri non validi: "),
    ] {
        let (st, h, text) = raw(&app, method.clone(), path, ct, body).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{method} {path} {body:?}: {text}");
        assert!(is_json(&h), "{method} {path}: {h:?}");
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{method} {path}: not JSON: {text}"));
        let msg = v["error"].as_str().unwrap();
        assert!(msg.contains(why), "{method} {path} {body:?}: {msg}");
        assert_eq!(v.as_object().unwrap().len(), 1, "only the `error` key: {v}");
        // No English from axum leaks through, and the old status is gone.
        for english in ["Failed to", "Expected request", "Invalid URL"] {
            assert!(!msg.contains(english), "{msg}");
        }
    }
}

#[tokio::test]
async fn a_path_parameter_that_cannot_be_read_is_a_400_json() {
    let (app, _e, _d) = fixture(None).await;
    // `%FF` is not UTF-8: the extractor refuses it before any handler runs.
    let (st, h, text) = raw(&app, Method::GET, "/api/sessions/%FF", None, "").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{text}");
    assert!(is_json(&h));
    assert!(serde_json::from_str::<Value>(&text).unwrap()["error"].as_str().unwrap().starts_with("indirizzo non valido: "), "{text}");
}

#[tokio::test]
async fn valid_requests_and_optional_bodies_still_work() {
    let (app, _e, _d) = fixture(None).await;
    // `Option<Json<_>>`: no body at all is fine...
    let (st, _, text) = raw(&app, Method::POST, "/api/workflows/non-esiste/run", None, "").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["error"], "workflow non-esiste non trovato");
    // ...a body that is there must be valid JSON...
    let (st, _, text) = raw(&app, Method::POST, "/api/workflows/non-esiste/run", Some("application/json"), "{").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{text}");
    // ...and a good one reaches the handler.
    let (st, _, text) = raw(&app, Method::POST, "/api/workflows/non-esiste/run", Some("application/json"), "{\"inputs\": {}}").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{text}");
    let (st, _, text) =
        raw(&app, Method::POST, "/api/sessions", Some("application/json; charset=utf-8"), "{\"agent_id\": \"operator\"}").await;
    assert_eq!(st, StatusCode::OK, "{text}");
    let (st, _, _) = raw(&app, Method::GET, "/api/models/search?provider=hf&q=x&limit=5", None, "").await;
    assert_ne!(st, StatusCode::BAD_REQUEST, "a valid query string is accepted (the provider itself is offline)");
}

// ------------------------------------------------------------------ messages

#[tokio::test]
async fn error_messages_are_italian_without_english_prefixes() {
    let (app, _e, _d) = fixture(None).await;
    for (method, path, body, status, text) in [
        ("GET", "/api/sessions/ses_x", None, 404, "sessione ses_x non trovata"),
        ("DELETE", "/api/models/nope", None, 404, "modello `nope` non trovato"),
        ("GET", "/api/models/remote?provider=nope&model=x", None, 404, "provider di modelli `nope` non trovato"),
        ("POST", "/api/runs/run_x/cancel", None, 404, "run run_x non trovato"),
        ("POST", "/api/approvals/apr_x", Some(json!({"decision": "allow"})), 404, "richiesta apr_x non trovata (forse già gestita)"),
        ("PUT", "/api/tools/inesistente", Some(json!({"enabled": false})), 404, "tool inesistente non trovato"),
        ("POST", "/api/pair", Some(json!({"code": "000000"})), 403, "codice di pairing non valido"),
        ("PUT", "/api/config", Some(json!({"layer": "nope", "key": "a", "value": 1})), 400, ""),
    ] {
        let (st, msg) = failure(&app, method, path, body).await;
        assert_eq!(st, status, "{method} {path}: {msg}");
        if !text.is_empty() {
            assert_eq!(msg, text, "{method} {path}");
        }
        for english in ["not found", "invalid input", "conflict:", "denied by policy", "runtime error", "not supported"] {
            assert!(!msg.contains(english), "{method} {path}: {msg}");
        }
    }
}

// ------------------------------------------------------------------ validation over HTTP

#[tokio::test]
async fn the_policy_route_refuses_bad_rules_and_keeps_the_stored_ones() {
    let (app, e, _d) = fixture(None).await;
    let (st, v) =
        dispatch(app.clone(), "PUT", "/api/policy", Some(json!({"rules": [{"tool": "shell", "pattern": "rm *", "action": "deny"}]}))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["rules"][0]["action"], "deny");
    for (body, why) in [
        (json!({}), "campo mancante `rules`"),
        (json!({"rules": null}), "dati non validi"),
        (json!({"rules": [{"tool": "shell", "action": "block"}]}), "regola 1: azione `block` non valida"),
        (json!({"rules": [{"tool": "", "action": "allow"}]}), "regola 1: manca il nome del tool"),
        (json!({"rules": [{"tool": "x"}]}), "campo mancante `action`"),
        (json!({"rules": [{"tool": "a", "action": "ask"}, {"tool": "b", "action": "Allow"}]}), "regola 2"),
    ] {
        let (st, msg) = failure(&app, "PUT", "/api/policy", Some(body.clone())).await;
        assert_eq!(st, 400, "{body}: {msg}");
        assert!(msg.contains(why), "{body}: {msg}");
    }
    assert_eq!(e.policy_rules().len(), 1, "none of the refusals emptied or replaced the policy");
    let (_, v) = dispatch(app.clone(), "GET", "/api/policy", None).await;
    assert_eq!(v["rules"][0]["tool"], "shell");
    // An explicit empty list is still how the rules are cleared.
    let (st, v) = dispatch(app, "PUT", "/api/policy", Some(json!({"rules": []}))).await;
    assert_eq!((st, v["rules"].as_array().unwrap().len()), (200, 0));
}

#[tokio::test]
async fn the_hooks_route_refuses_bad_hooks_and_keeps_the_stored_ones() {
    let (app, e, _d) = fixture(None).await;
    let good = json!({"hooks": [{"event": "PreToolUse", "matcher": "shell", "command": "./check.sh", "timeout_s": 10}, {"event": "Stop", "matcher": null, "command": "echo ok"}]});
    let (st, v) = dispatch(app.clone(), "PUT", "/api/hooks", Some(good)).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["hooks"][1]["timeout_s"], 30, "the default timeout applies");
    for (body, why) in [
        (json!({}), "campo mancante `hooks`"),
        (json!({"hooks": [{"event": "Nope", "command": "x"}]}), "hook 1: evento `Nope` non valido"),
        (json!({"hooks": [{"event": "Stop", "command": " "}]}), "hook 1: manca il comando"),
        (json!({"hooks": [{"event": "Stop", "command": "x", "timeout_s": 0}]}), "tra 1 e 600"),
        (json!({"hooks": [{"event": "Stop", "command": "x", "timeout_s": 601}]}), "tra 1 e 600"),
        (json!({"hooks": [{"event": "Stop", "command": "x", "timeout_s": -1}]}), "dati non validi"),
        (json!({"hooks": [{"event": "Stop"}]}), "campo mancante `command`"),
    ] {
        let (st, msg) = failure(&app, "PUT", "/api/hooks", Some(body.clone())).await;
        assert_eq!(st, 400, "{body}: {msg}");
        assert!(msg.contains(why), "{body}: {msg}");
    }
    assert_eq!(e.hooks().len(), 2);
}

#[tokio::test]
async fn the_agent_route_refuses_unknown_profiles_turns_and_tools() {
    let (app, e, _d) = fixture(None).await;
    let mut a = serde_json::to_value(e.agent("planner").unwrap()).unwrap();
    a["name"] = json!("Mio");
    let (st, v) = dispatch(app.clone(), "PUT", "/api/agents/mio", Some(a.clone())).await;
    assert_eq!(st, 200, "{v}");
    let with = |k: &str, v: Value| {
        let mut x = a.clone();
        x[k] = v;
        x
    };
    for (body, why) in [
        (with("inference_profile", json!("non-esiste")), "profilo di inferenza `non-esiste` non esiste"),
        (with("max_turns", json!(0)), "max_turns deve essere almeno 1"),
        (with("max_turns", json!(-3)), "dati non validi"),
        (with("tools", json!(["read_file", "volare", "teletrasporto"])), "strumenti sconosciuti: volare, teletrasporto"),
        (with("mode", json!("sempre")), "modalità non valida"),
    ] {
        let (st, msg) = failure(&app, "PUT", "/api/agents/mio", Some(body.clone())).await;
        assert_eq!(st, 400, "{msg}");
        assert!(msg.contains(why), "{msg}");
    }
    // Internal tools, `*` and MCP tools are fine.
    let (st, v) =
        dispatch(app.clone(), "PUT", "/api/agents/mio", Some(with("tools", json!(["*", "todo", "render_ui", "mcp__github__list_issues"]))))
            .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(e.agent("mio").unwrap().tools, ["*", "todo", "render_ui", "mcp__github__list_issues"]);
}

#[tokio::test]
async fn the_team_routes_validate_and_protect_member_agents() {
    let (app, e, _d) = fixture(None).await;
    let team = json!({
        "id": "x", "name": "Squadra", "description": "", "strategy": "supervisor", "supervisor": "planner",
        "members": [{"agent": "operator", "role": "Esegue"}]
    });
    let (st, v) = dispatch(app.clone(), "PUT", "/api/teams/squadra", Some(team.clone())).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["id"], "squadra", "the id comes from the path");
    let (st, v) = dispatch(app.clone(), "GET", "/api/teams/squadra", None).await;
    assert_eq!((st, v["name"].as_str()), (200, Some("Squadra")));
    let (_, all) = dispatch(app.clone(), "GET", "/api/teams", None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(dispatch(app.clone(), "GET", "/api/teams/nope", None).await.0, 404);

    let with = |k: &str, v: Value| {
        let mut x = team.clone();
        x[k] = v;
        x
    };
    let dup = json!([{"agent": "operator", "role": "a"}, {"agent": "operator", "role": "b"}]);
    let nine: Vec<Value> = (0..9).map(|i| json!({"agent": format!("remote:r{i}"), "role": "x"})).collect();
    for (body, why) in [
        (with("strategy", json!("coro")), "strategia non valida"),
        (with("supervisor", Value::Null), "richiede un supervisore"),
        (with("supervisor", json!("operator")), "non può essere anche un membro"),
        (with("supervisor", json!("fantasma")), "l'agente `fantasma` non esiste"),
        (with("members", json!([])), "da 1 a 8 membri"),
        (with("members", Value::Array(nine)), "da 1 a 8 membri"),
        (with("members", dup), "compare più volte"),
        (with("members", json!([{"agent": "fantasma", "role": ""}])), "l'agente `fantasma` non esiste"),
        (with("budget", json!({"max_tokens": 0})), "budget.max_tokens deve essere almeno 1"),
        (with("output_schema", json!({"type": "wibble"})), "output_schema non valido"),
        (with("aggregator", json!("fantasma")), "l'agente `fantasma` non esiste"),
    ] {
        let (st, msg) = failure(&app, "PUT", "/api/teams/squadra", Some(body.clone())).await;
        assert_eq!(st, 400, "{body}: {msg}");
        assert!(msg.contains(why), "{body}: {msg}");
    }
    // Remote members are accepted without being resolved.
    let (st, v) =
        dispatch(app.clone(), "PUT", "/api/teams/squadra", Some(with("members", json!([{"agent": "remote:r1", "role": "x"}])))).await;
    assert_eq!(st, 200, "{v}");

    // An agent a team uses can't be deleted; the team can, and then the agent can too.
    let mut mine = e.agent("planner").unwrap();
    mine.id = "revisore".into();
    mine.name = "Revisore".into();
    dispatch(app.clone(), "PUT", "/api/agents/revisore", Some(serde_json::to_value(&mine).unwrap())).await;
    let (st, v) = dispatch(app.clone(), "PUT", "/api/teams/squadra", Some(with("aggregator", json!("revisore")))).await;
    assert_eq!(st, 200, "{v}");
    let (st, msg) = failure(&app, "DELETE", "/api/agents/revisore", None).await;
    assert_eq!(st, 409);
    assert!(msg.contains("usato dai team: Squadra"), "{msg}");
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/teams/squadra", None).await.0, 200);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/teams/squadra", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/agents/revisore", None).await.0, 200);
}

#[tokio::test]
async fn the_mcp_route_validates_masks_secrets_and_404s_on_unknown_servers() {
    let (app, e, data) = fixture(None).await;
    for (body, why) in [
        (json!({"name": "x", "transport": "ws", "url": "http://x"}), "stdio o http"),
        (json!({"name": "x", "transport": "stdio"}), "richiede il comando"),
        (json!({"name": "x", "transport": "http"}), "richiede l'indirizzo"),
        (json!({"name": "x", "transport": "http", "url": "ftp://x"}), "http o https"),
        (json!({"name": "x", "transport": "http", "url": "boh"}), "indirizzo MCP non valido"),
        (json!({"name": "", "transport": "http", "url": "http://x"}), "serve un nome"),
        (json!({"transport": "http", "url": "http://x"}), "campo mancante `name`"),
    ] {
        let (st, msg) = failure(&app, "POST", "/api/mcp/servers", Some(body.clone())).await;
        assert_eq!(st, 400, "{body}: {msg}");
        assert!(msg.contains(why), "{body}: {msg}");
    }
    assert!(e.mcp_servers().is_empty(), "nothing was stored by the refusals");

    let body = json!({"name": "remoto", "transport": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false,
        "headers": {"Authorization": "Bearer s3cret", "Accept": "application/json"}, "env": {"API_KEY": "k-123"}});
    let (st, saved) = dispatch(app.clone(), "POST", "/api/mcp/servers", Some(body)).await;
    assert_eq!(st, 200, "{saved}");
    let id = saved["id"].as_str().unwrap().to_string();
    for shown in [&saved, &dispatch(app.clone(), "GET", "/api/mcp/servers", None).await.1[0]] {
        assert_eq!(shown["headers"]["Authorization"], "••••••", "{shown}");
        assert_eq!(shown["headers"]["Accept"], "application/json");
        assert_eq!(shown["env"]["API_KEY"], "••••••");
        assert!(!shown.to_string().contains("s3cret") && !shown.to_string().contains("k-123"), "{shown}");
    }
    // Saving what was read keeps the secrets.
    let (_, list) = dispatch(app.clone(), "GET", "/api/mcp/servers", None).await;
    let mut edited = list[0].clone();
    edited["name"] = json!("remoto2");
    let (st, v) = dispatch(app.clone(), "POST", "/api/mcp/servers", Some(edited)).await;
    assert_eq!((st, v["id"].as_str()), (200, Some(id.as_str())), "{v}");
    // The database holds the real secrets (the connection uses them); the API never shows them.
    let store = aiw_engine::store::Store::open(&data.path().join("aiw.db")).unwrap();
    let stored: McpServerConfig = store.get_doc("mcp", &id).unwrap().unwrap();
    assert_eq!(stored.name, "remoto2");
    assert_eq!(stored.headers.unwrap()["Authorization"], "Bearer s3cret");
    assert_eq!(stored.env.unwrap()["API_KEY"], "k-123");

    // Removing: 200 once, then 404 JSON.
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/mcp/servers/{id}"), None).await.0, 200);
    let (st, msg) = failure(&app, "DELETE", &format!("/api/mcp/servers/{id}"), None).await;
    assert_eq!((st, msg.as_str()), (404, format!("server MCP {id} non trovato").as_str()));
}

// ------------------------------------------------------------------ sessions and tools

#[tokio::test]
async fn unknown_sessions_are_404_on_every_route() {
    let (app, _e, _d) = fixture(None).await;
    for (method, path) in [
        ("GET", "/api/sessions/ses_assente"),
        ("GET", "/api/sessions/ses_assente/messages"),
        ("GET", "/api/sessions/ses_assente/active-run"),
        ("DELETE", "/api/sessions/ses_assente"),
        ("POST", "/api/sessions/ses_assente/runs"),
    ] {
        let body = (method == "POST").then(|| json!({"input": "ciao"}));
        let (st, msg) = failure(&app, method, path, body).await;
        assert_eq!(st, 404, "{method} {path}: {msg}");
        assert_eq!(msg, "sessione ses_assente non trovata", "{method} {path}");
    }
    // A real session: empty history and no active run are answers, not errors; deleting works once.
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "operator"}))).await;
    let sid = ses["id"].as_str().unwrap();
    let (st, msgs) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/messages"), None).await;
    assert_eq!((st, msgs), (200, json!([])));
    let (st, run) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/active-run"), None).await;
    assert_eq!((st, run), (200, json!({"run_id": null})));
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/sessions/{sid}"), None).await.0, 200);
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/sessions/{sid}"), None).await.0, 404);
    assert_eq!(dispatch(app, "GET", &format!("/api/sessions/{sid}/messages"), None).await.0, 404);
}

#[tokio::test]
async fn the_tool_list_has_the_internal_tools_and_they_cannot_be_switched() {
    let (app, _e, _d) = fixture(None).await;
    let (st, tools) = dispatch(app.clone(), "GET", "/api/tools", None).await;
    assert_eq!(st, 200);
    let tools = tools.as_array().unwrap();
    for name in ["todo", "agent", "skill", "render_ui", "code"] {
        let t = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} is not listed"));
        assert_eq!((t["source"].as_str(), t["enabled"].as_bool(), t["available"].as_bool()), (Some("internal"), Some(true), Some(true)));
        assert!(t.get("server").is_none(), "{t}");
        let (st, msg) = failure(&app, "PUT", &format!("/api/tools/{name}"), Some(json!({"enabled": false}))).await;
        assert_eq!(st, 400, "{msg}");
        assert!(msg.contains("interno"), "{msg}");
    }
    assert_eq!(tools.iter().find(|t| t["name"] == "read_file").unwrap()["source"], "builtin");
    assert_eq!(dispatch(app.clone(), "PUT", "/api/tools/read_file", Some(json!({"enabled": false}))).await.0, 200);
    // The MCP server of this app still exposes only the real tools.
    let (_, rpc) = dispatch(app, "POST", "/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;
    let names: Vec<&str> = rpc["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"list_dir") && !names.contains(&"todo") && !names.contains(&"render_ui"), "{names:?}");
}

/// Guided installation of runtimes: the plan is public, a runtime without a safe recipe is
/// refused with its manual instructions, and nothing runs for unknown ids.
#[tokio::test]
async fn runtime_installation_api() {
    let (app, _e, _d) = fixture(None).await;
    // LM Studio is installed from its website: no recipe, only the hint.
    let (st, plan) = dispatch(app.clone(), "GET", "/api/runtimes/lmstudio/install-plan", None).await;
    assert_eq!(st, 200, "{plan}");
    assert_eq!(plan["automatic"], false);
    assert!(plan["hint"].as_str().unwrap().contains("lmstudio.ai"), "{plan}");
    assert!(plan["steps"].as_array().unwrap().is_empty());
    let (st, msg) = failure(&app, "POST", "/api/runtimes/lmstudio/install", None).await;
    assert_eq!(st, 422);
    assert!(msg.contains("manuale") && msg.contains("lmstudio.ai"), "{msg}");
    // No installation has run.
    let (st, job) = dispatch(app.clone(), "GET", "/api/runtimes/lmstudio/install", None).await;
    assert_eq!(st, 200);
    assert!(job.is_null(), "{job}");
    // Unknown runtime.
    assert_eq!(failure(&app, "POST", "/api/runtimes/nope/install", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "GET", "/api/runtimes/nope/install-plan", None).await.0, 404);
    // A runtime with a recipe shows its steps (the plan is the same whatever this machine has).
    let (st, plan) = dispatch(app.clone(), "GET", "/api/runtimes/llama-server/install-plan", None).await;
    assert_eq!(st, 200, "{plan}");
    assert!(plan["runtime_name"].is_string());
}

// ---------------------------------------------------------------- updates

mod updates {
    use super::*;
    use axum::routing::get;
    use sha2::{Digest, Sha256};

    const REPO: &str = "davides93/ai-workspace-local";
    const FILE: &[u8] = b"installer bytes";

    /// A GitHub that serves the releases of the app: stable 0.3.0 and 0.4.0, beta 0.5.0-beta.1.
    async fn fake_github(good_checksum: bool) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let hash: String = Sha256::digest(FILE).iter().map(|b| format!("{b:02x}")).collect();
        let sums_hash = if good_checksum { hash } else { "0".repeat(64) };
        let asset = |name: &str| json!({"name": name, "size": FILE.len(), "browser_download_url": format!("{base}/dl/{name}")});
        let assets = |v: &str| {
            json!([
                asset(&format!("AI_Workspace_Local_{v}_universal.dmg")),
                asset(&format!("AI_Workspace_Local_{v}_x64_en-US.msi")),
                asset(&format!("AI_Workspace_Local_{v}_amd64.AppImage")),
                asset(&format!("AI_Workspace_Local_{v}_aarch64.AppImage")),
                asset("SHA256SUMS.txt"),
            ])
        };
        let rel = |tag: &str, v: &str| json!({"tag_name": tag, "body": format!("Novità {tag}"), "draft": false, "html_url": format!("https://github.com/{REPO}/releases/tag/{tag}"), "assets": assets(v)});
        let releases = json!([rel("v0.5.0-beta.1", "0.5.0-beta.1"), rel("v0.4.0", "0.4.0"), rel("v0.3.0", "0.3.0")]);
        let sums = ["0.4.0", "0.5.0-beta.1"]
            .iter()
            .flat_map(|v| {
                ["universal.dmg", "x64_en-US.msi", "amd64.AppImage", "aarch64.AppImage"]
                    .iter()
                    .map(|suffix| format!("{sums_hash}  AI_Workspace_Local_{v}_{suffix}\n"))
                    .collect::<Vec<_>>()
            })
            .collect::<String>();
        let app = Router::new()
            .route(&format!("/repos/{REPO}/releases"), get(move || async move { axum::Json(releases) }))
            .route("/dl/SHA256SUMS.txt", get(move || async move { sums }))
            .route("/dl/{name}", get(|| async { FILE.to_vec() }));
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        base
    }

    async fn engine_on(base: &str, current: &str, channel: &str) -> (Router, Engine, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let e = Engine::new(EngineOptions {
            data_dir: Some(data.path().into()),
            workspace_root: Some(data.path().join("ws")),
            mode: "daemon".into(),
            offline: true,
            update_api_base: Some(base.into()),
            current_version: Some(current.into()),
            ..Default::default()
        })
        .await
        .unwrap();
        e.set_config("global", "update.channel", json!(channel)).unwrap();
        (router(e.clone(), None), e, data)
    }

    #[tokio::test]
    async fn the_channel_decides_which_release_is_offered() {
        let base = fake_github(true).await;
        // Production, from 0.3.0: the stable 0.4.0, not the newer beta.
        let (app, _e, _d) = engine_on(&base, "0.3.0", "stable").await;
        let (st, s) = dispatch(app.clone(), "GET", "/api/update", None).await;
        assert_eq!(st, 200, "{s}");
        assert_eq!((s["state"].as_str(), s["current"].as_str(), s["channel"].as_str()), (Some("idle"), Some("0.3.0"), Some("stable")));
        assert!(s["available"].is_null());
        let (st, s) = dispatch(app.clone(), "POST", "/api/update/check", None).await;
        assert_eq!(st, 200, "{s}");
        assert_eq!(s["available"]["version"], "0.4.0");
        assert_eq!(s["available"]["channel"], "stable");
        assert_eq!(s["available"]["notes"], "Novità v0.4.0");
        assert!(s["checked_at"].is_string());
        // Beta: the newest version of all.
        let (app, _e, _d) = engine_on(&base, "0.3.0", "beta").await;
        let (_, s) = dispatch(app.clone(), "POST", "/api/update/check", None).await;
        assert_eq!(s["available"]["version"], "0.5.0-beta.1");
        assert_eq!(s["channel"], "beta");
        // `auto` on a beta build stays on betas; already at the newest: nothing to offer.
        let (app, _e, _d) = engine_on(&base, "0.5.0-beta.1", "auto").await;
        let (_, s) = dispatch(app.clone(), "POST", "/api/update/check", None).await;
        assert_eq!(s["channel"], "beta");
        assert!(s["available"].is_null(), "{s}");
        // A build older than every release, on auto = stable.
        let (app, _e, _d) = engine_on(&base, "0.1.0", "auto").await;
        let (_, s) = dispatch(app.clone(), "POST", "/api/update/check", None).await;
        assert_eq!((s["channel"].as_str(), s["available"]["version"].as_str()), (Some("stable"), Some("0.4.0")));
    }

    #[tokio::test]
    async fn download_is_verified_against_the_published_checksums() {
        let base = fake_github(true).await;
        let (app, e, data) = engine_on(&base, "0.3.0", "stable").await;
        // Nothing to download before a check.
        assert_eq!(failure(&app, "POST", "/api/update/download", None).await.0, 400);
        dispatch(app.clone(), "POST", "/api/update/check", None).await;
        let asset = e.update_status().available.unwrap().asset;
        // The test machine may have no installer in the release (an ARM Windows, for instance).
        let Some(asset) = asset else { return };
        let (st, s) = dispatch(app.clone(), "POST", "/api/update/download", None).await;
        assert_eq!(st, 200, "{s}");
        assert_eq!(s["state"], "ready");
        assert_eq!(s["received"], FILE.len());
        let file = std::path::PathBuf::from(s["file"].as_str().unwrap());
        assert!(file.starts_with(data.path().join("updates/0.4.0")), "{file:?}");
        assert_eq!(file.file_name().unwrap().to_str().unwrap(), asset.name);
        assert_eq!(std::fs::read(&file).unwrap(), FILE);
        assert!(!file.with_extension("part").exists());
        // A second request reuses the file.
        assert_eq!(dispatch(app.clone(), "POST", "/api/update/download", None).await.1["state"], "ready");
    }

    #[tokio::test]
    async fn a_file_that_does_not_match_its_checksum_is_discarded() {
        let base = fake_github(false).await;
        let (app, e, data) = engine_on(&base, "0.3.0", "stable").await;
        dispatch(app.clone(), "POST", "/api/update/check", None).await;
        if e.update_status().available.unwrap().asset.is_none() {
            return;
        }
        let (st, msg) = failure(&app, "POST", "/api/update/download", None).await;
        assert_eq!(st, 400);
        assert!(msg.contains("checksum"), "{msg}");
        let s = e.update_status();
        assert_eq!(s.file, None);
        assert_eq!(format!("{:?}", s.state), "Error");
        let leftovers = std::fs::read_dir(data.path().join("updates/0.4.0")).map(|d| d.count()).unwrap_or(0);
        assert_eq!(leftovers, 0, "neither the partial nor the final file stays");
        // Installing without a verified file is refused.
        assert_eq!(failure(&app, "POST", "/api/update/install", None).await.0, 400);
    }

    #[tokio::test]
    async fn an_unreachable_github_is_an_error_not_a_crash() {
        let (app, _e, _d) = engine_on("http://127.0.0.1:1", "0.3.0", "stable").await;
        let (st, msg) = failure(&app, "POST", "/api/update/check", None).await;
        assert!(st >= 400, "{st}");
        assert!(msg.contains("GitHub"), "{msg}");
        let (_, s) = dispatch(app, "GET", "/api/update", None).await;
        assert_eq!(s["state"], "error");
    }

    #[tokio::test]
    async fn the_channel_setting_is_validated_and_not_settable_by_a_workspace() {
        let base = fake_github(true).await;
        let (app, e, _d) = engine_on(&base, "0.3.0", "stable").await;
        assert!(e.set_config("global", "update.channel", json!("canary")).is_err());
        // What a repository ships in its own .aiw/config.json is ignored for the update settings.
        e.set_config("workspace", "update.channel", json!("alpha")).unwrap();
        e.set_config("workspace", "update.auto_download", json!(true)).unwrap();
        let (_, s) = dispatch(app, "GET", "/api/update", None).await;
        assert_eq!(s["channel"], "stable", "a repository cannot pick the update channel");
        assert_eq!(e.config().effective["update.auto_download"], json!(false));
    }
}

// ------------------------------------------------------------------ checkpoints

#[tokio::test]
async fn a_checkpoint_is_read_and_rewound_over_http() {
    let (app, e, data) = fixture(None).await;
    let ws = e.workspace_root();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("a.md"), "cambiato").unwrap();
    std::fs::write(ws.join("nuovo.md"), "creato").unwrap();
    let run = data.path().join("checkpoints/run_http");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(run.join("a.md"), "originale").unwrap();
    std::fs::write(run.join(".aiw-run.json"), json!({"root": ws.display().to_string(), "created": ["nuovo.md"]}).to_string()).unwrap();

    let (st, v) = dispatch(app.clone(), "GET", "/api/runs/run_http/checkpoint", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["restore"], json!(["a.md"]));
    assert_eq!(v["delete"], json!(["nuovo.md"]));
    assert_eq!(failure(&app, "GET", "/api/runs/run_assente/checkpoint", None).await.0, 404);
    assert_eq!(failure(&app, "POST", "/api/runs/run_assente/rewind", None).await.0, 404);

    let (st, v) = dispatch(app.clone(), "POST", "/api/runs/run_http/rewind", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["restored"], json!(["a.md"]));
    assert_eq!(v["deleted"], json!(["nuovo.md"]));
    assert_eq!(std::fs::read_to_string(ws.join("a.md")).unwrap(), "originale");
    assert!(!ws.join("nuovo.md").exists());
    assert_eq!(failure(&app, "POST", "/api/runs/run_http/rewind", None).await.0, 404);
}
