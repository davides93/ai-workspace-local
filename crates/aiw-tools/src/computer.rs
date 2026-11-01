//! Desktop computer-use (pattern P19): screenshot + mouse/keyboard. Built with the
//! `computer-use` feature (xcap + enigo); otherwise the tools are listed as unavailable.
//! Coordinates are expressed in the space of the last screenshot (scaled to ≤ 1280 px wide).

use crate::{Risk, Tool, ToolContext, ToolOutput};
use aiw_core::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Op {
    Screenshot,
    Click,
    Type,
    Key,
    Scroll,
}

pub fn tools() -> Vec<Arc<dyn Tool>> {
    [Op::Screenshot, Op::Click, Op::Type, Op::Key, Op::Scroll]
        .into_iter()
        .map(|op| Arc::new(ComputerTool { op }) as Arc<dyn Tool>)
        .collect()
}

struct ComputerTool {
    op: Op,
}

#[async_trait]
impl Tool for ComputerTool {
    fn name(&self) -> &str {
        match self.op {
            Op::Screenshot => "computer_screenshot",
            Op::Click => "computer_click",
            Op::Type => "computer_type",
            Op::Key => "computer_key",
            Op::Scroll => "computer_scroll",
        }
    }
    fn description(&self) -> &str {
        match self.op {
            Op::Screenshot => {
                "Cattura lo schermo principale (immagine). Le coordinate delle altre azioni si riferiscono a questa immagine."
            }
            Op::Click => "Clic del mouse alle coordinate x,y dell'ultimo screenshot (button: left|right|middle, double: bool).",
            Op::Type => "Digita del testo nella finestra attiva.",
            Op::Key => "Preme una combinazione di tasti, es. ctrl+c, cmd+space, enter, alt+tab.",
            Op::Scroll => "Scorre di dx,dy tacche (dy positivo = giù).",
        }
    }
    fn parameters(&self) -> Value {
        match self.op {
            Op::Screenshot => json!({"type":"object","properties":{}}),
            Op::Click => {
                json!({"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"},"button":{"type":"string","enum":["left","right","middle"]},"double":{"type":"boolean"}},"required":["x","y"]})
            }
            Op::Type => json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            Op::Key => json!({"type":"object","properties":{"keys":{"type":"string"}},"required":["keys"]}),
            Op::Scroll => json!({"type":"object","properties":{"dx":{"type":"integer"},"dy":{"type":"integer"}}}),
        }
    }
    fn risk(&self) -> Risk {
        match self.op {
            Op::Screenshot => Risk::Read,
            _ => Risk::Computer,
        }
    }
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("text").or_else(|| args.get("keys")).and_then(|v| v.as_str()).map(String::from)
    }
    fn available(&self) -> std::result::Result<(), String> {
        imp::available()
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let op = self.op;
        let cancel = ctx.cancel.clone();
        let task = tokio::task::spawn_blocking(move || imp::run(op_name(op), &args));
        tokio::select! {
            r = task => r.map_err(aiw_core::Error::other)?,
            _ = cancel.cancelled() => Err(aiw_core::Error::Cancelled),
        }
    }
}

fn op_name(op: Op) -> &'static str {
    match op {
        Op::Screenshot => "screenshot",
        Op::Click => "click",
        Op::Type => "type",
        Op::Key => "key",
        Op::Scroll => "scroll",
    }
}

#[cfg(not(feature = "computer-use"))]
mod imp {
    use super::*;
    pub fn available() -> std::result::Result<(), String> {
        Err("build senza la feature `computer-use` (attiva nelle app desktop)".into())
    }
    pub fn run(_op: &str, _args: &Value) -> Result<ToolOutput> {
        Err(aiw_core::Error::Unsupported("computer-use non incluso in questa build".into()))
    }
}

#[cfg(feature = "computer-use")]
mod imp {
    use super::*;
    use aiw_core::Error;
    use base64::Engine;
    use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
    use std::sync::Mutex;

    /// Scale between screenshot pixels and screen pixels, set by the last screenshot.
    static SCALE: Mutex<f64> = Mutex::new(1.0);
    const MAX_W: u32 = 1280;

    pub fn available() -> std::result::Result<(), String> {
        if cfg!(any(target_os = "ios", target_os = "android")) {
            return Err("su mobile il computer-use passa dal plugin nativo (AccessibilityService)".into());
        }
        Ok(())
    }

    fn enigo() -> Result<Enigo> {
        Enigo::new(&Settings::default()).map_err(|e| Error::Runtime(format!("input non disponibile: {e}")))
    }

    fn key_of(name: &str) -> Option<Key> {
        Some(match name.to_lowercase().as_str() {
            "ctrl" | "control" => Key::Control,
            "alt" | "option" => Key::Alt,
            "shift" => Key::Shift,
            "cmd" | "command" | "meta" | "win" | "super" => Key::Meta,
            "enter" | "return" => Key::Return,
            "tab" => Key::Tab,
            "esc" | "escape" => Key::Escape,
            "space" => Key::Space,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
            _ => return None,
        })
    }

    pub fn run(op: &str, args: &Value) -> Result<ToolOutput> {
        match op {
            "screenshot" => {
                let monitors = xcap::Monitor::all().map_err(|e| Error::Runtime(format!("cattura schermo: {e}")))?;
                let m = monitors
                    .into_iter()
                    .find(|m| m.is_primary().unwrap_or(false))
                    .ok_or_else(|| Error::Runtime("nessun monitor".into()))?;
                let img = m.capture_image().map_err(|e| Error::Runtime(format!("cattura schermo: {e}")))?;
                let (w, h) = (img.width(), img.height());
                let img = if w > MAX_W {
                    image::imageops::resize(&img, MAX_W, h * MAX_W / w, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                *SCALE.lock().unwrap() = w as f64 / img.width() as f64;
                let mut png = Vec::new();
                image::DynamicImage::ImageRgba8(img.clone())
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .map_err(Error::other)?;
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(ToolOutput {
                    content: format!("Screenshot {}x{} (schermo reale {w}x{h}).", img.width(), img.height()),
                    images: vec![format!("data:image/png;base64,{data}")],
                    is_error: false,
                })
            }
            "click" => {
                let s = *SCALE.lock().unwrap();
                let x = (args["x"].as_f64().unwrap_or(0.0) * s).round() as i32;
                let y = (args["y"].as_f64().unwrap_or(0.0) * s).round() as i32;
                let button = match args["button"].as_str().unwrap_or("left") {
                    "right" => Button::Right,
                    "middle" => Button::Middle,
                    _ => Button::Left,
                };
                let mut e = enigo()?;
                e.move_mouse(x, y, Coordinate::Abs).map_err(Error::other)?;
                e.button(button, Direction::Click).map_err(Error::other)?;
                if args["double"].as_bool().unwrap_or(false) {
                    e.button(button, Direction::Click).map_err(Error::other)?;
                }
                Ok(ToolOutput::text(format!("Clic a ({x},{y}) sullo schermo.")))
            }
            "type" => {
                let text = args["text"].as_str().unwrap_or_default();
                enigo()?.text(text).map_err(Error::other)?;
                Ok(ToolOutput::text(format!("Digitati {} caratteri.", text.chars().count())))
            }
            "key" => {
                let combo = args["keys"].as_str().unwrap_or_default();
                let keys: Vec<Key> = combo
                    .split('+')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(|k| key_of(k).ok_or_else(|| Error::Invalid(format!("tasto sconosciuto: {k}"))))
                    .collect::<Result<_>>()?;
                let mut e = enigo()?;
                for k in &keys[..keys.len().saturating_sub(1)] {
                    e.key(*k, Direction::Press).map_err(Error::other)?;
                }
                if let Some(last) = keys.last() {
                    e.key(*last, Direction::Click).map_err(Error::other)?;
                }
                for k in keys[..keys.len().saturating_sub(1)].iter().rev() {
                    e.key(*k, Direction::Release).map_err(Error::other)?;
                }
                Ok(ToolOutput::text(format!("Premuto {combo}.")))
            }
            "scroll" => {
                let mut e = enigo()?;
                let dx = args["dx"].as_i64().unwrap_or(0) as i32;
                let dy = args["dy"].as_i64().unwrap_or(0) as i32;
                if dy != 0 {
                    e.scroll(dy, Axis::Vertical).map_err(Error::other)?;
                }
                if dx != 0 {
                    e.scroll(dx, Axis::Horizontal).map_err(Error::other)?;
                }
                Ok(ToolOutput::text(format!("Scroll ({dx},{dy}).")))
            }
            other => Err(Error::Invalid(format!("operazione sconosciuta {other}"))),
        }
    }
}
