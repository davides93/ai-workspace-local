//! Speech to text for voice mode: clients for whisper.cpp's server (`POST /inference`) and for
//! the OpenAI-compatible `POST /v1/audio/transcriptions` (LocalAI, Nexa, another AIW device).

use aiw_core::{Error, Result};
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use std::time::Duration;

/// Wire protocol of a speech-to-text backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttApi {
    /// whisper.cpp `whisper-server` (`/inference`, WAV only unless started with `--convert`).
    Whisper,
    /// OpenAI `audio/transcriptions`.
    OpenAi,
}

/// Where to send audio.
#[derive(Debug, Clone)]
pub struct SttTarget {
    pub api: SttApi,
    /// whisper: server root (`http://127.0.0.1:8178`); OpenAI: API base (`…/v1`).
    pub endpoint: String,
    pub api_key: Option<String>,
    /// Model name for OpenAI-compatible servers (`whisper-1` when unset).
    pub model: Option<String>,
}

/// Audio to transcribe.
pub struct Audio<'a> {
    pub bytes: &'a [u8],
    /// MIME type (`audio/wav`, `audio/webm`…).
    pub mime: &'a str,
    /// ISO 639-1 code; `None` or `auto` lets the model detect it.
    pub language: Option<&'a str>,
    /// Context that biases the decoder (names, jargon).
    pub prompt: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
}

/// Header of a PCM WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub duration_ms: u64,
}

/// Reads the `fmt ` and `data` chunks of a RIFF/WAVE file (`None` if it is not one).
pub fn wav_info(b: &[u8]) -> Option<WavInfo> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    let u32_at = |i: usize| b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let mut i = 12usize;
    let mut fmt = None;
    let mut data_len = None;
    while i.checked_add(8).is_some_and(|end| end <= b.len()) {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4)? as usize;
        let body = i + 8;
        if id == b"fmt " && len >= 16 {
            fmt = Some((u16_at(body + 2)?, u32_at(body + 4)?, u16_at(body + 14)?));
        } else if id == b"data" {
            // Streamed WAVs can declare 0 or 0xFFFFFFFF: trust the bytes actually present.
            let present = b.len() - body;
            data_len = Some(if len == 0 || len > present { present } else { len });
            break;
        }
        i = match body.checked_add(len).and_then(|e| e.checked_add(len & 1)) {
            Some(next) => next,
            None => break,
        };
    }
    let (channels, sample_rate, bits_per_sample) = fmt?;
    let bytes_per_second = sample_rate as u64 * channels.max(1) as u64 * (bits_per_sample as u64 / 8).max(1);
    Some(WavInfo { sample_rate, channels, bits_per_sample, duration_ms: data_len? as u64 * 1000 / bytes_per_second.max(1) })
}

fn file_name(mime: &str) -> &'static str {
    match mime.split(';').next().unwrap_or_default().trim() {
        "audio/webm" => "audio.webm",
        "audio/ogg" => "audio.ogg",
        "audio/mpeg" | "audio/mp3" => "audio.mp3",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "audio/aac" => "audio.m4a",
        "audio/flac" => "audio.flac",
        _ => "audio.wav",
    }
}

/// Collapses whisper's line breaks and drops non-speech annotations such as `[BLANK_AUDIO]`.
pub fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let Some(len) = rest[start..].find(']') else { break };
        let inner = &rest[start + 1..start + len];
        out.push_str(&rest[..start]);
        if !inner.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c == ' ' || c == '-') || inner.trim().is_empty() {
            out.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse(body: &str) -> Result<Transcription> {
    let v: Value = serde_json::from_str(body)
        .map_err(|_| Error::Runtime(format!("risposta di trascrizione non valida: {}", body.chars().take(200).collect::<String>())))?;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        let msg = e["message"].as_str().or_else(|| e.as_str()).unwrap_or("errore sconosciuto");
        return Err(Error::Runtime(format!("trascrizione: {msg}")));
    }
    let text = v["text"].as_str().ok_or_else(|| Error::Runtime("la risposta di trascrizione non contiene il testo".into()))?;
    Ok(Transcription { text: clean_text(text), language: v["language"].as_str().filter(|l| !l.is_empty()).map(String::from) })
}

/// Sends `audio` to `target` and returns the text.
pub async fn transcribe(http: &reqwest::Client, target: &SttTarget, audio: &Audio<'_>) -> Result<Transcription> {
    if audio.bytes.is_empty() {
        return Err(Error::Invalid("nessun audio da trascrivere".into()));
    }
    let part = Part::bytes(audio.bytes.to_vec())
        .file_name(file_name(audio.mime))
        .mime_str(audio.mime)
        .map_err(|e| Error::Invalid(format!("tipo audio non valido ({}): {e}", audio.mime)))?;
    let base = target.endpoint.trim_end_matches('/');
    let (url, mut form) = match target.api {
        SttApi::Whisper => {
            if wav_info(audio.bytes).is_none() {
                return Err(Error::Invalid("whisper.cpp accetta audio WAV: registra dal microfono dell'app o converti il file".into()));
            }
            let form = Form::new().part("file", part).text("response_format", "json").text("temperature", "0.0");
            (format!("{base}/inference"), form)
        }
        SttApi::OpenAi => {
            let model = target.model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "whisper-1".into());
            (format!("{base}/audio/transcriptions"), Form::new().part("file", part).text("model", model).text("response_format", "json"))
        }
    };
    if let Some(l) = audio.language.map(str::trim).filter(|l| !l.is_empty() && *l != "auto") {
        form = form.text("language", l.to_string());
    }
    if let Some(p) = audio.prompt.map(str::trim).filter(|p| !p.is_empty()) {
        form = form.text("prompt", p.to_string());
    }
    let mut rb = http.post(&url).multipart(form).timeout(Duration::from_secs(600));
    if let Some(k) = target.api_key.as_deref().filter(|k| !k.is_empty()) {
        rb = rb.bearer_auth(k);
    }
    let r = rb.send().await.map_err(|e| Error::Runtime(format!("trascrizione ({base}): {e}")))?;
    let status = r.status();
    let body = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().or_else(|| v["error"].as_str()).map(String::from))
            .unwrap_or_else(|| body.chars().take(300).collect());
        return Err(Error::Runtime(format!("trascrizione: HTTP {status} {detail}")));
    }
    parse(&body)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 16 kHz mono 16-bit PCM WAV of `ms` milliseconds of silence.
    pub fn wav(ms: u32) -> Vec<u8> {
        let samples = 16 * ms;
        let data_len = samples * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&16_000u32.to_le_bytes());
        b.extend_from_slice(&32_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(b"INFO");
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + data_len as usize, 0);
        b
    }

    #[test]
    fn reads_wav_headers() {
        let info = wav_info(&wav(1500)).unwrap();
        assert_eq!(info, WavInfo { sample_rate: 16_000, channels: 1, bits_per_sample: 16, duration_ms: 1500 });
        // Streamed header (unknown length): the bytes present count.
        let mut streamed = wav(500);
        let at = streamed.windows(4).position(|w| w == b"data").unwrap() + 4;
        streamed[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(wav_info(&streamed).unwrap().duration_ms, 500);
        assert!(wav_info(b"OggS....").is_none());
        assert!(wav_info(&wav(10)[..30]).is_none());
    }

    #[test]
    fn cleans_whisper_output() {
        assert_eq!(clean_text(" [BLANK_AUDIO]\n Ciao,\n come stai? [MUSIC] "), "Ciao, come stai?");
        assert_eq!(clean_text("Premi [Invio] per [ok"), "Premi [Invio] per [ok");
        assert_eq!(parse(r#"{"text":" Buongiorno a tutti.\n"}"#).unwrap().text, "Buongiorno a tutti.");
        assert!(parse(r#"{"error":"failed to read WAV file"}"#).is_err());
        assert_eq!(parse(r#"{"text":"hi","language":"en"}"#).unwrap().language.as_deref(), Some("en"));
    }
}
