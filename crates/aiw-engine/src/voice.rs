//! Voice mode: speech to text through the best backend available here (whisper.cpp first, then a
//! local server that transcribes, then a paired device), configured under `voice.*`. Replies are
//! read aloud by the UI, with a neural engine of the engine ([`crate::tts`]) or the system's voices.

use crate::Engine;
use aiw_core::api::{LocalModel, RuntimeInfo, Transcript, VoiceStatus};
use aiw_core::{Error, Result};
use aiw_runtimes::stt::{self, Audio};
use serde_json::{Map, Value};
use std::sync::OnceLock;
use std::time::Instant;

/// Same cap as OpenAI's transcription API.
pub const MAX_AUDIO_BYTES: usize = 25 << 20;

/// Id of the platform speech recognizer (SFSpeechRecognizer on iOS/iPadOS), not a runtime.
pub const NATIVE_STT: &str = "native-stt";

/// Speech to text built into the operating system, registered by the app shell.
pub trait NativeStt: Send + Sync + 'static {
    /// Transcribes a WAV (blocking). `language`: BCP-47 hint, `None` = system language.
    fn transcribe(&self, wav: &[u8], language: Option<&str>) -> Result<String>;
}

/// Local servers (OpenAI API) that can transcribe when they are running.
const LOCAL_STT_SERVERS: &[&str] = &["localai", "nexa"];

#[derive(Debug, Clone, PartialEq)]
struct Choice {
    runtime: String,
    model: Option<String>,
    label: String,
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(reqwest::Client::new)
}

fn text<'a>(cfg: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    cfg.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

/// Picks the backend, given the runtimes' state, the installed speech models (largest first) and
/// the settings. Pure, so the order is testable.
fn choose(
    runtimes: &[RuntimeInfo],
    models: &[LocalModel],
    free_mb: u64,
    cfg: &Map<String, Value>,
    peer_configured: bool,
    native: bool,
) -> std::result::Result<Choice, String> {
    let wanted = text(cfg, "voice.stt_runtime").unwrap_or("auto");
    let rt = |id: &str| runtimes.iter().find(|r| r.id == id);
    let whisper_model = || {
        text(cfg, "voice.stt_model")
            .map(String::from)
            .or_else(|| models.iter().find(|m| m.size_bytes / (1 << 20) * 2 < free_mb).or(models.last()).map(|m| m.id.clone()))
    };
    let whisper = || -> std::result::Result<Choice, String> {
        let r = rt("whisper-server").ok_or("whisper-server non è nel catalogo dei runtime")?;
        let model = whisper_model();
        match r.status.as_str() {
            // Already serving (started by AIW or by the user): keep its model unless one is pinned.
            "running" => {
                let model = text(cfg, "voice.stt_model").map(String::from).or_else(|| r.loaded_models.first().cloned()).or(model);
                let label = model.as_ref().map(|m| format!("{} · {m}", r.name)).unwrap_or_else(|| r.name.clone());
                return Ok(Choice { runtime: r.id.clone(), model, label });
            }
            "available" => {}
            "unsupported" => return Err("whisper.cpp non è disponibile su questo sistema".into()),
            _ => return Err(format!("whisper.cpp non è installato: {}", r.install_hint.clone().unwrap_or_default())),
        }
        let model = model.ok_or("scarica un modello Whisper da Modelli (es. Whisper Small)")?;
        Ok(Choice { runtime: r.id.clone(), label: format!("{} · {model}", r.name), model: Some(model) })
    };
    let server = |id: &str| -> std::result::Result<Choice, String> {
        let r = rt(id).ok_or_else(|| format!("runtime sconosciuto: {id}"))?;
        if !r.capabilities.iter().any(|c| c == "stt") {
            return Err(format!("{} non trascrive l'audio", r.name));
        }
        match id {
            "aiw-peer" if !peer_configured => Err("nessun dispositivo accoppiato".into()),
            "aiw-peer" | "remote-openai" => Ok(Choice { runtime: id.into(), model: None, label: r.name.clone() }),
            _ if r.status == "running" => Ok(Choice { runtime: id.into(), model: None, label: r.name.clone() }),
            _ => Err(format!("{} non è in esecuzione", r.name)),
        }
    };
    let native_choice = || -> std::result::Result<Choice, String> {
        if native {
            Ok(Choice { runtime: NATIVE_STT.into(), model: None, label: "Riconoscimento vocale di sistema".into() })
        } else {
            Err("il riconoscimento vocale di sistema non è disponibile".into())
        }
    };
    if wanted == NATIVE_STT {
        return native_choice();
    }
    if wanted != "auto" {
        return if wanted == "whisper-server" { whisper() } else { server(wanted) };
    }
    let mut why = Vec::new();
    match whisper() {
        Ok(c) => return Ok(c),
        Err(e) => why.push(e),
    }
    if let Ok(c) = native_choice() {
        return Ok(c);
    }
    for id in LOCAL_STT_SERVERS {
        if let Ok(c) = server(id) {
            return Ok(c);
        }
    }
    if let Ok(c) = server("aiw-peer") {
        return Ok(c);
    }
    why.push("in alternativa avvia LocalAI o accoppia un dispositivo che trascrive".into());
    Err(why.join("; "))
}

impl Engine {
    /// Installed speech-to-text models, largest first.
    pub fn stt_models(&self) -> Vec<LocalModel> {
        let mut v: Vec<LocalModel> =
            self.0.models.read().unwrap().iter().filter(|m| m.capabilities.iter().any(|c| c == "stt")).cloned().collect();
        v.sort_by_key(|m| std::cmp::Reverse(m.size_bytes));
        v
    }

    /// `allow_peer = false` for requests that come from another device: forwarding them to a
    /// paired device could bounce audio back and forth between two devices without whisper.
    fn stt_choice(&self, allow_peer: bool) -> std::result::Result<Choice, String> {
        let cfg = self.config().effective;
        let peer =
            allow_peer && self.0.runtimes.settings("aiw-peer").ok().and_then(|s| text(&s, "base_url").map(|_| true)).unwrap_or(false);
        choose(
            &self.0.runtimes.list(),
            &self.stt_models(),
            self.device().available_memory_mb,
            &cfg,
            peer,
            self.0.native_stt.read().unwrap().is_some(),
        )
    }

    /// Registers the operating system's speech recognizer (iOS/iPadOS).
    pub fn register_native_stt(&self, stt: std::sync::Arc<dyn NativeStt>) {
        *self.0.native_stt.write().unwrap() = Some(stt);
    }

    pub fn voice_status(&self) -> VoiceStatus {
        let cfg = self.config().effective;
        let choice = self.stt_choice(true);
        VoiceStatus {
            available: choice.is_ok(),
            runtime: choice.as_ref().ok().map(|c| c.runtime.clone()),
            model: choice.as_ref().ok().and_then(|c| c.model.clone()),
            backend: choice.as_ref().ok().map(|c| c.label.clone()),
            detail: choice.err(),
            language: text(&cfg, "voice.language").unwrap_or("auto").to_string(),
            tts_voice: text(&cfg, "voice.tts_voice").unwrap_or_default().to_string(),
            models: self.stt_models().into_iter().map(|m| m.id).collect(),
            tts: self.tts_status(),
        }
    }

    /// Transcribes `audio` (WAV from the app's microphone; other formats only for OpenAI-compatible
    /// backends). `language` overrides `voice.language`.
    pub async fn transcribe(&self, audio: &[u8], mime: &str, language: Option<&str>) -> Result<Transcript> {
        self.transcribe_with(audio, mime, language, true).await
    }

    /// Like [`Engine::transcribe`], for requests from other devices: never forwarded to a peer.
    pub async fn transcribe_for_peer(&self, audio: &[u8], mime: &str, language: Option<&str>) -> Result<Transcript> {
        self.transcribe_with(audio, mime, language, false).await
    }

    async fn transcribe_with(&self, audio: &[u8], mime: &str, language: Option<&str>, allow_peer: bool) -> Result<Transcript> {
        if audio.is_empty() {
            return Err(Error::Invalid("nessun audio ricevuto".into()));
        }
        if audio.len() > MAX_AUDIO_BYTES {
            return Err(Error::Invalid(format!("audio troppo lungo: massimo {} MB", MAX_AUDIO_BYTES >> 20)));
        }
        let choice = self.stt_choice(allow_peer).map_err(|why| Error::Unsupported(format!("trascrizione non disponibile: {why}")))?;
        let cfg = self.config().effective;
        let language = language.map(str::trim).filter(|l| !l.is_empty()).or_else(|| text(&cfg, "voice.language")).map(String::from);
        let started = Instant::now();
        let out = if choice.runtime == NATIVE_STT {
            let native = self
                .0
                .native_stt
                .read()
                .unwrap()
                .clone()
                .ok_or_else(|| Error::Unsupported("riconoscimento vocale di sistema non registrato".into()))?;
            let (bytes, lang) = (audio.to_vec(), language.clone());
            let text = tokio::task::spawn_blocking(move || native.transcribe(&bytes, lang.as_deref().filter(|l| *l != "auto")))
                .await
                .map_err(|e| Error::Runtime(e.to_string()))??;
            stt::Transcription { text, language: None }
        } else {
            let target = self.0.runtimes.stt_target(&choice.runtime, choice.model.as_deref()).await?;
            stt::transcribe(http(), &target, &Audio { bytes: audio, mime, language: language.as_deref(), prompt: None }).await?
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let duration_ms = stt::wav_info(audio).map(|w| w.duration_ms);
        tracing::info!(backend = %choice.label, ?duration_ms, elapsed_ms, "trascrizione completata");
        Ok(Transcript {
            text: out.text,
            language: out.language.or(language.filter(|l| l != "auto")),
            backend: choice.label,
            duration_ms,
            elapsed_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rt(id: &str, status: &str, caps: &[&str]) -> RuntimeInfo {
        serde_json::from_value(json!({
            "id": id, "name": id, "vendor": "", "kind": "sidecar", "description": "", "platforms": [], "accelerators": [],
            "formats": [], "api": "openai", "capabilities": caps, "status": status, "loaded_models": [],
            "settings_schema": {"type": "object", "properties": {}}, "settings": {}, "install_hint": "brew install whisper-cpp"
        }))
        .unwrap()
    }

    fn model(id: &str, mb: u64) -> LocalModel {
        serde_json::from_value(json!({"id": id, "name": id, "format": "ggml", "size_bytes": mb << 20, "capabilities": ["stt"], "runtimes": ["whisper-server"]}))
            .unwrap()
    }

    fn cfg(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn picks_the_best_available_backend() {
        let models = [model("ggml-large-v3-turbo-q5_0", 547), model("ggml-small-q8_0", 252)];
        let all = |whisper: &str, localai: &str| {
            vec![
                rt("whisper-server", whisper, &["stt"]),
                rt("localai", localai, &["chat", "stt"]),
                rt("aiw-peer", "stopped", &["chat", "stt"]),
            ]
        };
        // whisper.cpp with the largest model that fits.
        let c = choose(&all("available", "running"), &models, 8000, &cfg(json!({})), true, false).unwrap();
        assert_eq!((c.runtime.as_str(), c.model.as_deref()), ("whisper-server", Some("ggml-large-v3-turbo-q5_0")));
        let c = choose(&all("available", "stopped"), &models, 800, &cfg(json!({})), false, false).unwrap();
        assert_eq!(c.model.as_deref(), Some("ggml-small-q8_0"), "little memory: the smaller model");
        // Without whisper.cpp: a running local server, then the paired device.
        assert_eq!(choose(&all("installable", "running"), &models, 8000, &cfg(json!({})), true, false).unwrap().runtime, "localai");
        assert_eq!(choose(&all("installable", "stopped"), &models, 8000, &cfg(json!({})), true, false).unwrap().runtime, "aiw-peer");
        let err = choose(&all("installable", "stopped"), &models, 8000, &cfg(json!({})), false, false).unwrap_err();
        assert!(err.contains("brew install whisper-cpp") && err.contains("LocalAI"), "{err}");
        // No speech model installed: fine for a server that is already running, not for one to start.
        assert!(choose(&all("available", "stopped"), &[], 8000, &cfg(json!({})), false, false).unwrap_err().contains("Whisper"));
        let running = choose(&all("running", "stopped"), &[], 8000, &cfg(json!({})), false, false).unwrap();
        assert_eq!((running.runtime.as_str(), running.model), ("whisper-server", None));
        // iOS: whisper.cpp is unsupported, the system recognizer comes before servers and peers.
        let c = choose(&all("unsupported", "running"), &models, 8000, &cfg(json!({})), true, true).unwrap();
        assert_eq!(c.runtime, NATIVE_STT);
        // Explicit choices win.
        let pinned = cfg(json!({"voice.stt_runtime": "whisper-server", "voice.stt_model": "ggml-small-q8_0"}));
        assert_eq!(
            choose(&all("available", "running"), &models, 8000, &pinned, true, false).unwrap().model.as_deref(),
            Some("ggml-small-q8_0")
        );
        assert!(choose(&all("available", "stopped"), &models, 8000, &cfg(json!({"voice.stt_runtime": "localai"})), true, false).is_err());
    }
}
