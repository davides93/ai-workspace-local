//! The live view of the controlled browser: what the person sees (a frame of the page, the state) and what
//! they can do (pause the agent, take the browser, type and click in the page, give it back).
//!
//! Everything here reaches the browser through the connection kept in `BrowserManager::live` and never
//! through `with()`: an action of the agent holds that lock for as long as it runs, and the person must be
//! able to look at the page meanwhile. Nothing here starts a browser: with none running, the frame is
//! `None` and the state says so.
//!
//! **Nothing of what goes through here is kept.** A frame is returned to the caller and forgotten; the keys and
//! the text the person types go to the page and nowhere else (no log, no event, no audit row: the callers
//! write the transitions of the mode, never the content). Input is accepted only while the person drives and
//! no action of the agent is in flight (`Control::human_begin`).

use super::control::ControlState;
use super::BrowserManager;
use aiw_core::{Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

/// Longest the live view waits for the browser (a page that does not answer must not hold the poll).
const LIVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Widest frame sent (CSS pixels of the page are scaled down to this; a phone shows it smaller still).
const FRAME_MAX_WIDTH: f64 = 1000.0;
/// JPEG quality of a frame.
const FRAME_QUALITY: u32 = 55;
/// Longest text the person can insert in one go.
pub const MAX_TEXT_CHARS: usize = 512;

/// The page in front, as little as the person needs to know where they are: no address, no query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PageInfo {
    pub title: String,
    /// `example.com`, never the path or the query.
    pub host: String,
    pub secure: bool,
}

/// A window of the site (`alert`, `confirm`, `prompt`, `beforeunload`) that waits for an answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DialogInfo {
    pub kind: String,
    pub message: String,
    /// What a `prompt` has in its field to begin with.
    pub default_text: String,
}

/// What the clients may know about the browser. The same for the agent's actions and the person's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LiveState {
    /// A browser is running (the frame and the controls exist only then).
    pub running: bool,
    /// The browser has no window of its own: the live view is the only way to see it.
    pub headless: bool,
    #[serde(flatten)]
    pub control: ControlState,
    pub page: Option<PageInfo>,
    pub dialog: Option<DialogInfo>,
    /// In words, the control that has the focus in the page (`campo password «Chiave»`): for who cannot see the frame.
    /// Never a value. Only while the person drives (see [`BrowserManager::live_state_with_focus`]).
    pub focus: Option<String>,
}

/// One frame of the page: JPEG bytes and the size of the viewport in CSS pixels it shows.
pub struct Frame {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// What the person does in the page. Positions are fractions (0 to 1) of the frame.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HumanInput {
    Click {
        x: f64,
        y: f64,
        #[serde(default)]
        button: Option<String>,
        #[serde(default)]
        count: Option<u32>,
    },
    Scroll {
        x: f64,
        y: f64,
        dy: f64,
    },
    /// A key: a named one (`Enter`, `Backspace`, `Tab`, `Escape`, the arrows, `Delete`, `Home`, `End`, `PageUp`,
    /// `PageDown`) or one printable character. `shift` only: no shortcut of the browser can be sent.
    Key {
        key: String,
        #[serde(default)]
        shift: bool,
    },
    /// Text inserted where the focus is (a password manager, a virtual keyboard, a pasted code).
    Text {
        text: String,
    },
}

/// `(name, windowsVirtualKeyCode)` of the keys the person may send besides one printable character.
const NAMED_KEYS: &[(&str, u32)] = &[
    ("Enter", 13),
    ("Backspace", 8),
    ("Tab", 9),
    ("Escape", 27),
    ("ArrowLeft", 37),
    ("ArrowUp", 38),
    ("ArrowRight", 39),
    ("ArrowDown", 40),
    ("Delete", 46),
    ("Home", 36),
    ("End", 35),
    ("PageUp", 33),
    ("PageDown", 34),
];

/// Which control has the focus, in words: its kind and its label (never what is typed in it).
const FOCUS_JS: &str = r#"(() => {
  const el = document.activeElement;
  if (!el || el === document.body || el === document.documentElement) return '';
  const tag = el.tagName.toLowerCase(), type = (el.getAttribute('type') || '').toLowerCase();
  const button = tag === 'button' || (tag === 'input' && (type === 'submit' || type === 'button'));
  const kind = tag === 'input' ? (type === 'password' ? 'campo password' : type === 'checkbox' ? 'casella' : type === 'radio' ? 'opzione' : button ? 'pulsante' : 'campo di testo')
    : tag === 'textarea' ? 'area di testo' : tag === 'select' ? 'menu a tendina' : tag === 'button' ? 'pulsante' : tag === 'a' ? 'collegamento' : (el.getAttribute('role') || tag);
  const label = (el.getAttribute('aria-label') || (el.labels && el.labels[0] ? el.labels[0].innerText : '') || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('name')
    || (button ? (tag === 'input' ? el.value : el.innerText) : '') || (tag === 'a' ? el.innerText : '') || '').trim().replace(/\s+/g, ' ').slice(0, 60);
  return label ? kind + ' «' + label + '»' : kind;
})()"#;

fn host_of(url: &str) -> Option<(String, bool)> {
    let u = reqwest::Url::parse(url).ok()?;
    Some((u.host_str()?.to_string(), u.scheme() == "https"))
}

fn live_err(e: tokio::time::error::Elapsed) -> Error {
    let _ = e;
    Error::Runtime("il browser non risponde".into())
}

impl BrowserManager {
    /// The connection of the running browser, if there is one (never starts it).
    pub(crate) fn live_cdp(&self) -> Option<std::sync::Arc<super::cdp::Cdp>> {
        self.live.lock().unwrap().clone()
    }

    /// The run whose action used the browser last (the live events are attached to it).
    pub fn last_run(&self) -> Option<String> {
        self.last_run.lock().unwrap().clone()
    }

    pub fn live_state(&self) -> LiveState {
        let cdp = self.live_cdp();
        let page = cdp
            .as_ref()
            .and_then(|c| c.active_page())
            .and_then(|(title, url)| host_of(&url).map(|(host, secure)| PageInfo { title: super::cdp::clip(&title, 120), host, secure }));
        let dialog = cdp.as_ref().and_then(|c| c.pending_dialog());
        LiveState { running: cdp.is_some(), headless: self.headless, control: self.control.state(), page, dialog, focus: None }
    }

    /// [`BrowserManager::live_state`] and, while the person drives, which control has the focus (read from the page; never
    /// its value). The page is not asked when a window of the site blocks it or when the agent is mid-action.
    pub async fn live_state_with_focus(&self) -> LiveState {
        let mut state = self.live_state();
        if state.running && state.control.mode == super::control::Mode::Human && !state.control.agent_busy && state.dialog.is_none() {
            if let Some(cdp) = self.live_cdp() {
                let r = tokio::time::timeout(LIVE_TIMEOUT, cdp.eval(FOCUS_JS)).await;
                state.focus = r.ok().and_then(|r| r.ok()).and_then(|v| v.as_str().map(String::from)).filter(|t| !t.is_empty());
            }
        }
        state
    }

    /// A frame of the page in front. `None` when no browser runs. While a window of the site waits for an answer
    /// the page cannot be captured: that is a `Conflict`, and the state carries the window.
    pub async fn frame(&self) -> Result<Option<Frame>> {
        let Some(cdp) = self.live_cdp() else { return Ok(None) };
        if cdp.pending_dialog().is_some() {
            return Err(Error::Conflict("una finestra del sito aspetta una risposta".into()));
        }
        let shot = tokio::time::timeout(LIVE_TIMEOUT, async {
            let metrics = cdp.call("Page.getLayoutMetrics", json!({})).await?;
            let vp = &metrics["cssVisualViewport"];
            let (w, h) = (vp["clientWidth"].as_f64().unwrap_or(0.0), vp["clientHeight"].as_f64().unwrap_or(0.0));
            let mut params = json!({"format": "jpeg", "quality": FRAME_QUALITY, "optimizeForSpeed": true});
            if w >= 1.0 && h >= 1.0 {
                params["clip"] = json!({
                    "x": vp["pageX"].as_f64().unwrap_or(0.0), "y": vp["pageY"].as_f64().unwrap_or(0.0),
                    "width": w, "height": h, "scale": (FRAME_MAX_WIDTH / w).min(1.0)});
            }
            let r = cdp.call("Page.captureScreenshot", params).await?;
            Ok::<_, Error>((r, w, h))
        })
        .await
        .map_err(live_err)?;
        let (r, w, h) = match shot {
            Ok(v) => v,
            Err(Error::Runtime(m)) if m.contains("nessuna scheda attiva") => return Ok(None),
            Err(e) => return Err(e),
        };
        let data = r["data"].as_str().ok_or_else(|| Error::Runtime("il browser non ha dato l'immagine".into()))?;
        let jpeg = base64::engine::general_purpose::STANDARD.decode(data).map_err(Error::other)?;
        Ok(Some(Frame { jpeg, width: w.round() as u32, height: h.round() as u32 }))
    }

    /// The person takes the browser. A headed browser also comes to the front, so the real window (a password
    /// manager, a passkey, a security key) is an option; the live view works in both.
    pub async fn take_control(&self) -> Result<ControlState> {
        let cdp = self
            .live_cdp()
            .ok_or_else(|| Error::Conflict("il browser non è attivo: nessuna azione dell'agente lo ha ancora aperto".into()))?;
        self.control.take();
        if !self.headless {
            let _ = tokio::time::timeout(LIVE_TIMEOUT, cdp.call("Page.bringToFront", json!({}))).await;
        }
        Ok(self.control.state())
    }

    /// Pauses the agent (the browser need not be running: the next action waits).
    pub fn pause(&self) -> ControlState {
        self.control.pause();
        self.control.state()
    }

    /// Lets the agent go on after a pause.
    pub fn resume(&self) -> ControlState {
        self.control.resume();
        self.control.state()
    }

    /// The browser goes back to the agent, which will look at the page again before it acts on an element.
    pub fn give_back(&self) -> ControlState {
        self.control.give_back();
        self.control.state()
    }

    /// The person clicks, scrolls or types in the page (only while they drive and the agent is not mid-action).
    pub async fn human_input(&self, input: HumanInput) -> Result<()> {
        let _turn = self.control.human_begin().map_err(|m| Error::Conflict(m.into()))?;
        let cdp = self.live_cdp().ok_or_else(|| Error::Conflict("il browser non è attivo".into()))?;
        if cdp.pending_dialog().is_some() {
            return Err(Error::Conflict("una finestra del sito aspetta una risposta: rispondi prima a quella".into()));
        }
        tokio::time::timeout(LIVE_TIMEOUT, send_input(&cdp, input)).await.map_err(live_err)?
    }

    /// The person answers the window of the site (they drive).
    pub async fn answer_dialog(&self, accept: bool, text: Option<String>) -> Result<()> {
        let _turn = self.control.human_begin().map_err(|m| Error::Conflict(m.into()))?;
        let cdp = self.live_cdp().ok_or_else(|| Error::Conflict("il browser non è attivo".into()))?;
        tokio::time::timeout(LIVE_TIMEOUT, cdp.answer_dialog(accept, text)).await.map_err(live_err)?
    }
}

fn frac(v: f64) -> Result<f64> {
    if v.is_finite() && (0.0..=1.0).contains(&v) {
        Ok(v)
    } else {
        Err(Error::Invalid("la posizione va da 0 a 1".into()))
    }
}

async fn send_input(cdp: &super::cdp::Cdp, input: HumanInput) -> Result<()> {
    match input {
        HumanInput::Click { x, y, button, count } => {
            let (x, y) = viewport_point(cdp, frac(x)?, frac(y)?).await?;
            let button = match button.as_deref().unwrap_or("left") {
                "left" => "left",
                "right" => "right",
                "middle" => "middle",
                other => return Err(Error::Invalid(format!("pulsante non valido: {other}"))),
            };
            let count = count.unwrap_or(1).clamp(1, 2);
            cdp.call("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": x, "y": y})).await?;
            cdp.call("Input.dispatchMouseEvent", json!({"type": "mousePressed", "x": x, "y": y, "button": button, "clickCount": count}))
                .await?;
            cdp.call("Input.dispatchMouseEvent", json!({"type": "mouseReleased", "x": x, "y": y, "button": button, "clickCount": count}))
                .await?;
        }
        HumanInput::Scroll { x, y, dy } => {
            let (x, y) = viewport_point(cdp, frac(x)?, frac(y)?).await?;
            if !dy.is_finite() {
                return Err(Error::Invalid("lo scorrimento non è un numero".into()));
            }
            cdp.call(
                "Input.dispatchMouseEvent",
                json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": dy.clamp(-2000.0, 2000.0)}),
            )
            .await?;
        }
        HumanInput::Key { key, shift } => {
            let mods = if shift { 8 } else { 0 };
            if let Some((name, code)) = NAMED_KEYS.iter().find(|(n, _)| *n == key) {
                let text = if *name == "Enter" { Some("\r") } else { None };
                let mut down = json!({"type": if text.is_some() { "keyDown" } else { "rawKeyDown" }, "key": name, "windowsVirtualKeyCode": code, "modifiers": mods});
                if let Some(t) = text {
                    down["text"] = json!(t);
                }
                cdp.call("Input.dispatchKeyEvent", down).await?;
                cdp.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": name, "windowsVirtualKeyCode": code, "modifiers": mods}))
                    .await?;
            } else if key.chars().count() == 1 && !key.chars().any(char::is_control) {
                cdp.call(
                    "Input.dispatchKeyEvent",
                    json!({"type": "keyDown", "key": key, "text": key, "unmodifiedText": key, "modifiers": mods}),
                )
                .await?;
                cdp.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": key, "modifiers": mods})).await?;
            } else {
                return Err(Error::Invalid("tasto non consentito".into()));
            }
        }
        HumanInput::Text { text } => {
            if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
                return Err(Error::Invalid(format!("il testo va da 1 a {MAX_TEXT_CHARS} caratteri")));
            }
            cdp.call("Input.insertText", json!({"text": text})).await?;
        }
    }
    Ok(())
}

/// A point of the frame (fractions) as a point of the viewport, in the CSS pixels the browser's input takes.
/// The scale of the display does not enter: the frame is the viewport, whatever the pixels behind it.
async fn viewport_point(cdp: &super::cdp::Cdp, fx: f64, fy: f64) -> Result<(f64, f64)> {
    let metrics = cdp.call("Page.getLayoutMetrics", json!({})).await?;
    let vp = &metrics["cssVisualViewport"];
    let (w, h) = (vp["clientWidth"].as_f64().unwrap_or(0.0), vp["clientHeight"].as_f64().unwrap_or(0.0));
    if w < 1.0 || h < 1.0 {
        return Err(Error::Runtime("il browser non ha detto che dimensioni ha la pagina".into()));
    }
    Ok(((fx * w).min(w - 1.0).max(0.0), (fy * h).min(h - 1.0).max(0.0)))
}
