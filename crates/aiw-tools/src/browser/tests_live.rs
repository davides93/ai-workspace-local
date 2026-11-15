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
async fn addresses_lose_query_and_fragment_after_a_handback_tab_by_tab_until_the_agent_navigates_that_tab() {
    // A login that lands on an address with a code in it, and a popup of the login on another tab.
    let base = page("https://e.example/cb?code=CODICE-DI-PROVA#access_token=TOKEN-DI-PROVA");
    let lv = Live::new(move |req, st| {
        if req.method == "Target.getTargets" {
            // What the browser says the two tabs are on (until the agent navigates one).
            for t in st.targets.iter_mut().filter(|t| t.url == "about:blank") {
                t.url = if t.id == "T1" {
                    "https://e.example/cb?code=CODICE-DI-PROVA#access_token=TOKEN-DI-PROVA"
                } else {
                    "https://login.example/popup?code=CODICE-DEL-POPUP"
                }
                .into();
            }
            return None;
        }
        base(req, st)
    })
    .await;
    let first = lv.run("browser_snapshot", json!({})).await;
    assert!(first.content.contains("code=CODICE-DI-PROVA"), "before a human turn the agent sees what it opened: {}", first.content);
    lv.run("browser_tab_new", json!({})).await;
    let tabs = lv.run("browser_tabs", json!({})).await;
    assert!(tabs.content.contains("code=CODICE-DEL-POPUP"), "before a human turn tabs show what they are on: {}", tabs.content);
    let control = lv.mgr.control();
    control.take();
    control.give_back();
    for format in ["elements", "text"] {
        let out = lv.run("browser_snapshot", json!({"format": format})).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("CODICE-DI-PROVA") && !out.content.contains("TOKEN-DI-PROVA"), "{format}: {}", out.content);
    }
    let tabs = lv.run("browser_tabs", json!({})).await;
    assert!(tabs.content.contains("https://e.example/cb") && tabs.content.contains("https://login.example/popup"), "{}", tabs.content);
    assert!(!tabs.content.contains("CODICE") && !tabs.content.contains("TOKEN"), "{}", tabs.content);
    // The agent takes the tab in front (the popup) to an address of its own: that tab's address is its own again, but the other
    // tab, which it did not navigate, stays hidden: one navigation does not give back the addresses of all.
    let nav = lv.run("browser_navigate", json!({"url": "http://8.8.8.8/x?y=ok"})).await;
    assert!(!nav.is_error, "{}", nav.content);
    let tabs = lv.run("browser_tabs", json!({})).await;
    assert!(tabs.content.contains("http://8.8.8.8/x?y=ok"), "{}", tabs.content);
    assert!(!tabs.content.contains("CODICE") && !tabs.content.contains("TOKEN"), "{}", tabs.content);
    // A navigation that fails does not give the address back either.
    control.take();
    control.give_back();
    let refused = lv.run("browser_navigate", json!({"url": "http://127.0.0.1:9/x?q=1"})).await;
    assert!(refused.is_error, "{}", refused.content);
    assert!(!lv.run("browser_tabs", json!({})).await.content.contains("CODICE"));
}

#[tokio::test]
async fn an_action_that_waited_for_the_browser_does_not_run_after_the_person_took_it() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.mgr.control().set_limits(Limits { op_wait: Duration::from_secs(10) });
    // `browser_wait` holds the browser for a second and a half; the click passes the gate meanwhile and waits for the lock.
    let long = lv.spawn("browser_wait", json!({"ms": 1500}));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let click = lv.spawn("browser_click", json!({"ref": "1"}));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let before = lv.fake.commands("Runtime.callFunctionOn").len();
    lv.mgr.take_control().await.unwrap();
    long.await.unwrap();
    let out = click.await.unwrap();
    assert!(out.is_error && out.content.contains("preso il browser") && out.content.contains("non è stata eseguita"), "{}", out.content);
    assert_eq!(lv.fake.commands("Runtime.callFunctionOn").len(), before, "the click never reached the page");
}

#[tokio::test]
async fn a_key_for_the_page_needs_a_fresh_snapshot_after_the_person_drove_and_a_window_notice_is_not_one() {
    let lv = Live::new(page_with_confirm_on_click()).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.mgr.take_control().await.unwrap();
    lv.mgr.give_back();
    // `browser_press` with no element goes to whatever has the focus, which is the person's now.
    let press = lv.run("browser_press", json!({"key": "Enter"})).await;
    assert!(press.is_error && press.content.contains("Rifai browser_snapshot"), "{}", press.content);
    // A result that only says a window of the site blocks the page is not a snapshot: the numbers are still not trusted.
    lv.fake.push(confirm_event("Sicuro?"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let blocked = lv.run("browser_wait", json!({"ms": 100})).await;
    assert!(blocked.content.contains("browser_dialog"), "{}", blocked.content);
    assert!(lv.mgr.control().is_stale());
}

// ---------------------------------------------------------------------- interactive dialogs

fn confirm_event(message: &str) -> Value {
    super::fake::event(
        Some("S1"),
        "Page.javascriptDialogOpening",
        json!({"type": "confirm", "message": message, "url": "http://e.example/"}),
    )
}

/// A page whose click blocks on a `confirm` (no answer until it is handled) and whose other commands work.
fn page_with_confirm_on_click() -> impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static {
    let base = page("http://e.example/");
    move |req, st| match req.method.as_str() {
        "Runtime.callFunctionOn" => Some(Reply::silent(vec![confirm_event("Cancellare tutto?")])),
        _ => base(req, st),
    }
}

#[tokio::test]
async fn a_click_that_opens_a_confirm_comes_back_at_once_and_the_agent_answers_it() {
    let lv = Live::new(page_with_confirm_on_click()).await;
    lv.run("browser_snapshot", json!({})).await;
    let started = std::time::Instant::now();
    let click = lv.run("browser_click", json!({"ref": "1"})).await;
    assert!(started.elapsed() < Duration::from_secs(5), "the click did not wait for the 30 s of the browser: {:?}", started.elapsed());
    assert!(!click.is_error, "{}", click.content);
    assert!(
        click.content.contains("una conferma (confirm): «Cancellare tutto?»") && click.content.contains("browser_dialog"),
        "{}",
        click.content
    );
    let dialog = lv.mgr.live_state().dialog.expect("the state carries the window");
    assert_eq!((dialog.kind.as_str(), dialog.message.as_str()), ("confirm", "Cancellare tutto?"));
    assert!(lv.fake.commands("Page.handleJavaScriptDialog").is_empty(), "nobody answered for the agent");
    // The page is blocked: whatever else the agent asks about it says so at once instead of hanging.
    let snap = lv.run("browser_snapshot", json!({})).await;
    assert!(!snap.is_error && snap.content.contains("browser_dialog"), "{}", snap.content);
    // The agent answers.
    let done = lv.run("browser_dialog", json!({"action": "accept"})).await;
    assert!(!done.is_error && done.content.contains("confermata con OK"), "{}", done.content);
    let sent = lv.fake.commands("Page.handleJavaScriptDialog");
    assert_eq!((sent.len(), sent[0].params["accept"].as_bool(), sent[0].session.as_deref()), (1, Some(true), Some("S1")));
    assert!(lv.mgr.live_state().dialog.is_none());
    let none = lv.run("browser_dialog", json!({"action": "dismiss"})).await;
    assert!(none.is_error && none.content.contains("Nessuna finestra"), "{}", none.content);
    assert!(lv.run("browser_dialog", json!({"action": "boh"})).await.is_error);
}

#[tokio::test]
async fn a_prompt_gets_the_text_of_the_answer_only_when_it_is_accepted() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.fake.push(super::fake::event(
        Some("S1"),
        "Page.javascriptDialogOpening",
        json!({"type": "prompt", "message": "Nome?", "defaultPrompt": "Anna"}),
    ));
    lv.fake.wait_for(|r| r.method == "Target.setDiscoverTargets").await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let out = lv.run("browser_dialog", json!({"action": "accept", "text": "Mario"})).await;
    assert!(!out.is_error, "{}", out.content);
    let sent = lv.fake.commands("Page.handleJavaScriptDialog").pop().unwrap();
    assert_eq!((sent.params["accept"].as_bool(), sent.params["promptText"].as_str()), (Some(true), Some("Mario")));
    lv.fake.push(super::fake::event(Some("S1"), "Page.javascriptDialogOpening", json!({"type": "prompt", "message": "Nome?"})));
    tokio::time::sleep(Duration::from_millis(150)).await;
    lv.run("browser_dialog", json!({"action": "dismiss", "text": "Mario"})).await;
    let sent = lv.fake.commands("Page.handleJavaScriptDialog").pop().unwrap();
    assert_eq!((sent.params["accept"].as_bool(), sent.params.get("promptText")), (Some(false), None));
}

#[tokio::test]
async fn a_dialog_nobody_answers_is_dismissed_after_the_hold_and_the_agent_is_told() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.mgr.set_dialog_hold(Duration::from_millis(300));
    lv.run("browser_snapshot", json!({})).await;
    lv.fake.push(confirm_event("Sicuro?"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(lv.mgr.live_state().dialog.is_some());
    let sent = lv.fake.wait_for(|r| r.method == "Page.handleJavaScriptDialog").await;
    assert_eq!(sent.params["accept"], false, "the safe answer");
    assert!(lv.mgr.live_state().dialog.is_none());
    let out = lv.run("browser_tabs", json!({})).await;
    assert!(out.content.contains("«Sicuro?»: annullata automaticamente, nessuno ha risposto"), "{}", out.content);
}

#[tokio::test]
async fn the_person_answers_a_dialog_only_while_they_drive() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.fake.push(confirm_event("Sicuro?"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(lv.mgr.answer_dialog(true, None).await.is_err(), "not their turn");
    lv.mgr.take_control().await.unwrap();
    lv.mgr.answer_dialog(true, None).await.unwrap();
    assert_eq!(lv.fake.commands("Page.handleJavaScriptDialog")[0].params["accept"], true);
    assert!(lv.mgr.live_state().dialog.is_none());
    // Nothing waits any more: a conflict (409), never a «not found» (404), which a client reads as «this backend has no such route».
    assert!(matches!(lv.mgr.answer_dialog(true, None).await, Err(aiw_core::Error::Conflict(_))));
}

// ---------------------------------------------------------------------- the live view

/// A page that tells its viewport and gives a «frame».
fn viewport_page() -> impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static {
    let base = page("http://e.example/");
    move |req, st| match req.method.as_str() {
        "Page.getLayoutMetrics" => {
            Some(Reply::ok(json!({"cssVisualViewport": {"clientWidth": 1000.0, "clientHeight": 500.0, "pageX": 0.0, "pageY": 40.0}})))
        }
        "Page.captureScreenshot" => Some(Reply::ok(json!({"data": "SlBFRy1EQVRJ"}))), // "JPEG-DATI"
        _ => base(req, st),
    }
}

#[tokio::test]
async fn the_live_view_never_starts_a_browser() {
    let lv = Live::new(page("http://e.example/")).await;
    assert!(lv.mgr.frame().await.unwrap().is_none());
    let state = lv.mgr.live_state();
    assert!(!state.running && state.page.is_none());
    assert!(lv.mgr.take_control().await.is_err(), "nothing to take");
    assert!(lv.mgr.human_input(HumanInput::Text { text: "x".into() }).await.is_err());
    assert!(lv.fake.log.lock().unwrap().is_empty(), "no connection, no command: the browser was never launched");
}

#[tokio::test]
async fn a_frame_is_scaled_to_the_viewport_and_comes_while_an_agent_action_holds_the_browser() {
    let lv = Live::new(viewport_page()).await;
    lv.run("browser_snapshot", json!({})).await;
    // `browser_wait` holds the session for as long as it waits: the live view must not wait for it.
    let long = lv.spawn("browser_wait", json!({"ms": 1500}));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = std::time::Instant::now();
    let frame = lv.mgr.frame().await.unwrap().expect("a frame");
    assert!(started.elapsed() < Duration::from_millis(1000), "the frame did not wait for the agent: {:?}", started.elapsed());
    assert_eq!((frame.jpeg.as_slice(), frame.width, frame.height), (&b"JPEG-DATI"[..], 1000, 500));
    let shot = lv.fake.commands("Page.captureScreenshot").pop().unwrap();
    assert_eq!(shot.params["format"], "jpeg");
    assert_eq!(
        (shot.params["clip"]["width"].as_f64(), shot.params["clip"]["y"].as_f64(), shot.params["clip"]["scale"].as_f64()),
        (Some(1000.0), Some(40.0), Some(1.0))
    );
    long.await.unwrap();
}

#[tokio::test]
async fn the_state_names_the_host_and_nothing_else_of_the_address() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.fake.push(super::fake::event(
        None,
        "Target.targetInfoChanged",
        json!({"targetInfo": {"targetId": "T1", "title": "Accesso", "url": "https://login.example/cb?code=CODICE-DI-PROVA#t=TOKEN"}}),
    ));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let state = lv.mgr.live_state();
    let page = state.page.clone().expect("the page in front");
    assert_eq!((page.host.as_str(), page.title.as_str(), page.secure), ("login.example", "Accesso", true));
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("CODICE-DI-PROVA") && !json.contains("TOKEN") && !json.contains("/cb"), "{json}");
    assert!(json.contains("\"mode\":\"agent\"") && json.contains("\"running\":true"), "{json}");
}

#[tokio::test]
async fn the_input_of_the_person_reaches_the_page_in_css_pixels_and_only_on_their_turn() {
    let lv = Live::new(viewport_page()).await;
    lv.run("browser_snapshot", json!({})).await;
    let click = || HumanInput::Click { x: 0.25, y: 0.5, button: None, count: None };
    assert!(lv.mgr.human_input(click()).await.is_err(), "the agent has the browser");
    assert!(lv.fake.commands("Input.dispatchMouseEvent").is_empty());
    // The agent is mid-action when the person takes the browser: they wait for it.
    let guard = lv.mgr.control().enter(&lv.ctx.cancel).await.unwrap();
    lv.mgr.take_control().await.unwrap();
    assert!(lv.mgr.human_input(click()).await.is_err());
    drop(guard);
    lv.mgr.human_input(click()).await.unwrap();
    let mouse = lv.fake.commands("Input.dispatchMouseEvent");
    let kinds: Vec<&str> = mouse.iter().map(|r| r.params["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["mouseMoved", "mousePressed", "mouseReleased"]);
    assert_eq!((mouse[1].params["x"].as_f64(), mouse[1].params["y"].as_f64()), (Some(250.0), Some(250.0)));
    lv.mgr.human_input(HumanInput::Text { text: "123456".into() }).await.unwrap();
    assert_eq!(lv.fake.commands("Input.insertText")[0].params["text"], "123456");
    lv.mgr.human_input(HumanInput::Key { key: "Enter".into(), shift: false }).await.unwrap();
    lv.mgr.human_input(HumanInput::Key { key: "a".into(), shift: true }).await.unwrap();
    let keys = lv.fake.commands("Input.dispatchKeyEvent");
    assert_eq!(keys.len(), 4);
    assert_eq!((keys[0].params["key"].as_str(), keys[0].params["text"].as_str()), (Some("Enter"), Some("\r")));
    assert_eq!((keys[2].params["text"].as_str(), keys[2].params["modifiers"].as_u64()), (Some("a"), Some(8)));
    // Nothing the browser would take as a shortcut or as a different request.
    for bad in [
        HumanInput::Key { key: "F5".into(), shift: false },
        HumanInput::Key { key: "Control".into(), shift: false },
        HumanInput::Key { key: "ab".into(), shift: false },
        HumanInput::Text { text: String::new() },
        HumanInput::Text { text: "x".repeat(live::MAX_TEXT_CHARS + 1) },
        HumanInput::Click { x: 1.5, y: 0.5, button: None, count: None },
        HumanInput::Click { x: 0.5, y: 0.5, button: Some("back".into()), count: None },
        HumanInput::Scroll { x: 0.5, y: 0.5, dy: f64::NAN },
    ] {
        assert!(lv.mgr.human_input(bad.clone()).await.is_err(), "{bad:?}");
    }
    // Given back, the person cannot touch the page any more.
    lv.mgr.give_back();
    assert!(lv.mgr.human_input(click()).await.is_err());
}

#[tokio::test]
async fn a_download_started_while_the_person_drives_is_not_kept() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.run("browser_snapshot", json!({})).await;
    lv.mgr.take_control().await.unwrap();
    lv.fake.push(super::fake::event(
        None,
        "Browser.downloadWillBegin",
        json!({"guid": "g1", "url": "http://e.example/f.bin", "suggestedFilename": "f.bin"}),
    ));
    let cancel = lv.fake.wait_for(|r| r.method == "Browser.cancelDownload").await;
    assert_eq!(cancel.params["guid"], "g1");
    lv.mgr.give_back();
    let list = lv.run("browser_downloads", json!({})).await;
    assert!(list.content.contains("non si scaricano file mentre l'utente guida il browser"), "{}", list.content);
}

#[tokio::test]
async fn the_state_says_which_control_has_the_focus_only_while_the_person_drives_and_never_its_value() {
    let lv = Live::new(|req, st| match req.method.as_str() {
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("document.activeElement") => {
            Some(Reply::ok(json!({"result": {"type": "string", "value": "campo password «Chiave»"}})))
        }
        _ => page("http://e.example/")(req, st),
    })
    .await;
    lv.run("browser_snapshot", json!({})).await;
    assert_eq!(lv.mgr.live_state_with_focus().await.focus, None, "the agent drives: the page is not asked");
    lv.mgr.take_control().await.unwrap();
    assert_eq!(lv.mgr.live_state_with_focus().await.focus.as_deref(), Some("campo password «Chiave»"));
    lv.mgr.give_back();
    assert_eq!(lv.mgr.live_state_with_focus().await.focus, None);
}

// ---------------------------------------------------------------------- somebody's own Chrome

/// A manager attached to the endpoint instead of launching a browser.
fn attached_to(endpoint: String) -> (Arc<BrowserManager>, Vec<Arc<dyn Tool>>, ToolContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut m = BrowserManager::new(dir.path().join("profile"));
    m.policy.resolve = Arc::new(|_, _| Box::pin(async { Err(std::io::Error::other("no dns")) }));
    assert!(m.set_attach(Some(endpoint)));
    let m = Arc::new(m);
    let t = tools(m.clone());
    let ctx = ToolContext::new("run1", dir.path().to_path_buf());
    (m, t, ctx, dir)
}

async fn call(t: &[Arc<dyn Tool>], ctx: &ToolContext, name: &str, args: Value) -> ToolOutput {
    let tool = t.iter().find(|x| x.name() == name).unwrap().clone();
    tool.call(args, ctx).await.unwrap_or_else(|e| ToolOutput::error(format!("ERR: {e}")))
}

/// Answers `GET /json/version` with the given WebSocket address (what Chrome does on its debugging port).
async fn version_server(ws: String) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            let ws = ws.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let body = json!({"Browser": "Chrome/150", "webSocketDebuggerUrl": ws}).to_string();
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    port
}

#[tokio::test]
async fn an_attached_chrome_keeps_the_persons_tabs_out_of_sight_and_stays_open() {
    let fake = fake::FakeCdp::start(page("http://e.example/")).await;
    let port = version_server(fake.url.clone()).await;
    let (mgr, t, ctx, _dir) = attached_to(format!("http://127.0.0.1:{port}"));
    let first = call(&t, &ctx, "browser_snapshot", json!({})).await;
    assert!(!first.is_error, "{}", first.content);
    // The agent works in a tab of its own, made on purpose; nothing of the person's was attached, no download behaviour
    // was changed, and the pages that already existed were not asked about.
    assert_eq!(fake.commands("Target.createTarget").len(), 1);
    assert!(fake.commands("Target.attachToTarget").is_empty() && fake.commands("Target.getTargets").is_empty());
    assert!(fake.commands("Browser.setDownloadBehavior").is_empty(), "a download stays the person's Chrome's");
    // The person's tab (T1) and a tab they open later attach by themselves (browser-level auto-attach): they are let go at
    // once, with no policy, no snapshot and no listing.
    fake.push(fake::attached(None, "T1", "S1", "https://bank.example/", None));
    fake.push(fake::attached(None, "T7", "S7", "https://mail.example/", None));
    let letgo = fake.wait_for(|r| r.method == "Target.detachFromTarget" && r.params["sessionId"] == "S7").await;
    assert_eq!(letgo.session, None);
    assert!(fake.commands("Target.detachFromTarget").iter().any(|r| r.params["sessionId"] == "S1"));
    for s in ["S1", "S7"] {
        assert!(fake.commands("Fetch.enable").iter().all(|r| r.session.as_deref() != Some(s)), "{s} was intercepted");
        assert!(fake.commands("Page.enable").iter().all(|r| r.session.as_deref() != Some(s)), "{s} was enabled");
    }
    // A popup of our own page is ours, under the same policy as in our own browser.
    fake.push(fake::attached(None, "T9", "S9", "about:blank", Some("T2")));
    fake.wait_for(|r| r.method == "Fetch.enable" && r.session.as_deref() == Some("S9")).await;
    let tabs = call(&t, &ctx, "browser_tabs", json!({})).await;
    assert!(
        tabs.content.contains("2 su 8") && !tabs.content.contains("bank.example") && !tabs.content.contains("mail.example"),
        "{}",
        tabs.content
    );
    // Closing closes our tabs and leaves Chrome open.
    assert!(mgr.close().await);
    let closed: Vec<String> =
        fake.commands("Target.closeTarget").iter().map(|r| r.params["targetId"].as_str().unwrap().to_string()).collect();
    assert!(closed.contains(&"T2".to_string()) && closed.contains(&"T9".to_string()) && !closed.contains(&"T1".to_string()), "{closed:?}");
    assert!(fake.commands("Browser.close").is_empty(), "somebody's own Chrome is never closed");
    // And the connection does not go on watching it: auto-attach is off again, so the person's new tabs are not paused any more.
    let off = fake.commands("Target.setAutoAttach").pop().unwrap();
    assert_eq!((off.params["autoAttach"].as_bool(), off.session), (Some(false), None));
    assert_eq!(fake.commands("Target.setDiscoverTargets").pop().unwrap().params["discover"], false);
}

#[tokio::test]
async fn only_a_chrome_on_this_computer_can_be_attached() {
    let fake = fake::FakeCdp::start(page("http://e.example/")).await;
    // An address that is not on this computer is refused before any connection.
    for bad in ["http://192.168.1.5:9222", "ws://192.168.1.5:9222/devtools/browser/x", "http://example.com:9222"] {
        let (_mgr, t, ctx, _dir) = attached_to(bad.to_string());
        let out = call(&t, &ctx, "browser_snapshot", json!({})).await;
        assert!(out.is_error && out.content.contains("questo computer"), "{bad}: {}", out.content);
    }
    // A Chrome on this computer that answers with an address somewhere else is refused too.
    let port = version_server("ws://192.168.1.5:9222/devtools/browser/x".into()).await;
    let (_mgr, t, ctx, _dir) = attached_to(format!("http://127.0.0.1:{port}"));
    let out = call(&t, &ctx, "browser_snapshot", json!({})).await;
    assert!(out.is_error && out.content.contains("questo computer"), "{}", out.content);
    assert!(fake.log.lock().unwrap().is_empty(), "nothing was contacted");
    // Nothing listening: a clear message, not a hang.
    let (_mgr, t, ctx, _dir) = attached_to("http://127.0.0.1:9".to_string());
    let out = call(&t, &ctx, "browser_snapshot", json!({})).await;
    assert!(out.is_error && out.content.contains("--remote-debugging-port"), "{}", out.content);
}

#[tokio::test]
async fn two_tabs_with_a_confirm_each_do_not_take_each_others_place() {
    let lv = Live::new(page("http://e.example/")).await;
    lv.mgr.set_dialog_hold(Duration::from_millis(400));
    lv.run("browser_snapshot", json!({})).await;
    lv.run("browser_tab_new", json!({})).await;
    // S1 is the first tab, S2 the second (the active one): each opens its own confirm.
    lv.fake.push(super::fake::event(Some("S1"), "Page.javascriptDialogOpening", json!({"type": "confirm", "message": "Prima scheda?"})));
    lv.fake.push(super::fake::event(Some("S2"), "Page.javascriptDialogOpening", json!({"type": "confirm", "message": "Seconda scheda?"})));
    tokio::time::sleep(Duration::from_millis(150)).await;
    // The one of the tab in front is the one that is shown and answered first.
    assert_eq!(lv.mgr.live_state().dialog.unwrap().message, "Seconda scheda?");
    lv.mgr.take_control().await.unwrap();
    lv.mgr.answer_dialog(true, None).await.unwrap();
    let first = lv.fake.commands("Page.handleJavaScriptDialog").remove(0);
    assert_eq!((first.session.as_deref(), first.params["accept"].as_bool()), (Some("S2"), Some(true)));
    assert_eq!(lv.mgr.live_state().dialog.unwrap().message, "Prima scheda?", "the other one is still there");
    // And the one nobody answers is dismissed on its own timer, in its own tab.
    let dismissed = lv.fake.wait_for(|r| r.method == "Page.handleJavaScriptDialog" && r.session.as_deref() == Some("S1")).await;
    assert_eq!(dismissed.params["accept"], false);
    assert!(lv.mgr.live_state().dialog.is_none());
}

// ---------------------------------------------------------------------- what the model is shown of a login

#[tokio::test]
async fn a_screenshot_for_the_model_draws_the_code_and_card_fields_as_dots_and_takes_the_mask_off() {
    let lv = Live::new(|req, st| match req.method.as_str() {
        "Page.captureScreenshot" => Some(Reply::ok(json!({"data": "SlBFRy1EQVRJ"}))),
        _ => page("http://e.example/")(req, st),
    })
    .await;
    let out = lv.run("browser_screenshot", json!({})).await;
    assert!(!out.is_error && out.images.len() == 1, "{}", out.content);
    let log = lv.fake.log.lock().unwrap().clone();
    let position = |pred: &dyn Fn(&Req) -> bool| log.iter().position(|r| pred(r)).unwrap_or_else(|| panic!("command not sent"));
    let mask = position(&|r| {
        r.method == "Runtime.evaluate"
            && r.params["expression"].as_str().is_some_and(|e| e.contains("aiw-secret-mask") && e.contains("createElement"))
    });
    let shot = position(&|r| r.method == "Page.captureScreenshot");
    let unmask = position(&|r| {
        r.method == "Runtime.evaluate"
            && r.params["expression"].as_str().is_some_and(|e| e.contains("aiw-secret-mask") && e.contains("remove()"))
    });
    assert!(mask < shot && shot < unmask, "mask, capture, unmask: {mask} {shot} {unmask}");
    let style = log[mask].params["expression"].as_str().unwrap();
    for field in ["one-time-code", "cc-", "current-password", "new-password", "type=password"] {
        assert!(style.contains(field), "{field} is masked: {style}");
    }
}

#[tokio::test]
async fn when_the_page_cannot_say_which_fields_are_secret_no_value_is_written_in_the_tree() {
    let lv = Live::new(|req, st| match req.method.as_str() {
        "Accessibility.getFullAXTree" => Some(Reply::ok(json!({"nodes": [
            {"nodeId": "1", "role": {"value": "RootWebArea"}, "name": {"value": "P"}, "childIds": ["2"], "properties": []},
            {"nodeId": "2", "parentId": "1", "role": {"value": "textbox"}, "name": {"value": "Codice"}, "value": {"value": "123456-DI-PROVA"},
             "backendDOMNodeId": 7, "childIds": [], "properties": []}]}))),
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("querySelectorAll('input,textarea')") => {
            Some(Reply::ok(json!({"exceptionDetails": {"exception": {"description": "TypeError: la pagina ha cambiato Array"}}})))
        }
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("document.title") => {
            Some(Reply::ok(json!({"result": {"type": "object", "value": {"t": "P", "u": "http://e.example/"}}})))
        }
        _ => page("http://e.example/")(req, st),
    })
    .await;
    let out = lv.run("browser_snapshot", json!({"format": "ax"})).await;
    assert!(!out.is_error, "{}", out.content);
    assert!(!out.content.contains("123456-DI-PROVA") && out.content.contains("value=\"(nascosto)\""), "{}", out.content);
}

#[tokio::test]
async fn a_tab_the_person_opens_while_ours_is_being_made_is_not_taken_for_ours() {
    // The person presses Ctrl+T at the very moment the agent's tab is created: their tab (`chrome://newtab/`) attaches first.
    let fake = fake::FakeCdp::start(|req, st| match req.method.as_str() {
        "Target.createTarget" => {
            let n = st.next;
            st.next += 1;
            let (id, session) = (format!("T{n}"), format!("S{n}"));
            st.targets.push(fake::Target { id: id.clone(), session: session.clone(), url: "about:blank".into(), title: "".into() });
            Some(Reply::with(
                json!({"targetId": id}),
                vec![fake::attached(None, "TU", "SU", "chrome://newtab/", None), fake::attached(None, &id, &session, "about:blank", None)],
            ))
        }
        _ => page("http://e.example/")(req, st),
    })
    .await;
    let port = version_server(fake.url.clone()).await;
    let (_mgr, t, ctx, _dir) = attached_to(format!("http://127.0.0.1:{port}"));
    let out = call(&t, &ctx, "browser_tabs", json!({})).await;
    assert!(!out.is_error && out.content.contains("1 su 8"), "{}", out.content);
    fake.wait_for(|r| r.method == "Target.detachFromTarget" && r.params["sessionId"] == "SU").await;
    assert!(fake.commands("Fetch.enable").iter().all(|r| r.session.as_deref() != Some("SU")), "the person's tab was not intercepted");
}
