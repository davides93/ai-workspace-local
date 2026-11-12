//! The proactive assistant: gates, the filter, reminders, the briefing, the proposal lifecycle and the
//! kill switch, with a scripted model and a clock the tests move.

use super::proactive::Wake;
use super::tests::{engine, wait_finished};
use super::*;
use aiw_runtimes::{ChatDelta, ScriptedBackend};
use chrono::{DateTime, Duration as D, Local, TimeZone};

fn at(h: u32, m: u32) -> DateTime<Local> {
    Local.from_local_datetime(&Local::now().date_naive().and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
}

fn on(e: &Engine, level: &str) {
    e.set_config("global", "proactive.level", json!(level)).unwrap();
    // Keep the briefing out of the way unless a test asks for it, whatever the real time of day is.
    e.set_config("global", "proactive.briefing_time", json!("23:59")).unwrap();
}

/// An engine whose scripted backend we can inspect afterwards.
async fn rig(turns: Vec<Vec<ChatDelta>>) -> (Engine, ScriptedBackend, tempfile::TempDir, tempfile::TempDir) {
    let (e, d, w) = engine(vec![]).await;
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    (e, backend, d, w)
}

fn calls(b: &ScriptedBackend) -> usize {
    b.requests.lock().unwrap().len()
}

fn open_proposals(e: &Engine) -> Vec<Proposal> {
    e.proposals(Some("open"), Some("proposal")).unwrap()
}

fn proposal(e: &Engine, instruction: &str) -> Proposal {
    let now = Local::now();
    let p = Proposal {
        id: new_id("prp"),
        kind: "proposal".into(),
        title: "Titolo".into(),
        noticed: "Notato".into(),
        instruction: instruction.into(),
        source: format!("test:{}", new_id("x")),
        agent_id: "operator".into(),
        status: "open".into(),
        created_at: now.to_rfc3339(),
        expires_at: (now + D::days(7)).to_rfc3339(),
        decided_at: None,
        run_id: None,
        session_id: None,
    };
    e.0.store.proposal_insert(&p).unwrap();
    p
}

fn due_reminder(e: &Engine, text: &str) -> aiw_core::api::Memory {
    e.memory_create(MemoryCreate {
        text: text.into(),
        kind: Some("reminder".into()),
        due_at: Some((Local::now() - D::days(2)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap()
}

#[tokio::test]
async fn it_is_off_by_default_and_says_so() {
    let (e, b, _d, _w) = rig(vec![]).await;
    assert_eq!(e.proactive_level(), "off");
    let r = e.proactive_tick_at(Wake::Manual, at(10, 0)).await.unwrap();
    assert_eq!((r.outcome.as_str(), calls(&b)), ("skipped", 0));
    assert!(r.reason.unwrap().contains("spento"));
    assert!(e.proactive_due_wakes(at(10, 0)).is_empty(), "no wake source runs while it is off");
    let s = e.proactive_status_at(at(10, 0));
    assert_eq!((s.level.as_str(), s.open_proposals, s.next_wake.is_none()), ("off", 0, true));
    assert!(s.skipped_reason.unwrap().contains("spento"));
}

#[tokio::test]
async fn quiet_hours_budget_power_and_a_busy_assistant_skip_the_tick_and_say_why() {
    let (e, b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    due_reminder(&e, "Chiamare il dentista");
    e.set_config("global", "proactive.quiet_hours", json!("22:00-07:00")).unwrap();
    let r = e.proactive_tick_at(Wake::Interval, at(23, 0)).await.unwrap();
    assert!(r.outcome == "skipped" && r.reason.as_deref().unwrap().contains("silenzio"), "{r:?}");
    assert!(open_proposals(&e).is_empty(), "nothing is prepared during the quiet hours");
    assert_eq!(e.proactive_status_at(at(23, 0)).skipped_reason.as_deref(), r.reason.as_deref());

    // Outside the quiet hours the same reminder becomes a proposal.
    e.set_config("global", "proactive.quiet_hours", json!("")).unwrap();
    e.set_config("global", "proactive.daily_budget_minutes", json!(0)).unwrap();
    let r = e.proactive_tick_at(Wake::Interval, at(10, 0)).await.unwrap();
    assert!(r.reason.as_deref().unwrap().contains("finito"), "an empty budget stops it: {r:?}");
    assert!(open_proposals(&e).is_empty());
    e.set_config("global", "proactive.daily_budget_minutes", json!(15)).unwrap();

    // On battery, only when the platform says so, and only if the setting asks for it.
    e.set_on_power(Some(false));
    assert!(e.proactive_tick_at(Wake::Interval, at(10, 0)).await.unwrap().reason.unwrap().contains("batteria"));
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let r = e.proactive_tick_at(Wake::Interval, at(10, 0)).await.unwrap();
    assert_eq!(r.outcome, "proposed", "{r:?}");
    assert_eq!(calls(&b), 0);
}

#[tokio::test]
async fn power_state_unknown_is_ignored() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    due_reminder(&e, "Pagare la bolletta");
    assert_eq!(e.0.config.get("proactive.only_on_power").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(e.proactive_tick_at(Wake::Interval, at(10, 0)).await.unwrap().outcome, "proposed");
}

#[tokio::test]
async fn an_empty_context_never_calls_a_model() {
    let (e, b, _d, _w) = rig(vec![ScriptedBackend::text_turn(r#"{"act": true, "item": 1}"#)]).await;
    on(&e, "propose");
    // Pinned memories and open proposals are background, not a reason to ask.
    e.memory_create(MemoryCreate { text: "Preferisco le risposte brevi".into(), pinned: Some(true), ..Default::default() }).unwrap();
    proposal(&e, "Fai qualcosa");
    let r = e.proactive_tick_at(Wake::Interval, at(6, 0)).await.unwrap();
    assert_eq!((r.outcome.as_str(), r.model_called, calls(&b)), ("nothing", false, 0), "{r:?}");
}

#[tokio::test]
async fn a_due_reminder_becomes_a_proposal_without_a_model_and_only_once() {
    let (e, b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    let m = due_reminder(&e, "Chiamare il dentista");
    // A reminder that is not due yet, and one that is forgotten, are left alone.
    e.memory_create(MemoryCreate {
        text: "Domani il compleanno".into(),
        kind: Some("reminder".into()),
        due_at: Some((Local::now() + D::days(2)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(e.proactive_due_wakes(Local::now()), vec![Wake::Reminder], "a due reminder is a wake source");
    let r = e.proactive_tick_at(Wake::Reminder, Local::now()).await.unwrap();
    assert_eq!((r.outcome.as_str(), r.proposals.len(), r.model_called, calls(&b)), ("proposed", 1, false, 0), "{r:?}");
    let p = &open_proposals(&e)[0];
    assert_eq!((p.source.as_str(), p.status.as_str(), p.agent_id.as_str()), (format!("reminder:{}", m.id).as_str(), "open", "operator"));
    assert!(p.title.contains("dentista") && p.instruction.contains("dentista"));
    // Idempotent: the same tick again proposes nothing new, and the wake source is quiet.
    let again = e.proactive_tick_at(Wake::Reminder, Local::now()).await.unwrap();
    assert!(again.proposals.is_empty());
    assert_eq!(open_proposals(&e).len(), 1);
    assert!(e.proactive_due_wakes(Local::now()).is_empty());
}

#[tokio::test]
async fn the_filter_picks_one_item_and_the_agent_writes_a_read_only_proposal() {
    let (e, b, _d, ws) = rig(vec![
        ScriptedBackend::text_turn(r#"{"act": true, "item": 1, "why": "file nuovo"}"#),
        ScriptedBackend::text_turn(r#"{"title": "Ordina le fatture", "noticed": "Sono arrivate due fatture.", "instruction": "Sposta le fatture in Documenti/Fatture"}"#),
    ])
    .await;
    on(&e, "propose");
    let inbox = ws.path().join("Scaricati");
    std::fs::create_dir(&inbox).unwrap();
    e.set_config("global", "proactive.watch_folders", json!([inbox.display().to_string()])).unwrap();
    // Nothing new yet: no question.
    let r = e.proactive_tick_at(Wake::Interval, Local::now() - D::seconds(5)).await.unwrap();
    assert_eq!((r.outcome.as_str(), calls(&b)), ("nothing", 0));
    std::fs::write(inbox.join("fattura-001.pdf"), "x").unwrap();
    let r = e.proactive_tick_at(Wake::Interval, Local::now() + D::seconds(5)).await.unwrap();
    assert_eq!((r.outcome.as_str(), r.model_called), ("proposed", true), "{r:?}");
    assert_eq!(calls(&b), 2, "one question to the filter, one run of the agent");
    {
        let reqs = b.requests.lock().unwrap();
        // The filter's question is tiny and strict; the run only has tools that read.
        assert!(reqs[0].messages[1].text().contains("fattura-001.pdf") && reqs[0].tools.is_empty());
        let names: Vec<&str> = reqs[1].tools.iter().map(|t| t.function.name.as_str()).collect();
        assert!(
            !names.is_empty() && names.iter().all(|n| !["write_file", "edit_file", "shell", "move_file", "agent", "handoff"].contains(n)),
            "{names:?}"
        );
    }
    let p = &open_proposals(&e)[0];
    assert_eq!((p.title.as_str(), p.instruction.as_str()), ("Ordina le fatture", "Sposta le fatture in Documenti/Fatture"));
    assert!(p.source.starts_with("files:"));
    // The same files are not proposed again.
    let r = e.proactive_tick_at(Wake::Interval, Local::now() + D::seconds(10)).await.unwrap();
    assert_eq!((r.model_called, open_proposals(&e).len()), (false, 1));
}

#[tokio::test]
async fn a_no_or_a_malformed_filter_answer_proposes_nothing() {
    for answer in [r#"{"act": false}"#, "Secondo me sì!", r#"{"act": true, "item": 9}"#] {
        let (e, b, _d, ws) = rig(vec![ScriptedBackend::text_turn(answer)]).await;
        on(&e, "propose");
        let dir = ws.path().join("in");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        e.set_config("global", "proactive.watch_folders", json!([dir.display().to_string()])).unwrap();
        let r = e.proactive_tick_at(Wake::Interval, Local::now()).await.unwrap();
        assert_eq!((r.outcome.as_str(), r.model_called, calls(&b)), ("nothing", true, 1), "{answer}");
        assert!(open_proposals(&e).is_empty());
    }
}

#[tokio::test]
async fn the_morning_briefing_is_made_from_data_once_a_day_and_says_when_there_is_nothing() {
    let (e, b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    // Before the briefing time: nothing.
    assert!(e.proactive_tick_at(Wake::Interval, at(7, 0)).await.unwrap().proposals.is_empty());
    // After it, with nothing to report: a note that says so, already read, no badge.
    let r = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    assert_eq!(r.proposals.len(), 1);
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert_eq!((brief.kind.as_str(), brief.status.as_str()), ("briefing", "done"));
    assert!(brief.noticed.contains("Niente da segnalare"));
    assert!(e.proactive_tick_at(Wake::Interval, at(9, 0)).await.unwrap().proposals.is_empty(), "once a day");
    assert_eq!(open_proposals(&e).len(), 0);

    // Another day, with something to say, works with no connector and no model.
    let tomorrow = at(8, 5) + D::days(1);
    e.memory_create(MemoryCreate {
        text: "Riunione con Anna".into(),
        kind: Some("reminder".into()),
        due_at: Some((tomorrow + D::hours(3)).to_rfc3339()),
        ..Default::default()
    })
    .unwrap();
    proposal(&e, "Una cosa in sospeso");
    let r = e.proactive_tick_at(Wake::Briefing, tomorrow).await.unwrap();
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert_eq!((brief.kind.as_str(), brief.status.as_str()), ("briefing", "open"));
    assert!(brief.noticed.contains("Riunione con Anna") && brief.noticed.contains("Proposte in attesa: 1"), "{}", brief.noticed);
    assert_eq!(calls(&b), 0);
}

#[tokio::test]
async fn the_wake_sources_use_the_trigger_machinery() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    let t0 = at(7, 59) + D::seconds(30);
    assert!(e.proactive_due_wakes(t0).is_empty());
    assert_eq!(e.proactive_due_wakes(t0 + D::seconds(40)), vec![Wake::Briefing], "the daily time");
    assert!(e.proactive_due_wakes(t0 + D::seconds(60)).is_empty(), "fires once");
    // Default interval: every 30 minutes while the app runs.
    assert_eq!(e.proactive_due_wakes(t0 + D::minutes(31)), vec![Wake::Interval]);
    assert!(e.proactive_due_wakes(t0 + D::minutes(32)).is_empty());
}

#[tokio::test]
async fn only_one_tick_runs_at_a_time() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    let guard = e.0.proactive.lock.lock().await;
    let r = e.proactive_tick_at(Wake::Manual, at(10, 0)).await.unwrap();
    assert!(r.outcome == "skipped" && r.reason.as_deref().unwrap().contains("già in corso"), "{r:?}");
    drop(guard);
    assert_ne!(e.proactive_tick_at(Wake::Manual, at(10, 0)).await.unwrap().outcome, "skipped");
}

#[tokio::test]
async fn the_kill_switch_stops_it_until_the_level_is_set_again() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    due_reminder(&e, "Chiamare");
    e.kill().await;
    assert!(e.proactive_status().stopped_by_kill_switch);
    let r = e.proactive_tick_at(Wake::Manual, at(10, 0)).await.unwrap();
    assert!(r.outcome == "skipped" && r.reason.as_deref().unwrap().contains("emergenza"), "{r:?}");
    assert!(e.proactive_due_wakes(at(10, 0)).is_empty() || e.proactive_status().stopped_by_kill_switch);
    assert!(open_proposals(&e).is_empty());
    on(&e, "propose");
    assert!(!e.proactive_status().stopped_by_kill_switch);
    assert_eq!(e.proactive_tick_at(Wake::Manual, at(10, 0)).await.unwrap().outcome, "proposed");
}

#[tokio::test]
async fn proposals_are_dismissed_and_expire() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    let (a, b2) = (proposal(&e, "Uno"), proposal(&e, "Due"));
    let d = e.proposal_dismiss(&a.id).unwrap();
    assert_eq!((d.status.as_str(), d.decided_at.is_some()), ("dismissed", true));
    assert!(matches!(e.proposal_dismiss(&a.id), Err(Error::Conflict(_))), "a second click changes nothing");
    assert!(matches!(e.proposal("prp_none"), Err(Error::NotFound(_))));
    assert!(matches!(e.proposals(Some("boh"), None), Err(Error::Invalid(_))));
    // Past its date an open proposal expires (the clock is the stored date).
    let mut old = e.0.store.proposal_get(&b2.id).unwrap().unwrap();
    old.id = new_id("prp");
    old.expires_at = (Local::now() - D::hours(1)).to_rfc3339();
    e.0.store.proposal_insert(&old).unwrap();
    assert_eq!(e.proposal(&old.id).unwrap().status, "expired");
    assert_eq!(e.proposals(Some("open"), None).unwrap().len(), 1);
}

#[tokio::test]
async fn approving_starts_a_normal_run_with_the_usual_policy_and_links_it() {
    let (e, _b, _d, ws) = rig(vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        ScriptedBackend::text_turn("Fatto."),
    ])
    .await;
    let p = proposal(&e, "Crea una nota");
    let edited = e.proposal_approve(&p.id, Some("  Crea una nota intitolata Spesa  ")).await.unwrap();
    assert_eq!((edited.status.as_str(), edited.instruction.as_str()), ("approved", "Crea una nota intitolata Spesa"));
    let (run, session) = (edited.run_id.clone().unwrap(), edited.session_id.clone().unwrap());
    // The usual policy: the agent asks before writing; nothing was written yet.
    let mut seen = None;
    for _ in 0..200 {
        if let Some(a) = e.approvals().first() {
            seen = Some(a.clone());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let ask = seen.expect("the run asks for approval like any run");
    assert_eq!(ask.tool, "write_file");
    assert!(!ws.path().join("nota.md").exists());
    e.respond_approval(&ask.id, true, false).unwrap();
    wait_finished(&e, &run).await;
    assert_eq!(std::fs::read_to_string(ws.path().join("nota.md")).unwrap(), "# Nota\n");
    // The first message of the new session is the instruction, and the proposal is `done` once the run ended.
    assert_eq!(e.messages(&session).unwrap()[0].content, "Crea una nota intitolata Spesa");
    assert_eq!(e.proposal(&p.id).unwrap().status, "done");
    // Approving twice, or approving a briefing, is refused.
    assert!(matches!(e.proposal_approve(&p.id, None).await, Err(Error::Conflict(_))));
    let brief =
        Proposal { kind: "briefing".into(), instruction: String::new(), source: "briefing:x".into(), id: new_id("prp"), ..p.clone() };
    e.0.store.proposal_insert(&brief).unwrap();
    assert!(matches!(e.proposal_approve(&brief.id, None).await, Err(Error::Conflict(_))));
    // An empty instruction is a bad request.
    let empty = proposal(&e, "  ");
    assert!(matches!(e.proposal_approve(&empty.id, None).await, Err(Error::Invalid(_))));
}

#[tokio::test]
async fn an_agent_that_is_gone_leaves_the_proposal_open() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    let mut p = proposal(&e, "Fai");
    p.id = new_id("prp");
    p.agent_id = "non-esiste".into();
    p.source = "x".into();
    e.0.store.proposal_insert(&p).unwrap();
    assert!(matches!(e.proposal_approve(&p.id, None).await, Err(Error::Conflict(_))));
    assert_eq!(e.proposal(&p.id).unwrap().status, "open");
}

#[tokio::test]
async fn the_proposal_agent_is_never_more_permissive_than_reading() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    let mut a = e.agent("operator").unwrap();
    a.mode = "yolo".into();
    a.sandbox = "off".into();
    a.max_turns = 50;
    e.save_agent(a).unwrap();
    let p = e.proactive_agent("operator").unwrap();
    assert_eq!((p.mode.as_str(), p.sandbox.as_str()), ("plan", "read-only"));
    assert!(p.max_turns <= 6 && p.budget.as_ref().unwrap().max_seconds.unwrap() <= 120);
    for t in ["write_file", "shell", "edit_file", "agent", "handoff", "browser_click"] {
        assert!(!p.tools.iter().any(|n| n == t), "{t}");
    }
    assert!(p.tools.iter().any(|t| t == "read_file"));
    assert!(!p.tools.iter().any(|t| t == "web_fetch"), "no network at `propose`");
    on(&e, "act_readonly");
    let q = e.proactive_agent("operator").unwrap();
    assert!(q.tools.iter().any(|t| t == "web_fetch") || !e.tool_registry().all().iter().any(|t| t.name() == "web_fetch"));
    assert_eq!(q.mode, "plan");
}

#[tokio::test]
async fn the_status_reports_the_budget_the_last_tick_and_the_next_wake() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    due_reminder(&e, "Chiamare");
    let now = at(10, 0);
    e.proactive_tick_at(Wake::Manual, now).await.unwrap();
    let s = e.proactive_status_at(now);
    assert_eq!((s.level.as_str(), s.budget_minutes, s.open_proposals), ("propose", 15, 1));
    assert_eq!(s.last_tick.as_ref().map(|t| t.outcome.as_str()), Some("proposed"));
    assert!(s.skipped_reason.is_none());
    let next = DateTime::parse_from_rfc3339(&s.next_wake.unwrap()).unwrap();
    assert!(next > now && next <= now + D::minutes(30) + D::seconds(1), "{next}");
}

#[test]
fn the_proposals_table_migrates_safely_on_an_old_database() {
    // A database from before the table, opened twice: the table appears and nothing else changes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE documents (kind TEXT NOT NULL, id TEXT NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(kind, id)); INSERT INTO documents VALUES('x','1','{}','now');").unwrap();
    }
    for _ in 0..2 {
        let s = store::Store::open(&path).unwrap();
        assert!(s.proposal_list(None, 10).unwrap().is_empty());
        assert!(s.get_doc::<Value>("x", "1").unwrap().is_some());
    }
}

// ------------------------------------------------------------------------------------ connectors

/// A calendar file with one event today at `h`:00 and a Maildir with one message dated `mail_at`.
fn connector_fixtures(dir: &std::path::Path, mail_at: DateTime<Local>) -> (String, String) {
    let today = Local::now().date_naive().format("%Y%m%d").to_string();
    let cal = dir.join("agenda.ics");
    std::fs::write(
        &cal,
        format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:{today}T150000\r\nDTEND:{today}T160000\r\nSUMMARY:Dentista\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"),
    )
    .unwrap();
    let md = dir.join("Posta");
    for s in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md.join(s)).unwrap();
    }
    std::fs::write(
        md.join("new/1"),
        format!(
            "From: Anna Verdi <anna@example.org>\r\nSubject: Contratto da firmare\r\nDate: {}\r\n\r\nTi allego il contratto.\r\n",
            mail_at.to_rfc2822()
        ),
    )
    .unwrap();
    (cal.display().to_string(), md.display().to_string())
}

#[tokio::test]
async fn the_connectors_are_off_by_default_and_add_nothing_to_the_briefing() {
    let (e, b, d, _w) = rig(vec![]).await;
    on(&e, "propose");
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    let (cal, md) = connector_fixtures(d.path(), at(7, 30));
    // The sources are set but the switches are off: the tools are not offered and nothing is read.
    e.set_config("global", "connectors.calendar.sources", json!([cal])).unwrap();
    e.set_config("global", "connectors.mail.sources", json!([md])).unwrap();
    assert!(e.tool_registry().get("calendar_today").unwrap().available().is_err());
    let r = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert!(brief.noticed.contains("Niente da segnalare"), "{}", brief.noticed);
    assert_eq!(calls(&b), 0);
}

#[tokio::test]
async fn the_briefing_lists_todays_events_and_new_mail_from_data_only_and_once() {
    let (e, _b, d, _w) = rig(vec![ScriptedBackend::text_turn(r#"{"act": false}"#)]).await;
    on(&e, "propose");
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    let (cal, md) = connector_fixtures(d.path(), at(7, 30));
    e.set_config("global", "connectors.calendar.sources", json!([cal])).unwrap();
    e.set_config("global", "connectors.mail.sources", json!([md])).unwrap();
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "connectors.mail.enabled", json!(true)).unwrap();
    assert!(e.tool_registry().get("mail_recent").unwrap().available().is_ok());

    let r = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert_eq!((brief.kind.as_str(), brief.status.as_str()), ("briefing", "open"), "{}", brief.noticed);
    assert!(brief.noticed.contains("In agenda oggi: 1 evento: 15:00–16:00 · Dentista"), "{}", brief.noticed);
    assert!(brief.noticed.contains("Posta nuova: 1 messaggio: «Contratto da firmare» di Anna Verdi"), "{}", brief.noticed);
    // The briefing needs no model; the heartbeat context does (once): the filter was asked about the calendar and the mail.
    assert!(r.model_called);
}

#[tokio::test]
async fn the_heartbeat_offers_the_calendar_and_the_mail_to_the_filter_once() {
    let (e, b, d, _w) = rig(vec![ScriptedBackend::text_turn(r#"{"act": false}"#), ScriptedBackend::text_turn(r#"{"act": false}"#)]).await;
    on(&e, "propose");
    let (cal, md) = connector_fixtures(d.path(), at(9, 0));
    e.set_config("global", "connectors.calendar.sources", json!([cal])).unwrap();
    e.set_config("global", "connectors.mail.sources", json!([md])).unwrap();
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "connectors.mail.enabled", json!(true)).unwrap();

    let r = e.proactive_tick_at(Wake::Interval, at(10, 0)).await.unwrap();
    assert!(r.model_called, "{r:?}");
    let question = b.requests.lock().unwrap()[0].messages[1].text();
    assert!(question.contains("In agenda oggi") && question.contains("Dentista"), "{question}");
    assert!(question.contains("Posta nuova: 1 messaggio") && question.contains("Contratto da firmare"), "{question}");
    assert_eq!(calls(&b), 1);

    // The mail is older than the last tick now, and the filter said no to the calendar: it is not asked again
    // about the same mail. (The calendar item is the same events, so its key is the same too once proposed;
    // a "no" does not record the key, so only the mail drops out.)
    let r = e.proactive_tick_at(Wake::Interval, at(10, 30)).await.unwrap();
    let question = b.requests.lock().unwrap()[1].messages[1].text();
    assert!(question.contains("Dentista") && !question.contains("Contratto"), "{question} / {r:?}");
}

#[tokio::test]
async fn a_proposal_run_may_use_the_connectors_when_they_are_on() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    on(&e, "propose");
    let names = |a: aiw_core::api::AgentProfile| a.tools;
    let off = names(e.proactive_agent("operator").unwrap());
    assert!(!off.iter().any(|t| t == "calendar_today" || t == "mail_search"), "{off:?}");
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "connectors.mail.enabled", json!(true)).unwrap();
    let on_ = names(e.proactive_agent("operator").unwrap());
    for t in ["calendar_today", "calendar_upcoming", "mail_recent", "mail_search"] {
        assert!(on_.iter().any(|x| x == t), "{t} in {on_:?}");
    }
}

#[tokio::test]
async fn a_workspace_cannot_switch_the_connectors_on() {
    let (e, _b, _d, _w) = rig(vec![]).await;
    // The workspace layer is repository content: its values for these keys are ignored.
    let _ = e.set_config("workspace", "connectors.mail.enabled", json!(true));
    let _ = e.set_config("workspace", "connectors.calendar.sources", json!(["/x"]));
    assert!(!e.0.config.get_bool("connectors.mail.enabled"));
    assert!(e.0.config.get("connectors.calendar.sources").is_none_or(|v| v == json!([])));
    assert!(e.tool_registry().get("mail_recent").unwrap().available().is_err());
}
