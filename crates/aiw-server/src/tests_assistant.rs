//! What the server tells the engine about a run (its origin), who may answer the confirmation of a
//! management tool, and what the MCP server and the policy page show of the management tools.

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use aiw_core::api::AgentProfile;
use aiw_engine::RunOrigin;
use aiw_runtimes::ScriptedBackend;
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use std::sync::{Arc, Mutex};

/// A management tool that does nothing but note the origin of the run it was called from.
struct Probe {
    name: &'static str,
    risk: Risk,
    engine: Engine,
    seen: Arc<Mutex<Vec<Option<RunOrigin>>>>,
}

#[async_trait::async_trait]
impl Tool for Probe {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "Strumento di prova."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> Risk {
        self.risk
    }
    fn explicit_grant(&self) -> bool {
        true
    }
    async fn call(&self, _args: Value, ctx: &ToolContext) -> aiw_core::Result<ToolOutput> {
        self.seen.lock().unwrap().push(self.engine.run_origin(&ctx.run_id));
        Ok(ToolOutput::text("visto"))
    }
}

type Seen = Arc<Mutex<Vec<Option<RunOrigin>>>>;

/// The fixture with two probes registered (`aiw_model_search`: reads, `aiw_setting_set`: changes) and the agent `gestore`
/// that lists both, in `ask` mode; the model calls `tool` once per run and then answers.
async fn setup(runs: usize, tool: &str) -> (Router, Engine, Seen, tempfile::TempDir, tempfile::TempDir) {
    let turns = (0..runs)
        .flat_map(|i| [ScriptedBackend::tool_turn(&format!("c{i}"), tool, json!({})), ScriptedBackend::text_turn("fatto")])
        .collect();
    let (app, e, d, w) = fixture(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(turns)));
    let seen: Seen = Default::default();
    for (name, risk) in [("aiw_model_search", Risk::Network), ("aiw_setting_set", Risk::Write)] {
        e.tool_registry().register(Arc::new(Probe { name, risk, engine: e.clone(), seen: seen.clone() }));
    }
    let a = AgentProfile {
        id: "gestore".into(),
        name: "Gestore".into(),
        mode: "ask".into(),
        tools: vec!["aiw_model_search".into(), "aiw_setting_set".into()],
        ..e.agent("planner").unwrap()
    };
    e.save_agent(a).unwrap();
    (app, e, seen, d, w)
}

fn device_token(e: &Engine) -> String {
    format!("Bearer {}", e.create_api_token("Telefono", "device", None, None, None).unwrap().token)
}

async fn wait_done(e: &Engine, run_id: &str) {
    for _ in 0..250 {
        if e.run_done(run_id) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("run {run_id} did not finish");
}

const ALL: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };

#[tokio::test]
async fn a_chat_run_carries_the_origin_of_the_caller() {
    let (app, e, seen, _d, _w) = setup(2, "aiw_model_search").await;
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "gestore"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    // The owner (the app on this computer): everything.
    let (st, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "ciao"}))).await;
    assert_eq!(st, 200, "{run}");
    wait_done(&e, run["run_id"].as_str().unwrap()).await;
    // A paired phone starts a run in the same chat: it is not the owner, so no secrets either.
    let auth = device_token(&e);
    let r = req(
        "POST",
        &format!("/api/sessions/{sid}/runs"),
        Some(LAN),
        &[("host", "192.168.1.5:7777"), ("authorization", &auth)],
        Some(json!({"input": "ciao dal telefono"})),
    );
    let (st, _, run) = send(&app, r).await;
    assert_eq!(st, StatusCode::OK, "{run}");
    wait_done(&e, run["run_id"].as_str().unwrap()).await;
    assert_eq!(*seen.lock().unwrap(), [Some(ALL), Some(RunOrigin { owner: false, questions: true, secrets: false })]);
}

/// One AG-UI request to `/agui/gestore`, to the end of the stream.
async fn agui(app: &Router, thread: &str, props: Option<Value>, peer: Option<([u8; 4], &str)>) -> StatusCode {
    let mut body = json!({"threadId": thread, "runId": "r1", "messages": [{"id": "m1", "role": "user", "content": "ciao"}]});
    if let Some(p) = props {
        body["forwardedProps"] = p;
    }
    let r = match peer {
        None => req("POST", "/agui/gestore", None, &[], Some(body)),
        Some((ip, auth)) => req("POST", "/agui/gestore", Some(ip), &[("host", "192.168.1.5:7777"), ("authorization", auth)], Some(body)),
    };
    let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
    let status = res.status();
    tokio::time::timeout(Duration::from_secs(10), axum::body::to_bytes(res.into_body(), 1 << 22)).await.expect("the stream ends").unwrap();
    status
}

#[tokio::test]
async fn an_agui_run_takes_what_the_client_can_show_from_forwarded_props() {
    let (app, e, seen, _d, _w) = setup(7, "aiw_model_search").await;
    let both = json!({"aiw": {"interactive": ["questions", "secrets"]}});
    // The owner's client, with and without a declaration (a missing, null or malformed one counts as none).
    let cases: [(Option<Value>, RunOrigin); 6] = [
        (None, RunOrigin { owner: true, questions: false, secrets: false }),
        (Some(Value::Null), RunOrigin { owner: true, questions: false, secrets: false }),
        (Some(json!({"aiw": {"interactive": "secrets"}})), RunOrigin { owner: true, questions: false, secrets: false }),
        (Some(json!("questions")), RunOrigin { owner: true, questions: false, secrets: false }),
        (Some(json!({"aiw": {"interactive": ["questions", "altro"]}})), RunOrigin { owner: true, questions: true, secrets: false }),
        (Some(both.clone()), ALL),
    ];
    for (i, (props, want)) in cases.into_iter().enumerate() {
        assert_eq!(agui(&app, &format!("t{i}"), props.clone(), None).await, StatusCode::OK, "{props:?}");
        assert_eq!(seen.lock().unwrap().pop(), Some(Some(want)), "{props:?}");
    }
    // A paired device that declares both: it shows questions, but cannot give a secret (that endpoint needs full access).
    let auth = device_token(&e);
    assert_eq!(agui(&app, "t-device", Some(both), Some((LAN, &auth))).await, StatusCode::OK);
    assert_eq!(seen.lock().unwrap().pop(), Some(Some(RunOrigin { owner: false, questions: true, secrets: false })));
}

#[tokio::test]
async fn only_the_owner_can_answer_the_confirmation_of_a_management_tool_over_the_api() {
    let (app, e, seen, _d, _w) = setup(1, "aiw_setting_set").await;
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "gestore"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (_, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "cambia"}))).await;
    let rid = run["run_id"].as_str().unwrap().to_string();
    let mut approval = None;
    for _ in 0..250 {
        approval = e.approvals().into_iter().next();
        if approval.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let approval = approval.expect("the change asks for a confirmation");
    assert_eq!(approval.tool, "aiw_setting_set");
    let uri = format!("/api/approvals/{}", approval.id);
    let allow = json!({"decision": "allow", "remember": true});

    // A paired phone is a device, not the owner: 403, and the request is still there.
    let phone = device_token(&e);
    let r = req("POST", &uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", &phone)], Some(allow.clone()));
    let (st, _, body) = send(&app, r).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{body}");
    assert!(body["error"].as_str().unwrap().contains("proprietario"), "{body}");
    // The owner token of this computer used from the network without TLS is a device's, too.
    let owner = format!("Bearer {}", e.token());
    let r = req("POST", &uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", &owner)], Some(allow.clone()));
    assert_eq!(send(&app, r).await.0, StatusCode::FORBIDDEN);
    assert_eq!(e.approvals().len(), 1, "the request stays open");
    assert!(seen.lock().unwrap().is_empty(), "nothing was changed");
    assert!(!e.run_done(&rid));

    // The app on this computer (no token, no network peer) is the owner; and so is the owner token over TLS.
    let mut r = req("POST", &uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", &owner)], Some(allow));
    r.extensions_mut().insert(security::TlsConn);
    assert_eq!(send(&app, r).await.0, StatusCode::OK);
    wait_done(&e, &rid).await;
    assert_eq!(*seen.lock().unwrap(), [Some(ALL)], "the tool ran once, in the run of the owner");

    // An ordinary tool stays answerable from a paired phone, as ever: that is what "device" is for.
    let (st, body) = dispatch(app.clone(), "POST", "/api/approvals/apr_non_esiste", Some(json!({"decision": "allow"}))).await;
    assert_eq!(st, 404, "{body}");
}

fn rpc(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
}

#[tokio::test]
async fn the_mcp_server_does_not_offer_the_management_tools_and_refuses_them_by_name() {
    let (app, e, seen, _d, _w) = setup(0, "aiw_model_search").await;
    let (_, listed) = dispatch(app.clone(), "POST", "/mcp", Some(rpc("tools/list", json!({})))).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"list_dir") && names.contains(&"shell"), "{names:?}");
    // The two registered management tools are not there, and nothing of the plan's catalogue is either.
    for hidden in aiw_engine::management::ALL.iter().chain(["aiw_setting_set", "aiw_model_search"].iter()) {
        assert!(!names.contains(hidden), "{hidden} is offered over MCP");
    }
    // `aiw_agent_run` is the server's own tool for delegating to an agent: it stays.
    assert!(names.contains(&"aiw_agent_run"), "{names:?}");
    // Called by name anyway, the tool refuses and nobody is asked to confirm it.
    for tool in ["aiw_setting_set", "aiw_model_search"] {
        let (_, called) = dispatch(app.clone(), "POST", "/mcp", Some(rpc("tools/call", json!({"name": tool, "arguments": {}})))).await;
        assert_eq!(called["result"]["isError"], true, "{tool}: {called}");
        assert!(
            called["result"]["content"][0]["text"].as_str().unwrap().contains("disponibile solo all'Assistente in una chat"),
            "{called}"
        );
    }
    assert!(e.approvals().is_empty());
    assert!(seen.lock().unwrap().is_empty(), "the tools never ran");
}

#[tokio::test]
async fn the_policy_page_shows_only_the_rules_of_the_person() {
    let (app, e, _seen, _d, _w) = setup(0, "aiw_model_search").await;
    let (_, policy) = dispatch(app.clone(), "GET", "/api/policy", None).await;
    assert_eq!(policy["rules"], json!([]), "the rules built into the app are not the person's");
    assert!(!e.run_rules().is_empty(), "…though every run is judged by them");
    let own = json!({"rules": [{"tool": "shell", "pattern": "rm *", "action": "deny"}]});
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/policy", Some(own.clone())).await;
    assert_eq!(st, 200, "{saved}");
    assert_eq!(saved["rules"], own["rules"]);
    let (_, policy) = dispatch(app, "GET", "/api/policy", None).await;
    assert_eq!(policy["rules"], own["rules"]);
    assert!(!policy.to_string().contains("aiw_"), "{policy}");
}

#[tokio::test]
async fn the_assistant_is_listed_and_saved_back_by_the_api_and_the_tool_list_breaks_on_nothing() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let (st, agents) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    assert_eq!(st, 200, "{agents}");
    let a = agents.as_array().unwrap().iter().find(|a| a["id"] == "assistant").expect("the Assistant is among the agents").clone();
    assert_eq!((a["name"].as_str(), a["mode"].as_str(), a["sandbox"].as_str()), (Some("Assistente"), Some("ask"), Some("workspace-write")));
    assert_eq!(a["inference_profile"], "auto:agent");
    assert_eq!(a["max_turns"], 40);
    assert_eq!(a["delegate_tools"], json!(["*"]));
    assert_eq!(a["tools"].as_array().unwrap().len(), 25);
    // Its tool list names tools this build has not registered: the editor of the Developer view saves it back all the same,
    // and the extra field survives.
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/agents/assistant", Some(a.clone())).await;
    assert_eq!(st, 200, "{saved}");
    assert_eq!(saved["delegate_tools"], json!(["*"]));
    assert_eq!(saved["tools"], a["tools"]);
    // An agent seeded by the app is not deleted.
    let (st, body) = dispatch(app.clone(), "DELETE", "/api/agents/assistant", None).await;
    assert_eq!(st, 409, "{body}");
    // The Developer view's list of tools holds what is registered (the engine registers `aiw_inspect`) and answers 200.
    let (st, tools) = dispatch(app.clone(), "GET", "/api/tools", None).await;
    assert_eq!(st, 200);
    let names: Vec<&str> = tools.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"aiw_inspect") && names.contains(&"shell"), "{names:?}");
    let inspect = tools.as_array().unwrap().iter().find(|t| t["name"] == "aiw_inspect").unwrap();
    assert_eq!(inspect["risk"], "read");
    // A run of the Assistant started from the chat route works with the tools it has, and `aiw_inspect` answers in it.
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![
            ScriptedBackend::tool_turn("i1", "aiw_inspect", json!({"topic": "overview"})),
            ScriptedBackend::text_turn("Vedo il tuo computer."),
        ])),
    );
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "assistant"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (st, run) =
        dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "come sta il mio computer?"}))).await;
    assert_eq!(st, 200, "{run}");
    wait_done(&e, run["run_id"].as_str().unwrap()).await;
    let messages = e.messages(&sid).unwrap();
    let result = messages.iter().find(|m| m.role == "tool").expect("the tool answered").content.clone();
    assert!(result.contains("installed_models") && !result.contains("disponibile solo"), "{result}");
    assert_eq!(messages.last().unwrap().content, "Vedo il tuo computer.");
}
