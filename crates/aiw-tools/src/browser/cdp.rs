//! The CDP connection of the controlled browser and what the tools need to know about it: the open
//! tabs, JS dialogs, the console, downloads and in-flight requests.
//!
//! The connection is made to the *browser* (not to a page), with `Target.setAutoAttach` and
//! `waitForDebuggerOnStart`: every new page target, including the ones the site opens by itself
//! (`window.open`, `target=_blank`), attaches paused, gets `Fetch.enable` (the network policy) and
//! only then is resumed. So no tab ever makes a request before the policy sees it.

use super::{ax, fetch_patterns, handle_paused, send_command, Policy};
use crate::ToolContext;
use aiw_core::{Error, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// Open tabs at most (the site cannot open more either: the extra ones are closed).
pub(crate) const MAX_TABS: usize = 8;
/// Downloads a single run may start.
pub(crate) const MAX_DOWNLOADS_PER_RUN: usize = 10;
/// Largest download kept (it is cancelled as soon as it is known to be larger).
pub(crate) const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
/// Console entries kept per tab.
const CONSOLE_CAP: usize = 100;
/// Notes (dialogs, new tabs) waiting to be shown in the next result.
const NOTES_CAP: usize = 20;

/// How long a `confirm` or `prompt` of the site waits for somebody to answer it (the agent with `browser_dialog`, the
/// person in the live view) before it is dismissed, the safe answer, so a page never stays blocked for good.
pub(crate) const DIALOG_HOLD: Duration = Duration::from_secs(120);
/// The error of a call that was interrupted because the page is blocked by a window of the site.
pub(crate) const DIALOG_PENDING: &str = "finestra del sito in attesa di risposta";

/// A window of the site that waits for an answer: `confirm` and `prompt` (the other kinds have no choice and are
/// answered at once).
#[derive(Clone, Debug)]
pub(crate) struct PendingDialog {
    pub session: String,
    pub kind: String,
    pub message: String,
    pub default_text: String,
    /// Tells this dialog from the next one that opens in its place (the fallback timer is bound to it).
    pub id: u64,
}

/// How the post-action snapshot is written: the elements list or the accessibility tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum SnapFmt {
    #[default]
    Elements,
    Ax,
}

#[derive(Default)]
pub(crate) struct Tab {
    pub target_id: String,
    pub session: Option<String>,
    pub title: String,
    pub url: String,
    /// Opened by the page itself (`window.open`, `target=_blank`), not by a tool.
    pub by_site: bool,
    pub console: VecDeque<String>,
    /// Main-frame navigations and load events seen (for `browser_wait`).
    pub navs: u64,
    pub loads: u64,
    pub fmt: SnapFmt,
    pub refs: ax::Refs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DlState {
    Running,
    Done,
    Failed(String),
}

pub(crate) struct Download {
    pub guid: String,
    pub url: String,
    pub name: String,
    pub state: DlState,
    pub path: Option<PathBuf>,
    pub bytes: u64,
    /// The call that was running when the download started: its run, workspace and sandbox decide.
    pub ctx: ToolContext,
}

pub(crate) enum Note {
    Text(String),
    NewTab(String),
}

#[derive(Default)]
pub(crate) struct State {
    pub tabs: Vec<Tab>,
    pub active: Option<String>,
    pub downloads: Vec<Download>,
    pub notes: Vec<Note>,
    /// `session:requestId` of the requests still running.
    pub inflight: HashSet<String>,
    /// The person drove the browser and gave it back: addresses lose their query and fragment (they may
    /// carry a code or a token) until the agent navigates by itself. See `control.rs`.
    pub redact: bool,
    /// A `confirm` or `prompt` waits for an answer (see [`DIALOG_HOLD`]).
    pub dialog: Option<PendingDialog>,
    pub dialog_hold: Option<Duration>,
    pub dialogs_opened: u64,
    /// Attached to somebody's own Chrome: the targets this connection created (or that its pages opened). Everything else
    /// in that browser is not ours to look at.
    pub own: HashSet<String>,
    /// A tab of ours was just asked for: the next page that attaches without an opener is it.
    pub expecting_own: bool,
}

impl State {
    fn tab_by_session(&mut self, session: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.session.as_deref() == Some(session))
    }
    fn note(&mut self, n: Note) {
        if self.notes.len() < NOTES_CAP {
            self.notes.push(n);
        }
    }
    pub(crate) fn active_tab(&mut self) -> Option<&mut Tab> {
        let id = self.active.clone()?;
        self.tabs.iter_mut().find(|t| t.target_id == id)
    }
}

pub(crate) struct TabInfo {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub active: bool,
    pub by_site: bool,
}

/// What the reader task needs to react to events on its own.
#[derive(Clone)]
struct Hub {
    st: Arc<Mutex<State>>,
    policy: Policy,
    tx: mpsc::UnboundedSender<Message>,
    next: Arc<AtomicU64>,
    /// Woken when a window of the site opens or closes: the calls blocked by it end (see `call_on`).
    dialog_notify: Arc<tokio::sync::Notify>,
    /// The browser is somebody's own Chrome (see `State::own`), not one we launched.
    own_only: bool,
    /// Where the browser writes downloads: they are moved into the workspace only once complete
    /// and checked, so the browser itself never writes into the workspace.
    tmp: PathBuf,
}

pub(crate) struct Cdp {
    tx: mpsc::UnboundedSender<Message>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next: Arc<AtomicU64>,
    pub(crate) st: Arc<Mutex<State>>,
    dialog_notify: Arc<tokio::sync::Notify>,
    /// Connected to somebody's own Chrome: only the tabs this connection opens are ours.
    own_only: bool,
    tmp: PathBuf,
}

pub(crate) fn clip(t: &str, max: usize) -> String {
    if t.chars().count() <= max {
        return t.to_string();
    }
    format!("{}…", t.chars().take(max).collect::<String>())
}

fn safe_component(s: &str) -> String {
    let c: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if c.is_empty() {
        "run".into()
    } else {
        c
    }
}

/// A file name that is safe to create: no folders, no control characters, no leading dots.
pub(crate) fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' '])
        .to_string();
    if cleaned.is_empty() {
        return "download".into();
    }
    if cleaned.chars().count() <= 120 {
        return cleaned;
    }
    match cleaned.rsplit_once('.') {
        Some((stem, ext)) if ext.chars().count() <= 10 => format!("{}.{ext}", stem.chars().take(100).collect::<String>()),
        _ => cleaned.chars().take(120).collect(),
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..1000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

fn human_size(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else if b >= 1 << 10 {
        format!("{:.1} kB", b as f64 / 1024.0)
    } else {
        format!("{b} byte")
    }
}

impl Cdp {
    pub(crate) async fn connect(ws_url: &str, policy: Policy, tmp: PathBuf, own_only: bool) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url).await.map_err(|e| Error::Runtime(format!("CDP: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Default::default();
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if sink.send(m).await.is_err() {
                    break;
                }
            }
        });
        let st: Arc<Mutex<State>> = Default::default();
        let next = Arc::new(AtomicU64::new(1));
        let dialog_notify = Arc::new(tokio::sync::Notify::new());
        let hub = Hub {
            st: st.clone(),
            policy,
            tx: tx.clone(),
            next: next.clone(),
            dialog_notify: dialog_notify.clone(),
            own_only,
            tmp: tmp.clone(),
        };
        let p2 = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Ok(text) = msg.into_text() else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_ref()) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(s) = p2.lock().unwrap().remove(&id) {
                        let _ = s.send(v);
                    }
                } else {
                    // Everything is handled here, in the reader itself: the network policy must
                    // never miss a paused request, which a lossy broadcast channel could do.
                    on_event(&hub, &v);
                }
            }
            // The browser went away: whoever waits for an answer is told.
            p2.lock().unwrap().clear();
        });
        Ok(Self { tx, pending, next, st, dialog_notify, own_only, tmp })
    }

    pub(crate) async fn call_on(&self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        // A page blocked by a window of the site answers nothing (a click that opens a `confirm` does not return
        // until it is answered): a call to that page ends at once with `DIALOG_PENDING`, so the caller can say so.
        let interruptible = session.is_some() && method != "Page.handleJavaScriptDialog";
        if interruptible && self.dialog_blocks(session) {
            return Err(Error::Conflict(DIALOG_PENDING.into()));
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (s, r) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, s);
        let mut m = json!({"id": id, "method": method, "params": params});
        if let Some(sid) = session {
            m["sessionId"] = json!(sid);
        }
        self.tx.send(Message::Text(m.to_string().into())).map_err(|_| Error::Runtime("connessione al browser chiusa".into()))?;
        let reply = tokio::time::timeout(Duration::from_secs(30), r);
        let v = if interruptible {
            tokio::select! {
                r = reply => r,
                _ = self.wait_dialog(session) => {
                    self.pending.lock().unwrap().remove(&id);
                    return Err(Error::Conflict(DIALOG_PENDING.into()));
                }
            }
        } else {
            reply.await
        }
        .map_err(|_| Error::Runtime(format!("CDP {method}: tempo scaduto")))?
        .map_err(|_| Error::Runtime("browser chiuso".into()))?;
        if let Some(e) = v.get("error") {
            return Err(Error::Runtime(format!("CDP {method}: {e}")));
        }
        Ok(v["result"].clone())
    }

    /// A window of the site is waiting for an answer on this session.
    fn dialog_blocks(&self, session: Option<&str>) -> bool {
        let st = self.st.lock().unwrap();
        matches!((&st.dialog, session), (Some(d), Some(s)) if d.session == s)
    }

    /// Ends when a window of the site is waiting on this session.
    async fn wait_dialog(&self, session: Option<&str>) {
        loop {
            let notified = self.dialog_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.dialog_blocks(session) {
                return;
            }
            notified.await;
        }
    }

    /// The window of the site that waits for an answer, if any.
    pub(crate) fn pending_dialog(&self) -> Option<super::live::DialogInfo> {
        self.st.lock().unwrap().dialog.as_ref().map(|d| super::live::DialogInfo {
            kind: d.kind.clone(),
            message: d.message.clone(),
            default_text: d.default_text.clone(),
        })
    }

    /// Answers the window that waits: accepts or dismisses it (`text` is what a `prompt` gets).
    pub(crate) async fn answer_dialog(&self, accept: bool, text: Option<String>) -> Result<()> {
        let d = self
            .st
            .lock()
            .unwrap()
            .dialog
            .clone()
            .ok_or_else(|| Error::NotFound("nessuna finestra del sito aspetta una risposta".into()))?;
        let mut params = json!({"accept": accept});
        if let (true, Some(t)) = (accept && d.kind == "prompt", text) {
            params["promptText"] = json!(t);
        }
        self.call_on(Some(&d.session), "Page.handleJavaScriptDialog", params).await?;
        let mut st = self.st.lock().unwrap();
        if st.dialog.as_ref().is_some_and(|x| x.id == d.id) {
            st.dialog = None;
        }
        drop(st);
        self.dialog_notify.notify_waiters();
        Ok(())
    }

    /// Title and address of the tab in front.
    pub(crate) fn active_page(&self) -> Option<(String, String)> {
        let mut st = self.st.lock().unwrap();
        st.active_tab().map(|t| (t.title.clone(), t.url.clone()))
    }

    pub(crate) fn set_dialog_hold(&self, hold: Duration) {
        self.st.lock().unwrap().dialog_hold = Some(hold);
    }

    pub(crate) fn set_redact(&self, on: bool) {
        self.st.lock().unwrap().redact = on;
    }

    /// An address as the model may see it (see `State::redact`).
    pub(crate) fn shown_url(&self, url: &str) -> String {
        if self.st.lock().unwrap().redact {
            super::without_query(url)
        } else {
            url.to_string()
        }
    }

    /// A snapshot as the model may read it: the address lines without query and fragment when they are hidden.
    pub(crate) fn scrub(&self, text: &str) -> String {
        if self.st.lock().unwrap().redact {
            super::control::scrub_url_lines(text)
        } else {
            text.to_string()
        }
    }

    /// A command for the browser itself (targets, downloads).
    pub(crate) async fn call_root(&self, method: &str, params: Value) -> Result<Value> {
        self.call_on(None, method, params).await
    }

    /// A command for the active tab.
    pub(crate) async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let s = self.active_session()?;
        self.call_on(Some(&s), method, params).await
    }

    pub(crate) fn active_session(&self) -> Result<String> {
        let mut st = self.st.lock().unwrap();
        st.active_tab().and_then(|t| t.session.clone()).ok_or_else(|| Error::Runtime("nessuna scheda attiva nel browser".into()))
    }

    /// Runs one of *our own* fixed scripts in the controlled page via `Runtime.evaluate`.
    /// Only two kinds of values are ever interpolated into them: the numeric `ref` (u64) and
    /// typed text encoded with `serde_json::to_string`, i.e. always as a string literal, never
    /// as code. Page content cannot reach this function.
    pub(crate) async fn eval(&self, expr: &str) -> Result<Value> {
        let r = self.call("Runtime.evaluate", json!({"expression": expr, "returnByValue": true, "awaitPromise": true})).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(Error::Runtime(format!("errore JavaScript: {}", ex["exception"]["description"].as_str().unwrap_or("sconosciuto"))));
        }
        Ok(r["result"]["value"].clone())
    }

    /// Browser-level setup, once per connection.
    pub(crate) async fn init(&self) -> Result<()> {
        if self.own_only {
            return self.init_attached().await;
        }
        std::fs::create_dir_all(&self.tmp)?;
        self.call_root("Target.setDiscoverTargets", json!({"discover": true})).await?;
        // New pages (popups included) attach paused: the network policy is in place before they run.
        self.call_root("Target.setAutoAttach", json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true})).await?;
        // Downloads go to a folder of ours and are moved into the workspace once complete and checked.
        let _ = self
            .call_root(
                "Browser.setDownloadBehavior",
                json!({"behavior": "allowAndName", "downloadPath": self.tmp.to_string_lossy(), "eventsEnabled": true}),
            )
            .await;
        // The pages that already exist.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let targets = self.call_root("Target.getTargets", json!({})).await?;
        for t in targets["targetInfos"].as_array().cloned().unwrap_or_default() {
            let id = t["targetId"].as_str().unwrap_or_default().to_string();
            if t["type"] != "page" || id.is_empty() || self.has_tab(&id) {
                continue;
            }
            let _ = self.call_root("Target.attachToTarget", json!({"targetId": id, "flatten": true})).await;
        }
        if self.tab_count() == 0 && !self.wait_for_any_tab().await {
            self.new_tab().await?;
        }
        Ok(())
    }

    /// Setup against somebody's own Chrome: nothing that is theirs is touched. The pages that exist are not attached (the
    /// ones that attach by themselves are let go again, see `on_attached`), the download behaviour is left alone (so a
    /// download stays Chrome's, and `browser_downloads` has nothing), and the agent works in a tab of its own.
    async fn init_attached(&self) -> Result<()> {
        self.call_root("Target.setDiscoverTargets", json!({"discover": true})).await?;
        // The popups of our own pages attach paused like in our own browser; whatever else attaches is let go.
        self.call_root("Target.setAutoAttach", json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true})).await?;
        self.new_tab().await?;
        Ok(())
    }

    /// Closes the tabs this connection opened and nothing else (somebody's own Chrome stays open).
    pub(crate) async fn close_own_tabs(&self) {
        let ids: Vec<String> = self.st.lock().unwrap().tabs.iter().map(|t| t.target_id.clone()).collect();
        for id in ids {
            let _ = self.call_root("Target.closeTarget", json!({"targetId": id})).await;
        }
    }

    async fn wait_for_any_tab(&self) -> bool {
        for _ in 0..100 {
            if self.st.lock().unwrap().active.is_some() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    pub(crate) fn tab_count(&self) -> usize {
        self.st.lock().unwrap().tabs.len()
    }

    fn has_tab(&self, target_id: &str) -> bool {
        self.st.lock().unwrap().tabs.iter().any(|t| t.target_id == target_id)
    }

    /// There is always a tab to act on: when none is left a blank one is opened.
    pub(crate) async fn ensure_tab(&self) -> Result<()> {
        if self.active_session().is_err() {
            if self.tab_count() == 0 {
                self.new_tab().await?;
            } else {
                let first = self.st.lock().unwrap().tabs.first().map(|t| t.target_id.clone());
                self.st.lock().unwrap().active = first;
            }
        }
        Ok(())
    }

    async fn wait_attached(&self, target_id: &str) -> Result<()> {
        for _ in 0..400 {
            if self.st.lock().unwrap().tabs.iter().any(|t| t.target_id == target_id && t.session.is_some()) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Err(Error::Runtime("la nuova scheda non si è aperta in tempo".into()))
    }

    /// Opens a blank tab and makes it the active one. The caller checks `MAX_TABS` first.
    pub(crate) async fn new_tab(&self) -> Result<String> {
        if self.own_only {
            self.st.lock().unwrap().expecting_own = true;
        }
        let r = self.call_root("Target.createTarget", json!({"url": "about:blank"})).await;
        if r.is_err() {
            self.st.lock().unwrap().expecting_own = false;
        }
        let r = r?;
        let id = r["targetId"].as_str().ok_or_else(|| Error::Runtime("CDP: nessun identificativo per la nuova scheda".into()))?.to_string();
        self.wait_attached(&id).await?;
        self.select_target(&id).await;
        Ok(id)
    }

    async fn select_target(&self, target_id: &str) {
        self.st.lock().unwrap().active = Some(target_id.to_string());
        let _ = self.call_root("Target.activateTarget", json!({"targetId": target_id})).await;
    }

    /// Titles and addresses as the browser has them now (the change events can lag behind a load).
    pub(crate) async fn refresh_tabs(&self) {
        let Ok(r) = self.call_root("Target.getTargets", json!({})).await else { return };
        let mut st = self.st.lock().unwrap();
        for info in r["targetInfos"].as_array().into_iter().flatten() {
            if let Some(t) = st.tabs.iter_mut().find(|t| t.target_id == info["targetId"].as_str().unwrap_or_default()) {
                t.title = info["title"].as_str().unwrap_or_default().to_string();
                t.url = info["url"].as_str().unwrap_or_default().to_string();
            }
        }
    }

    pub(crate) fn tabs(&self) -> Vec<TabInfo> {
        let st = self.st.lock().unwrap();
        st.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| TabInfo {
                index: i + 1,
                title: t.title.clone(),
                url: t.url.clone(),
                active: st.active.as_deref() == Some(&t.target_id),
                by_site: t.by_site,
            })
            .collect()
    }

    fn target_at(&self, index: usize) -> Option<String> {
        self.st.lock().unwrap().tabs.get(index.checked_sub(1)?).map(|t| t.target_id.clone())
    }

    pub(crate) fn active_index(&self) -> Option<usize> {
        let st = self.st.lock().unwrap();
        let a = st.active.clone()?;
        st.tabs.iter().position(|t| t.target_id == a).map(|i| i + 1)
    }

    pub(crate) async fn select_tab(&self, index: usize) -> Result<()> {
        let id = self.target_at(index).ok_or_else(|| Error::Invalid(format!("la scheda {index} non esiste: usa browser_tabs")))?;
        self.select_target(&id).await;
        Ok(())
    }

    /// Closes tab `index`. The last tab is replaced by a blank one: the browser always has a page.
    pub(crate) async fn close_tab(&self, index: usize) -> Result<()> {
        let id = self.target_at(index).ok_or_else(|| Error::Invalid(format!("la scheda {index} non esiste: usa browser_tabs")))?;
        let was_active = self.st.lock().unwrap().active.as_deref() == Some(&id);
        if self.tab_count() <= 1 {
            self.new_tab().await?;
        }
        self.call_root("Target.closeTarget", json!({"targetId": id})).await?;
        for _ in 0..120 {
            if !self.has_tab(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        // The event handler drops the tab; make sure the active one is still a tab that exists.
        let mut st = self.st.lock().unwrap();
        st.tabs.retain(|t| t.target_id != id);
        if was_active || st.active.as_deref() == Some(&id) {
            st.active = st.tabs.last().map(|t| t.target_id.clone());
        }
        Ok(())
    }

    /// Dialog and tab notes since the last result, as lines.
    pub(crate) fn take_notes(&self) -> Vec<String> {
        let mut st = self.st.lock().unwrap();
        let notes = std::mem::take(&mut st.notes);
        let redact = st.redact;
        notes
            .into_iter()
            .filter_map(|n| match n {
                Note::Text(t) => Some(t),
                Note::NewTab(id) => {
                    let i = st.tabs.iter().position(|t| t.target_id == id)?;
                    let t = &st.tabs[i];
                    let active = st.active.as_deref() == Some(&id);
                    Some(format!(
                        "Il sito ha aperto una nuova scheda [{}] «{}» ({}){}.",
                        i + 1,
                        clip(&t.title, 60),
                        clip(&if redact { super::without_query(&t.url) } else { t.url.clone() }, 90),
                        if active { ": ora è quella attiva, torna alla precedente con browser_tab_select" } else { "" }
                    ))
                }
            })
            .collect()
    }

    // ------------------------------------------------------------------ elements by reference

    /// Remote object of the element a `ref` names: the numeric ones of the elements snapshot
    /// (a `data-aiw-ref` attribute) and the `eN` ones of the accessibility snapshot (the DOM node).
    /// `None` when it is not on the page any more.
    pub(crate) async fn object_for_ref(&self, r: &RefArg) -> Result<Option<String>> {
        match r {
            RefArg::Num(n) => {
                let v = self
                    .call(
                        "Runtime.evaluate",
                        json!({"expression": format!("document.querySelector('[data-aiw-ref=\"{n}\"]')"), "returnByValue": false}),
                    )
                    .await?;
                Ok(v["result"]["objectId"].as_str().map(String::from))
            }
            RefArg::Ax(a) => {
                let Some(backend) = self.backend_of(a) else { return Ok(None) };
                match self.call("DOM.resolveNode", json!({"backendNodeId": backend})).await {
                    Ok(v) => Ok(v["object"]["objectId"].as_str().map(String::from)),
                    Err(_) => Ok(None),
                }
            }
        }
    }

    pub(crate) fn backend_of(&self, r: &str) -> Option<i64> {
        self.st.lock().unwrap().active_tab()?.refs.backend(r)
    }

    /// Runs `body` (the body of `function(el) { ... }`) on the element of `r`. `"missing"` when the
    /// element is gone. Same rule as `eval` for what goes into the script.
    pub(crate) async fn on_ref(&self, r: &RefArg, body: &str) -> Result<Value> {
        if let RefArg::Ax(_) = r {
            if let Some(t) = self.st.lock().unwrap().active_tab() {
                t.fmt = SnapFmt::Ax;
            }
        }
        let Some(obj) = self.object_for_ref(r).await? else { return Ok(json!("missing")) };
        let v = self
            .call(
                "Runtime.callFunctionOn",
                json!({"objectId": obj, "functionDeclaration": format!("function() {{ return (function(el) {{ {body} }})(this); }}"),
                       "returnByValue": true, "awaitPromise": true,
                       // A click done for the model counts as a click of the person: without it the
                       // browser's popup blocker would swallow `target=_blank` and `window.open`.
                       "userGesture": true}),
            )
            .await?;
        if let Some(ex) = v.get("exceptionDetails") {
            return Err(Error::Runtime(format!("errore JavaScript: {}", ex["exception"]["description"].as_str().unwrap_or("sconosciuto"))));
        }
        Ok(v["result"]["value"].clone())
    }

    /// Runs `body` with an optional element: on the element of `r`, or with `el` null.
    pub(crate) async fn run_body(&self, r: Option<&RefArg>, body: &str) -> Result<Value> {
        match r {
            Some(r) => self.on_ref(r, body).await,
            None => self.eval(&format!("(function(el) {{ {body} }})(null)")).await,
        }
    }

    // ------------------------------------------------------------------ snapshots

    pub(crate) fn fmt(&self) -> SnapFmt {
        self.st.lock().unwrap().active_tab().map(|t| t.fmt).unwrap_or_default()
    }

    pub(crate) fn set_fmt(&self, f: SnapFmt) {
        if let Some(t) = self.st.lock().unwrap().active_tab() {
            t.fmt = f;
        }
    }

    /// `Scheda attiva: [2] di 3` when there is more than one tab.
    pub(crate) fn tab_header(&self) -> String {
        let n = self.tab_count();
        match (n > 1, self.active_index()) {
            (true, Some(i)) => format!("Scheda attiva: [{i}] di {n} (browser_tabs le elenca)\n"),
            _ => String::new(),
        }
    }

    /// Accessibility snapshot of the active tab.
    pub(crate) async fn ax_snapshot(&self) -> Result<String> {
        let tree = self.call("Accessibility.getFullAXTree", json!({})).await?;
        let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
        // The values of the fields a person types who they are in: never written out (see `ax::render`).
        let secrets: Vec<String> = self
            .eval(super::SECRET_VALUES_JS)
            .await
            .ok()
            .and_then(|v| v.as_str().and_then(|t| serde_json::from_str(t).ok()))
            .unwrap_or_default();
        let head = self.eval("({t: document.title, u: location.href})").await.unwrap_or(Value::Null);
        let body = {
            let mut st = self.st.lock().unwrap();
            let tab = st.active_tab().ok_or_else(|| Error::Runtime("nessuna scheda attiva nel browser".into()))?;
            tab.fmt = SnapFmt::Ax;
            ax::render(&nodes, &mut tab.refs, &secrets)
        };
        Ok(format!(
            "Titolo: {}\nURL: {}\n\nAlbero di accessibilità (usa [eN] come ref; i ref restano validi finché la pagina non cambia):\n{body}",
            head["t"].as_str().unwrap_or_default(),
            self.shown_url(head["u"].as_str().unwrap_or_default())
        ))
    }

    /// Main-frame navigations and load events of the active tab so far (what `browser_wait` compares).
    pub(crate) fn tab_loads(&self) -> (u64, u64) {
        self.st.lock().unwrap().active_tab().map(|t| (t.navs, t.loads)).unwrap_or((0, 0))
    }

    pub(crate) fn inflight(&self) -> usize {
        self.st.lock().unwrap().inflight.len()
    }

    // ------------------------------------------------------------------ console and downloads

    pub(crate) fn console(&self, clear: bool) -> Vec<String> {
        let mut st = self.st.lock().unwrap();
        let Some(t) = st.active_tab() else { return vec![] };
        if clear {
            t.console.drain(..).collect()
        } else {
            t.console.iter().cloned().collect()
        }
    }

    pub(crate) fn downloads_len(&self) -> usize {
        self.st.lock().unwrap().downloads.len()
    }

    /// Waits (bounded) for the downloads from `since` on to finish, then describes them.
    pub(crate) async fn downloads_since(&self, since: usize, wait: Duration) -> Vec<String> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let running = self.st.lock().unwrap().downloads.iter().skip(since).any(|d| d.state == DlState::Running);
            if !running || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let st = self.st.lock().unwrap();
        st.downloads.iter().skip(since).map(describe_download).collect()
    }

    pub(crate) fn all_downloads(&self) -> Vec<String> {
        let st = self.st.lock().unwrap();
        st.downloads.iter().enumerate().map(|(i, d)| format!("[{}] {}", i + 1, describe_download(d))).collect()
    }

    /// Closes the browser. Whatever is left of the downloads folder goes with it.
    pub(crate) fn forget_downloads_tmp(&self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

fn describe_download(d: &Download) -> String {
    // Inside the workspace the path is shown relative and with `/` on every OS (the file tools accept it).
    let shown = |p: &Path| match p.strip_prefix(&d.ctx.workspace_root) {
        Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => p.display().to_string(),
    };
    match (&d.state, &d.path) {
        (DlState::Done, Some(p)) => format!("Scaricato «{}» ({}) in {}", d.name, human_size(d.bytes), shown(p)),
        (DlState::Done, None) => format!("Scaricato «{}»", d.name),
        (DlState::Running, _) => format!("Download in corso: «{}» da {}", d.name, clip(&d.url, 90)),
        (DlState::Failed(why), _) => format!("Download non riuscito: «{}» ({why})", d.name),
    }
}

/// A reference the model gives: the number of the elements snapshot or an `eN` of the accessibility one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RefArg {
    Num(u64),
    Ax(String),
}

impl RefArg {
    pub(crate) fn parse(v: &Value) -> Option<Self> {
        match v {
            Value::Number(n) => n.as_u64().map(RefArg::Num),
            Value::String(s) => {
                let t = s.trim().trim_matches(['[', ']']).trim();
                if let Ok(n) = t.parse::<u64>() {
                    return Some(RefArg::Num(n));
                }
                let digits = t.strip_prefix(['e', 'E'])?;
                (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then(|| RefArg::Ax(format!("e{digits}")))
            }
            _ => None,
        }
    }

    pub(crate) fn label(&self) -> String {
        match self {
            RefArg::Num(n) => n.to_string(),
            RefArg::Ax(a) => a.clone(),
        }
    }
}

// ---------------------------------------------------------------------- events

fn send(h: &Hub, session: Option<&str>, method: &str, params: Value) {
    send_command(&h.tx, &h.next, session, method, params);
}

/// Reacts to one event of the browser. Network policy first: a paused request is answered here.
fn on_event(h: &Hub, v: &Value) {
    let session = v["sessionId"].as_str();
    let p = &v["params"];
    match v["method"].as_str().unwrap_or_default() {
        "Fetch.requestPaused" => handle_paused(p.clone(), session.map(String::from), h.policy.clone(), h.tx.clone(), h.next.clone()),
        "Target.attachedToTarget" => on_attached(h, session, p),
        "Target.detachedFromTarget" => {
            if let Some(sid) = p["sessionId"].as_str() {
                remove_tabs(h, |t| t.session.as_deref() == Some(sid));
            }
        }
        "Target.targetDestroyed" => {
            if let Some(id) = p["targetId"].as_str() {
                remove_tabs(h, |t| t.target_id == id);
            }
        }
        "Target.targetInfoChanged" => {
            let info = &p["targetInfo"];
            let mut st = h.st.lock().unwrap();
            if let Some(t) = st.tabs.iter_mut().find(|t| t.target_id == info["targetId"].as_str().unwrap_or_default()) {
                t.title = info["title"].as_str().unwrap_or_default().to_string();
                t.url = info["url"].as_str().unwrap_or_default().to_string();
            }
        }
        "Page.javascriptDialogOpening" => {
            // A dialog stops the page until somebody answers. `alert` has only one way out and `beforeunload` is
            // accepted (the page is being left on purpose): both are answered at once. A `confirm` or `prompt` is a
            // real question (a click may be asking «delete it?»): it waits for the agent (`browser_dialog`) or the
            // person (live view), and is dismissed, the safe answer, if nobody answers in time.
            let ty = p["type"].as_str().unwrap_or("alert");
            let msg = clip(p["message"].as_str().unwrap_or_default(), 200);
            if matches!(ty, "alert" | "beforeunload") {
                send(h, session, "Page.handleJavaScriptDialog", json!({"accept": true}));
                h.st.lock().unwrap().note(Note::Text(format!("Finestra del sito ({ty}): «{msg}»: chiusa con OK automaticamente.")));
            } else if let Some(sid) = session {
                let (id, hold) = {
                    let mut st = h.st.lock().unwrap();
                    st.dialogs_opened += 1;
                    let id = st.dialogs_opened;
                    st.dialog = Some(PendingDialog {
                        session: sid.to_string(),
                        kind: ty.to_string(),
                        message: msg.clone(),
                        default_text: clip(p["defaultPrompt"].as_str().unwrap_or_default(), 200),
                        id,
                    });
                    (id, st.dialog_hold.unwrap_or(DIALOG_HOLD))
                };
                h.dialog_notify.notify_waiters();
                let h = h.clone();
                let sid = sid.to_string();
                let ty = ty.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(hold).await;
                    let still = {
                        let mut st = h.st.lock().unwrap();
                        let same = st.dialog.as_ref().is_some_and(|d| d.id == id);
                        if same {
                            st.dialog = None;
                            st.note(Note::Text(format!(
                                "Finestra del sito ({ty}): «{msg}»: annullata automaticamente, nessuno ha risposto."
                            )));
                        }
                        same
                    };
                    if still {
                        send(&h, Some(&sid), "Page.handleJavaScriptDialog", json!({"accept": false}));
                        h.dialog_notify.notify_waiters();
                    }
                });
            }
        }
        "Page.javascriptDialogClosed" => {
            // Answered by somebody else (the browser's own window, the page itself): nothing waits any more.
            if let Some(sid) = session {
                let mut st = h.st.lock().unwrap();
                if st.dialog.as_ref().is_some_and(|d| d.session == sid) {
                    st.dialog = None;
                }
            }
            h.dialog_notify.notify_waiters();
        }
        "Runtime.consoleAPICalled" => {
            let ty = p["type"].as_str().unwrap_or_default();
            if matches!(ty, "error" | "warning" | "assert") {
                let text: Vec<String> = p["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| match &x["value"] {
                                Value::String(s) => s.clone(),
                                Value::Null => x["description"].as_str().unwrap_or_default().to_string(),
                                other => other.to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let level = if ty == "warning" { "avviso" } else { "errore" };
                console_push(h, session, format!("{level}: {}", text.join(" ")));
            }
        }
        "Runtime.exceptionThrown" => {
            let d = &p["exceptionDetails"];
            let text = d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("eccezione");
            console_push(h, session, format!("eccezione: {}", text.lines().next().unwrap_or_default()));
        }
        "Log.entryAdded" => {
            let e = &p["entry"];
            let level = match e["level"].as_str() {
                Some("error") => "errore",
                Some("warning") => "avviso",
                _ => return,
            };
            let url = e["url"].as_str().filter(|u| !u.is_empty()).map(|u| format!(" ({})", clip(u, 80))).unwrap_or_default();
            console_push(h, session, format!("{level}: {}{url}", e["text"].as_str().unwrap_or_default()));
        }
        "Page.frameNavigated" => {
            if p["frame"].get("parentId").is_none() {
                if let Some(sid) = session {
                    let mut st = h.st.lock().unwrap();
                    if let Some(tab) = st.tab_by_session(sid) {
                        tab.navs += 1;
                        // The nodes of the old document are gone, and so are their references.
                        tab.refs.clear();
                    }
                }
            }
        }
        "Page.navigatedWithinDocument" => {
            if let Some(sid) = session {
                let mut st = h.st.lock().unwrap();
                if let Some(tab) = st.tab_by_session(sid) {
                    tab.navs += 1;
                    tab.loads += 1;
                }
            }
        }
        "Page.loadEventFired" => {
            if let Some(sid) = session {
                if let Some(tab) = h.st.lock().unwrap().tab_by_session(sid) {
                    tab.loads += 1;
                }
            }
        }
        "Network.requestWillBeSent" => {
            // Streams that never end do not count as work in progress.
            if !matches!(p["type"].as_str(), Some("EventSource" | "Ping" | "WebSocket")) {
                if let (Some(sid), Some(id)) = (session, p["requestId"].as_str()) {
                    h.st.lock().unwrap().inflight.insert(format!("{sid}:{id}"));
                }
            }
        }
        "Network.loadingFinished" | "Network.loadingFailed" => {
            if let (Some(sid), Some(id)) = (session, p["requestId"].as_str()) {
                h.st.lock().unwrap().inflight.remove(&format!("{sid}:{id}"));
            }
        }
        "Browser.downloadWillBegin" => download_begins(h, p),
        "Browser.downloadProgress" => download_progress(h, p),
        _ => {}
    }
}

fn console_push(h: &Hub, session: Option<&str>, line: String) {
    let Some(sid) = session else { return };
    let mut st = h.st.lock().unwrap();
    if let Some(t) = st.tab_by_session(sid) {
        if t.console.len() >= CONSOLE_CAP {
            t.console.pop_front();
        }
        t.console.push_back(clip(&line, 300));
    }
}

fn remove_tabs(h: &Hub, pred: impl Fn(&Tab) -> bool) {
    let mut st = h.st.lock().unwrap();
    let gone: Vec<(String, Option<String>)> =
        st.tabs.iter().filter(|t| pred(t)).map(|t| (t.target_id.clone(), t.session.clone())).collect();
    if gone.is_empty() {
        return;
    }
    st.tabs.retain(|t| !pred(t));
    for (id, session) in gone {
        if let Some(s) = session {
            if st.dialog.as_ref().is_some_and(|d| d.session == s) {
                st.dialog = None;
            }
            let prefix = format!("{s}:");
            st.inflight.retain(|k| !k.starts_with(&prefix));
        }
        if st.active.as_deref() == Some(&id) {
            st.active = st.tabs.last().map(|t| t.target_id.clone());
        }
    }
}

fn on_attached(h: &Hub, parent: Option<&str>, p: &Value) {
    let Some(sid) = p["sessionId"].as_str() else { return };
    let info = &p["targetInfo"];
    let ty = info["type"].as_str().unwrap_or_default();
    let target_id = info["targetId"].as_str().unwrap_or_default().to_string();
    let is_tab = ty == "page" && parent.is_none();
    if h.own_only && parent.is_none() {
        // Somebody's own Chrome: only the tabs this connection opened (or that its pages opened) are ours. Anything else that
        // attaches by itself (their tabs, workers, extensions) is let go at once, without a single command of ours.
        let mine = is_tab && {
            let mut st = h.st.lock().unwrap();
            let opener = info["openerId"].as_str().unwrap_or_default();
            let known = st.own.contains(&target_id) || (!opener.is_empty() && st.own.contains(opener));
            let fresh = !known && opener.is_empty() && st.expecting_own;
            if fresh {
                st.expecting_own = false;
            }
            if known || fresh {
                st.own.insert(target_id.clone());
            }
            known || fresh
        };
        if !mine {
            send(h, Some(sid), "Runtime.runIfWaitingForDebugger", json!({}));
            send(h, None, "Target.detachFromTarget", json!({"sessionId": sid}));
            return;
        }
    }
    if is_tab {
        let mut st = h.st.lock().unwrap();
        if st.tabs.iter().any(|t| t.target_id == target_id && t.session.is_some()) {
            // Attached twice (auto-attach and the explicit attach of the start-up): keep the first.
            drop(st);
            send(h, None, "Target.detachFromTarget", json!({"sessionId": sid}));
            return;
        }
        let by_site = info["openerId"].as_str().is_some_and(|o| !o.is_empty());
        if st.tabs.len() >= MAX_TABS {
            st.note(Note::Text(format!("Una pagina aperta dal sito è stata chiusa: ci sono già {MAX_TABS} schede (il massimo).")));
            drop(st);
            send(h, None, "Target.closeTarget", json!({"targetId": target_id}));
            return;
        }
        let tab = Tab {
            target_id: target_id.clone(),
            session: Some(sid.to_string()),
            title: info["title"].as_str().unwrap_or_default().to_string(),
            url: info["url"].as_str().unwrap_or_default().to_string(),
            by_site,
            ..Default::default()
        };
        match st.tabs.iter_mut().find(|t| t.target_id == target_id) {
            Some(t) => *t = tab,
            None => st.tabs.push(tab),
        }
        if st.active.is_none() || by_site {
            st.active = Some(target_id.clone());
        }
        if by_site {
            st.note(Note::NewTab(target_id));
        }
    }
    // Same setup for pages and for everything that hangs off them (frames, workers): the network
    // policy first, then the resume, so nothing runs before it is in place.
    let s = Some(sid);
    send(h, s, "Fetch.enable", fetch_patterns());
    send(h, s, "Target.setAutoAttach", json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true}));
    if matches!(ty, "page" | "iframe") {
        send(h, s, "Page.enable", json!({}));
        send(h, s, "Runtime.enable", json!({}));
        send(h, s, "Log.enable", json!({}));
        send(h, s, "Network.enable", json!({}));
    }
    send(h, s, "Runtime.runIfWaitingForDebugger", json!({}));
}

// ---------------------------------------------------------------------- downloads

fn cancel_download(h: &Hub, guid: &str) {
    send(h, None, "Browser.cancelDownload", json!({"guid": guid}));
    let _ = std::fs::remove_file(h.tmp.join(guid));
}

fn fail_download(h: &Hub, guid: &str, why: String) {
    cancel_download(h, guid);
    let mut st = h.st.lock().unwrap();
    if let Some(d) = st.downloads.iter_mut().find(|d| d.guid == guid) {
        d.state = DlState::Failed(why);
    }
}

fn download_begins(h: &Hub, p: &Value) {
    let guid = p["guid"].as_str().unwrap_or_default().to_string();
    let ctx = h.policy.ctx.lock().unwrap().clone();
    let name = sanitize_filename(p["suggestedFilename"].as_str().unwrap_or_default());
    let run = ctx.run_id.clone();
    let count = h.st.lock().unwrap().downloads.iter().filter(|d| d.ctx.run_id == run).count();
    h.st.lock().unwrap().downloads.push(Download {
        guid: guid.clone(),
        url: p["url"].as_str().unwrap_or_default().to_string(),
        name,
        state: DlState::Running,
        path: None,
        bytes: 0,
        ctx: ctx.clone(),
    });
    if h.policy.human.load(Ordering::SeqCst) {
        // What the person starts in the live view is not the run's doing: it would land in that run's workspace and
        // count against its limit, so it is not kept.
        fail_download(h, &guid, "non si scaricano file mentre l'utente guida il browser".into());
    } else if ctx.sandbox == crate::SandboxMode::ReadOnly {
        fail_download(
            h,
            &guid,
            "la sandbox è in sola lettura (o è attiva la modalità piano): serve il permesso di scrittura nel workspace".into(),
        );
    } else if count >= MAX_DOWNLOADS_PER_RUN {
        fail_download(h, &guid, format!("raggiunto il limite di {MAX_DOWNLOADS_PER_RUN} download per questo run"));
    }
}

fn download_progress(h: &Hub, p: &Value) {
    let guid = p["guid"].as_str().unwrap_or_default().to_string();
    let total = p["totalBytes"].as_u64().unwrap_or(0);
    let received = p["receivedBytes"].as_u64().unwrap_or(0);
    let running = h.st.lock().unwrap().downloads.iter().any(|d| d.guid == guid && d.state == DlState::Running);
    if !running {
        return;
    }
    match p["state"].as_str() {
        Some("completed") => {
            let h = h.clone();
            // The move can be a copy of a big file: not on the reader task.
            tokio::task::spawn_blocking(move || finalize_download(&h, &guid));
        }
        Some("canceled") => fail_download(h, &guid, "annullato dal browser".into()),
        _ => {
            if total > MAX_DOWNLOAD_BYTES || received > MAX_DOWNLOAD_BYTES {
                fail_download(h, &guid, format!("più grande del limite di {} MB", MAX_DOWNLOAD_BYTES >> 20));
            }
        }
    }
}

/// Moves a finished download from the browser's folder to `<workspace>/downloads/<run>/`.
fn finalize_download(h: &Hub, guid: &str) {
    let (ctx, name) = {
        let st = h.st.lock().unwrap();
        let Some(d) = st.downloads.iter().find(|d| d.guid == guid) else { return };
        (d.ctx.clone(), d.name.clone())
    };
    let outcome = (|| -> std::result::Result<(PathBuf, u64), String> {
        let src = h.tmp.join(guid);
        let size = std::fs::metadata(&src).map_err(|e| format!("file scaricato non trovato: {e}"))?.len();
        if size > MAX_DOWNLOAD_BYTES {
            let _ = std::fs::remove_file(&src);
            return Err(format!("più grande del limite di {} MB", MAX_DOWNLOAD_BYTES >> 20));
        }
        if ctx.sandbox == crate::SandboxMode::ReadOnly {
            let _ = std::fs::remove_file(&src);
            return Err("la sandbox è in sola lettura".into());
        }
        let dir = ctx.resolve(&format!("downloads/{}", safe_component(&ctx.run_id)), true).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let dest = unique_path(&dir, &name);
        // Resolved again with the final name: a symlink planted in the folder must not lead out.
        let dest = ctx.resolve(&dest.to_string_lossy(), true).map_err(|e| e.to_string())?;
        if std::fs::rename(&src, &dest).is_err() {
            crate::atomic::copy_atomic(&src, &dest).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&src);
        }
        ctx.record_created(&dest);
        Ok((dest, size))
    })();
    let mut st = h.st.lock().unwrap();
    if let Some(d) = st.downloads.iter_mut().find(|d| d.guid == guid) {
        match outcome {
            Ok((path, size)) => {
                d.state = DlState::Done;
                d.path = Some(path);
                d.bytes = size;
            }
            Err(why) => d.state = DlState::Failed(why),
        }
    }
}
