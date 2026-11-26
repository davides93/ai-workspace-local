//! The morning briefing over sources that look like real ones: a calendar file with the things real calendars hold
//! (events that are over, all-day, recurring with an exception, cancelled, tomorrow's), mail in a Maildir (encoded
//! headers, quoted-printable, HTML only), in an mbox and on an IMAP server, with nothing read ever written back.
//! The sources are doubles: what is NOT proved here is how a real calendar app, a subscription feed or a real mail
//! provider behaves (see `docs/mobile/BACKGROUND.md`).

use super::proactive::Wake;
use super::tests::engine;
use super::*;
use aiw_core::api::MailAccount;
use aiw_core::datetime::{DateTime, Duration as D, Local, Utc, Weekday};
use aiw_runtimes::ScriptedBackend;
use aiw_tools::connectors::testing::{FakeImap, ImapMessage, ImapOptions, Security};
use std::path::Path;

fn at(h: u32, m: u32) -> DateTime<Local> {
    Local.from_local_datetime(&Local::now().date_naive().and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
}

fn day(offset: i64) -> aiw_core::datetime::NaiveDate {
    Local::now().date_naive() + D::days(offset)
}

fn weekday_code(d: Weekday) -> &'static str {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][d.num_days_from_monday() as usize]
}

/// A day of a real-looking calendar: a few things that must show and a few that must not.
fn calendar_text() -> String {
    let today = day(0).format("%Y%m%d").to_string();
    let tomorrow = day(1).format("%Y%m%d").to_string();
    let two_weeks_ago = day(-14).format("%Y%m%d").to_string();
    let last_week = day(-7).format("%Y%m%d").to_string();
    let utc = |h: u32, m: u32| {
        Local
            .from_local_datetime(&day(0).and_hms_opt(h, m, 0).unwrap())
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string()
    };
    let ev = |lines: &str| format!("BEGIN:VEVENT\r\n{lines}END:VEVENT\r\n");
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//IT\r\n");
    // Over before the briefing: not shown.
    s += &ev(&format!("UID:palestra\r\nDTSTART:{today}T070000\r\nDTEND:{today}T073000\r\nSUMMARY:Palestra\r\n"));
    // In UTC, with a folded line and an escaped comma.
    s += &ev(&format!(
        "UID:colloquio\r\nDTSTART:{}\r\nDTEND:{}\r\nSUMMARY:Colloquio con Bianchi\\, Rossi\r\n  e soci\r\n",
        utc(9, 30),
        utc(10, 30)
    ));
    // All day.
    s += &ev(&format!(
        "UID:compleanno\r\nDTSTART;VALUE=DATE:{today}\r\nDTEND;VALUE=DATE:{}\r\nSUMMARY:Compleanno di Anna\r\n",
        day(1).format("%Y%m%d")
    ));
    // Weekly, started two weeks ago, with last week skipped: today it happens.
    s += &ev(&format!(
        "UID:team\r\nDTSTART:{two_weeks_ago}T110000\r\nDTEND:{two_weeks_ago}T114500\r\nRRULE:FREQ=WEEKLY;BYDAY={}\r\nEXDATE:{last_week}T110000\r\nSUMMARY:Riunione di team\r\n",
        weekday_code(day(0).weekday())
    ));
    // Cancelled, and tomorrow's: not shown.
    s += &ev(&format!("UID:annullata\r\nDTSTART:{today}T120000\r\nSTATUS:CANCELLED\r\nSUMMARY:Annullata\r\n"));
    s += &ev(&format!("UID:domani\r\nDTSTART:{tomorrow}T090000\r\nSUMMARY:Domani\r\n"));
    s += &ev(&format!("UID:dentista\r\nDTSTART:{today}T150000\r\nDTEND:{today}T160000\r\nSUMMARY:Dentista\r\nLOCATION:Via Roma 1\r\n"));
    s + "END:VCALENDAR\r\n"
}

fn message(from: &str, subject: &str, date: DateTime<Local>, headers: &str, body: &str) -> String {
    format!("From: {from}\r\nTo: io@example.org\r\nSubject: {subject}\r\nDate: {}\r\n{headers}\r\n{body}\r\n", date.to_rfc2822())
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.strip_prefix(dir).unwrap().display().to_string(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

struct Sources {
    imap: FakeImap,
    mail_dir: tempfile::TempDir,
}

async fn connect_sources(e: &Engine, calendar: &str) -> Sources {
    let mail_dir = tempfile::tempdir().unwrap();
    let cal = mail_dir.path().join("agenda.ics");
    std::fs::write(&cal, calendar).unwrap();
    // A Maildir: an old message (not new), an encoded one, an HTML-only one.
    let md = mail_dir.path().join("Posta");
    for s in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md.join(s)).unwrap();
    }
    std::fs::write(md.join("new/1"), message("Newsletter <news@example.org>", "Vecchia newsletter", at(8, 0) - D::days(3), "", "Ciao"))
        .unwrap();
    std::fs::write(
        md.join("new/2"),
        message(
            "=?UTF-8?Q?Ren=C3=A9_Bianchi?= <rene@example.org>",
            "=?UTF-8?Q?Preventivo_per_l=27ufficio?=",
            at(7, 30),
            "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n",
            "Ti mando il preventivo, =C3=A8 allegato.",
        ),
    )
    .unwrap();
    std::fs::write(
        md.join(if cfg!(windows) { "cur/3;2,S" } else { "cur/3:2,S" }),
        message(
            "Shop <shop@example.org>",
            "Offerta del giorno",
            at(7, 45),
            "Content-Type: text/html; charset=utf-8\r\n",
            "<html><body><p>Sconto del <b>20%</b></p></body></html>",
        ),
    )
    .unwrap();
    // An mbox.
    let mbox = mail_dir.path().join("archivio.mbox");
    std::fs::write(
        &mbox,
        format!(
            "From verbali@example.org Mon Jan  1 00:00:00 2030\r\n{}\r\n",
            message("Verbali <verbali@example.org>", "Verbale di ieri", at(6, 0), "", "In allegato.")
        ),
    )
    .unwrap();
    // An IMAP server.
    let imap = FakeImap::start(ImapOptions::new(
        Security::StartTls,
        &[(
            "INBOX",
            vec![ImapMessage {
                uid: 1,
                date: Local::now().format("%Y-%m-%d").to_string(),
                raw: message("Banca <info@banca.example>", "Bonifico ricevuto", at(7, 50), "", "Hai ricevuto un bonifico.").into_bytes(),
            }],
        )],
    ))
    .await;
    e.set_tls_roots_for_tests(vec![imap.cert.clone()]);
    e.set_mail_accounts(vec![MailAccount {
        id: "casa".into(),
        host: "127.0.0.1".into(),
        port: imap.addr.port(),
        username: "mario".into(),
        password: "segreto".into(),
        folders: vec![],
    }])
    .unwrap();
    e.set_config("global", "connectors.calendar.sources", json!([cal.display().to_string()])).unwrap();
    e.set_config("global", "connectors.mail.sources", json!([md.display().to_string(), mbox.display().to_string()])).unwrap();
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "connectors.mail.enabled", json!(true)).unwrap();
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    Sources { imap, mail_dir }
}

#[tokio::test]
async fn the_briefing_over_a_realistic_calendar_and_mail_from_three_kinds_of_source_changes_nothing_in_them() {
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn(r#"{"act": false}"#)])));
    let src = connect_sources(&e, &calendar_text()).await;
    let before = snapshot(src.mail_dir.path());

    let r = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert_eq!((brief.kind.as_str(), brief.status.as_str()), ("briefing", "open"), "{}", brief.noticed);
    let text = brief.noticed.clone();

    // The calendar: what is over, cancelled or tomorrow's is left out, the rest is in time order (all-day first).
    assert!(text.contains("In agenda oggi: 4 eventi:"), "{text}");
    assert!(text.contains("tutto il giorno · Compleanno di Anna"), "{text}");
    assert!(text.contains("09:30–10:30 · Colloquio con Bianchi, Rossi e soci"), "{text}");
    assert!(
        text.contains("11:00–11:45 · Riunione di team e altri 1"),
        "the recurring event happens today, the dentist is the one left out of the three: {text}"
    );
    for gone in ["Palestra", "Annullata", "Domani"] {
        assert!(!text.contains(gone), "{gone} must not be in the briefing: {text}");
    }
    // The mail: the Maildir (the old message is out), the mbox and the IMAP server together, newest first, with the
    // encoded sender and subject decoded.
    assert!(text.contains("Posta nuova: 4 messaggi: «Bonifico ricevuto» di Banca; «Offerta del giorno» di Shop; «Preventivo per l'ufficio» di René Bianchi e altri 1."), "{text}");
    assert!(!text.contains("Vecchia newsletter"));
    // Nothing is unreadable: no complaint.
    assert!(!text.contains("Non ho potuto leggere"), "{text}");

    // Nothing was written, moved or flagged: the files are the same, the server saw only reading commands.
    assert_eq!(snapshot(src.mail_dir.path()), before, "the sources are untouched");
    let cmds = src.imap.commands();
    assert!(cmds.iter().any(|c| c.starts_with("EXAMINE")) && cmds.iter().any(|c| c.contains("BODY.PEEK")), "{cmds:?}");
    for forbidden in ["STORE", "EXPUNGE", "COPY", "MOVE", "APPEND", "DELETE", "SELECT", "CREATE", "RENAME"] {
        assert!(!cmds.iter().any(|c| c.contains(forbidden)), "{forbidden} in {cmds:?}");
    }
    assert!(src.imap.seen.lock().unwrap().is_empty(), "no message was marked as read");
    // The password is nowhere the model or the brief can see.
    assert!(!text.contains("segreto"));
}

#[tokio::test]
async fn the_briefing_is_written_once_a_day_even_across_a_restart_and_the_next_day_has_its_own() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let open = |data: &Path, ws: &Path| {
        let (d, w) = (data.to_path_buf(), ws.to_path_buf());
        async move {
            Engine::new(EngineOptions {
                data_dir: Some(d),
                workspace_root: Some(w),
                mode: "embedded".into(),
                offline: true,
                ..Default::default()
            })
            .await
            .unwrap()
        }
    };
    let e = open(data.path(), ws.path()).await;
    let cal = tempfile::tempdir().unwrap();
    let file = cal.path().join("agenda.ics");
    std::fs::write(&file, calendar_text()).unwrap();
    e.set_config("global", "connectors.calendar.sources", json!([file.display().to_string()])).unwrap();
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let first = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    assert_eq!(first.proposals.len(), 1);
    // The same morning, a second wake (the interval, a phone shortcut): nothing new.
    let again = e.proactive_tick_at(Wake::Interval, at(8, 40)).await.unwrap();
    assert!(!again.proposals.iter().any(|id| e.proposal(id).map(|p| p.kind == "briefing").unwrap_or(false)));
    drop(e);
    // After a restart of the app the briefing of the day is still there: it is not written again.
    let e = open(data.path(), ws.path()).await;
    let after = e.proactive_tick_at(Wake::Briefing, at(9, 0)).await.unwrap();
    assert!(!after.proposals.iter().any(|id| e.proposal(id).map(|p| p.kind == "briefing").unwrap_or(false)), "{after:?}");
    let briefings: Vec<_> = e.proposals(None, Some("briefing")).unwrap();
    assert_eq!(briefings.len(), 1, "one briefing for the day: {briefings:?}");
    // The next morning (the clock moves a day) it is written again.
    let next = e.proactive_tick_at(Wake::Briefing, at(8, 5) + D::days(1)).await.unwrap();
    assert_eq!(e.proposals(None, Some("briefing")).unwrap().len(), 2, "{next:?}");
    assert_eq!(next.proposals.len(), 1);
}

#[tokio::test]
async fn a_source_that_cannot_be_read_is_said_instead_of_passing_for_a_free_day() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_config("global", "connectors.calendar.sources", json!(["/percorso/che/non/esiste.ics"])).unwrap();
    e.set_config("global", "connectors.mail.sources", json!(["/posta/che/non/esiste"])).unwrap();
    e.set_config("global", "connectors.calendar.enabled", json!(true)).unwrap();
    e.set_config("global", "connectors.mail.enabled", json!(true)).unwrap();
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let r = e.proactive_tick_at(Wake::Briefing, at(8, 1)).await.unwrap();
    let brief = e.proposal(&r.proposals[0]).unwrap();
    assert_eq!(brief.status, "open", "not the «niente da segnalare» note: {}", brief.noticed);
    assert!(brief.noticed.contains("Non ho potuto leggere 1 fonte del calendario"), "{}", brief.noticed);
    assert!(brief.noticed.contains("Non ho potuto leggere 1 fonte della posta"), "{}", brief.noticed);
    assert!(!brief.noticed.contains("/percorso") && !brief.noticed.contains("/posta"), "no path in a text that may go to a channel");
}

#[tokio::test]
async fn a_day_has_one_briefing_whatever_the_hour_and_the_next_day_its_own() {
    // The briefing key is the date: ticks at any hour of one day add no second briefing (a summer-time day is not simulated).
    let (e, _d, _w) = engine(vec![]).await;
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("08:00")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let base = Local::now().date_naive();
    for (i, h) in [(0i64, 8u32), (0, 12), (0, 23), (1, 0), (1, 8)] {
        let when = Local.from_local_datetime(&(base + D::days(i)).and_hms_opt(h, 5, 0).unwrap()).earliest().unwrap();
        e.proactive_tick_at(Wake::Interval, when).await.unwrap();
    }
    let n = e.proposals(None, Some("briefing")).unwrap().len();
    assert_eq!(n, 2, "two days, two briefings (the tick at 00:05 of the second day waits for 08:00)");
}
