//! A fake CDP endpoint for the unit tests: a WebSocket server that answers the commands of the
//! browser tools the way a Chromium would (targets, sessions, evaluation) and lets a test push
//! events (popups, dialogs, downloads, console) and look at every command it received.

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone, Debug)]
pub(super) struct Req {
    pub session: Option<String>,
    pub method: String,
    pub params: Value,
}

pub(super) struct Reply {
    pub result: Value,
    /// Events sent after the answer.
    pub events: Vec<Value>,
}

impl Reply {
    pub fn ok(result: Value) -> Self {
        Reply { result, events: vec![] }
    }
    pub fn with(result: Value, events: Vec<Value>) -> Self {
        Reply { result, events }
    }
}

/// Targets the fake browser has.
pub(super) struct Target {
    pub id: String,
    pub session: String,
    pub url: String,
    pub title: String,
}

#[derive(Default)]
pub(super) struct FakeState {
    pub targets: Vec<Target>,
    pub next: u32,
}

type Handler = dyn Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync;

pub(super) struct FakeCdp {
    pub url: String,
    pub log: Arc<Mutex<Vec<Req>>>,
    push: mpsc::UnboundedSender<String>,
}

pub(super) fn attached(session_of_parent: Option<&str>, id: &str, session: &str, url: &str, opener: Option<&str>) -> Value {
    let mut ev = json!({"method": "Target.attachedToTarget", "params": {
        "sessionId": session, "waitingForDebugger": true,
        "targetInfo": {"targetId": id, "type": "page", "title": "", "url": url, "openerId": opener.unwrap_or_default()}}});
    if let Some(p) = session_of_parent {
        ev["sessionId"] = json!(p);
    }
    ev
}

pub(super) fn event(session: Option<&str>, method: &str, params: Value) -> Value {
    let mut ev = json!({"method": method, "params": params});
    if let Some(s) = session {
        ev["sessionId"] = json!(s);
    }
    ev
}

impl FakeCdp {
    /// Starts the server. `custom` answers first; what it leaves (`None`) gets the default answers.
    pub async fn start(custom: impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static) -> FakeCdp {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://127.0.0.1:{}/devtools/browser/fake", listener.local_addr().unwrap().port());
        let log: Arc<Mutex<Vec<Req>>> = Default::default();
        let (push, mut push_rx) = mpsc::unbounded_channel::<String>();
        let state = Arc::new(Mutex::new(FakeState {
            targets: vec![Target { id: "T1".into(), session: "S1".into(), url: "about:blank".into(), title: "".into() }],
            next: 2,
        }));
        let handler: Arc<Handler> = Arc::new(custom);
        let log2 = log.clone();
        tokio::spawn(async move {
            // One connection per browser launch: a relaunch is a new connection.
            while let Ok((tcp, _)) = listener.accept().await {
                let Ok(ws) = tokio_tungstenite::accept_async(tcp).await else { continue };
                let (mut sink, mut stream) = ws.split();
                let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
                let writer = tokio::spawn(async move {
                    while let Some(m) = out_rx.recv().await {
                        if sink.send(Message::Text(m.into())).await.is_err() {
                            break;
                        }
                    }
                });
                // Events pushed by the test go to the current connection.
                loop {
                    tokio::select! {
                        Some(ev) = push_rx.recv() => { let _ = out_tx.send(ev); }
                        msg = stream.next() => {
                            let Some(Ok(msg)) = msg else { break };
                            let Ok(text) = msg.into_text() else { continue };
                            let Ok(v) = serde_json::from_str::<Value>(text.as_ref()) else { continue };
                            let req = Req {
                                session: v["sessionId"].as_str().map(String::from),
                                method: v["method"].as_str().unwrap_or_default().to_string(),
                                params: v["params"].clone(),
                            };
                            log2.lock().unwrap().push(req.clone());
                            let reply = {
                                let mut st = state.lock().unwrap();
                                handler(&req, &mut st).unwrap_or_else(|| default_reply(&req, &mut st))
                            };
                            let mut answer = json!({"id": v["id"], "result": reply.result});
                            if let Some(s) = &req.session {
                                answer["sessionId"] = json!(s);
                            }
                            let _ = out_tx.send(answer.to_string());
                            for e in reply.events {
                                let _ = out_tx.send(e.to_string());
                            }
                        }
                    }
                }
                writer.abort();
            }
        });
        FakeCdp { url, log, push }
    }

    /// A handle to push events from another task.
    pub fn sender(&self) -> mpsc::UnboundedSender<String> {
        self.push.clone()
    }

    /// An event out of the blue (a popup, a dialog, a download, a console message...).
    pub fn push(&self, ev: Value) {
        let _ = self.push.send(ev.to_string());
    }

    pub fn commands(&self, method: &str) -> Vec<Req> {
        self.log.lock().unwrap().iter().filter(|r| r.method == method).cloned().collect()
    }

    /// Position of the first command satisfying `pred`, in the order received.
    pub fn position(&self, pred: impl Fn(&Req) -> bool) -> Option<usize> {
        self.log.lock().unwrap().iter().position(pred)
    }

    /// Waits until a command satisfying `pred` has been received.
    pub async fn wait_for(&self, pred: impl Fn(&Req) -> bool) -> Req {
        for _ in 0..200 {
            if let Some(r) = self.log.lock().unwrap().iter().find(|r| pred(r)).cloned() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("the command never arrived; received: {:?}", self.log.lock().unwrap().iter().map(|r| r.method.clone()).collect::<Vec<_>>());
    }
}

fn default_reply(req: &Req, st: &mut FakeState) -> Reply {
    match req.method.as_str() {
        "Target.getTargets" => Reply::ok(json!({"targetInfos": st.targets.iter().map(|t| json!({
            "targetId": t.id, "type": "page", "title": t.title, "url": t.url, "attached": false})).collect::<Vec<_>>()})),
        "Target.attachToTarget" => {
            let id = req.params["targetId"].as_str().unwrap_or_default();
            match st.targets.iter().find(|t| t.id == id) {
                Some(t) => Reply::with(json!({"sessionId": t.session}), vec![attached(None, &t.id, &t.session, &t.url, None)]),
                None => Reply::ok(json!({})),
            }
        }
        "Target.createTarget" => {
            let n = st.next;
            st.next += 1;
            let (id, session) = (format!("T{n}"), format!("S{n}"));
            let url = req.params["url"].as_str().unwrap_or("about:blank").to_string();
            st.targets.push(Target { id: id.clone(), session: session.clone(), url: url.clone(), title: "".into() });
            Reply::with(json!({"targetId": id}), vec![attached(None, &id, &session, &url, None)])
        }
        "Target.closeTarget" => {
            let id = req.params["targetId"].as_str().unwrap_or_default().to_string();
            let gone = st.targets.iter().position(|t| t.id == id).map(|i| st.targets.remove(i));
            let events = gone
                .map(|t| {
                    vec![
                        event(None, "Target.detachedFromTarget", json!({"sessionId": t.session, "targetId": t.id})),
                        event(None, "Target.targetDestroyed", json!({"targetId": t.id})),
                    ]
                })
                .unwrap_or_default();
            Reply::with(json!({"success": true}), events)
        }
        "Page.navigate" => {
            let s = req.session.clone();
            let url = req.params["url"].as_str().unwrap_or_default().to_string();
            if let Some(t) = st.targets.iter_mut().find(|t| Some(&t.session) == s.as_ref()) {
                t.url = url.clone();
            }
            Reply::with(
                json!({"frameId": "F"}),
                vec![
                    event(s.as_deref(), "Page.frameNavigated", json!({"frame": {"id": "F", "url": url}})),
                    event(s.as_deref(), "Page.loadEventFired", json!({"timestamp": 1})),
                ],
            )
        }
        "Runtime.evaluate" => Reply::ok(json!({"result": {"type": "string", "value": ""}})),
        _ => Reply::ok(json!({})),
    }
}
