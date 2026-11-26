//! Email drafts (#76): prepared, reviewed and sent only after the owner approves exactly what they saw. Everything runs
//! against a fake IMAP/SMTP server on 127.0.0.1: **no real mailbox was used**, so what is proven here is the behaviour
//! of the engine and of the SMTP client with a fake transport, not delivery to a real provider.

use super::tests_channels::{header, mail, raw, until, AUTH_OK};
use super::*;
use aiw_runtimes::ScriptedBackend;
use aiw_tools::connectors::testing::{FakeImap, FakeSmtp, ImapOptions, Security, SmtpFault};

struct Rig {
    e: Engine,
    backend: ScriptedBackend,
    smtp: FakeSmtp,
    imap: FakeImap,
    channel: String,
    ws: tempfile::TempDir,
    _d: tempfile::TempDir,
}

/// An engine with an email channel that is turned on (it is what sends the drafts).
async fn rig(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> Rig {
    let (e, d, ws) = super::tests::engine(vec![]).await;
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let imap = FakeImap::start(ImapOptions::new(Security::StartTls, &[("INBOX", vec![])])).await;
    let smtp = FakeSmtp::start(Security::StartTls, "assistente@casa.example", "pw-smtp").await;
    e.set_tls_roots_for_tests(vec![imap.cert.clone(), smtp.cert.clone()]);
    e.set_channel_endpoints_for_tests(None, None, 0, 20, 60);
    let settings = json!({
        "imap_host": "127.0.0.1", "imap_port": imap.addr.port(), "imap_user": "mario",
        "smtp_host": "127.0.0.1", "smtp_port": smtp.addr.port(), "smtp_user": "assistente@casa.example",
        "from": "assistente@casa.example"
    });
    let ch = e
        .channel_create(ChannelCreate {
            kind: "email".into(),
            name: "Posta".into(),
            allowed: vec!["mario@example.it".into()],
            secrets: [("imap_password".to_string(), "segreto".to_string()), ("smtp_password".to_string(), "pw-smtp".to_string())].into(),
            settings: settings.as_object().unwrap().clone(),
            ..Default::default()
        })
        .unwrap();
    e.channel_patch(&ch.channel.id, ChannelPatch { enabled: Some(true), ..Default::default() }).unwrap();
    let (e2, id) = (e.clone(), ch.channel.id.clone());
    until("email channel connected", move || e2.channel_status(&id).unwrap().state == "connected").await;
    Rig { e, backend, smtp, imap, channel: ch.channel.id, ws, _d: d }
}

fn draft(to: &str, subject: &str, body: &str) -> EmailDraftCreate {
    EmailDraftCreate { to: vec![to.into()], subject: subject.into(), body: body.into(), ..Default::default() }
}

fn send_req(d: &EmailDraft) -> EmailDraftSend {
    EmailDraftSend { approved_hash: d.content_hash.clone(), resend: false }
}

impl Rig {
    fn mails(&self) -> Vec<(String, Vec<String>, String)> {
        self.smtp.mails.lock().unwrap().clone()
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.ws.path().join(name), bytes).unwrap();
    }
}

#[tokio::test]
async fn a_draft_shows_everything_and_goes_out_only_after_the_owner_approves_it() {
    let r = rig(vec![]).await;
    r.write("preventivo.pdf", b"%PDF-1.4 contenuto di prova");
    let d =
        r.e.email_draft_create(EmailDraftCreate {
            to: vec!["Anna@Cliente.it".into(), "anna@cliente.it".into()],
            cc: vec!["capo@esempio.it".into()],
            bcc: vec!["archivio@esempio.it".into()],
            subject: "Preventivo\r\nBcc: spia@x.it".into(),
            body: "Buongiorno Anna,\nin allegato il preventivo.\n\n.Una riga che comincia con un punto\nMario".into(),
            in_reply_to: Some("<richiesta@cliente.it>".into()),
            attachments: vec!["preventivo.pdf".into()],
            source: Some("mail:<richiesta@cliente.it>".into()),
            ..Default::default()
        })
        .unwrap();
    // Everything the owner has to see is there, and nothing has left.
    assert_eq!(d.status, "draft");
    assert_eq!(d.to, vec!["Anna@Cliente.it"], "the same address twice counts once");
    assert_eq!((d.cc.clone(), d.bcc.clone()), (vec!["capo@esempio.it".to_string()], vec!["archivio@esempio.it".to_string()]));
    assert_eq!(d.subject, "Preventivo Bcc: spia@x.it", "a line break cannot add a header");
    assert_eq!(
        (d.from.as_str(), d.created_by.as_str(), d.source.as_str()),
        ("assistente@casa.example", "user", "mail:<richiesta@cliente.it>")
    );
    assert_eq!(d.attachments.len(), 1);
    assert_eq!(
        (d.attachments[0].name.as_str(), d.attachments[0].mime.as_str(), d.attachments[0].size),
        ("preventivo.pdf", "application/pdf", 27)
    );
    assert_eq!(d.content_hash.len(), 64);
    assert!(r.mails().is_empty(), "a draft is not sent");
    assert_eq!(r.e.email_drafts(Some("draft")).unwrap().len(), 1);
    assert!(r.e.email_drafts(Some("sent")).unwrap().is_empty());
    // The owner approves this content.
    let sent = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap();
    assert_eq!(sent.status, "sent");
    assert!(sent.sent_at.is_some());
    assert_eq!(sent.message_id.as_deref(), Some(format!("<draft-{}@aiw.local>", d.id).as_str()));
    let mails = r.mails();
    assert_eq!(mails.len(), 1);
    let (from, rcpt, data) = &mails[0];
    assert_eq!(from, "assistente@casa.example");
    assert_eq!(rcpt, &vec!["Anna@Cliente.it".to_string(), "capo@esempio.it".to_string(), "archivio@esempio.it".to_string()]);
    assert_eq!(header(data, "To"), Some("Anna@Cliente.it"));
    assert_eq!(header(data, "Cc"), Some("capo@esempio.it"));
    assert!(header(data, "Bcc").is_none() && !data.contains("archivio@esempio.it"), "hidden recipients are in the envelope only");
    assert_eq!(header(data, "Subject"), Some("Preventivo Bcc: spia@x.it"));
    assert_eq!(header(data, "In-Reply-To"), Some("<richiesta@cliente.it>"));
    assert_eq!(header(data, "Message-ID"), Some(format!("<draft-{}@aiw.local>", d.id).as_str()));
    assert!(header(data, "Auto-Submitted").is_none(), "an approved message is not an automatic answer");
    assert!(header(data, "Content-Type").unwrap().starts_with("multipart/mixed"));
    // The text and the attachment are in the message, intact.
    use base64::Engine as _;
    let parts: Vec<&str> = data.split("\r\n\r\n").collect();
    let text_b64: String = parts[2].split("\r\n--").next().unwrap().replace("\r\n", "");
    let text = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(text_b64).unwrap()).unwrap();
    assert!(text.contains(".Una riga che comincia con un punto"), "{text}");
    assert!(data.contains("filename=\"preventivo.pdf\"") && data.contains("Content-Type: application/pdf"));
    let file_b64: String = parts[3].split("\r\n--").next().unwrap().replace("\r\n", "");
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(file_b64).unwrap(), b"%PDF-1.4 contenuto di prova");
    // The log says who wrote it, who approved it and that it went; never the text of the message.
    let log: Vec<AuditEntry> = r.e.audit().into_iter().filter(|a| a.action.starts_with("email.")).collect();
    let actions: Vec<&str> = log.iter().map(|a| a.action.as_str()).collect();
    for want in ["email.draft", "email.approve", "email.send"] {
        assert!(actions.contains(&want), "{want} in {actions:?}");
    }
    assert!(log.iter().all(|a| !a.detail.contains("Buongiorno Anna")), "the body is not logged");
    assert!(log.iter().any(|a| a.action == "email.send" && a.detail.contains("Anna@Cliente.it") && a.detail.contains("preventivo.pdf")));
    assert!(log.iter().any(|a| a.action == "email.approve" && a.decision.as_deref() == Some("approved")));
}

#[tokio::test]
async fn the_approval_names_the_final_content_and_a_changed_draft_is_refused() {
    let r = rig(vec![]).await;
    r.write("nota.txt", b"versione uno");
    let d =
        r.e.email_draft_create(EmailDraftCreate { attachments: vec!["nota.txt".into()], ..draft("anna@cliente.it", "Ciao", "Testo uno") })
            .unwrap();
    let wrong = EmailDraftSend { approved_hash: "0".repeat(64), resend: false };
    let err = r.e.email_draft_send(&d.id, wrong).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    assert!(r.mails().is_empty());
    // A change after the review is a new revision with a new hash: the old approval does not cover it.
    let changed =
        r.e.email_draft_patch(
            &d.id,
            EmailDraftPatch {
                body: Some("Testo due, con un altro destinatario nascosto".into()),
                bcc: Some(vec!["altri@x.it".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(changed.revision, d.revision + 1);
    assert_ne!(changed.content_hash, d.content_hash);
    let err = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("cambiata"), "{err}");
    assert!(r.mails().is_empty(), "nothing went out with a stale approval");
    // The attachment is the copy made when it was attached: changing the original does not change what is sent.
    r.write("nota.txt", b"versione DUE, cambiata di nascosto");
    let sent = r.e.email_draft_send(&d.id, send_req(&changed)).await.unwrap();
    assert_eq!(sent.status, "sent");
    let data = r.mails()[0].2.clone();
    assert!(r.mails()[0].1.contains(&"altri@x.it".to_string()), "the new hidden recipient is in the envelope: {:?}", r.mails()[0].1);
    use base64::Engine as _;
    let file_b64: String = data.split("\r\n\r\n").nth(3).unwrap().split("\r\n--").next().unwrap().replace("\r\n", "");
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(file_b64).unwrap(), b"versione uno");
    // A sender that changed after the review also invalidates the approval.
    let d2 = r.e.email_draft_create(draft("bruno@x.it", "Altro", "Altro testo")).unwrap();
    r.e.channel_patch(
        &r.channel,
        ChannelPatch { settings: Some(json!({"from": "altro@casa.example"}).as_object().unwrap().clone()), ..Default::default() },
    )
    .unwrap();
    let err = r.e.email_draft_send(&d2.id, send_req(&d2)).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
}

#[tokio::test]
async fn sending_twice_sends_once() {
    let r = rig(vec![]).await;
    let d = r.e.email_draft_create(draft("anna@cliente.it", "Una volta sola", "Testo")).unwrap();
    // Two requests at the same moment (a double click, an impatient client).
    let (a, b) = tokio::join!(r.e.email_draft_send(&d.id, send_req(&d)), r.e.email_draft_send(&d.id, send_req(&d)));
    assert!([&a, &b].iter().any(|x| x.is_ok()), "{a:?} {b:?}");
    for x in [&a, &b] {
        if let Err(e) = x {
            assert!(matches!(e, Error::Conflict(_)), "{e}");
        }
    }
    assert_eq!(r.mails().len(), 1, "one message");
    // A retry afterwards gets the draft as it is: sent, and nothing more goes out.
    let again = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap();
    assert_eq!(again.status, "sent");
    assert_eq!(r.mails().len(), 1);
    assert_eq!(r.e.audit().iter().filter(|x| x.action == "email.send").count(), 1);
    // The same content as a new draft is pointed out as a duplicate.
    let twin = r.e.email_draft_create(draft("anna@cliente.it", "Una volta sola", "Testo")).unwrap();
    assert_eq!(twin.duplicate_of.as_deref(), Some(d.id.as_str()));
}

#[tokio::test]
async fn refusing_or_cancelling_never_sends() {
    let r = rig(vec![]).await;
    let a = r.e.email_draft_create(draft("anna@cliente.it", "Da rifiutare", "Testo")).unwrap();
    let b = r.e.email_draft_create(draft("bruno@cliente.it", "Da annullare", "Testo")).unwrap();
    let rejected = r.e.email_draft_reject(&a.id, Some("  il tono\nnon va  ")).unwrap();
    assert_eq!((rejected.status.as_str(), rejected.note.as_deref()), ("rejected", Some("il tono non va")));
    let cancelled = r.e.email_draft_cancel(&b.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    for d in [&a, &b] {
        let e = r.e.email_draft_send(&d.id, send_req(d)).await.unwrap_err();
        assert!(matches!(e, Error::Conflict(_)), "{e}");
        assert!(
            r.e.email_draft_patch(&d.id, EmailDraftPatch { body: Some("x".into()), ..Default::default() }).is_err(),
            "a finished draft is not edited"
        );
        assert!(r.e.email_draft_reject(&d.id, None).is_err() && r.e.email_draft_cancel(&d.id).is_err());
    }
    assert!(r.mails().is_empty());
    let actions: Vec<String> = r.e.audit().into_iter().map(|x| x.action).collect();
    assert!(actions.contains(&"email.reject".to_string()) && actions.contains(&"email.cancel".to_string()));
    assert!(!actions.contains(&"email.send".to_string()) && !actions.contains(&"email.approve".to_string()));
    // A finished draft can be removed from the list; one that is waiting cannot.
    let c = r.e.email_draft_create(draft("carla@cliente.it", "In attesa", "Testo")).unwrap();
    assert!(r.e.email_draft_delete(&c.id).is_err());
    r.e.email_draft_delete(&a.id).unwrap();
    assert!(r.e.email_draft(&a.id).is_err());
}

#[tokio::test]
async fn a_failure_is_told_apart_from_a_message_that_may_have_gone_and_is_never_retried_by_itself() {
    let r = rig(vec![]).await;
    // The server refuses a recipient: nothing went out, not even to the others.
    let d =
        r.e.email_draft_create(EmailDraftCreate { cc: vec!["rotto@cliente.it".into()], ..draft("anna@cliente.it", "Rifiutata", "Testo") })
            .unwrap();
    *r.smtp.fault.lock().unwrap() = SmtpFault::RejectRecipient("rotto@cliente.it".into());
    let e = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err();
    assert!(e.to_string().contains("rotto@cliente.it"), "{e}");
    let after = r.e.email_draft(&d.id).unwrap();
    assert_eq!(after.status, "failed");
    assert!(after.note.as_deref().unwrap().contains("non è partito nulla"), "{:?}", after.note);
    assert!(r.mails().is_empty());
    // The server answers «refused» after the message: it did not take it, so it can be tried again.
    *r.smtp.fault.lock().unwrap() = SmtpFault::RejectMessage;
    assert!(r.e.email_draft_send(&d.id, send_req(&after)).await.is_err());
    assert_eq!(r.e.email_draft(&d.id).unwrap().status, "failed");
    // Fixed: the same approved content goes.
    *r.smtp.fault.lock().unwrap() = SmtpFault::None;
    r.e.email_draft_patch(&d.id, EmailDraftPatch { cc: Some(vec![]), ..Default::default() }).unwrap();
    let fixed = r.e.email_draft(&d.id).unwrap();
    assert_eq!(fixed.status, "draft", "editing a draft that failed makes it a draft again");
    assert_eq!(r.e.email_draft_send(&d.id, send_req(&fixed)).await.unwrap().status, "sent");
    assert_eq!(r.mails().len(), 1);
    // The connection drops after the message was handed over: nobody knows.
    let u = r.e.email_draft_create(draft("luca@cliente.it", "Incerta", "Testo")).unwrap();
    *r.smtp.fault.lock().unwrap() = SmtpFault::DropAfterData;
    let e = r.e.email_draft_send(&u.id, send_req(&u)).await.unwrap_err();
    assert!(e.to_string().contains("Controlla Posta inviata"), "{e}");
    assert_eq!(r.e.email_draft(&u.id).unwrap().status, "uncertain");
    assert_eq!(r.mails().len(), 2, "the fake server did take it");
    *r.smtp.fault.lock().unwrap() = SmtpFault::None;
    // It is not sent again by itself, and not by an ordinary «send»: only when the owner says so.
    // A repeat that the server refuses cleanly does not make the first attempt «not sent»: it stays unknown.
    *r.smtp.fault.lock().unwrap() = SmtpFault::RejectRecipient("luca@cliente.it".into());
    let e = r.e.email_draft_send(&u.id, EmailDraftSend { approved_hash: u.content_hash.clone(), resend: true }).await.unwrap_err();
    assert!(e.to_string().contains("primo tentativo potrebbe essere arrivato"), "{e}");
    assert_eq!(r.e.email_draft(&u.id).unwrap().status, "uncertain");
    assert_eq!(r.mails().len(), 2);
    *r.smtp.fault.lock().unwrap() = SmtpFault::None;
    let e = r.e.email_draft_send(&u.id, send_req(&u)).await.unwrap_err();
    assert!(matches!(e, Error::Conflict(_)) && e.to_string().contains("non so se il messaggio è partito"), "{e}");
    assert_eq!(r.mails().len(), 2);
    let again = r.e.email_draft_send(&u.id, EmailDraftSend { approved_hash: u.content_hash.clone(), resend: true }).await.unwrap();
    assert_eq!(again.status, "sent");
    assert_eq!(r.mails().len(), 3, "sent again on the owner's explicit say-so, with the same Message-ID");
    let ids: Vec<String> = r.mails().iter().skip(1).map(|m| header(&m.2, "Message-ID").unwrap().to_string()).collect();
    assert_eq!(ids[0], ids[1]);
    assert!(r.e.audit().iter().any(|a| a.action == "email.approve" && a.detail.contains("ripetuto")));
    assert!(r.e.audit().iter().any(|a| a.action == "email.failed"));
}

#[tokio::test]
async fn a_send_the_app_never_finished_is_uncertain_after_a_restart() {
    let r = rig(vec![]).await;
    let d = r.e.email_draft_create(draft("anna@cliente.it", "Interrotta", "Testo")).unwrap();
    // The state a crash in the middle of a send leaves behind.
    let mut stuck = r.e.draft_doc(&d.id).unwrap();
    stuck.status = "sending".into();
    stuck.updated_at = (aiw_core::datetime::Utc::now() - aiw_core::datetime::Duration::minutes(10)).to_rfc3339();
    r.e.0.store.put_doc("email_draft", &stuck.id, &stuck).unwrap();
    let after = r.e.email_draft(&d.id).unwrap();
    assert_eq!(after.status, "uncertain");
    assert!(after.note.as_deref().unwrap().contains("interrotto"));
    assert!(r.mails().is_empty());
}

#[tokio::test]
async fn the_kill_switch_and_a_channel_that_is_off_stop_the_sending() {
    let r = rig(vec![]).await;
    let d = r.e.email_draft_create(draft("anna@cliente.it", "Ferma", "Testo")).unwrap();
    r.e.channel_patch(&r.channel, ChannelPatch { enabled: Some(false), ..Default::default() }).unwrap();
    let e = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err();
    assert!(matches!(e, Error::Denied(_)) && e.to_string().contains("spento"), "{e}");
    assert_eq!(r.e.email_draft(&d.id).unwrap().status, "draft", "a refusal leaves it as it was");
    r.e.channel_patch(&r.channel, ChannelPatch { enabled: Some(true), ..Default::default() }).unwrap();
    // The emergency stop: nothing leaves, even though the person turns the channel back on later in this test.
    r.e.kill().await;
    let e = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err();
    assert!(matches!(e, Error::Denied(_)), "{e}");
    assert!(r.mails().is_empty());
}

#[tokio::test]
async fn attachments_follow_the_rules_of_the_file_tools() {
    let r = rig(vec![]).await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("segreto.txt"), "fuori dal workspace").unwrap();
    let with = |path: String| EmailDraftCreate { attachments: vec![path], ..draft("anna@cliente.it", "Allegato", "Testo") };
    // Outside the workspace, in the app's data folder, a link that leads out: refused, so the file is never read.
    let e = r.e.email_draft_create(with(outside.path().join("segreto.txt").display().to_string())).unwrap_err();
    assert!(matches!(e, Error::Denied(_)), "{e}");
    let db = r.e.0.dirs.root.join("aiw.db");
    let e = r.e.email_draft_create(with(db.display().to_string())).unwrap_err();
    assert!(matches!(e, Error::Denied(_)), "{e}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path().join("segreto.txt"), r.ws.path().join("collegamento.txt")).unwrap();
        assert!(matches!(r.e.email_draft_create(with("collegamento.txt".into())).unwrap_err(), Error::Denied(_)));
    }
    assert!(matches!(r.e.email_draft_create(with("../fuori.txt".into())).unwrap_err(), Error::Denied(_)));
    // Size and number.
    r.write("grande.bin", &vec![7u8; (5 << 20) + 1]);
    assert!(matches!(r.e.email_draft_create(with("grande.bin".into())).unwrap_err(), Error::Invalid(_)));
    for i in 0..6 {
        r.write(&format!("f{i}.txt"), format!("file {i}").as_bytes());
    }
    let many =
        EmailDraftCreate { attachments: (0..6).map(|i| format!("f{i}.txt")).collect(), ..draft("anna@cliente.it", "Troppi", "Testo") };
    assert!(r.e.email_draft_create(many).unwrap_err().to_string().contains("al massimo 5 allegati"));
    assert!(r.e.email_drafts(None).unwrap().is_empty(), "a refused draft leaves nothing behind");
    // A good one, removed again by its hash.
    let ok = r.e.email_draft_create(with("f0.txt".into())).unwrap();
    let sha = ok.attachments[0].sha256.clone();
    let without = r.e.email_draft_patch(&ok.id, EmailDraftPatch { remove_attachments: vec![sha], ..Default::default() }).unwrap();
    assert!(without.attachments.is_empty());
}

#[tokio::test]
async fn addresses_and_sizes_are_checked() {
    let r = rig(vec![]).await;
    for bad in ["senza-chiocciola", "a b@x.it", "a@x.it,b@x.it", "<a@x.it>", "a@x.it\r\nBcc: z@z.it"] {
        let e = r.e.email_draft_create(draft(bad, "x", "y")).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{bad}: {e}");
    }
    let many: Vec<String> = (0..21).map(|i| format!("p{i}@x.it")).collect();
    assert!(r.e.email_draft_create(EmailDraftCreate { to: many, ..Default::default() }).unwrap_err().to_string().contains("al massimo 20"));
    assert!(r.e.email_draft_create(draft("a@x.it", &"s".repeat(301), "y")).is_err());
    assert!(r.e.email_draft_create(draft("a@x.it", "x", &"b".repeat(100_001))).is_err());
    // No recipient: it can be saved to be completed, but not sent.
    let d =
        r.e.email_draft_create(EmailDraftCreate { subject: "Senza destinatario".into(), body: "x".into(), ..Default::default() }).unwrap();
    assert!(matches!(r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err(), Error::Invalid(_)));
    // Without an email channel there is nothing to send through.
    r.e.channel_delete(&r.channel).unwrap();
    assert!(r.e.email_draft_create(draft("a@x.it", "x", "y")).is_err());
    assert!(r.e.tool_registry().get("email_draft").unwrap().available().is_err());
}

#[tokio::test]
async fn the_assistant_can_prepare_and_revise_a_draft_but_has_no_way_to_send_it() {
    let r = rig(vec![]).await;
    let reg = r.e.tool_registry();
    let tool = reg.get("email_draft").expect("registered");
    assert_eq!(tool.risk(), aiw_tools::Risk::Write, "it writes: not offered to the read-only runs");
    assert!(tool.available().is_ok());
    // No registered tool can send mail, and the read-only runs of the proactive assistant and of the channels do not get this one.
    assert!(!reg.all().iter().any(|t| t.name().contains("send") && t.name().contains("mail") || t.name() == "email_send"));
    for tools in [r.e.channel_agent(channels::RunKind::ReadOnly, "operator").unwrap().tools, r.e.proactive_agent("operator").unwrap().tools]
    {
        assert!(!tools.iter().any(|t| t == "email_draft"), "{tools:?}");
    }
    assert!(r.e.channel_agent(channels::RunKind::Ask, "operator").unwrap().tools.iter().any(|t| t == "*" || t == "email_draft"));
    // A call from a run: a draft in review, nothing sent.
    let ctx = aiw_tools::ToolContext::new("run_x", r.ws.path().to_path_buf());
    let args = json!({"to": ["anna@cliente.it"], "subject": "Risposta", "body": "Gentile Anna, confermo.", "in_reply_to": "<q@cliente.it>", "source": "mail:<q@cliente.it>"});
    let out = tool.call(args.clone(), &ctx).await.unwrap();
    assert!(out.content.contains("Non è stata inviata"), "{}", out.content);
    let all = r.e.email_drafts(None).unwrap();
    assert_eq!(all.len(), 1);
    let d = all[0].clone();
    assert_eq!((d.created_by.as_str(), d.status.as_str(), d.source.as_str()), ("assistant", "draft", "mail:<q@cliente.it>"));
    assert!(r.mails().is_empty());
    // The same call again (a retry of the model) does not make a second draft.
    let again = tool.call(args, &ctx).await.unwrap();
    assert!(again.content.contains("già una bozza identica"), "{}", again.content);
    assert_eq!(r.e.email_drafts(None).unwrap().len(), 1);
    // It revises its own draft, which becomes a new revision (so the old approval no longer counts) ...
    let out = tool.call(json!({"draft_id": d.id, "body": "Gentile Anna, confermo e allego il programma."}), &ctx).await.unwrap();
    assert!(out.content.contains("revisione 2"), "{}", out.content);
    let revised = r.e.email_draft(&d.id).unwrap();
    assert_ne!(revised.content_hash, d.content_hash);
    // ... but not one the owner wrote, and not one that is not waiting any more.
    let mine = r.e.email_draft_create(draft("bruno@x.it", "Mia", "Mio testo")).unwrap();
    let e = tool.call(json!({"draft_id": mine.id, "body": "cambiato"}), &ctx).await.unwrap_err();
    assert!(matches!(e, Error::Denied(_)), "{e}");
    r.e.email_draft_send(&d.id, send_req(&revised)).await.unwrap();
    let e = tool.call(json!({"draft_id": d.id, "body": "dopo l'invio"}), &ctx).await.unwrap_err();
    assert!(matches!(e, Error::Conflict(_)), "{e}");
    // After it went, an identical draft is not made again.
    let after = tool.call(json!({"to": ["anna@cliente.it"], "subject": "Risposta", "body": "Gentile Anna, confermo e allego il programma.", "in_reply_to": "<q@cliente.it>"}), &ctx).await.unwrap();
    assert!(after.content.contains("già stata inviata"), "{}", after.content);
    assert_eq!(r.mails().len(), 1);
}

#[tokio::test]
async fn a_run_of_the_assistant_leaves_a_draft_through_the_normal_policy_and_a_channel_cannot_approve_it() {
    let turns = vec![
        ScriptedBackend::tool_turn(
            "c1",
            "email_draft",
            json!({"to": ["anna@cliente.it"], "subject": "Conferma", "body": "Confermo l'appuntamento.", "source": "calendar:Riunione"}),
        ),
        ScriptedBackend::text_turn("Ho preparato la bozza: la trovi nell'app."),
        ScriptedBackend::text_turn("Non decido io le bozze."),
    ];
    let r = rig(turns).await;
    let s = r.e.create_session("operator", Some("Posta".into())).unwrap();
    let run = r.e.start_run(&s.id, "Rispondi ad Anna per la riunione".into()).await.unwrap();
    // The write goes through the policy like any other: here it is asked.
    until("confirmation or end", || r.e.run_done(&run) || !r.e.approvals().is_empty()).await;
    if let Some(a) = r.e.approvals().first() {
        assert_eq!(a.tool, "email_draft");
        r.e.respond_approval_as(&a.id.clone(), true, false, true).unwrap();
    }
    super::tests::wait_finished(&r.e, &run).await;
    let drafts = r.e.email_drafts(Some("draft")).unwrap();
    assert_eq!(drafts.len(), 1, "{:?}", r.e.messages(&s.id).unwrap().last().map(|m| m.content.clone()));
    assert_eq!((drafts[0].created_by.as_str(), drafts[0].source.as_str()), ("assistant", "calendar:Riunione"));
    assert_eq!(drafts[0].session_id.as_deref(), Some(s.id.as_str()), "linked to the conversation it came from");
    assert!(r.mails().is_empty());
    // A message on the channel, even a «sì» from the person on the list, does not touch the draft.
    r.imap.add("INBOX", mail(2, &raw("Mario <mario@example.it>", "Bozza", "<s1@example.it>", AUTH_OK, "sì")));
    until("the channel answered the message", || r.mails().iter().any(|m| header(&m.2, "Auto-Submitted").is_some())).await;
    assert_eq!(r.e.email_draft(&drafts[0].id).unwrap().status, "draft");
    assert!(!r.mails().iter().any(|m| header(&m.2, "Message-ID").is_some_and(|i| i.contains(&drafts[0].id))));
}

#[tokio::test]
async fn drafts_are_distinct_from_the_automatic_replies_of_the_channel() {
    let r = rig(vec![]).await;
    // The reply of the channel is an automatic answer to the people on its list, in their thread.
    let sent = r.e.channel_send_text(&r.channel, Some("mario@example.it"), "Promemoria").await.unwrap();
    assert!(sent.contains("Messaggio inviato"));
    assert_eq!(header(&r.mails()[0].2, "Auto-Submitted"), Some("auto-replied"));
    // `channel_send` refuses anyone who is not on the list; a draft is the way to write to someone else, with a review.
    assert!(r.e.channel_send_text(&r.channel, Some("anna@cliente.it"), "Ciao").await.is_err());
    let d = r.e.email_draft_create(draft("anna@cliente.it", "Ciao", "Ciao Anna")).unwrap();
    assert_eq!(r.mails().len(), 1, "the draft is waiting, not sent");
    r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap();
    assert_eq!(r.mails().len(), 2);
    assert!(header(&r.mails()[1].2, "Auto-Submitted").is_none());
}

#[tokio::test]
async fn a_proactive_suggestion_becomes_a_draft_only_through_the_owner_twice() {
    use super::proactive::Wake;
    let instruction = "Prepara una bozza di risposta ad anna@cliente.it con il preventivo";
    let turns = vec![
        ScriptedBackend::text_turn(r#"{"act": true, "item": 1, "why": "file nuovo"}"#),
        ScriptedBackend::text_turn(&format!(
            r#"{{"title": "Rispondi ad Anna", "noticed": "Anna ha chiesto un preventivo.", "instruction": "{instruction}"}}"#
        )),
        ScriptedBackend::tool_turn(
            "c1",
            "email_draft",
            json!({"to": ["anna@cliente.it"], "subject": "Preventivo", "body": "Gentile Anna, ecco il preventivo.", "source": "mail:<richiesta@cliente.it>"}),
        ),
        ScriptedBackend::text_turn("La bozza è pronta nell'app."),
    ];
    let r = rig(turns).await;
    r.e.set_config("global", "proactive.level", json!("propose")).unwrap();
    r.e.set_config("global", "proactive.briefing_time", json!("23:59")).unwrap();
    let inbox = r.ws.path().join("Posta");
    std::fs::create_dir(&inbox).unwrap();
    r.e.set_config("global", "proactive.watch_folders", json!([inbox.display().to_string()])).unwrap();
    r.e.proactive_tick_at(Wake::Interval, aiw_core::datetime::Local::now() - aiw_core::datetime::Duration::seconds(5)).await.unwrap();
    std::fs::write(inbox.join("richiesta-anna.txt"), "preventivo per favore").unwrap();
    let tick =
        r.e.proactive_tick_at(Wake::Interval, aiw_core::datetime::Local::now() + aiw_core::datetime::Duration::seconds(5)).await.unwrap();
    assert_eq!(tick.outcome, "proposed", "{tick:?}");
    // The assistant is told how an email gets written, and its read-only run has no way to write one.
    {
        let reqs = r.backend.requests.lock().unwrap();
        assert!(reqs[1].messages.iter().any(|m| m.text().contains("email_draft") && m.text().contains("le email non partono da sole")));
        assert!(!reqs[1].tools.iter().any(|t| t.function.name == "email_draft"), "the proactive run only reads");
    }
    assert!(r.e.email_drafts(None).unwrap().is_empty(), "a suggestion is not a draft");
    // The owner approves the proposal: a normal run, under the usual policy, writes the draft. Nothing is sent.
    let p = r.e.proposals(Some("open"), Some("proposal")).unwrap().remove(0);
    assert_eq!(p.instruction, instruction);
    let approved = r.e.proposal_approve(&p.id, None).await.unwrap();
    let run = approved.run_id.clone().unwrap();
    until("the draft or a confirmation", || r.e.run_done(&run) || !r.e.approvals().is_empty()).await;
    if let Some(a) = r.e.approvals().first() {
        assert_eq!(a.tool, "email_draft");
        r.e.respond_approval_as(&a.id.clone(), true, false, true).unwrap();
    }
    super::tests::wait_finished(&r.e, &run).await;
    let drafts = r.e.email_drafts(Some("draft")).unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!((drafts[0].created_by.as_str(), drafts[0].source.as_str()), ("assistant", "mail:<richiesta@cliente.it>"));
    assert_eq!(drafts[0].session_id, approved.session_id);
    assert!(r.mails().is_empty(), "the second approval is the owner's, on the draft");
    // The owner reads the draft and approves it.
    let sent = r.e.email_draft_send(&drafts[0].id, send_req(&drafts[0])).await.unwrap();
    assert_eq!(sent.status, "sent");
    assert_eq!(r.mails().len(), 1);
}

#[tokio::test]
async fn invisible_and_direction_characters_never_get_into_a_draft() {
    let r = rig(vec![]).await;
    // Zero-width and «tag» characters can carry text the owner cannot read on the card; a right-to-left override makes a file
    // name read as another one.
    let tag = |s: &str| s.chars().map(|c| char::from_u32(0xE0000 + c as u32).unwrap()).collect::<String>();
    r.write("rapporto\u{202E}fdp.exe", b"x");
    let d =
        r.e.email_draft_create(EmailDraftCreate {
            subject: format!("Ci\u{200B}ao\u{202E} {}", tag("segreto")),
            body: format!("Testo{}\u{FEFF} visibile\u{2066}", tag("password=hunter2")),
            attachments: vec!["rapporto\u{202E}fdp.exe".into()],
            ..draft("anna\u{200B}@cliente.it", "", "")
        })
        .unwrap();
    assert_eq!((d.subject.as_str(), d.body.as_str()), ("Ciao", "Testo visibile"));
    assert_eq!(d.to, vec!["anna@cliente.it"]);
    assert_eq!(d.attachments[0].name, "rapportofdp.exe");
    let edited = r.e.email_draft_patch(&d.id, EmailDraftPatch { body: Some(format!("a{}b", tag("x"))), ..Default::default() }).unwrap();
    assert_eq!(edited.body, "ab");
}

#[tokio::test]
async fn an_edit_made_on_a_draft_that_changed_meanwhile_is_refused_instead_of_overwriting_it() {
    let r = rig(vec![]).await;
    let d = r.e.email_draft_create(draft("anna@cliente.it", "Ciao", "Testo uno")).unwrap();
    // The assistant revises it while the owner has the editor open on revision 1.
    let tool = r.e.tool_registry().get("email_draft").unwrap();
    let ctx = aiw_tools::ToolContext::new("run_x", r.ws.path().to_path_buf());
    // (a draft the owner wrote cannot be revised by the assistant: make it the assistant's)
    let a = tool.call(json!({"to": ["anna@cliente.it"], "subject": "Altro", "body": "Primo"}), &ctx).await.unwrap();
    assert!(a.content.contains("creata"));
    let theirs = r.e.email_drafts(None).unwrap().into_iter().find(|x| x.created_by == "assistant").unwrap();
    tool.call(json!({"draft_id": theirs.id, "body": "Rivisto dall'assistente"}), &ctx).await.unwrap();
    let e =
        r.e.email_draft_patch(
            &theirs.id,
            EmailDraftPatch { body: Some("Il mio testo".into()), expected_revision: Some(theirs.revision), ..Default::default() },
        )
        .unwrap_err();
    assert!(matches!(e, Error::Conflict(_)) && e.to_string().contains("è cambiata mentre la modificavi"), "{e}");
    assert_eq!(r.e.email_draft(&theirs.id).unwrap().body, "Rivisto dall'assistente", "nothing was overwritten");
    // With the revision they are looking at, the edit goes through.
    let now = r.e.email_draft(&theirs.id).unwrap();
    let ok =
        r.e.email_draft_patch(
            &theirs.id,
            EmailDraftPatch { body: Some("Il mio testo".into()), expected_revision: Some(now.revision), ..Default::default() },
        )
        .unwrap();
    assert_eq!((ok.body.as_str(), ok.revision), ("Il mio testo", now.revision + 1));
    let _ = d;
}

#[tokio::test]
async fn a_draft_whose_attachment_bytes_no_longer_match_what_was_approved_is_not_sent() {
    let r = rig(vec![]).await;
    r.write("a.txt", b"originale");
    let d =
        r.e.email_draft_create(EmailDraftCreate { attachments: vec!["a.txt".into()], ..draft("anna@cliente.it", "Allegato", "Testo") })
            .unwrap();
    // The stored copy is damaged (or swapped) after the review.
    let mut files: Vec<serde_json::Value> = r.e.0.store.get_doc("email_draft_files", &d.id).unwrap().unwrap();
    {
        use base64::Engine as _;
        files[0]["data"] = json!(base64::engine::general_purpose::STANDARD.encode(b"altro contenuto"));
    }
    r.e.0.store.put_doc("email_draft_files", &d.id, &files).unwrap();
    let e = r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap_err();
    assert!(matches!(e, Error::Conflict(_)) && e.to_string().contains("non corrispondono"), "{e}");
    assert_eq!(r.e.email_draft(&d.id).unwrap().status, "draft", "refused before anything was claimed");
    assert!(r.mails().is_empty());
    // A corrupt copy is refused too, never sent as an empty file.
    files[0]["data"] = json!("!!non-base64!!");
    r.e.0.store.put_doc("email_draft_files", &d.id, &files).unwrap();
    assert!(r.e.email_draft_send(&d.id, send_req(&d)).await.is_err());
    assert!(r.mails().is_empty());
}

#[tokio::test]
async fn a_policy_rule_on_the_recipient_sees_every_recipient() {
    let r = rig(vec![]).await;
    let tool = r.e.tool_registry().get("email_draft").unwrap();
    let subject = tool.subject(&json!({"to": ["a@x.it"], "cc": ["b@y.it"], "bcc": ["c@z.it"]}));
    assert_eq!(subject.as_deref(), Some("a@x.it, b@y.it, c@z.it"));
    assert_eq!(tool.subject(&json!({"to": "solo@x.it"})).as_deref(), Some("solo@x.it"));
    assert_eq!(tool.subject(&json!({})), None);
}

#[tokio::test]
async fn a_message_with_a_long_subject_and_many_recipients_goes_out_as_plain_ascii_with_short_lines() {
    let r = rig(vec![]).await;
    r.write("è.txt", b"allegato");
    let d =
        r.e.email_draft_create(EmailDraftCreate {
            to: (0..12).map(|i| format!("destinatario.numero.{i}@un-dominio-molto-lungo.example.org")).collect(),
            subject: "Perché è così? ".repeat(20),
            body: "Testo con è e ü".into(),
            attachments: vec!["è.txt".into()],
            ..Default::default()
        })
        .unwrap();
    r.e.email_draft_send(&d.id, send_req(&d)).await.unwrap();
    let data = r.mails()[0].2.clone();
    assert!(data.is_ascii(), "nothing 8-bit goes to a server that did not offer 8BITMIME");
    assert!(data.split("\r\n").all(|l| l.len() <= 78), "{:?}", data.split("\r\n").find(|l| l.len() > 78));
    assert_eq!(r.mails()[0].1.len(), 12);
}

#[tokio::test]
async fn with_the_sandbox_off_the_workspace_boundary_is_gone_but_never_the_app_data() {
    let r = rig(vec![]).await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("fuori.txt"), "fuori dal workspace").unwrap();
    let with = |path: String| EmailDraftCreate { attachments: vec![path], ..draft("anna@cliente.it", "Allegato", "Testo") };
    let far = outside.path().join("fuori.txt").display().to_string();
    // Sandboxed (the default): outside the workspace is refused.
    assert!(matches!(r.e.email_draft_create(with(far.clone())).unwrap_err(), Error::Denied(_)));
    // The owner switched the containment off: like the file tools, any readable file ...
    r.e.set_config("global", "sandbox.mode", json!("off")).unwrap();
    assert_eq!(r.e.email_draft_create(with(far)).unwrap().attachments[0].name, "fuori.txt");
    // ... except the data of the app (and the credential folders), which a file that leaves by email must never be.
    let db = r.e.0.dirs.root.join("aiw.db").display().to_string();
    let e = r.e.email_draft_create(with(db)).unwrap_err();
    assert!(matches!(e, Error::Denied(_)) && e.to_string().contains("nascosto"), "{e}");
}
