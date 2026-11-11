//! The proactive check woken by the system: summary, reasons, the time budget, and the model step on a phone that
//! has no model (skipped with a recorded reason, or delegated to a paired device).

use super::background::{normalize_reason, run_background_blocking};
use super::tests::engine;
use super::*;
use aiw_core::api::{BackgroundRequest, PeerDevice, PeerHardware};
use aiw_runtimes::{ChatBackend, ScriptedBackend};
use chrono::{DateTime, Duration as D, Local, TimeZone};
use std::time::Duration;

fn at(h: u32, m: u32) -> DateTime<Local> {
    Local.from_local_datetime(&Local::now().date_naive().and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
}

fn on(e: &Engine) {
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    e.set_config("global", "proactive.quiet_hours", json!("")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
}

fn req(reason: &str) -> BackgroundRequest {
    BackgroundRequest { reason: reason.into(), budget_s: None, on_power: None }
}

/// A phone with no model: the default profile has none and nothing is installed.
fn without_model(e: &Engine) {
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Nessuno".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: String::new(),
        params: InferenceParams::default(),
    })
    .unwrap();
}

fn due_reminder(e: &Engine, text: &str) {
    e.memory_create(MemoryCreate {
        text: text.into(),
        kind: Some("reminder".into()),
        due_at: Some((Local::now() - D::days(2)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn reasons_are_normalised() {
    assert_eq!(normalize_reason("briefing"), "briefing");
    assert_eq!(normalize_reason(" shortcut "), "shortcut");
    assert_eq!(normalize_reason("interval"), "interval");
    assert_eq!(normalize_reason(""), "interval");
    assert_eq!(normalize_reason("boh"), "interval");
}

#[tokio::test]
async fn while_it_is_off_the_check_is_skipped_and_says_why() {
    let (e, _d, _w) = engine(vec![]).await;
    let s = e.proactive_background_tick_at(&req("interval"), at(10, 0)).await.unwrap();
    assert_eq!((s.outcome.as_str(), s.new_proposals, s.timed_out), ("skipped", 0, false), "{s:?}");
    assert!(s.skipped_reason.as_deref().unwrap().contains("spento"), "{s:?}");
    assert!(s.notification.is_none() && s.briefing.is_none());
    assert_eq!(s.model_step, "skipped");
}

#[tokio::test]
async fn with_no_model_the_model_free_parts_still_run_and_make_a_notification() {
    let (e, _d, _w) = engine(vec![]).await;
    without_model(&e);
    on(&e);
    due_reminder(&e, "Chiamare il dentista");
    let s = e.proactive_background_tick_at(&req("interval"), at(10, 0)).await.unwrap();
    assert_eq!((s.outcome.as_str(), s.new_proposals, s.model_step.as_str()), ("proposed", 1, "not_needed"), "{s:?}");
    let n = s.notification.expect("a notification");
    assert_eq!(n.title, "Una proposta per te");
    assert!(n.body.contains("dentista"), "{n:?}");
    // The same check again finds nothing new and does not notify again.
    let again = e.proactive_background_tick_at(&req("interval"), at(10, 31)).await.unwrap();
    assert_eq!((again.outcome.as_str(), again.new_proposals), ("nothing", 0), "{again:?}");
    assert!(again.notification.is_none());
}

#[tokio::test]
async fn the_briefing_reasons_give_todays_text_and_write_it_when_it_is_missing() {
    let (e, _d, _w) = engine(vec![]).await;
    without_model(&e);
    on(&e);
    e.memory_create(MemoryCreate {
        text: "Riunione con Anna".into(),
        kind: Some("reminder".into()),
        due_at: Some((at(7, 0) + D::hours(5)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap();
    // Before the briefing time an interval check writes nothing, and neither does «Controlla adesso»...
    let early = e.proactive_background_tick_at(&req("interval"), at(7, 0)).await.unwrap();
    assert!(early.briefing.is_none() && early.notification.is_none(), "{early:?}");
    assert!(e.proactive_background_tick_at(&req("shortcut"), at(7, 2)).await.unwrap().briefing.is_none());
    // ...but «Riepilogo di oggi» (reason briefing) writes it now.
    let s = e.proactive_background_tick_at(&req("briefing"), at(7, 5)).await.unwrap();
    assert!(s.briefing.as_deref().unwrap().contains("Riunione con Anna"), "{s:?}");
    // Asked again, by the morning wake or by «Controlla adesso», it hands the same text back without writing a second one.
    let again = e.proactive_background_tick_at(&req("briefing"), at(8, 1)).await.unwrap();
    assert_eq!(again.briefing, s.briefing);
    assert_eq!(e.proactive_background_tick_at(&req("shortcut"), at(8, 2)).await.unwrap().briefing, s.briefing);
    assert_eq!(e.proposals(None, Some("briefing")).unwrap().len(), 1);
    // The interval check does not repeat it as news.
    let interval = e.proactive_background_tick_at(&req("interval"), at(8, 40)).await.unwrap();
    assert!(interval.briefing.is_none() && interval.notification.is_none(), "{interval:?}");
}

#[tokio::test]
async fn the_morning_briefing_makes_a_notification_when_it_has_something_to_say() {
    let (e, _d, _w) = engine(vec![]).await;
    without_model(&e);
    on(&e);
    e.memory_create(MemoryCreate {
        text: "Portare il passaporto".into(),
        kind: Some("reminder".into()),
        due_at: Some((at(8, 5) + D::hours(3)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap();
    let s = e.proactive_background_tick_at(&req("briefing"), at(8, 5)).await.unwrap();
    assert_eq!(s.outcome, "nothing");
    let n = s.notification.unwrap();
    assert_eq!(n.title, "Il riepilogo di oggi");
    assert!(n.body.contains("passaporto"), "{n:?}");
}

fn watch_new_file(e: &Engine, ws: &std::path::Path) -> std::path::PathBuf {
    let inbox = ws.join("Scaricati");
    std::fs::create_dir(&inbox).unwrap();
    e.set_config("global", "proactive.watch_folders", json!([inbox.display().to_string()])).unwrap();
    inbox
}

#[tokio::test]
async fn something_to_look_at_and_no_model_skips_the_filter_and_records_the_reason() {
    let (e, _d, w) = engine(vec![]).await;
    without_model(&e);
    on(&e);
    let inbox = watch_new_file(&e, w.path());
    assert_eq!(e.proactive_background_tick_at(&req("interval"), Local::now() - D::seconds(5)).await.unwrap().model_step, "not_needed");
    std::fs::write(inbox.join("fattura.pdf"), "x").unwrap();
    let s = e.proactive_background_tick_at(&req("interval"), Local::now() + D::seconds(5)).await.unwrap();
    assert_eq!((s.outcome.as_str(), s.model_step.as_str()), ("nothing", "skipped"), "{s:?}");
    assert!(s.model_note.as_deref().unwrap().contains("non c'è un modello"), "{s:?}");
    // The reason is in the status the app shows, too.
    let last = e.proactive_status().last_tick.unwrap();
    assert!(last.model_skipped.as_deref().unwrap().contains("modello") && !last.model_called, "{last:?}");
}

fn pair_fake_peer(e: &Engine) {
    e.0.store
        .put_doc(
            "peer",
            "studio",
            &PeerDevice {
                id: "studio".into(),
                name: "Studio-PC".into(),
                os: "windows".into(),
                address: "http://192.0.2.1:7777".into(),
                status: "online".into(),
                shared_runtimes: vec!["modello-grande".into()],
                hardware: Some(PeerHardware {
                    arch: "x86_64".into(),
                    tier: "xl".into(),
                    memory_mb: 65536,
                    vram_mb: 24576,
                    accelerators: vec![],
                    unified_memory: false,
                }),
            },
        )
        .unwrap();
    let mut settings = serde_json::Map::new();
    settings.insert("base_url".into(), json!("http://192.0.2.1:7777/v1"));
    settings.insert("api_key".into(), json!("x"));
    e.update_runtime_settings("aiw-peer", settings).unwrap();
}

#[tokio::test]
async fn a_paired_device_answers_the_filter_when_this_one_has_no_model() {
    let (e, _d, w) = engine(vec![]).await;
    without_model(&e);
    on(&e);
    let peer = ScriptedBackend::new(vec![ScriptedBackend::text_turn(r#"{"act": true, "item": 1}"#)]);
    e.register_native_runtime("aiw-peer", Arc::new(peer.clone()));
    pair_fake_peer(&e);
    let inbox = watch_new_file(&e, w.path());
    e.proactive_background_tick_at(&req("interval"), Local::now() - D::seconds(5)).await.unwrap();
    std::fs::write(inbox.join("fattura.pdf"), "x").unwrap();
    let s = e.proactive_background_tick_at(&req("interval"), Local::now() + D::seconds(5)).await.unwrap();
    assert_eq!((s.model_step.as_str(), s.model_note.as_deref()), ("delegated", Some("Studio-PC")), "{s:?}");
    {
        let asked = peer.requests.lock().unwrap();
        assert_eq!(asked.len(), 1, "only the filter question goes to the other device; there is no local model to write with");
        assert!(asked[0].messages[1].text().contains("fattura.pdf"), "the question went to the paired device");
        assert_eq!(asked[0].model, "modello-grande");
    }
    // The item itself became the proposal.
    assert_eq!((s.outcome.as_str(), s.new_proposals), ("proposed", 1), "{s:?}");
    let p = &e.proposals(Some("open"), Some("proposal")).unwrap()[0];
    assert!(p.noticed.contains("fattura.pdf") && p.instruction.contains("fattura.pdf"), "{p:?}");
}

/// A model that never answers.
struct Silent;

#[async_trait::async_trait]
impl ChatBackend for Silent {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["muto".into()])
    }
    async fn chat(
        &self,
        _req: aiw_runtimes::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<aiw_runtimes::DeltaStream> {
        cancel.cancelled().await;
        Err(Error::Cancelled)
    }
}

#[tokio::test]
async fn the_time_budget_stops_the_check_and_keeps_what_was_done() {
    let (e, _d, w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(Silent));
    on(&e);
    let inbox = watch_new_file(&e, w.path());
    e.proactive_background_tick_at(&req("interval"), Local::now() - D::seconds(5)).await.unwrap();
    due_reminder(&e, "Pagare la bolletta");
    std::fs::write(inbox.join("nuovo.txt"), "x").unwrap();
    let started = std::time::Instant::now();
    let s = e.background_tick_within(&req("interval"), Local::now() + D::seconds(5), Duration::from_millis(300)).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "the budget is hard");
    assert!(s.timed_out, "{s:?}");
    assert_eq!(s.model_step, "skipped");
    assert_eq!(s.new_proposals, 1, "the reminder written before the model step is kept: {s:?}");
    assert!(s.notification.is_some());
}

#[tokio::test]
async fn the_power_state_from_the_platform_is_honoured() {
    let (e, _d, _w) = engine(vec![]).await;
    on(&e);
    e.set_config("global", "proactive.only_on_power", json!(true)).unwrap();
    let s = e
        .proactive_background_tick_at(&BackgroundRequest { reason: "interval".into(), budget_s: None, on_power: Some(false) }, at(10, 0))
        .await
        .unwrap();
    assert_eq!(s.outcome, "skipped");
    assert!(s.skipped_reason.as_deref().unwrap().contains("batteria"), "{s:?}");
}

#[test]
fn the_native_glue_takes_json_and_answers_json() {
    let bad = run_background_blocking("non json");
    assert!(serde_json::from_str::<serde_json::Value>(&bad).unwrap()["error"].as_str().unwrap().contains("non valida"));
    // No app running in this process and no data folder given: a clear error, not a crash.
    let no_dir = serde_json::from_str::<serde_json::Value>(&run_background_blocking(r#"{"reason":"interval"}"#)).unwrap();
    assert!(no_dir["error"].as_str().unwrap().contains("cartella dei dati"), "{no_dir}");
    // A headless engine on a data folder: the level is off by default, so the check is skipped, with a summary.
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let out = run_background_blocking(
        &json!({"reason": "shortcut", "budgetS": 10, "dataDir": data.path(), "workspaceRoot": ws.path()}).to_string(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["reason"].as_str(), v["outcome"].as_str()), (Some("shortcut"), Some("skipped")), "{out}");
    assert!(v["skipped_reason"].as_str().unwrap().contains("spento"));
}
