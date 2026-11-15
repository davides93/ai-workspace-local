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
    assert!(lv.mgr.answer_dialog(true, None).await.is_err(), "nothing waits any more");
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
