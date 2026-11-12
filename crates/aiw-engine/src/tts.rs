//! Neural text to speech: reads replies aloud with Piper (a local executable and a `.onnx` voice)
//! or with an OpenAI-compatible speech server (`POST {base}/v1/audio/speech`, which is how Kokoro
//! is supported). The mirror image of [`crate::voice`]: same `voice.*` settings, same status shape.
//!
//! Selection (`voice.tts.engine`):
//!
//! | setting  | what speaks                                                                       |
//! |----------|-----------------------------------------------------------------------------------|
//! | `auto`   | Piper when its executable and a voice for the reply language are installed; else   |
//! |          | the speech server when its address is set; else the system voices (the UI's job). |
//! | `system` | always the voices of the operating system, spoken by the app.                      |
//! | `piper`  | Piper only; an error says what is missing.                                         |
//! | `openai` | the speech server only; an error says what is missing.                             |
//!
//! The text is cut into sentences, synthesised one at a time (a request holds the session's gate,
//! so two requests never synthesise together) and the WAVs are joined. Dropping the request, or
//! cancelling its token, kills the running Piper process.

use crate::Engine;
use aiw_core::api::{LocalModel, TtsStatus, TtsVoice};
use aiw_core::{Error, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Most characters of one request.
pub const MAX_CHARS: usize = 4000;
/// A sentence longer than this is cut at a comma or a space.
const MAX_SENTENCE: usize = 400;
/// Time allowed to synthesise one sentence.
const SYNTH_TIMEOUT: Duration = Duration::from_secs(120);

/// What the speech server is asked for when no voice is chosen (Kokoro voice names).
fn default_server_voice(lang: Option<&str>) -> &'static str {
    match lang {
        Some("it") => "if_sara",
        _ => "af_heart",
    }
}

/// Voices a Kokoro server offers, suggested in the picker (a server can offer others).
const SERVER_VOICES: &[(&str, &str, &str)] = &[
    ("if_sara", "Sara (Kokoro)", "it"),
    ("im_nicola", "Nicola (Kokoro)", "it"),
    ("af_heart", "Heart (Kokoro)", "en"),
    ("am_michael", "Michael (Kokoro)", "en"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct PiperVoice {
    pub id: String,
    pub name: String,
    /// ISO 639-1 code from the voice id (`it_IT-paola-medium` → `it`).
    pub language: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub base: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TtsChoice {
    Piper {
        bin: PathBuf,
        voice: PiperVoice,
    },
    OpenAi {
        server: Server,
        voice: String,
    },
    /// The voices of the operating system, which only the app can speak.
    System,
}

/// What the choice depends on.
#[derive(Clone, Copy)]
pub struct TtsEnv<'a> {
    pub setting: &'a str,
    pub piper_bin: Option<&'a Path>,
    pub voices: &'a [PiperVoice],
    /// `voice.tts.voice`: language → voice id.
    pub picks: &'a Map<String, Value>,
    pub server: Option<&'a Server>,
}

/// Language code of a voice id (`it_IT-paola-medium`, `en-us-lessac`) or `None`.
pub fn voice_language(id: &str) -> Option<String> {
    let head = id.split(['_', '-']).next()?.to_ascii_lowercase();
    (head.len() == 2 && head.chars().all(|c| c.is_ascii_lowercase())).then_some(head)
}

/// Two-letter code of a language tag (`it-IT`, `IT`, `auto` → `None`).
pub fn language_code(tag: &str) -> Option<String> {
    let t = tag.trim().to_ascii_lowercase();
    let head = t.split(['-', '_']).next()?;
    (head.len() == 2 && head.chars().all(|c| c.is_ascii_lowercase())).then(|| head.to_string())
}

/// Guesses Italian or English from common words (`None` when unsure): enough to choose a voice for
/// a reply when the caller does not say which language it is in.
pub fn detect_language(text: &str) -> Option<&'static str> {
    const IT: &[&str] = &[
        "il", "lo", "la", "gli", "le", "che", "di", "è", "un", "una", "per", "non", "sono", "con", "come", "del", "della", "anche",
        "questo", "questa", "ma", "più", "ho", "hai", "nel", "alla", "ciao", "grazie",
    ];
    const EN: &[&str] = &[
        "the", "and", "is", "are", "of", "to", "that", "with", "for", "this", "it", "you", "have", "not", "was", "be", "on", "hello",
        "what", "can", "will",
    ];
    let lower = text.to_lowercase();
    let (mut it, mut en) = (0usize, 0usize);
    for w in lower.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()) {
        it += IT.contains(&w) as usize;
        en += EN.contains(&w) as usize;
    }
    it += lower.chars().filter(|c| "àèéìòù".contains(*c)).count().min(3);
    match (it, en) {
        (i, e) if i >= 2 && i > e => Some("it"),
        (i, e) if e >= 2 && e > i => Some("en"),
        _ => None,
    }
}

/// Cuts `text` into sentences to synthesise one by one. Line breaks end a sentence; `.`, `!`, `?`
/// and `…` end one when a space (or the end) follows, so `3.14` and `e.g.` stay whole. A sentence
/// longer than [`MAX_SENTENCE`] is cut at a comma or a space, and fragments without a letter or a
/// digit are dropped.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut cur = String::new();
        for (i, &c) in chars.iter().enumerate() {
            cur.push(c);
            let ends = matches!(c, '.' | '!' | '?' | '…')
                && chars.get(i + 1).is_none_or(|n| n.is_whitespace() || matches!(n, '"' | ')' | '»' | '”'));
            // Keep a closing quote or bracket with its sentence.
            let closing = matches!(chars.get(i + 1), Some('"' | ')' | '»' | '”'));
            if ends && !closing {
                raw.push(std::mem::take(&mut cur));
            }
        }
        raw.push(cur);
    }
    let mut out = Vec::new();
    for piece in raw {
        let piece = piece.split_whitespace().collect::<Vec<_>>().join(" ");
        if !piece.chars().any(char::is_alphanumeric) {
            continue;
        }
        let mut rest = piece.as_str();
        while rest.chars().count() > MAX_SENTENCE {
            let cut_at = rest.char_indices().nth(MAX_SENTENCE).map(|(i, _)| i).unwrap_or(rest.len());
            let head = &rest[..cut_at];
            let at = head.rfind([',', ';', ':']).or_else(|| head.rfind(' ')).map(|i| i + 1).unwrap_or(cut_at);
            out.push(rest[..at].trim().to_string());
            rest = rest[at..].trim_start();
        }
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}

/// The voice for `lang`: the explicit one, the one picked in the settings, else the first of the
/// language (ids in order, so the choice is stable). `lang = None` accepts any language.
pub fn pick_voice<'a>(
    voices: &'a [PiperVoice],
    lang: Option<&str>,
    picks: &Map<String, Value>,
    explicit: Option<&str>,
) -> Option<&'a PiperVoice> {
    let by_id = |id: &str| voices.iter().find(|v| v.id.eq_ignore_ascii_case(id) || v.name.eq_ignore_ascii_case(id));
    if let Some(v) = explicit.map(str::trim).filter(|s| !s.is_empty()).and_then(by_id) {
        return Some(v);
    }
    if let Some(v) = lang.and_then(|l| picks.get(l)).and_then(Value::as_str).and_then(by_id) {
        return Some(v);
    }
    let mut candidates: Vec<&PiperVoice> = voices.iter().filter(|v| lang.is_none_or(|l| v.language == l)).collect();
    candidates.sort_by(|a, b| a.id.cmp(&b.id));
    candidates.into_iter().next()
}

/// Picks the engine. Pure, so the order is testable. `Err` only for an explicit engine that cannot
/// run (the message says what to do); `auto` ends on [`TtsChoice::System`] instead.
pub fn choose_tts(env: &TtsEnv, lang: Option<&str>, explicit_voice: Option<&str>) -> std::result::Result<TtsChoice, String> {
    let piper = || -> std::result::Result<TtsChoice, String> {
        let bin = env.piper_bin.ok_or(
            "Piper non è installato: installa l'eseguibile `piper` (vedi Impostazioni → Voce) o indica il percorso nel runtime Piper",
        )?;
        if env.voices.is_empty() {
            return Err("nessuna voce di Piper installata: scaricane una da Modelli (per esempio «Voce paola»)".into());
        }
        let voice = pick_voice(env.voices, lang, env.picks, explicit_voice)
            .ok_or_else(|| format!("nessuna voce di Piper per la lingua «{}»: scaricane una da Modelli", lang.unwrap_or("?")))?;
        Ok(TtsChoice::Piper { bin: bin.to_path_buf(), voice: voice.clone() })
    };
    let server = || -> std::result::Result<TtsChoice, String> {
        let s = env.server.ok_or("indica l'indirizzo del server della voce (per esempio http://localhost:8880)")?;
        let voice = explicit_voice
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
            .or_else(|| lang.and_then(|l| env.picks.get(l)).and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| default_server_voice(lang).to_string());
        Ok(TtsChoice::OpenAi { server: s.clone(), voice })
    };
    match env.setting {
        "system" => Ok(TtsChoice::System),
        "piper" => piper(),
        "openai" => server(),
        _ => Ok(piper().or_else(|_| server()).unwrap_or(TtsChoice::System)),
    }
}

/// Why `auto` ended on the system voices (for the settings page), or what is missing.
fn why_not_neural(env: &TtsEnv, lang: Option<&str>) -> Option<String> {
    match env.setting {
        "system" => None,
        "openai" => choose_tts(env, lang, None).err(),
        _ => {
            let piper = choose_tts(&TtsEnv { setting: "piper", ..*env }, lang, None).err();
            match (env.setting, piper) {
                ("piper", e) => e,
                (_, Some(p)) if env.server.is_none() => Some(format!("{p}; in alternativa indica un server della voce")),
                _ => None,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------- WAV

/// Header fields and samples of a PCM WAV.
struct Pcm<'a> {
    /// The 16 bytes of the `fmt ` chunk that describe the format.
    fmt: [u8; 16],
    data: &'a [u8],
}

fn parse_wav(b: &[u8]) -> Option<Pcm<'_>> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12usize;
    let mut fmt = None;
    while i.checked_add(8).is_some_and(|e| e <= b.len()) {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = i + 8;
        if id == b"fmt " && len >= 16 && body + 16 <= b.len() {
            let mut f = [0u8; 16];
            f.copy_from_slice(&b[body..body + 16]);
            fmt = Some(f);
        } else if id == b"data" {
            let present = b.len() - body;
            // Streamed WAVs declare 0 or 0xFFFFFFFF: trust the bytes that are there.
            let n = if len == 0 || len > present { present } else { len };
            return Some(Pcm { fmt: fmt?, data: &b[body..body + n] });
        }
        i = body.checked_add(len)?.checked_add(len & 1)?;
    }
    None
}

/// A PCM WAV from raw samples.
pub fn wav_from_pcm(sample_rate: u32, channels: u16, bits: u16, pcm: &[u8]) -> Vec<u8> {
    let block = channels * (bits / 8).max(1);
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * block as u32).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// Joins WAVs of the same format into one.
pub fn join_wavs(parts: &[Vec<u8>]) -> Result<Vec<u8>> {
    let bad = || Error::Upstream("il motore vocale non ha restituito un audio WAV valido".into());
    let first = parse_wav(parts.first().ok_or_else(bad)?).ok_or_else(bad)?;
    if parts.len() == 1 {
        return Ok(parts[0].clone());
    }
    let mut pcm = Vec::new();
    for p in parts {
        let w = parse_wav(p).ok_or_else(bad)?;
        if w.fmt != first.fmt {
            return Err(Error::Upstream("il motore vocale ha restituito audio in formati diversi".into()));
        }
        pcm.extend_from_slice(w.data);
    }
    let f = first.fmt;
    let rate = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
    Ok(wav_from_pcm(rate, u16::from_le_bytes([f[2], f[3]]), u16::from_le_bytes([f[14], f[15]]), &pcm))
}

// ----------------------------------------------------------------------------------- synthesis

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(reqwest::Client::new)
}

fn temp_wav() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("aiw-tts-{}-{}.wav", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// Runs Piper on one sentence: text on stdin, a WAV in a temporary file.
async fn run_piper(bin: &Path, voice: &PiperVoice, text: &str, speed: f64) -> Result<Vec<u8>> {
    use tokio::io::AsyncWriteExt;
    let out = temp_wav();
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-m").arg(&voice.path).arg("-f").arg(&out);
    if (speed - 1.0).abs() > 0.01 {
        // Piper speaks slower with a longer phoneme length.
        cmd.arg("--length_scale").arg(format!("{:.2}", 1.0 / speed));
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| Error::Runtime(format!("non riesco ad avviare Piper: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A voice that exits early closes the pipe: its exit status tells why.
        let _ = stdin.write_all(text.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
    }
    let result = tokio::time::timeout(SYNTH_TIMEOUT, child.wait_with_output()).await;
    let read = async {
        let output = result.map_err(|_| Error::Runtime("Piper non ha risposto in tempo".into()))?.map_err(Error::Io)?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("nessun dettaglio");
            return Err(Error::Runtime(format!("Piper è terminato con un errore: {last}")));
        }
        tokio::fs::read(&out).await.map_err(|_| Error::Runtime("Piper non ha prodotto l'audio".into()))
    };
    let r = read.await;
    let _ = tokio::fs::remove_file(&out).await;
    r
}

/// `{base}/v1/audio/speech` for a server address with or without `/v1`.
pub fn speech_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let b = b.strip_suffix("/v1").unwrap_or(b);
    format!("{b}/v1/audio/speech")
}

async fn run_server(server: &Server, voice: &str, text: &str, speed: f64) -> Result<Vec<u8>> {
    let mut body = json!({"model": server.model, "input": text, "voice": voice, "response_format": "wav"});
    if (speed - 1.0).abs() > 0.01 {
        body["speed"] = json!(speed);
    }
    let res = http()
        .post(speech_url(&server.base))
        .timeout(SYNTH_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Upstream(format!("il server della voce non risponde ({}): {e}", server.base)))?;
    let status = res.status();
    let bytes = res.bytes().await.map_err(|e| Error::Upstream(format!("risposta del server della voce interrotta: {e}")))?;
    if !status.is_success() {
        let msg = String::from_utf8_lossy(&bytes);
        return Err(Error::Upstream(format!("il server della voce ha risposto {status}: {}", msg.chars().take(200).collect::<String>())));
    }
    Ok(bytes.to_vec())
}

/// A request to speak.
#[derive(Debug, Clone, Default)]
pub struct SpeakRequest {
    pub text: String,
    /// Language of the text (`it`, `en`…); detected from the text when absent.
    pub language: Option<String>,
    /// A voice id (Piper voice or server voice); overrides the settings.
    pub voice: Option<String>,
    /// 0.5–2; overrides `voice.tts.speed`.
    pub speed: Option<f64>,
}

/// Synthesised audio.
#[derive(Debug, Clone)]
pub struct SpeechAudio {
    pub wav: Vec<u8>,
    pub backend: String,
    pub elapsed_ms: u64,
}

fn text_of<'a>(cfg: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    cfg.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

struct Gathered {
    setting: String,
    piper_bin: Option<PathBuf>,
    voices: Vec<PiperVoice>,
    picks: Map<String, Value>,
    server: Option<Server>,
    speed: f64,
    language: Option<String>,
}

impl Gathered {
    fn env(&self) -> TtsEnv<'_> {
        TtsEnv {
            setting: &self.setting,
            piper_bin: self.piper_bin.as_deref(),
            voices: &self.voices,
            picks: &self.picks,
            server: self.server.as_ref(),
        }
    }
}

fn piper_voices(models: &[LocalModel]) -> Vec<PiperVoice> {
    let mut v: Vec<PiperVoice> = models
        .iter()
        .filter(|m| m.format == "piper" && m.capabilities.iter().any(|c| c == "tts"))
        .filter_map(|m| {
            Some(PiperVoice {
                id: m.id.clone(),
                name: m.name.clone(),
                language: voice_language(&m.id)?,
                path: PathBuf::from(m.path.as_ref()?),
            })
        })
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

impl Engine {
    fn tts_gather(&self) -> Gathered {
        let cfg = self.config().effective;
        let setting = text_of(&cfg, "voice.tts.engine").unwrap_or("auto").to_string();
        let server = text_of(&cfg, "voice.tts.server_url")
            .map(|u| Server { base: u.to_string(), model: text_of(&cfg, "voice.tts.server_model").unwrap_or("kokoro").to_string() });
        Gathered {
            setting,
            piper_bin: self.0.runtimes.binary_for("piper"),
            voices: piper_voices(&self.0.models.read().unwrap()),
            picks: cfg.get("voice.tts.voice").and_then(Value::as_object).cloned().unwrap_or_default(),
            server,
            speed: cfg.get("voice.tts.speed").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.5, 2.0),
            language: cfg.get("voice.language").and_then(Value::as_str).and_then(language_code),
        }
    }

    /// The neural speech engine in use and what it can speak, for `GET /api/voice`.
    pub fn tts_status(&self) -> TtsStatus {
        let g = self.tts_gather();
        let env = g.env();
        let lang = g.language.as_deref();
        let choice = choose_tts(&env, lang, None);
        let (engine, neural, backend) = match &choice {
            Ok(TtsChoice::Piper { voice, .. }) => ("piper", true, Some(format!("Piper · {}", voice.id))),
            Ok(TtsChoice::OpenAi { server, voice }) => ("openai", true, Some(format!("{} · {voice}", server.base))),
            _ => ("system", false, None),
        };
        let piper_ready = g.piper_bin.is_some() && g.setting != "system" && g.setting != "openai";
        let server_ready = g.server.is_some() && g.setting != "system" && g.setting != "piper";
        let mut voices: Vec<TtsVoice> = Vec::new();
        let mut languages: Vec<String> = Vec::new();
        let mut picks: BTreeMap<String, String> = BTreeMap::new();
        if piper_ready || g.setting == "piper" {
            for v in &g.voices {
                voices.push(TtsVoice { id: v.id.clone(), name: v.name.clone(), language: v.language.clone(), engine: "piper".into() });
                if piper_ready && !languages.contains(&v.language) {
                    languages.push(v.language.clone());
                }
            }
        }
        if server_ready || g.setting == "openai" {
            for (id, name, language) in SERVER_VOICES {
                voices.push(TtsVoice { id: (*id).into(), name: (*name).into(), language: (*language).into(), engine: "openai".into() });
            }
        }
        languages.sort();
        for l in languages.iter().cloned().chain(["it".to_string(), "en".to_string()]) {
            if picks.contains_key(&l) {
                continue;
            }
            // The voice that would speak `l` now.
            match choose_tts(&env, Some(&l), None) {
                Ok(TtsChoice::Piper { voice, .. }) => picks.insert(l, voice.id),
                Ok(TtsChoice::OpenAi { voice, .. }) => picks.insert(l, voice),
                _ => None,
            };
        }
        TtsStatus {
            setting: g.setting.clone(),
            engine: engine.into(),
            neural,
            backend,
            detail: if neural { None } else { why_not_neural(&env, lang) },
            voices,
            languages,
            any_language: server_ready || (g.setting == "openai" && g.server.is_some()),
            picks,
            speed: g.speed,
            max_chars: MAX_CHARS,
        }
    }

    /// Reads `req.text` aloud with the neural engine. `Err(Unsupported)` when the system voices
    /// are the ones to use (the app speaks those itself). Requests wait for each other: one
    /// synthesis at a time.
    pub async fn speak(&self, req: SpeakRequest, cancel: &CancellationToken) -> Result<SpeechAudio> {
        let text = req.text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("nessun testo da leggere".into()));
        }
        if text.chars().count() > MAX_CHARS {
            return Err(Error::Invalid(format!("testo troppo lungo: massimo {MAX_CHARS} caratteri")));
        }
        let g = self.tts_gather();
        let lang = req
            .language
            .as_deref()
            .and_then(language_code)
            .or_else(|| detect_language(text).map(String::from))
            .or_else(|| g.language.clone());
        let choice = choose_tts(&g.env(), lang.as_deref(), req.voice.as_deref())
            .map_err(|why| Error::Unsupported(format!("la voce neurale non è disponibile: {why}")))?;
        if choice == TtsChoice::System {
            return Err(Error::Unsupported("la voce neurale non è attiva: nessun motore installato, si usano le voci del sistema".into()));
        }
        let speed = req.speed.unwrap_or(g.speed).clamp(0.5, 2.0);
        let sentences = split_sentences(text);
        if sentences.is_empty() {
            return Err(Error::Invalid("nessun testo da leggere".into()));
        }
        let _gate = tokio::select! {
            g = self.0.tts_gate.lock() => g,
            _ = cancel.cancelled() => return Err(Error::Cancelled),
        };
        let started = Instant::now();
        let mut parts = Vec::with_capacity(sentences.len());
        for s in &sentences {
            let one = async {
                match &choice {
                    TtsChoice::Piper { bin, voice } => run_piper(bin, voice, s, speed).await,
                    TtsChoice::OpenAi { server, voice } => run_server(server, voice, s, speed).await,
                    TtsChoice::System => unreachable!("handled above"),
                }
            };
            let wav = tokio::select! {
                r = one => r?,
                _ = cancel.cancelled() => return Err(Error::Cancelled),
            };
            parts.push(wav);
        }
        let backend = match &choice {
            TtsChoice::Piper { voice, .. } => format!("Piper · {}", voice.id),
            TtsChoice::OpenAi { server, voice } => format!("{} · {voice}", server.base),
            TtsChoice::System => String::new(),
        };
        let wav = join_wavs(&parts)?;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(%backend, sentences = sentences.len(), elapsed_ms, "sintesi vocale completata");
        Ok(SpeechAudio { wav, backend, elapsed_ms })
    }
}
