//! Neural speech: engine choice, text splitting, WAV joining, Piper (a fake executable) and an
//! OpenAI-compatible speech server (a fake one). No real voice, model or internet.

use super::*;
use crate::tts::*;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn voice(id: &str) -> PiperVoice {
    PiperVoice { id: id.into(), name: id.into(), language: voice_language(id).unwrap(), path: PathBuf::from(format!("{id}.onnx")) }
}

fn server() -> Server {
    Server { base: "http://localhost:8880".into(), model: "kokoro".into() }
}

fn picks(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

/// A tiny WAV of `ms` milliseconds of silence at 8 kHz, 16 bit, mono.
fn wav(ms: u32) -> Vec<u8> {
    wav_from_pcm(8000, 1, 16, &vec![0u8; (ms * 16) as usize])
}

#[test]
fn auto_chooses_piper_then_the_server_then_the_system() {
    let bin = PathBuf::from("piper");
    let voices = [voice("it_it-paola-medium"), voice("en_us-lessac-medium")];
    let none = Map::new();
    let srv = server();
    let pick = |setting: &str, piper: bool, vs: &[PiperVoice], with_server: bool, lang: &str| {
        choose_tts(
            &TtsEnv { setting, piper_bin: piper.then_some(bin.as_path()), voices: vs, picks: &none, server: with_server.then_some(&srv) },
            Some(lang),
            None,
        )
    };
    // Piper with a voice for the language.
    assert!(matches!(pick("auto", true, &voices, true, "it"), Ok(TtsChoice::Piper { voice, .. }) if voice.id == "it_it-paola-medium"));
    assert!(matches!(pick("auto", true, &voices, false, "en"), Ok(TtsChoice::Piper { voice, .. }) if voice.language == "en"));
    // No voice for the language: the server, else the system.
    assert!(matches!(pick("auto", true, &voices, true, "de"), Ok(TtsChoice::OpenAi { .. })));
    assert_eq!(pick("auto", true, &voices, false, "de"), Ok(TtsChoice::System));
    // No executable, or no voices: the server, else the system.
    assert!(matches!(pick("auto", false, &voices, true, "it"), Ok(TtsChoice::OpenAi { voice, .. }) if voice == "if_sara"));
    assert!(matches!(pick("auto", true, &[], true, "en"), Ok(TtsChoice::OpenAi { voice, .. }) if voice == "af_heart"));
    assert_eq!(pick("auto", false, &[], false, "it"), Ok(TtsChoice::System));
    // Explicit choices win or say what is missing.
    assert_eq!(pick("system", true, &voices, true, "it"), Ok(TtsChoice::System));
    assert!(matches!(pick("openai", true, &voices, true, "it"), Ok(TtsChoice::OpenAi { .. })));
    assert!(pick("openai", true, &voices, false, "it").unwrap_err().contains("indirizzo"));
    assert!(matches!(pick("piper", true, &voices, true, "en"), Ok(TtsChoice::Piper { .. })));
    assert!(pick("piper", false, &voices, true, "it").unwrap_err().contains("Piper non è installato"));
    assert!(pick("piper", true, &[], true, "it").unwrap_err().contains("nessuna voce"));
    assert!(pick("piper", true, &voices, true, "fr").unwrap_err().contains("«fr»"));
}

#[test]
fn voices_follow_the_settings_then_the_language() {
    let voices = [voice("it_it-paola-medium"), voice("it_it-riccardo-x_low"), voice("en_us-lessac-medium")];
    let chosen = picks(json!({"it": "it_it-riccardo-x_low", "en": "gone"}));
    assert_eq!(pick_voice(&voices, Some("it"), &chosen, None).unwrap().id, "it_it-riccardo-x_low");
    // A picked voice that is not installed any more falls back to the first of the language.
    assert_eq!(pick_voice(&voices, Some("en"), &chosen, None).unwrap().id, "en_us-lessac-medium");
    // An explicit voice (a request) beats the settings, by id or by name.
    assert_eq!(pick_voice(&voices, Some("it"), &chosen, Some("IT_IT-PAOLA-MEDIUM")).unwrap().id, "it_it-paola-medium");
    assert!(pick_voice(&voices, Some("de"), &chosen, None).is_none());
    assert_eq!(pick_voice(&voices, None, &Map::new(), None).unwrap().id, "en_us-lessac-medium");
    assert_eq!(voice_language("it_IT-paola-medium").as_deref(), Some("it"));
    assert_eq!(voice_language("paola"), None);
    assert_eq!(language_code("it-IT").as_deref(), Some("it"));
    assert_eq!(language_code("auto"), None);
}

#[test]
fn text_is_split_into_sentences_and_capped() {
    let s = split_sentences("Ciao a tutti. Come stai? Bene!\nUna riga nuova senza punto\n\n3.14 è pi greco, così.");
    assert_eq!(s, ["Ciao a tutti.", "Come stai?", "Bene!", "Una riga nuova senza punto", "3.14 è pi greco, così."]);
    // Markup-only and punctuation-only fragments are dropped.
    assert_eq!(split_sentences("... \n --- \n ok"), ["ok"]);
    assert!(split_sentences("   ").is_empty());
    // A sentence that never ends is cut at a space, none longer than the limit.
    let long = "parola ".repeat(200);
    let parts = split_sentences(&long);
    assert!(parts.len() >= 3 && parts.iter().all(|p| p.chars().count() <= 400), "{}", parts.len());
    assert_eq!(parts.join(" ").split_whitespace().count(), 200, "nothing is lost");
    // Text without spaces is cut at the limit, on a character boundary.
    let blob = "è".repeat(1000);
    assert!(split_sentences(&blob).iter().all(|p| p.chars().count() <= 400));
    assert_eq!(split_sentences("Disse «ciao». Poi andò via."), ["Disse «ciao».", "Poi andò via."]);
}

#[test]
fn the_language_is_guessed_from_the_text() {
    assert_eq!(detect_language("Il gatto è sul tavolo e non vuole scendere"), Some("it"));
    assert_eq!(detect_language("The cat is on the table and it will not come down"), Some("en"));
    assert_eq!(detect_language("42"), None);
    assert_eq!(detect_language(""), None);
}

#[test]
fn wavs_are_joined_only_when_they_match() {
    let joined = join_wavs(&[wav(100), wav(250)]).unwrap();
    // 8 kHz mono 16 bit: 16 bytes per millisecond.
    assert_eq!(joined.len(), 44 + 350 * 16);
    assert_eq!(&joined[0..4], b"RIFF");
    assert_eq!(join_wavs(&[wav(100)]).unwrap(), wav(100));
    let other = wav_from_pcm(16000, 1, 16, &[0; 320]);
    assert!(join_wavs(&[wav(10), other]).unwrap_err().to_string().contains("formati diversi"));
    assert!(join_wavs(&[b"not a wav".to_vec()]).is_err());
    assert!(join_wavs(&[]).is_err());
}

#[test]
fn the_speech_url_accepts_the_address_with_or_without_v1() {
    for base in ["http://h:8880", "http://h:8880/", "http://h:8880/v1", "http://h:8880/v1/"] {
        assert_eq!(speech_url(base), "http://h:8880/v1/audio/speech", "{base}");
    }
}

async fn engine_with_voices() -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let (e, d, w) = crate::tests::engine(vec![]).await;
    for stem in ["it_IT-paola-medium", "en_US-lessac-medium"] {
        std::fs::write(e.0.models_dir.join(format!("{stem}.onnx")), b"onnx").unwrap();
        std::fs::write(e.0.models_dir.join(format!("{stem}.onnx.json")), r#"{"audio":{"sample_rate":22050},"phoneme_id_map":{}}"#).unwrap();
    }
    assert_eq!(e.rescan_models().iter().filter(|m| m.format == "piper").count(), 2);
    (e, d, w)
}

fn set(e: &Engine, key: &str, v: Value) {
    e.set_config("device", key, v).unwrap();
}

#[tokio::test]
async fn settings_validate_the_speech_server_address() {
    let (e, _d, _w) = engine_with_voices().await;
    assert!(e.set_config("device", "voice.tts.server_url", json!("localhost:8880")).is_err());
    assert!(e.set_config("device", "voice.tts.engine", json!("festival")).is_err());
    assert!(e.set_config("device", "voice.tts.speed", json!(5)).is_err());
    assert!(e.set_config("device", "voice.tts.server_url", json!("http://localhost:8880")).is_ok());
    assert!(e.set_config("device", "voice.tts.server_url", json!("")).is_ok());
    // The address of a server (where the text is sent) is not taken from a workspace's own file.
    let _ = e.set_config("workspace", "voice.tts.server_url", json!("http://evil.example"));
    assert_eq!(e.config().effective.get("voice.tts.server_url").and_then(|v| v.as_str()), Some(""));
}

#[tokio::test]
async fn status_explains_what_is_missing_and_lists_the_voices() {
    let (e, _d, _w) = engine_with_voices().await;
    // Voices but no executable: the system voices speak, and the status says why.
    let t = e.voice_status().tts;
    assert_eq!((t.setting.as_str(), t.engine.as_str(), t.neural), ("auto", "system", false), "{t:?}");
    assert!(t.detail.as_deref().unwrap().contains("Piper non è installato"), "{t:?}");
    assert_eq!(t.max_chars, 4000);
    // A server address makes the server the engine.
    set(&e, "voice.tts.server_url", json!("http://localhost:8880"));
    let t = e.tts_status();
    assert_eq!((t.engine.as_str(), t.neural, t.any_language), ("openai", true, true), "{t:?}");
    assert_eq!(t.picks.get("it").map(String::as_str), Some("if_sara"));
    set(&e, "voice.tts.voice", json!({"it": "im_nicola"}));
    assert_eq!(e.tts_status().picks.get("it").map(String::as_str), Some("im_nicola"));
    // The system setting turns it all off.
    set(&e, "voice.tts.engine", json!("system"));
    let t = e.tts_status();
    assert_eq!((t.engine.as_str(), t.neural, t.voices.len()), ("system", false, 0), "{t:?}");
}

#[tokio::test]
async fn speaking_without_a_neural_engine_says_so_and_checks_the_input() {
    let (e, _d, _w) = engine_with_voices().await;
    let cancel = CancellationToken::new();
    let say = |text: &str| SpeakRequest { text: text.into(), ..Default::default() };
    let err = e.speak(say("Ciao a tutti"), &cancel).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Unsupported(_)), "{err}");
    let err = e.speak(say("   "), &cancel).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Invalid(_)), "{err}");
    let err = e.speak(say(&"a".repeat(MAX_CHARS + 1)), &cancel).await.unwrap_err();
    assert!(err.to_string().contains("4000"), "{err}");
}

#[derive(Default, Clone)]
struct Seen(std::sync::Arc<Mutex<Vec<Value>>>);

/// A fake OpenAI-compatible speech server: a WAV whose length tells the sentence it was asked for.
async fn fake_speech_server(seen: Seen, status: u16) -> String {
    use axum::{http::StatusCode, routing::post, Json, Router};
    let app = Router::new().route(
        "/v1/audio/speech",
        post(move |Json(body): Json<Value>| {
            let seen = seen.clone();
            async move {
                let ms = body["input"].as_str().unwrap_or_default().chars().count() as u32 * 10;
                seen.0.lock().unwrap().push(body);
                if status != 200 {
                    return (StatusCode::from_u16(status).unwrap(), b"model not found".to_vec());
                }
                (StatusCode::OK, wav(ms))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn the_speech_server_speaks_sentence_by_sentence() {
    let (e, _d, _w) = engine_with_voices().await;
    let seen = Seen::default();
    let base = fake_speech_server(seen.clone(), 200).await;
    set(&e, "voice.tts.server_url", json!(base));
    set(&e, "voice.tts.server_model", json!("kokoro-82m"));
    set(&e, "voice.tts.speed", json!(1.5));
    let out = e
        .speak(
            SpeakRequest { text: "Ciao a tutti. Come stai oggi?".into(), language: Some("it".into()), ..Default::default() },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    // Two sentences (13 and 15 characters): 130 ms + 150 ms of audio, joined in order.
    assert_eq!(out.wav.len(), 44 + 280 * 16);
    assert!(out.backend.contains("if_sara"), "{}", out.backend);
    let log = seen.0.lock().unwrap().clone();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0]["input"], "Ciao a tutti.");
    assert_eq!(log[1]["input"], "Come stai oggi?");
    assert_eq!(
        (log[0]["model"].as_str(), log[0]["voice"].as_str(), log[0]["response_format"].as_str(), log[0]["speed"].as_f64()),
        (Some("kokoro-82m"), Some("if_sara"), Some("wav"), Some(1.5))
    );
    // The language is guessed from the text, and an explicit voice wins.
    let out = e
        .speak(
            SpeakRequest { text: "The weather is nice and you can go out.".into(), voice: Some("am_michael".into()), ..Default::default() },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(out.backend.contains("am_michael"), "{}", out.backend);
    let last = seen.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["voice"], "am_michael");
}

#[tokio::test]
async fn a_failing_speech_server_is_an_error_with_its_message() {
    let (e, _d, _w) = engine_with_voices().await;
    let base = fake_speech_server(Seen::default(), 404).await;
    set(&e, "voice.tts.server_url", json!(base));
    let err = e.speak(SpeakRequest { text: "Ciao.".into(), ..Default::default() }, &CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Upstream(_)), "{err}");
    assert!(err.to_string().contains("404") && err.to_string().contains("model not found"), "{err}");
    // Nothing listens there.
    set(&e, "voice.tts.server_url", json!("http://127.0.0.1:9"));
    let err = e.speak(SpeakRequest { text: "Ciao.".into(), ..Default::default() }, &CancellationToken::new()).await.unwrap_err();
    assert!(err.to_string().contains("non risponde"), "{err}");
}

#[tokio::test]
async fn a_cancelled_request_stops_before_synthesising() {
    let (e, _d, _w) = engine_with_voices().await;
    let seen = Seen::default();
    set(&e, "voice.tts.server_url", json!(fake_speech_server(seen.clone(), 200).await));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = e.speak(SpeakRequest { text: "Ciao a tutti.".into(), ..Default::default() }, &cancel).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Cancelled), "{err}");
    // Requests wait for the gate: a held gate blocks a request until it is cancelled.
    let held = e.0.tts_gate.lock().await;
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        c2.cancel();
    });
    let err = e.speak(SpeakRequest { text: "Ciao a tutti.".into(), ..Default::default() }, &cancel).await.unwrap_err();
    assert!(matches!(err, aiw_core::Error::Cancelled), "{err}");
    drop(held);
    assert!(seen.0.lock().unwrap().is_empty(), "nothing reached the server");
}

#[cfg(unix)]
mod piper {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake `piper`: records its arguments and text, then writes a WAV (of the fixture) to `-f`.
    fn fake_piper(dir: &std::path::Path, exit_code: i32) -> PathBuf {
        let fixture = dir.join("fixture.wav");
        std::fs::write(&fixture, wav(200)).unwrap();
        let script = dir.join("piper");
        let body = format!(
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in -f) out=\"$2\"; shift;; esac\n  echo \"$1\" >> \"{log}\"\n  shift\ndone\ncat >> \"{log}\"\nif [ {code} -ne 0 ]; then echo 'voice file is corrupt' >&2; exit {code}; fi\ncp \"{fixture}\" \"$out\"\n",
            log = dir.join("calls.log").display(),
            code = exit_code,
            fixture = fixture.display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn use_binary(e: &Engine, bin: &std::path::Path) {
        let mut m = Map::new();
        m.insert("binary".into(), json!(bin.display().to_string()));
        e.update_runtime_settings("piper", m).unwrap();
    }

    #[tokio::test]
    async fn piper_speaks_with_the_voice_of_the_language() {
        let (e, _d, _w) = engine_with_voices().await;
        let tools = tempfile::tempdir().unwrap();
        use_binary(&e, &fake_piper(tools.path(), 0));
        let t = e.tts_status();
        assert_eq!((t.engine.as_str(), t.neural), ("piper", true), "{t:?}");
        assert_eq!(t.languages, ["en", "it"]);
        assert_eq!(t.picks.get("it").map(String::as_str), Some("it_it-paola-medium"));
        assert!(!t.any_language);

        let out = e
            .speak(
                SpeakRequest { text: "Ciao a tutti. Come stai?".into(), language: Some("it".into()), ..Default::default() },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        // Two sentences of 200 ms each, joined.
        assert_eq!(out.wav.len(), 44 + 400 * 16);
        assert!(out.backend.starts_with("Piper · it_it-paola-medium"), "{}", out.backend);
        let log = std::fs::read_to_string(tools.path().join("calls.log")).unwrap();
        assert!(log.contains("it_IT-paola-medium.onnx") && log.contains("Ciao a tutti.") && log.contains("Come stai?"), "{log}");
        assert!(!log.contains("length_scale"), "normal speed passes no length scale: {log}");

        // The voice follows the language of the text, and a slower or faster speed becomes length_scale.
        std::fs::remove_file(tools.path().join("calls.log")).unwrap();
        let out = e
            .speak(
                SpeakRequest { text: "The weather is nice and you can go out.".into(), speed: Some(2.0), ..Default::default() },
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.backend.contains("en_us-lessac-medium"), "{}", out.backend);
        let log = std::fs::read_to_string(tools.path().join("calls.log")).unwrap();
        assert!(log.contains("en_US-lessac-medium.onnx") && log.contains("--length_scale") && log.contains("0.50"), "{log}");
        // No temporary file is left behind.
        let leftovers = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|f| f.ok())
            .filter(|f| f.file_name().to_string_lossy().starts_with(&format!("aiw-tts-{}-", std::process::id())))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[tokio::test]
    async fn piper_failures_carry_its_message() {
        let (e, _d, _w) = engine_with_voices().await;
        let tools = tempfile::tempdir().unwrap();
        use_binary(&e, &fake_piper(tools.path(), 3));
        let err = e
            .speak(
                SpeakRequest { text: "Ciao a tutti.".into(), language: Some("it".into()), ..Default::default() },
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("voice file is corrupt"), "{err}");
        // A language without a voice: auto has nowhere to go, the system voices speak.
        let err = e
            .speak(
                SpeakRequest { text: "Hallo zusammen.".into(), language: Some("de".into()), ..Default::default() },
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, aiw_core::Error::Unsupported(_)), "{err}");
        // Piper pinned and a missing voice: the error says so.
        set(&e, "voice.tts.engine", json!("piper"));
        let err = e
            .speak(
                SpeakRequest { text: "Hallo zusammen.".into(), language: Some("de".into()), ..Default::default() },
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("«de»"), "{err}");
    }
}
