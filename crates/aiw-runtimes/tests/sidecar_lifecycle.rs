//! End-to-end checks of the sidecar lifecycle against a fake single-model OpenAI server
//! (a small Python script started as a real child process, with a worker process of its own).
#![cfg(unix)]

use aiw_runtimes::registry::RuntimeRegistry;
use aiw_runtimes::types::{ChatDelta, ChatRequest};
use futures::StreamExt;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const FAKE_SERVER: &str = r#"#!/usr/bin/env python3
import json, subprocess, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

a = sys.argv[1:]
opt = lambda k: a[a.index(k) + 1]
port, model, log, child = int(opt("--port")), opt("--model"), opt("--log"), opt("--child")
worker = subprocess.Popen(["sleep", "777"])
with open(child, "a") as f:
    f.write(f"{worker.pid}\n")
time.sleep(0.4)  # a slow start widens the window of a concurrent second start
with open(log, "a") as f:
    f.write("start\n")

class H(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def send(self, code, body, ctype="application/json"):
        data = body.encode()
        self.send_response(code); self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(data))); self.end_headers(); self.wfile.write(data)
    def do_GET(self):
        if self.path == "/health": self.send(200, "ok")
        elif self.path == "/v1/models": self.send(200, json.dumps({"data": [{"id": model}]}))
        else: self.send(404, "{}")
    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers["content-length"])))
        if req["model"] != model:
            return self.send(404, json.dumps({"error": f"The model {req['model']} does not exist"}))
        chunk = {"choices": [{"delta": {"content": "served:" + req["model"]}, "finish_reason": "stop"}]}
        self.send(200, "data: " + json.dumps(chunk) + "\n\ndata: [DONE]\n\n", "text/event-stream")

ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
"#;

struct Fixture {
    reg: Arc<RuntimeRegistry>,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn log(&self) -> PathBuf {
        self.dir.path().join("start.log")
    }
    fn starts(&self) -> usize {
        std::fs::read_to_string(self.log()).map(|s| s.lines().count()).unwrap_or(0)
    }
    fn workers(&self) -> Vec<u32> {
        std::fs::read_to_string(self.dir.path().join("child.pids"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }
}

/// A registry with one user-defined single-model runtime, `fakesrv`, that serves its model
/// under the name it was started with (the local path when the model is resolved to one).
fn fixture(local_model: Option<(&str, &Path)>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("fakesrv");
    std::fs::write(&script, FAKE_SERVER).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let manifests = dir.path().join("manifests");
    std::fs::create_dir_all(&manifests).unwrap();
    let manifest = json!({
        "id": "fakesrv", "name": "Fake server", "kind": "sidecar", "platforms": ["linux", "macos", "windows"],
        "detect": {"binaries": ["fakesrv"]},
        "launch": {
            "command": "fakesrv",
            "args": ["--port", "{port}", "--model", "{model}",
                     "--log", dir.path().join("start.log"), "--child", dir.path().join("child.pids")],
            "settings_args": {"ctx_size": ["--ctx", "{value}"]},
            "ready_timeout_s": 20, "requires_model": true
        },
        "settings": {"type": "object", "properties": {
            "ctx_size": {"type": "integer", "title": "ctx", "default": 4096, "minimum": 512, "maximum": 1048576}
        }}
    });
    std::fs::write(manifests.join("fakesrv.json"), manifest.to_string()).unwrap();
    let reg = RuntimeRegistry::new("linux", Some(&manifests), vec![bin], dir.path().to_path_buf());
    if let Some((id, path)) = local_model {
        let (id, path) = (id.to_string(), path.to_path_buf());
        reg.set_model_resolver(Arc::new(move |m| (m == id).then(|| path.clone())));
    }
    Fixture { reg: Arc::new(reg), dir }
}

async fn ask(backend: &Arc<dyn aiw_runtimes::types::ChatBackend>, model: &str) -> Result<String, String> {
    let req = ChatRequest { model: model.to_string(), ..Default::default() };
    let mut stream = backend.chat(req, CancellationToken::new()).await.map_err(|e| e.to_string())?;
    let mut text = String::new();
    while let Some(d) = stream.next().await {
        if let ChatDelta::Text(t) = d.map_err(|e| e.to_string())? {
            text.push_str(&t);
        }
    }
    Ok(text)
}

fn alive(pid: u32) -> bool {
    // A reaped or zombie process counts as dead.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s.rsplit(')').next().is_some_and(|rest| !rest.trim_start().starts_with('Z')),
        Err(_) => false,
    }
}

#[tokio::test]
async fn chat_uses_the_name_the_server_serves_for_a_local_path() {
    let models = tempfile::tempdir().unwrap();
    let path = models.path().join("qwen3-4b-mlx");
    std::fs::create_dir_all(&path).unwrap();
    let fx = fixture(Some(("qwen3-4b", &path)));
    // The server is started with the path and serves the model under it; the profile id is
    // `qwen3-4b`, and the chat must still work.
    let backend = fx.reg.backend("fakesrv", Some("qwen3-4b")).await.unwrap();
    let reply = ask(&backend, "qwen3-4b").await.unwrap();
    assert_eq!(reply, format!("served:{}", path.display()));
    // A backend handed out while the sidecar is already running behaves the same.
    let again = fx.reg.backend("fakesrv", Some("qwen3-4b")).await.unwrap();
    assert_eq!(ask(&again, "qwen3-4b").await.unwrap(), format!("served:{}", path.display()));
    fx.reg.stop_all().await;
}

#[tokio::test]
async fn concurrent_starts_spawn_one_process_and_both_backends_work() {
    let fx = fixture(None);
    let (a, b) = (fx.reg.clone(), fx.reg.clone());
    let (ra, rb) = tokio::join!(
        tokio::spawn(async move { a.backend_with("fakesrv", Some("m"), Some(4096)).await }),
        tokio::spawn(async move { b.backend_with("fakesrv", Some("m"), Some(4096)).await }),
    );
    let (ba, bb) = (ra.unwrap().unwrap(), rb.unwrap().unwrap());
    assert_eq!(fx.starts(), 1, "one process for two concurrent starts");
    assert_eq!(ask(&ba, "m").await.unwrap(), "served:m");
    assert_eq!(ask(&bb, "m").await.unwrap(), "served:m");
    // Explicit starts reuse the healthy sidecar too.
    let (c, d) = (fx.reg.clone(), fx.reg.clone());
    let (rc, rd) = tokio::join!(
        tokio::spawn(async move { c.start_with("fakesrv", Some("m".into()), Some(4096)).await }),
        tokio::spawn(async move { d.start_with("fakesrv", Some("m".into()), Some(2048)).await }),
    );
    rc.unwrap().unwrap();
    rd.unwrap().unwrap();
    assert_eq!(fx.starts(), 1);
    fx.reg.stop_all().await;
}

#[tokio::test]
async fn stop_kills_the_workers_of_the_sidecar() {
    let fx = fixture(None);
    fx.reg.backend_with("fakesrv", Some("m"), Some(4096)).await.unwrap();
    let workers = fx.workers();
    assert_eq!(workers.len(), 1);
    assert!(alive(workers[0]), "worker is running");
    fx.reg.stop("fakesrv").await.unwrap();
    for _ in 0..50 {
        if !alive(workers[0]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(!alive(workers[0]), "the worker of the sidecar outlived its stop");
}

#[tokio::test]
async fn larger_context_does_not_restart_while_requests_are_in_flight() {
    let fx = fixture(None);
    let backend = fx.reg.backend_with("fakesrv", Some("m"), Some(4096)).await.unwrap();
    // An unfinished stream counts as a request in flight.
    let stream = backend.chat(ChatRequest { model: "m".into(), ..Default::default() }, CancellationToken::new()).await.unwrap();
    let busy = fx.reg.backend_with("fakesrv", Some("m"), Some(16384)).await.unwrap();
    assert_eq!(fx.starts(), 1, "no restart with a request in flight");
    assert_eq!(ask(&busy, "m").await.unwrap(), "served:m");
    drop(stream);
    // Idle again: the larger context restarts the sidecar.
    fx.reg.backend_with("fakesrv", Some("m"), Some(16384)).await.unwrap();
    assert_eq!(fx.starts(), 2);
    fx.reg.stop_all().await;
}
