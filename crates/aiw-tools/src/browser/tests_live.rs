//! Tests of who drives the browser (agent, pause, person) against the fake CDP endpoint: mutual
//! exclusion, resuming, references that go stale and addresses that are hidden after a human turn.

use super::fake::{FakeState, Reply, Req};
use super::*;
use std::time::Duration;

struct Live {
    fake: fake::FakeCdp,
    mgr: Arc<BrowserManager>,
    t: Vec<Arc<dyn Tool>>,
    ctx: ToolContext,
    _dir: tempfile::TempDir,
}

impl Live {
    async fn new(custom: impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static) -> Live {
        let fake = fake::FakeCdp::start(custom).await;
        let dir = tempfile::tempdir().unwrap();
        let resolve: ResolveFn = Arc::new(|_, _| Box::pin(async { Err(std::io::Error::other("no dns")) }));
        let mgr = Arc::new(BrowserManager::for_endpoint(dir.path().join("profile"), fake.url.clone(), resolve));
        mgr.control().set_limits(Limits { op_wait: Duration::from_millis(600) });
        let t = tools(mgr.clone());
        let ctx = ToolContext::new("run1", dir.path().to_path_buf());
        Live { fake, mgr, t, ctx, _dir: dir }
    }

    fn tool(&self, name: &str) -> Arc<dyn Tool> {
        self.t.iter().find(|x| x.name() == name).unwrap_or_else(|| panic!("tool {name} not registered")).clone()
    }

    async fn run(&self, name: &str, args: Value) -> ToolOutput {
        self.tool(name).call(args, &self.ctx).await.unwrap_or_else(|e| ToolOutput::error(format!("ERR: {e}")))
    }

    /// The call as a task, to look at it while it waits.
    fn spawn(&self, name: &str, args: Value) -> tokio::task::JoinHandle<ToolOutput> {
        let (tool, ctx) = (self.tool(name), self.ctx.clone());
        tokio::spawn(async move { tool.call(args, &ctx).await.unwrap_or_else(|e| ToolOutput::error(format!("ERR: {e}"))) })
    }

    fn evaluations(&self) -> usize {
        self.fake.commands("Runtime.evaluate").len()
    }
}

/// A page whose snapshot reports `url` and has one element.
fn page(url: &'static str) -> impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static {
    move |req, _| match req.method.as_str() {
        "Runtime.evaluate" => {
            let expr = req.params["expression"].as_str().unwrap_or_default();
            if expr.starts_with("document.querySelector(") {
                // The element of a ref.
                Some(Reply::ok(json!({"result": {"type": "object", "objectId": "obj1"}})))
            } else if expr.contains("Elementi interattivi") {
                Some(Reply::ok(
                    json!({"result": {"type": "string", "value": format!("Titolo: P\nURL: {url}\nPosizione: tutta la pagina è in vista\n\nElementi interattivi (usa il numero come ref):\n[1] button \"Invia\"")}}),
                ))
            } else if expr == "location.href" {
                Some(Reply::ok(json!({"result": {"type": "string", "value": url}})))
            } else {
                None
            }
        }
        "Runtime.callFunctionOn" => Some(Reply::ok(json!({"result": {"type": "string", "value": "ok"}}))),
        _ => None,
    }
}

#[tokio::test]
async fn an_agent_call_waits_while_the_person_drives_and_goes_on_when_the_browser_is_given_back() {
    let lv = Live::new(page("http://e.example/")).await;
    assert!(!lv.run("browser_snapshot", json!({})).await.is_error);
    let control = lv.mgr.control();
    control.take();
    let before = lv.evaluations();
    let waiting = lv.spawn("browser_snapshot", json!({}));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!waiting.is_finished(), "the agent does not act while the person drives");
    assert_eq!(lv.evaluations(), before, "not a single command reached the page");
    control.give_back();
    let out = waiting.await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    assert!(lv.evaluations() > before, "the task goes on where it was");
}

#[tokio::test]
async fn pausing_stops_the_agent_and_resuming_lets_it_go_on() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    let control = lv.mgr.control();
    assert_eq!(control.pause(), Mode::Paused);
    let before = lv.evaluations();
    let waiting = lv.spawn("browser_scroll", json!({}));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!waiting.is_finished());
    assert_eq!(lv.evaluations(), before);
    assert_eq!(control.resume(), Mode::Agent);
    assert!(!waiting.await.unwrap().is_error);
    // A pause is not a human turn: the references are still good.
    assert!(!control.is_stale());
}

#[tokio::test]
async fn a_call_that_waits_too_long_says_so_and_does_not_touch_the_page() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.mgr.control().take();
    let before = lv.evaluations();
    let out = lv.run("browser_click", json!({"ref": "1"})).await;
    assert!(out.is_error && out.content.contains("L'utente sta usando il browser"), "{}", out.content);
    assert_eq!(lv.evaluations(), before);
    // Paused, the same: the wait ends with a message that tells the model not to insist.
    lv.mgr.control().give_back();
    lv.mgr.control().pause();
    let out = lv.run("browser_snapshot", json!({})).await;
    assert!(out.is_error && out.content.contains("messo in pausa"), "{}", out.content);
    assert_eq!(lv.evaluations(), before);
}

#[tokio::test]
async fn a_stopped_run_ends_the_wait_and_the_kill_switch_gives_the_browser_back() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.mgr.control().set_limits(Limits { op_wait: Duration::from_secs(30) });
    lv.mgr.control().take();
    let waiting = lv.spawn("browser_snapshot", json!({}));
    tokio::time::sleep(Duration::from_millis(100)).await;
    lv.ctx.cancel.cancel();
    let out = waiting.await.unwrap();
    assert!(out.is_error && out.content.contains("Annullato"), "{}", out.content);
    // The kill switch closes the browser and the person's turn with it.
    assert!(lv.mgr.close().await);
    assert_eq!(lv.mgr.control().state().mode, Mode::Agent);
}

#[tokio::test]
async fn references_go_stale_after_a_human_turn_until_the_agent_looks_again() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    assert!(!lv.run("browser_click", json!({"ref": "1"})).await.is_error);
    let control = lv.mgr.control();
    control.take();
    control.give_back();
    let stale = lv.run("browser_click", json!({"ref": "1"})).await;
    assert!(stale.is_error && stale.content.contains("Rifai browser_snapshot"), "{}", stale.content);
    assert!(lv.run("browser_type", json!({"ref": "1", "text": "x"})).await.is_error);
    assert!(lv.run("browser_scroll", json!({"ref": "1"})).await.is_error, "a scroll to an element names one too");
    // What names no element is fine, and a fresh snapshot gives references that can be trusted.
    assert!(!lv.run("browser_tabs", json!({})).await.is_error);
    assert!(lv.mgr.control().is_stale(), "tabs does not renew the references");
    assert!(!lv.run("browser_snapshot", json!({"format": "text"})).await.is_error);
    assert!(lv.mgr.control().is_stale(), "the text format has no references");
    assert!(!lv.run("browser_snapshot", json!({})).await.is_error);
    assert!(!lv.mgr.control().is_stale());
    assert!(!lv.run("browser_click", json!({"ref": "1"})).await.is_error);
}

#[tokio::test]
async fn addresses_lose_query_and_fragment_after_a_handback_until_the_agent_navigates() {
    // A login that lands on an address with a code in it.
    let lv = Live::new(page("https://e.example/cb?code=CODICE-DI-PROVA#access_token=TOKEN-DI-PROVA")).await;
    let first = lv.run("browser_snapshot", json!({})).await;
    assert!(first.content.contains("code=CODICE-DI-PROVA"), "before a human turn the agent sees what it opened: {}", first.content);
    let control = lv.mgr.control();
    control.take();
    control.give_back();
    for format in ["elements", "text"] {
        let out = lv.run("browser_snapshot", json!({"format": format})).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("CODICE-DI-PROVA") && !out.content.contains("TOKEN-DI-PROVA"), "{format}: {}", out.content);
    }
    let tabs = lv.run("browser_tabs", json!({})).await;
    assert!(!tabs.content.contains("CODICE-DI-PROVA"), "{}", tabs.content);
    // The agent goes to an address of its own: it sees addresses as they are again.
    let nav = lv.run("browser_navigate", json!({"url": "http://8.8.8.8/x"})).await;
    assert!(!nav.is_error, "{}", nav.content);
    assert!(!lv.mgr.control().redacting());
}
