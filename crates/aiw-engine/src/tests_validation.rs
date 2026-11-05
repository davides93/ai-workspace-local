//! What the engine refuses before it stores anything (policy, hooks, agents, MCP servers), how
//! secrets of MCP servers are shown and kept, the internal tools in the tool list, and the
//! "not found" answers for unknown sessions and servers.

use super::*;

async fn engine() -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    (e, data, ws)
}

fn invalid<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(e @ Error::Invalid(_)) => {
            assert_eq!(e.status(), 400);
            e.to_string()
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

// ------------------------------------------------------------------ policy and hooks

fn rule(tool: &str, pattern: Option<&str>, action: &str) -> PolicyRule {
    PolicyRule { tool: tool.into(), pattern: pattern.map(String::from), action: action.into() }
}

#[tokio::test]
async fn policy_rules_are_validated_and_a_bad_list_changes_nothing() {
    let (e, _d, _w) = engine().await;
    e.set_policy_rules(vec![rule("shell", Some("rm *"), "deny")]).unwrap();
    for (bad, why) in [
        (rule("shell", None, "block"), "allow, ask o deny"),
        (rule("", None, "allow"), "nome del tool"),
        (rule("  ", None, "ask"), "nome del tool"),
        (rule("sh[ell", None, "deny"), "glob"),
    ] {
        let msg = invalid(e.set_policy_rules(vec![rule("write_file", None, "ask"), bad]));
        assert!(msg.starts_with("regola 2: ") && msg.contains(why), "{msg}");
    }
    assert_eq!(e.policy_rules(), vec![rule("shell", Some("rm *"), "deny")], "the stored rules survived every refusal");
    // Rules are stored trimmed; an empty pattern means any subject. An empty list is a valid choice.
    e.set_policy_rules(vec![rule(" shell ", Some("  "), "ask")]).unwrap();
    assert_eq!(e.policy_rules(), vec![rule("shell", None, "ask")]);
    e.set_policy_rules(vec![]).unwrap();
    assert!(e.policy_rules().is_empty());
}

fn hook(event: &str, command: &str, timeout_s: u64) -> HookConfig {
    HookConfig { event: event.into(), matcher: None, command: command.into(), timeout_s }
}

#[tokio::test]
async fn hooks_are_validated_and_a_bad_list_changes_nothing() {
    let (e, _d, _w) = engine().await;
    e.set_hooks(vec![hook("Stop", "echo fatto", 5)]).unwrap();
    for (bad, why) in [
        (hook("Nope", "echo", 30), "PreToolUse"),
        (hook("Stop", "  ", 30), "comando"),
        (hook("Stop", "echo", 0), "tra 1 e 600"),
        (hook("Stop", "echo", 601), "tra 1 e 600"),
    ] {
        let msg = invalid(e.set_hooks(vec![hook("PreToolUse", "echo ok", 30), bad]));
        assert!(msg.starts_with("hook 2: ") && msg.contains(why), "{msg}");
    }
    assert_eq!(e.hooks(), vec![hook("Stop", "echo fatto", 5)]);
    e.set_hooks(vec![HookConfig { matcher: Some(" shell ".into()), ..hook("PreToolUse", " ./check.sh ", 600) }]).unwrap();
    let h = e.hooks();
    assert_eq!((h[0].matcher.as_deref(), h[0].command.as_str()), (Some("shell"), "./check.sh"));
}

// ------------------------------------------------------------------ agents

fn agent(e: &Engine) -> AgentProfile {
    AgentProfile { id: "mio".into(), name: "Mio".into(), ..e.agent("planner").unwrap() }
}

#[tokio::test]
async fn an_agent_needs_an_existing_profile_turns_and_known_tools() {
    let (e, _d, _w) = engine().await;
    let ok = agent(&e);
    e.save_agent(ok.clone()).unwrap();

    let msg = invalid(e.save_agent(AgentProfile { inference_profile: "non-esiste".into(), ..ok.clone() }));
    assert!(msg.contains("profilo di inferenza `non-esiste` non esiste"), "{msg}");
    invalid(e.save_agent(AgentProfile { inference_profile: String::new(), ..ok.clone() }));
    let msg = invalid(e.save_agent(AgentProfile { max_turns: 0, ..ok.clone() }));
    assert!(msg.contains("max_turns"), "{msg}");

    // The output schema must be usable; a good one is stored as given.
    let msg = invalid(e.save_agent(AgentProfile { output_schema: Some(serde_json::json!({"type": "stringa"})), ..ok.clone() }));
    assert!(msg.contains("output_schema"), "{msg}");
    invalid(e.save_agent(AgentProfile { output_schema: Some(serde_json::json!("object")), ..ok.clone() }));
    let schema = serde_json::json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]});
    assert_eq!(e.save_agent(AgentProfile { output_schema: Some(schema.clone()), ..ok.clone() }).unwrap().output_schema, Some(schema));
    e.save_agent(ok.clone()).unwrap();

    // Unknown tools are all listed.
    let msg =
        invalid(e.save_agent(AgentProfile { tools: vec!["read_file".into(), "volare".into(), "teletrasporto".into()], ..ok.clone() }));
    assert!(msg.contains("volare, teletrasporto") && !msg.contains("read_file,"), "{msg}");
    assert_eq!(e.agent("mio").unwrap().tools, ok.tools, "a refused save changes nothing");

    // `*`, the registry tools, the internal tools and MCP tools (their server may be offline) are known.
    let tools: Vec<String> = [
        "*",
        "read_file",
        "shell",
        "browser_click",
        "todo",
        "agent",
        "skill",
        "render_ui",
        "code",
        "mcp__github__list_issues",
        "mcp__x__*",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(e.save_agent(AgentProfile { tools: tools.clone(), ..ok.clone() }).unwrap().tools, tools);
    assert!(e.save_agent(AgentProfile { tools: vec![], ..ok.clone() }).is_ok(), "an agent without tools is allowed");
    // Every agent the app ships with passes its own validation.
    for a in e.agents().unwrap() {
        e.save_agent(a).unwrap();
    }
}

// ------------------------------------------------------------------ MCP servers

fn http_server(url: &str) -> McpServerConfig {
    McpServerConfig {
        id: String::new(),
        name: "Mio server".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(url.into()),
        env: None,
        headers: None,
        enabled: false,
    }
}

fn stdio_server(command: Option<&str>) -> McpServerConfig {
    McpServerConfig { transport: "stdio".into(), command: command.map(String::from), url: None, ..http_server("") }
}

#[tokio::test]
async fn an_mcp_server_is_checked_before_it_is_saved() {
    let (e, _d, _w) = engine().await;
    for (bad, why) in [
        (McpServerConfig { name: "  ".into(), ..http_server("http://x/mcp") }, "nome"),
        (McpServerConfig { transport: "ws".into(), ..http_server("http://x/mcp") }, "stdio o http"),
        (McpServerConfig { transport: String::new(), ..http_server("http://x/mcp") }, "stdio o http"),
        (stdio_server(None), "comando"),
        (stdio_server(Some("   ")), "comando"),
        (http_server(""), "indirizzo"),
        (McpServerConfig { url: None, ..http_server("") }, "indirizzo"),
        (http_server("non è un url"), "non valido"),
        (http_server("ftp://x/mcp"), "http o https"),
        (http_server("file:///etc/passwd"), "http o https"),
        (http_server("http://"), "non valido"),
    ] {
        let msg = invalid(e.save_mcp_server(bad.clone()).await);
        assert!(msg.contains(why), "{bad:?}: {msg}");
    }
    assert!(e.mcp_servers().is_empty(), "nothing was stored by a refused save");

    let saved = e.save_mcp_server(http_server("  http://127.0.0.1:9/mcp ")).await.unwrap();
    assert_eq!(saved.config.url.as_deref(), Some("http://127.0.0.1:9/mcp"));
    assert_eq!(saved.config.name, "Mio_server");
    assert!(e.save_mcp_server(stdio_server(Some("npx"))).await.is_ok());
    assert_eq!(e.mcp_servers().len(), 2);
}

#[tokio::test]
async fn mcp_secrets_are_masked_on_read_and_kept_when_the_mask_comes_back() {
    let (e, _d, _w) = engine().await;
    let mut cfg = http_server("http://127.0.0.1:9/mcp");
    cfg.headers = serde_json::from_value(json!({"Authorization": "Bearer s3cret", "Accept": "application/json"})).unwrap();
    cfg.env = serde_json::from_value(json!({"GITHUB_TOKEN": "ghp_xyz", "MODE": "dev"})).unwrap();
    let saved = e.save_mcp_server(cfg).await.unwrap();
    let id = saved.config.id.clone();
    // The answer to the save and every later read show the mask, never the secret.
    let shown = |info: &McpServerInfo| serde_json::to_string(info).unwrap();
    assert!(!shown(&saved).contains("s3cret") && !shown(&saved).contains("ghp_xyz"), "{}", shown(&saved));
    let read = e.mcp_servers().into_iter().find(|s| s.config.id == id).unwrap();
    assert_eq!(read.config.headers.as_ref().unwrap()["Authorization"], SECRET_MASK);
    assert_eq!(read.config.headers.as_ref().unwrap()["Accept"], "application/json", "ordinary values stay readable");
    assert_eq!(read.config.env.as_ref().unwrap()["GITHUB_TOKEN"], SECRET_MASK);
    assert_eq!(read.config.env.as_ref().unwrap()["MODE"], "dev");
    // What is stored (and what the connection uses) is the real value.
    let stored: McpServerConfig = e.0.store.get_doc("mcp", &id).unwrap().unwrap();
    assert_eq!(stored.headers.as_ref().unwrap()["Authorization"], "Bearer s3cret");

    // The editor sends the masked config back with another change: the secrets stay.
    let mut edited = read.config.clone();
    edited.url = Some("http://127.0.0.1:9/other".into());
    edited.headers.as_mut().unwrap().insert("Accept".into(), json!("text/plain"));
    e.save_mcp_server(edited).await.unwrap();
    let stored: McpServerConfig = e.0.store.get_doc("mcp", &id).unwrap().unwrap();
    assert_eq!(stored.url.as_deref(), Some("http://127.0.0.1:9/other"));
    assert_eq!(stored.headers.as_ref().unwrap()["Authorization"], "Bearer s3cret");
    assert_eq!(stored.headers.as_ref().unwrap()["Accept"], "text/plain");
    assert_eq!(stored.env.as_ref().unwrap()["GITHUB_TOKEN"], "ghp_xyz");

    // A new secret replaces the stored one; a removed row is removed; a mask with nothing behind
    // it (a new key) is not stored as a secret.
    let mut edited = e.mcp_servers().into_iter().find(|s| s.config.id == id).unwrap().config;
    edited.headers.as_mut().unwrap().insert("Authorization".into(), json!("Bearer nuovo"));
    edited.headers.as_mut().unwrap().insert("X-Api-Key".into(), json!(SECRET_MASK));
    edited.env.as_mut().unwrap().remove("GITHUB_TOKEN");
    e.save_mcp_server(edited).await.unwrap();
    let stored: McpServerConfig = e.0.store.get_doc("mcp", &id).unwrap().unwrap();
    assert_eq!(stored.headers.as_ref().unwrap()["Authorization"], "Bearer nuovo");
    assert!(!stored.headers.as_ref().unwrap().contains_key("X-Api-Key"));
    assert!(!stored.env.as_ref().unwrap().contains_key("GITHUB_TOKEN"));
}

#[tokio::test]
async fn removing_an_unknown_mcp_server_is_not_found() {
    let (e, _d, _w) = engine().await;
    let err = e.remove_mcp_server("mcp_assente").await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)) && err.status() == 404, "{err:?}");
    assert_eq!(err.to_string(), "server MCP mcp_assente non trovato");
    let saved = e.save_mcp_server(http_server("http://127.0.0.1:9/mcp")).await.unwrap();
    e.remove_mcp_server(&saved.config.id).await.unwrap();
    assert!(matches!(e.remove_mcp_server(&saved.config.id).await, Err(Error::NotFound(_))), "the second removal finds nothing");
}

// ------------------------------------------------------------------ tools

#[tokio::test]
async fn the_internal_tools_are_listed_and_cannot_be_switched() {
    let (e, _d, _w) = engine().await;
    let tools = e.tools();
    for name in ["todo", "agent", "skill", "render_ui", "code"] {
        let t = tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!((t.source.as_str(), t.server.as_deref(), t.enabled, t.available), ("internal", None, true, true), "{name}");
        assert!(!t.description.is_empty() && t.parameters["type"] == "object", "{name}");
        let msg = invalid(e.set_tool_enabled(name, false));
        assert!(msg.contains("interno"), "{msg}");
    }
    // The registry tools keep their source and their switch.
    let shell = tools.iter().find(|t| t.name == "shell").unwrap();
    assert_eq!(shell.source, "builtin");
    e.set_tool_enabled("shell", false).unwrap();
    assert!(!e.tools().iter().find(|t| t.name == "shell").unwrap().enabled);
    assert!(matches!(e.set_tool_enabled("inesistente", false), Err(Error::NotFound(_))));
    // Every name an agent of the app lists is a name of this list.
    let listed: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    for a in e.agents().unwrap() {
        for t in a.tools.iter().filter(|t| *t != "*") {
            assert!(listed.contains(t), "{} lists `{t}`, which GET /api/tools does not show", a.id);
        }
    }
}

// ------------------------------------------------------------------ sessions

#[tokio::test]
async fn an_unknown_session_is_not_found_everywhere() {
    let (e, _d, _w) = engine().await;
    for err in [e.messages("ses_assente").unwrap_err(), e.delete_session("ses_assente").unwrap_err()] {
        assert!(matches!(err, Error::NotFound(_)) && err.status() == 404, "{err:?}");
        assert_eq!(err.to_string(), "sessione ses_assente non trovata");
    }
    let s = e.create_session("operator", None).unwrap();
    assert!(e.messages(&s.id).unwrap().is_empty(), "a known session with no messages is an empty list");
    e.delete_session(&s.id).unwrap();
    assert!(matches!(e.delete_session(&s.id), Err(Error::NotFound(_))), "the second delete finds nothing");
    assert!(matches!(e.messages(&s.id), Err(Error::NotFound(_))));
}
