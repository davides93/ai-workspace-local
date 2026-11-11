//! The connector tools end to end, on fixture files.

use super::*;
use chrono::NaiveDate;

fn day(offset: i64) -> NaiveDate {
    Local::now().date_naive() + Duration::days(offset)
}

fn ics_for(date: NaiveDate, h: u32, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{summary}\r\nDTSTART:{}T{h:02}0000\r\nDTEND:{}T{:02}0000\r\nSUMMARY:{summary}\r\nLOCATION:Ufficio\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        date.format("%Y%m%d"),
        date.format("%Y%m%d"),
        h + 1
    )
}

fn ctx() -> ToolContext {
    ToolContext::new("r", std::env::temp_dir())
}

fn tool(shared: &Shared, name: &str) -> Arc<dyn Tool> {
    tools(shared.clone()).into_iter().find(|t| t.name() == name).unwrap()
}

#[test]
fn tools_are_hidden_until_switched_on_and_are_read_only() {
    let shared = ConnectorSettings::new(Config::default());
    for t in tools(shared.clone()) {
        assert_eq!(t.risk(), Risk::Read, "{}", t.name());
        assert!(t.available().is_err(), "{} is off by default", t.name());
    }
    shared.set(Config { calendar_enabled: true, ..Default::default() });
    assert!(tool(&shared, "calendar_today").available().is_ok());
    assert!(tool(&shared, "mail_recent").available().is_err(), "mail has its own switch");
    let names: Vec<String> = tools(shared).iter().map(|t| t.name().to_string()).collect();
    assert_eq!(names, ["calendar_today", "calendar_upcoming", "mail_recent", "mail_search"]);
}

#[tokio::test]
async fn calendar_today_and_upcoming_read_a_file_and_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lavoro.ics"), ics_for(day(0), 22, "Chiusura")).unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/altro.ics"), ics_for(day(3), 10, "Dentista")).unwrap();
    std::fs::write(dir.path().join("note.txt"), "non un calendario").unwrap();
    let shared = ConnectorSettings::new(Config {
        calendar_enabled: true,
        calendar_sources: vec![dir.path().display().to_string(), dir.path().join("manca.ics").display().to_string()],
        ..Default::default()
    });
    let today = tool(&shared, "calendar_today").call(json!({}), &ctx()).await.unwrap().content;
    assert!(today.contains("Chiusura") && today.contains("22:00–23:00") && today.contains("(Ufficio)"), "{today}");
    assert!(!today.contains("Dentista"));
    assert!(today.contains("manca.ics") && today.contains("non esiste"), "an unreadable source is reported: {today}");

    let week = tool(&shared, "calendar_upcoming").call(json!({"days": 7}), &ctx()).await.unwrap().content;
    assert!(week.contains("Dentista") && week.contains("10:00–11:00"), "{week}");
    let short = tool(&shared, "calendar_upcoming").call(json!({"days": 1}), &ctx()).await.unwrap().content;
    assert!(!short.contains("Dentista"), "{short}");
}

#[tokio::test]
async fn without_sources_the_tools_say_what_to_do() {
    let shared = ConnectorSettings::new(Config { calendar_enabled: true, mail_enabled: true, ..Default::default() });
    let c = tool(&shared, "calendar_today").call(json!({}), &ctx()).await.unwrap().content;
    assert!(c.contains("Nessun calendario scelto"));
    let m = tool(&shared, "mail_recent").call(json!({}), &ctx()).await.unwrap().content;
    assert!(m.contains("Nessuna cartella di posta"));
    // A disabled tool refuses to run even if called directly.
    shared.set(Config::default());
    assert!(tool(&shared, "mail_recent").call(json!({}), &ctx()).await.is_err());
}

fn eml(from: &str, subject: &str, date: DateTime<Local>, body: &str) -> String {
    format!("From: {from}\r\nSubject: {subject}\r\nDate: {}\r\n\r\n{body}\r\n", date.to_rfc2822())
}

#[tokio::test]
async fn mail_recent_and_search_on_a_maildir_and_an_eml() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("Maildir");
    for s in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(inbox.join(s)).unwrap();
    }
    let now = Local::now();
    std::fs::write(
        inbox.join("new/a"),
        eml("Anna <anna@example.org>", "Fattura di marzo", now - Duration::hours(2), "Ti mando la fattura da pagare entro venerdì."),
    )
    .unwrap();
    std::fs::write(inbox.join("cur/b:2,S"), eml("Bruno <b@example.org>", "Cena", now - Duration::hours(30), "Sabato alle otto?")).unwrap();
    let single = dir.path().join("ciao.eml");
    std::fs::write(&single, eml("Carla <c@example.org>", "Ciao", now - Duration::minutes(10), "Come stai?")).unwrap();
    let shared = ConnectorSettings::new(Config {
        mail_enabled: true,
        mail_sources: vec![inbox.display().to_string(), single.display().to_string()],
        ..Default::default()
    });
    let recent = tool(&shared, "mail_recent").call(json!({"hours": 24}), &ctx()).await.unwrap().content;
    assert!(recent.contains("Fattura di marzo") && recent.contains("Anna") && recent.contains("Ti mando la fattura"), "{recent}");
    assert!(recent.contains("Ciao") && !recent.contains("Cena"), "{recent}");
    assert!(recent.find("Ciao").unwrap() < recent.find("Fattura").unwrap(), "newest first");
    let found = tool(&shared, "mail_search").call(json!({"query": "fattura venerdì"}), &ctx()).await.unwrap().content;
    assert!(found.contains("Fattura di marzo") && !found.contains("Cena"), "{found}");
    let none = tool(&shared, "mail_search").call(json!({"query": "inesistente"}), &ctx()).await.unwrap().content;
    assert!(none.contains("nessun messaggio"));
    assert!(tool(&shared, "mail_search").call(json!({}), &ctx()).await.is_err());
    // Read-only: nothing was moved.
    assert!(inbox.join("new/a").exists());
}

#[test]
fn urls_are_recognised_and_secrets_are_not_shown() {
    assert!(is_url("webcal://example.org/a.ics") && is_url("HTTPS://x.y/z") && !is_url("/home/x/a.ics"));
    assert_eq!(as_http("webcal://example.org/a.ics"), "https://example.org/a.ics");
    assert_eq!(shorten_url("https://example.org/cal.ics?token=SEGRETO"), "https://example.org/cal.ics");
}

#[test]
fn day_labels_are_in_italian() {
    let d = Local.from_local_datetime(&NaiveDate::from_ymd_opt(2030, 6, 10).unwrap().and_hms_opt(9, 0, 0).unwrap()).earliest().unwrap();
    assert_eq!(day_label(d), "lunedì 10 giugno");
}
