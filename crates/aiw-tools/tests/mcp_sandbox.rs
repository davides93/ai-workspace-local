//! A real MCP stdio server, run under bubblewrap, that tries to read and write what it must not.
//!
//! The "server" is this very test executable started again with `AIW_FAKE_MCP=1`: the test
//! `fake_server_main` then speaks just enough MCP on stdin/stdout (no Python, no extra binary).
//! Linux only; skipped, with a note on stderr, when bubblewrap is missing or user namespaces are not
//! allowed on the machine.
#![cfg(target_os = "linux")]

use aiw_core::api::McpServerConfig;
use aiw_tools::mcp::launch::stdio_sandbox;
use aiw_tools::mcp::{McpClient, McpEvents, McpOptions};
use aiw_tools::ShellSandbox;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Entry point of the fake server. A no-op in a normal test run.
#[test]
fn fake_server_main() {
    if std::env::var("AIW_FAKE_MCP").is_err() {
        return;
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    // The test harness has just written "test fake_server_main ... " without a newline.
    let _ = writeln!(out);
    let _ = out.flush();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = req.get("id").cloned() else { continue };
        let result = match req["method"].as_str().unwrap_or_default() {
            "initialize" => {
                json!({"protocolVersion": req["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}})
            }
            "tools/list" => json!({"tools": [{"name": "probe", "description": "tries things", "inputSchema": {"type": "object"}}]}),
            "tools/call" => {
                let a = &req["params"]["arguments"];
                let mut report = String::new();
                for p in a["read"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    match std::fs::read_to_string(p) {
                        Ok(t) => report.push_str(&format!("read {p}: OK {}\n", t.trim())),
                        Err(e) => report.push_str(&format!("read {p}: ERR {e}\n")),
                    }
                }
                for p in a["write"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    match std::fs::write(p, "x") {
                        Ok(()) => report.push_str(&format!("write {p}: OK\n")),
                        Err(e) => report.push_str(&format!("write {p}: ERR {e}\n")),
                    }
                }
                if let Some(addr) = a["connect"].as_str() {
                    let addr = addr.parse().unwrap();
                    match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)) {
                        Ok(_) => report.push_str("connect: OK\n"),
                        Err(e) => report.push_str(&format!("connect: ERR {e}\n")),
                    }
                }
                if let Some(names) = a["env"].as_array() {
                    for n in names.iter().filter_map(Value::as_str) {
                        report.push_str(&format!("env {n}: {}\n", std::env::var(n).unwrap_or_else(|_| "unset".into())));
                    }
                }
                if let Some(p) = a["pid"].as_u64() {
                    report
                        .push_str(&format!("proc {p}: {}\n", if Path::new(&format!("/proc/{p}")).exists() { "visible" } else { "hidden" }));
                }
                json!({"content": [{"type": "text", "text": report}]})
            }
            _ => json!({}),
        };
        let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "result": result}));
        let _ = out.flush();
    }
    std::process::exit(0);
}

struct NoEvents;
impl McpEvents for NoEvents {}

/// A folder outside `/tmp`: bubblewrap gives the server a private `/tmp`.
fn outside_tmp() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap()
}

fn config(env: Value) -> McpServerConfig {
    let exe = std::env::current_exe().unwrap();
    McpServerConfig {
        id: "fake1".into(),
        name: "fake".into(),
        transport: "stdio".into(),
        command: Some(exe.display().to_string()),
        args: Some(["--exact", "fake_server_main", "--nocapture", "--test-threads=1"].map(String::from).to_vec()),
        url: None,
        env: Some(
            json!({"AIW_FAKE_MCP": "1"})
                .as_object()
                .unwrap()
                .clone()
                .into_iter()
                .chain(env.as_object().cloned().unwrap_or_default())
                .collect(),
        ),
        headers: None,
        enabled: true,
        sampling: false,
        oauth_client_id: None,
        sandbox: "off".into(),
        sandbox_network: true,
    }
}

async fn probe(cfg: &McpServerConfig, sandbox: Option<aiw_tools::mcp::launch::StdioSandbox>, args: Value) -> Option<String> {
    let opts = McpOptions { sandbox, max_attempts: 1, ..Default::default() };
    let client = McpClient::with_options(cfg, Arc::new(NoEvents), opts);
    if let Err(e) = client.start().await {
        eprintln!("skipped: the sandboxed server did not start here: {e}");
        return None;
    }
    let tools = McpClient::tools(&client);
    assert!(tools.iter().any(|t| t.name == "probe"), "{tools:?}");
    let out = client.call_tool("probe", args, None).await.unwrap();
    client.shutdown().await;
    Some(out.content)
}

fn bwrap_usable() -> bool {
    if which_bwrap().is_none() {
        eprintln!("skipped: bwrap is not installed");
        return false;
    }
    let ok = std::process::Command::new("bwrap")
        .args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--", "/bin/true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipped: bwrap cannot create a sandbox here (user namespaces not allowed?)");
    }
    ok
}

fn which_bwrap() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("bwrap")).find(|c| c.is_file()))
}

#[tokio::test]
async fn an_isolated_server_cannot_read_the_data_folder_or_the_deny_list() {
    if !bwrap_usable() {
        return;
    }
    let base = outside_tmp();
    let data = base.path().join("data");
    std::fs::create_dir_all(data.join("skills")).unwrap();
    std::fs::write(data.join("token"), "TOP-SECRET").unwrap();
    std::fs::write(data.join("skills/s.md"), "SKILL-OK").unwrap();
    let ssh = base.path().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id"), "PRIVATE-KEY").unwrap();
    let other = base.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("ok.txt"), "VISIBLE").unwrap();
    let ws = tempfile::tempdir().unwrap();
    let home = data.join("mcp-home/fake1");
    let shell = ShellSandbox { network: false, hidden: vec![data.clone(), ssh.clone()], env_allow: vec![] };
    let sb = stdio_sandbox("workspace-write", true, ws.path(), home.clone(), &shell, &[data.join("skills")]).unwrap();
    let cfg = config(json!({"MY_SERVER_KEY": "configured"}));
    // The daemon's own environment must not reach the server.
    std::env::set_var("AIW_TEST_MCP_DAEMON_TOKEN", "daemon-secret");
    let args = json!({
        "read": [data.join("token"), ssh.join("id"), data.join("skills/s.md"), other.join("ok.txt")],
        "write": [data.join("planted"), ws.path().join("inside.txt"), other.join("planted"), home.join("cache")],
        "env": ["AIW_TEST_MCP_DAEMON_TOKEN", "MY_SERVER_KEY", "HOME", "PATH"],
        "pid": std::process::id(),
    });
    let Some(report) = probe(&cfg, Some(sb), args).await else { return };
    let line =
        |needle: &str| report.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} not in:\n{report}")).to_string();
    assert!(line("/data/token").contains("ERR"), "{report}");
    assert!(line("/ssh/id").contains("ERR"), "{report}");
    assert!(!report.contains("TOP-SECRET") && !report.contains("PRIVATE-KEY"), "{report}");
    assert!(line("s.md").contains("OK SKILL-OK"), "skills stay readable: {report}");
    assert!(line("ok.txt").contains("OK VISIBLE"), "{report}");
    // Inside the sandbox the data folder is an empty private tmpfs: a write may "succeed" there, but
    // nothing reaches the real folder.
    assert!(!data.join("planted").exists(), "{report}");
    assert!(!other.join("planted").exists(), "writes outside the workspace are refused: {report}");
    assert!(line("inside.txt").contains("OK"), "{report}");
    assert!(ws.path().join("inside.txt").exists());
    assert!(line("cache").contains("OK"), "the private home is writable: {report}");
    assert!(home.join("cache").exists());
    assert!(line("env AIW_TEST_MCP_DAEMON_TOKEN").contains("unset"), "{report}");
    assert!(line("env MY_SERVER_KEY").contains("configured"), "{report}");
    assert!(line("env HOME").contains(&home.display().to_string()), "{report}");
    assert!(line("env PATH").contains("/"), "{report}");
    assert!(line("proc ").contains("hidden"), "the daemon is not visible through /proc: {report}");
}

#[tokio::test]
async fn the_network_of_an_isolated_server_can_be_closed() {
    if !bwrap_usable() {
        return;
    }
    // A local listener: reachable through the host's network namespace, not from a closed one.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let home = ws.path().join("home");
    let shell = ShellSandbox::default();
    let cfg = config(json!({}));
    let closed = stdio_sandbox("workspace-write", false, ws.path(), home.clone(), &shell, &[]).unwrap();
    let Some(report) = probe(&cfg, Some(closed), json!({"connect": addr.to_string()})).await else { return };
    assert!(report.contains("connect: ERR"), "network off: {report}");
    let open = stdio_sandbox("workspace-write", true, ws.path(), home, &shell, &[]).unwrap();
    let Some(report) = probe(&cfg, Some(open), json!({"connect": addr.to_string()})).await else { return };
    assert!(report.contains("connect: OK"), "network on: {report}");
}

#[tokio::test]
async fn a_read_only_server_cannot_write_the_workspace() {
    if !bwrap_usable() {
        return;
    }
    let ws = tempfile::tempdir().unwrap();
    let home = ws.path().join("home");
    let sb = stdio_sandbox("read-only", true, ws.path(), home, &ShellSandbox::default(), &[]).unwrap();
    let Some(report) = probe(&config(json!({})), Some(sb), json!({"write": [ws.path().join("x.txt")]})).await else { return };
    assert!(report.contains("ERR"), "{report}");
    assert!(!ws.path().join("x.txt").exists());
}

#[tokio::test]
async fn a_server_that_is_not_isolated_still_inherits_the_environment() {
    // The "off" mode keeps working as it always did (existing servers).
    std::env::set_var("AIW_TEST_MCP_OFF_VAR", "inherited");
    let Some(report) = probe(&config(json!({})), None, json!({"env": ["AIW_TEST_MCP_OFF_VAR"]})).await else { return };
    assert!(report.contains("inherited"), "{report}");
}
