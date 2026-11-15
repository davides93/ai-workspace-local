//! Shared contexts (#72): one conversation that goes on across Telegram and email, against a fake Bot API and a fake
//! IMAP/SMTP server (and a fake Slack for the direct-message rule). No test touches the internet and none talks to a
//! real account: what is proven here is the behaviour of the engine with fake transports.

use super::tests_channels::*;
use super::*;
use aiw_runtimes::ScriptedBackend;
use aiw_tools::connectors::testing::{FakeImap, FakeSmtp, ImapOptions, Security};

struct Rig {
    e: Engine,
    backend: ScriptedBackend,
    tg: TgState,
    imap: FakeImap,
    smtp: FakeSmtp,
    telegram: String,
    email: String,
    ws: tempfile::TempDir,
    _d: tempfile::TempDir,
}

/// An engine whose model is a script we can read back, with a Telegram channel (allowed: 111) and an email channel
/// (allowed: mario@example.it), both turned on.
async fn rig(turns: Vec<Vec<aiw_runtimes::ChatDelta>>, tg_mode: &str, mail_mode: &str, agent: Option<&str>) -> Rig {
    let backend = ScriptedBackend::new(turns);
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        device: None,
        extra_bin_dirs: vec![],
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
    let (base, tg) = fake_telegram().await;
    let imap = FakeImap::start(ImapOptions::new(Security::StartTls, &[("INBOX", vec![])])).await;
    let smtp = FakeSmtp::start(Security::StartTls, "assistente@casa.example", "pw-smtp").await;
    e.set_tls_roots_for_tests(vec![imap.cert.clone(), smtp.cert.clone()]);
    e.set_channel_endpoints_for_tests(Some(base), None, 0, 20, 60);
    let tgc = e
        .channel_create(ChannelCreate {
            kind: "telegram".into(),
            name: "Il mio bot".into(),
            agent_id: agent.map(String::from),
            mode: Some(tg_mode.into()),
            allowed: vec!["111".into()],
            secrets: [("bot_token".to_string(), TOKEN.to_string())].into(),
            ..Default::default()
        })
        .unwrap();
    let settings = json!({
        "imap_host": "127.0.0.1", "imap_port": imap.addr.port(), "imap_user": "mario",
        "smtp_host": "127.0.0.1", "smtp_port": smtp.addr.port(), "smtp_user": "assistente@casa.example",
        "from": "assistente@casa.example"
    });
    let mail = e
        .channel_create(ChannelCreate {
            kind: "email".into(),
            name: "Posta".into(),
            agent_id: agent.map(String::from),
            mode: Some(mail_mode.into()),
            allowed: vec!["mario@example.it".into()],
            secrets: [("imap_password".to_string(), "segreto".to_string()), ("smtp_password".to_string(), "pw-smtp".to_string())].into(),
            settings: settings.as_object().unwrap().clone(),
        })
        .unwrap();
    for id in [&tgc.channel.id, &mail.channel.id] {
        e.channel_patch(id, ChannelPatch { enabled: Some(true), ..Default::default() }).unwrap();
    }
    let r = Rig { e, backend, tg, imap, smtp, telegram: tgc.channel.id, email: mail.channel.id, ws, _d: data };
    r.connected().await;
    r
}

impl Rig {
    async fn connected(&self) {
        let (e, a, b) = (self.e.clone(), self.telegram.clone(), self.email.clone());
        until("both channels connected", move || {
            e.channel_status(&a).unwrap().state == "connected" && e.channel_status(&b).unwrap().state == "connected"
        })
        .await;
        let (e, b) = (self.e.clone(), self.email.clone());
        until("mailbox position recorded", move || e.channel_state(&b).email_uidvalidity != 0).await;
    }

    fn link(&self) -> ContextInfo {
        self.e
            .context_create(ContextCreate {
                name: "Mario".into(),
                members: vec![
                    ContextMember { channel_id: self.telegram.clone(), sender: "111".into() },
                    ContextMember { channel_id: self.email.clone(), sender: "mario@example.it".into() },
                ],
            })
            .unwrap()
    }

    fn tg_say(&self, id: i64, text: &str) {
        self.tg.lock().unwrap().updates.push_back(msg_update(id, 111, text));
    }

    fn mail_say(&self, uid: u32, msg_id: &str, text: &str) {
        self.imap.add("INBOX", mail(uid, &raw("Mario Rossi <mario@example.it>", "Domanda", msg_id, AUTH_OK, text)));
    }

    fn tg_texts(&self) -> Vec<String> {
        sent_texts(&self.tg)
    }

    fn mail_bodies(&self) -> Vec<String> {
        self.smtp.mails.lock().unwrap().iter().map(|(_, _, d)| decoded_body(d)).collect()
    }

    /// Everything the model was sent in request `i`, as one text.
    fn prompt(&self, i: usize) -> String {
        let reqs = self.backend.requests.lock().unwrap();
        reqs[i].messages.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n")
    }

    fn requests(&self) -> usize {
        self.backend.requests.lock().unwrap().len()
    }

    fn sessions_of(&self, kind: &str) -> Vec<Session> {
        self.e.sessions().unwrap().into_iter().filter(|s| s.channel.as_deref() == Some(kind)).collect()
    }
}

fn ctx_member(channel: &str, sender: &str) -> ContextMember {
    ContextMember { channel_id: channel.into(), sender: sender.into() }
}

fn create(r: &Rig, members: Vec<ContextMember>) -> Result<ContextInfo> {
    r.e.context_create(ContextCreate { name: "Prova".into(), members })
}

#[tokio::test]
async fn without_a_link_two_channels_never_share_anything() {
    let r =
        rig(vec![ScriptedBackend::text_turn("Ricevuto"), ScriptedBackend::text_turn("Non lo so")], "read_only", "read_only", None).await;
    r.tg_say(1, "Il codice del cantiere è BLU-42, ricordalo");
    until("telegram answer", || r.tg_texts().len() == 1).await;
    r.mail_say(2, "<m1@example.it>", "Qual è il codice del cantiere?");
    until("email answer", || r.mail_bodies().len() == 1).await;
    assert!(!r.prompt(1).contains("BLU-42"), "the second channel does not see the first one's history");
    assert_eq!(r.e.sessions().unwrap().len(), 2, "one session per external conversation");
    assert!(r.e.sessions().unwrap().iter().all(|s| s.channel.as_deref() != Some("shared")));
}

#[tokio::test]
async fn the_same_conversation_goes_on_across_two_channels_without_losing_context() {
    let r = rig(
        vec![
            ScriptedBackend::text_turn("Ricevuto: il codice è BLU-42"),
            ScriptedBackend::text_turn("Il codice è BLU-42"),
            ScriptedBackend::text_turn("Su Telegram ho tutto"),
        ],
        "read_only",
        "read_only",
        None,
    )
    .await;
    let ctx = r.link();
    assert_eq!(ctx.mode, "read_only");
    assert!(ctx.members_info.iter().all(|m| m.problem.is_none()), "{:?}", ctx.members_info);
    r.tg_say(1, "Il codice del cantiere è BLU-42, ricordalo");
    until("telegram answer", || r.tg_texts().len() == 1).await;
    r.mail_say(2, "<m1@example.it>", "Qual è il codice del cantiere?");
    until("email answer", || r.mail_bodies().len() == 1).await;
    // What was said on Telegram is in the request of the turn that came from the mail.
    assert!(r.prompt(1).contains("BLU-42"), "the mail turn sees the Telegram history: {}", r.prompt(1));
    assert!(r.mail_bodies()[0].contains("BLU-42"));
    // Each answer went back where its question came from.
    assert_eq!(r.tg_texts(), vec!["Ricevuto: il codice è BLU-42".to_string()]);
    let (_, rcpt, _) = r.smtp.mails.lock().unwrap()[0].clone();
    assert_eq!(rcpt, vec!["mario@example.it".to_string()]);
    // And back: Telegram sees what the mail added.
    r.tg_say(3, "E adesso?");
    until("second telegram answer", || r.tg_texts().len() == 2).await;
    assert!(r.prompt(2).contains("Qual è il codice del cantiere?") && r.prompt(2).contains("BLU-42"));
    // One session, listed as shared, restricted like the channels.
    let sessions = r.e.sessions().unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].channel.as_deref(), Some("shared"));
    assert_eq!(sessions[0].id, ctx.context.session_id);
    assert!(sessions[0].agent_id.starts_with("channel:ro:"), "{}", sessions[0].agent_id);
    let users: Vec<String> = r.e.messages(&sessions[0].id).unwrap().into_iter().filter(|m| m.role == "user").map(|m| m.content).collect();
    assert_eq!(users.len(), 3, "{users:?}");
    // The log says the message belonged to the shared context.
    assert!(r.e.audit().iter().any(|a| a.action == "channel.in" && a.detail.contains("contesto condiviso «Mario»")));
    assert!(r.e.audit().iter().any(|a| a.action == "context.create" && a.actor == "user"));
}

#[tokio::test]
async fn linking_is_explicit_and_checked() {
    let r = rig(vec![], "read_only", "read_only", None).await;
    let (tg, mail) = (r.telegram.clone(), r.email.clone());
    let err = |res: Result<ContextInfo>| res.expect_err("must be refused").to_string();
    assert!(err(create(&r, vec![])).contains("almeno un account"));
    assert!(err(create(&r, vec![ctx_member("chn_nope", "1")])).contains("non esiste"));
    assert!(
        err(create(&r, vec![ctx_member(&tg, "222")])).contains("non è nella lista"),
        "an account that is not on the list cannot be linked"
    );
    // A group id (negative) on the list is not a person.
    r.e.channel_patch(&tg, ChannelPatch { allowed: Some(vec!["111".into(), "222".into(), "-100777".into()]), ..Default::default() })
        .unwrap();
    assert!(err(create(&r, vec![ctx_member(&tg, "-100777")])).contains("gruppo"));
    // One account per channel.
    assert!(err(create(&r, vec![ctx_member(&tg, "111"), ctx_member(&tg, "222")])).contains("compare due volte"));
    // An email channel that does not check who writes cannot take part.
    r.e.channel_patch(
        &mail,
        ChannelPatch { settings: Some(json!({"require_auth": false}).as_object().unwrap().clone()), ..Default::default() },
    )
    .unwrap();
    assert!(err(create(&r, vec![ctx_member(&mail, "mario@example.it")])).contains("controllo del mittente"));
    r.e.channel_patch(
        &mail,
        ChannelPatch { settings: Some(json!({"require_auth": true}).as_object().unwrap().clone()), ..Default::default() },
    )
    .unwrap();
    // The channels of one context use the same assistant.
    r.e.channel_patch(&mail, ChannelPatch { agent_id: Some("coder".into()), ..Default::default() }).unwrap();
    assert!(err(create(&r, vec![ctx_member(&tg, "111"), ctx_member(&mail, "Mario@Example.it")])).contains("assistenti diversi"));
    r.e.channel_patch(&mail, ChannelPatch { agent_id: Some("operator".into()), ..Default::default() }).unwrap();
    // Valid (the address is normalised) ...
    let ok = create(&r, vec![ctx_member(&tg, "111"), ctx_member(&mail, "Mario@Example.it")]).unwrap();
    assert_eq!(ok.context.members[1].sender, "mario@example.it");
    // ... an account is in one context at most ...
    assert!(err(create(&r, vec![ctx_member(&tg, "111")])).contains("fa già parte del contesto"));
    // ... and a channel of a context cannot change assistant on its own.
    let e = r.e.channel_patch(&mail, ChannelPatch { agent_id: Some("coder".into()), ..Default::default() }).expect_err("refused");
    assert!(e.to_string().contains("scollegalo prima"), "{e}");
    assert!(r.e.audit().iter().any(|a| a.action == "context.create"));
}

#[tokio::test]
async fn the_same_message_delivered_twice_makes_one_run() {
    let r = rig(
        vec![ScriptedBackend::text_turn("Una sola risposta"), ScriptedBackend::text_turn("Una sola risposta per la posta")],
        "read_only",
        "read_only",
        None,
    )
    .await;
    r.link();
    // Telegram retries: the same message under two update ids.
    let dup = |update: i64| json!({"update_id": update, "message": {"message_id": 5, "from": {"id": 111, "first_name": "Mario"}, "chat": {"id": 111, "type": "private"}, "text": "Mandami il riepilogo"}});
    {
        let mut t = r.tg.lock().unwrap();
        t.updates.push_back(dup(1));
        t.updates.push_back(dup(2));
    }
    until("answer", || !r.tg_texts().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(r.tg_texts().len(), 1, "{:?}", r.tg_texts());
    assert_eq!(r.requests(), 1, "one run, not two");
    assert!(r.e.audit().iter().any(|a| a.action == "channel.duplicate"));
    // The same mail twice (two UIDs, one Message-ID).
    r.mail_say(2, "<same@example.it>", "Scrivimi due righe");
    r.mail_say(3, "<same@example.it>", "Scrivimi due righe");
    until("email answer", || !r.mail_bodies().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(r.mail_bodies().len(), 1);
    assert_eq!(r.requests(), 2);
    // A message that only looks alike (another id) is a new one.
    r.tg_say(9, "Mandami il riepilogo");
    until("third answer", || r.tg_texts().len() == 2).await;
}

#[tokio::test]
async fn messages_of_two_channels_wait_their_turn_and_a_yes_from_the_other_channel_decides_nothing() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        ScriptedBackend::text_turn("Ho creato nota.md"),
        ScriptedBackend::text_turn("Sono le dieci"),
    ];
    let r = rig(turns, "ask", "ask", None).await;
    r.link();
    r.tg_say(1, "Crea una nota");
    until("the question in Telegram", || !r.tg_texts().is_empty()).await;
    assert_eq!(r.e.approvals().len(), 1);
    // A mail arrives while the run waits for the answer: it waits in line, nobody gets an error.
    r.mail_say(2, "<m1@example.it>", "Che ore sono?");
    // «sì» from the mail is not an answer to a question that was asked on Telegram.
    // A real mail client answers with its own quote under the word: «Il giorno … ha scritto:».
    r.mail_say(
        3,
        "<m2@example.it>",
        "sì\n\nIl giorno dom 15 nov 2026 alle ore 10:00 Assistente <assistente@casa.example> ha scritto:\n> Mi serve il tuo permesso",
    );
    until("the notice in the mail", || r.mail_bodies().iter().any(|b| b.contains("in attesa su «Il mio bot»"))).await;
    assert_eq!(r.e.approvals().len(), 1, "still waiting");
    // The buttons follow the same rule: a tap from the other channel is told where to answer, and decides nothing.
    let pending = r.e.approvals()[0].id.clone();
    assert!(r.e.channel_callback(&r.email, &["mario@example.it".to_string()], &pending, true).contains("fatta altrove"));
    assert_eq!(r.e.approvals().len(), 1);
    assert!(!r.ws.path().join("nota.md").exists(), "nothing was written");
    assert_eq!(r.requests(), 1, "the mail message waits behind the running turn");
    let keys = r.e.channel_queue_keys();
    assert_eq!(keys.len(), 1, "both channels wait in one queue: {keys:?}");
    assert!(keys[0].starts_with("ctx|"), "{keys:?}");
    assert!(!r.tg_texts().iter().chain(r.mail_bodies().iter()).any(|t| t.contains("Non riesco")), "no conflict error");
    // The answer where it was asked.
    r.tg_say(4, "Sì");
    until("both answered", || {
        r.tg_texts().iter().any(|t| t.contains("Ho creato")) && r.mail_bodies().iter().any(|b| b.contains("Sono le dieci"))
    })
    .await;
    assert!(r.ws.path().join("nota.md").exists());
    assert_eq!(r.e.sessions().unwrap().len(), 1);
    // Each answer is in its own channel.
    assert!(!r.tg_texts().iter().any(|t| t.contains("Sono le dieci")));
    assert!(!r.mail_bodies().iter().any(|b| b.contains("Ho creato")));
    assert_eq!(r.e.audit().iter().filter(|a| a.action == "approval" && a.decision.as_deref() == Some("approved")).count(), 1);
}

#[tokio::test]
async fn a_confirmation_of_a_run_started_in_the_app_is_not_answered_from_a_channel() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "app.md", "content": "x"})),
        ScriptedBackend::text_turn("fatto"),
    ];
    let r = rig(turns, "ask", "ask", None).await;
    let ctx = r.link();
    // The owner works in the shared session from the app.
    let run = r.e.start_run(&ctx.context.session_id, "Crea app.md".into()).await.unwrap();
    until("the confirmation", || r.e.approvals().len() == 1).await;
    r.tg_say(1, "sì");
    until("told where to answer", || r.tg_texts().iter().any(|t| t.contains("nell'app"))).await;
    let id = r.e.approvals()[0].id.clone();
    assert!(r.e.channel_callback(&r.telegram, &["111".to_string()], &id, true).contains("è nell'app"));
    assert_eq!(r.e.approvals().len(), 1, "a channel did not approve it");
    assert!(!r.ws.path().join("app.md").exists());
    // The owner does, from the app.
    let id = r.e.approvals()[0].id.clone();
    r.e.respond_approval_as(&id, true, false, true).unwrap();
    super::tests::wait_finished(&r.e, &run).await;
    assert!(r.ws.path().join("app.md").exists());
}

#[tokio::test]
async fn a_channel_run_cannot_use_the_management_tools_even_in_a_shared_context() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "aiw_setting_set", json!({"key": "ui.theme", "value": "dark"})),
        ScriptedBackend::text_turn("Non posso cambiarlo da qui"),
    ];
    let r = rig(turns, "ask", "ask", Some("assistant")).await;
    r.link();
    r.tg_say(1, "Metti il tema scuro");
    until("answer", || r.tg_texts().iter().any(|t| t.contains("Non posso cambiarlo"))).await;
    assert!(r.e.approvals().is_empty(), "refused before any confirmation was asked");
    assert_ne!(r.e.0.config.get_str("ui.theme").as_deref(), Some("dark"));
    assert!(r.e.audit().iter().any(|a| a.tool.as_deref() == Some("aiw_setting_set") && a.decision.as_deref() == Some("deny")));
}

#[tokio::test]
async fn the_most_restricted_channel_decides_and_it_is_checked_at_every_turn() {
    let r = rig(vec![ScriptedBackend::text_turn("uno"), ScriptedBackend::text_turn("due")], "ask", "ask", None).await;
    let ctx = r.link();
    assert_eq!(ctx.mode, "ask");
    r.tg_say(1, "Primo messaggio");
    until("first answer", || r.tg_texts().len() == 1).await;
    assert!(r.e.session(&ctx.context.session_id).unwrap().agent_id.starts_with("channel:ask:"));
    // The mail channel becomes read-only a moment later: the next turn is already read-only, for both channels.
    r.e.channel_patch(&r.email, ChannelPatch { mode: Some("read_only".into()), ..Default::default() }).unwrap();
    assert_eq!(r.e.context(&ctx.context.id).unwrap().mode, "read_only");
    r.tg_say(2, "Secondo messaggio");
    until("second answer", || r.tg_texts().len() == 2).await;
    assert!(r.e.session(&ctx.context.session_id).unwrap().agent_id.starts_with("channel:ro:"));
}

#[tokio::test]
async fn groups_and_people_who_are_not_linked_keep_their_own_conversations() {
    let r = rig(
        vec![
            ScriptedBackend::text_turn("Ricevuto BLU-42"),
            ScriptedBackend::text_turn("Nel gruppo"),
            ScriptedBackend::text_turn("Per Anna"),
        ],
        "read_only",
        "read_only",
        None,
    )
    .await;
    r.e.channel_patch(&r.telegram, ChannelPatch { allowed: Some(vec!["111".into(), "222".into()]), ..Default::default() }).unwrap();
    let ctx = r.link();
    r.tg_say(1, "Il codice del cantiere è BLU-42");
    until("shared answer", || r.tg_texts().len() == 1).await;
    // The same person writes in a group: the answer would go to everyone there, so it is not the shared conversation.
    r.tg.lock().unwrap().updates.push_back(json!({"update_id": 2, "message": {"message_id": 2, "from": {"id": 111, "first_name": "Mario", "last_name": "Rossi"}, "chat": {"id": -100777, "type": "group"}, "text": "Qual è il codice?"}}));
    until("group answer", || r.tg_texts().len() == 2).await;
    assert!(!r.prompt(1).contains("BLU-42"), "the group does not see the private history");
    // Another person on the list who is not linked.
    r.tg.lock().unwrap().updates.push_back(json!({"update_id": 3, "message": {"message_id": 3, "from": {"id": 222, "first_name": "Anna"}, "chat": {"id": 222, "type": "private"}, "text": "Dimmi il codice"}}));
    until("Anna's answer", || r.tg_texts().len() == 3).await;
    assert!(!r.prompt(2).contains("BLU-42"), "a person who is not linked sees nothing");
    let shared = r.sessions_of("shared");
    assert_eq!(shared.len(), 1);
    let history: Vec<String> = r.e.messages(&ctx.context.session_id).unwrap().into_iter().map(|m| m.content).collect();
    assert!(!history.iter().any(|m| m.contains("Qual è il codice?") || m.contains("Dimmi il codice")), "{history:?}");
    assert_eq!(r.sessions_of("telegram").len(), 2, "the group and Anna each have their own session");
}

#[tokio::test]
async fn a_mail_channel_that_stops_checking_the_sender_leaves_the_shared_conversation() {
    let r = rig(
        vec![ScriptedBackend::text_turn("Ricevuto BLU-42"), ScriptedBackend::text_turn("Non so nulla")],
        "read_only",
        "read_only",
        None,
    )
    .await;
    let ctx = r.link();
    r.tg_say(1, "Il codice del cantiere è BLU-42");
    until("shared answer", || r.tg_texts().len() == 1).await;
    // The switch is turned off after the link: the mail is answered, but a sender that could be forged reads nothing shared.
    r.e.channel_patch(
        &r.email,
        ChannelPatch { settings: Some(json!({"require_auth": false}).as_object().unwrap().clone()), ..Default::default() },
    )
    .unwrap();
    let info = r.e.context(&ctx.context.id).unwrap();
    assert!(info.members_info[1].problem.as_deref().unwrap_or("").contains("controllo del mittente"), "{:?}", info.members_info);
    let (e, b) = (r.e.clone(), r.email.clone());
    until("reconnected with the new setting", move || e.channel_status(&b).unwrap().state == "connected").await;
    r.mail_say(2, "<m1@example.it>", "Qual è il codice del cantiere?");
    until("mail answer", || !r.mail_bodies().is_empty()).await;
    assert!(!r.prompt(1).contains("BLU-42"));
    assert_eq!(r.sessions_of("email").len(), 1, "the mail has a conversation of its own");
}

#[tokio::test]
async fn an_account_leaves_when_it_is_taken_off_the_list_or_the_channel_is_deleted() {
    let r = rig(vec![], "read_only", "read_only", None).await;
    let ctx = r.link();
    // Taken off the Telegram list: the Telegram account leaves, the mail one stays.
    r.e.channel_patch(&r.telegram, ChannelPatch { allowed: Some(vec![]), ..Default::default() }).unwrap();
    let after = r.e.context(&ctx.context.id).unwrap();
    assert_eq!(after.context.members.len(), 1);
    assert_eq!(after.context.members[0].channel_id, r.email);
    assert!(r.e.session(&ctx.context.session_id).is_ok(), "the session stays while another account remains");
    // The last channel is deleted: the context goes, the session of the app stays.
    r.e.channel_delete(&r.email).unwrap();
    assert!(r.e.contexts().unwrap().is_empty());
    assert!(r.e.session(&ctx.context.session_id).is_ok());
    assert!(r.e.audit().iter().any(|a| a.action == "context.delete"));
}

#[tokio::test]
async fn deleting_a_context_keeps_the_history_and_every_account_goes_back_to_its_own_conversation() {
    let r = rig(vec![ScriptedBackend::text_turn("uno"), ScriptedBackend::text_turn("due")], "read_only", "read_only", None).await;
    let ctx = r.link();
    r.tg_say(1, "Primo");
    until("answer", || r.tg_texts().len() == 1).await;
    r.e.context_delete(&ctx.context.id).unwrap();
    assert!(r.e.context(&ctx.context.id).is_err());
    assert_eq!(r.e.messages(&ctx.context.session_id).unwrap().iter().filter(|m| m.role == "user").count(), 1, "the history is kept");
    r.tg_say(2, "Secondo");
    until("answer", || r.tg_texts().len() == 2).await;
    assert_eq!(
        r.e.messages(&ctx.context.session_id).unwrap().iter().filter(|m| m.role == "user").count(),
        1,
        "it is not written there any more"
    );
    assert_eq!(r.sessions_of("telegram").len(), 1);
}

#[tokio::test]
async fn in_slack_only_a_direct_message_is_part_of_the_shared_conversation() {
    let first = vec![
        slack_event(
            "e1",
            json!({"type": "message", "channel_type": "im", "user": "U111", "text": "il codice è BLU-42", "channel": "D1", "ts": "1.1"}),
        ),
        slack_event(
            "e2",
            json!({"type": "app_mention", "user": "U111", "text": "<@UBOT> qual è il codice?", "channel": "C1", "ts": "2.1"}),
        ),
        // The same event again (a retry), and the same message as a mention: one run.
        slack_event(
            "e3",
            json!({"type": "message", "channel_type": "im", "user": "U111", "text": "il codice è BLU-42", "channel": "D1", "ts": "1.1"}),
        ),
    ];
    let (api, fake) = fake_slack(vec![first]).await;
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("Ricevuto"), ScriptedBackend::text_turn("Nel canale")]);
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        device: None,
        extra_bin_dirs: vec![],
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
    e.set_channel_endpoints_for_tests(None, Some(api), 0, 20, 100);
    let ch = e
        .channel_create(ChannelCreate {
            kind: "slack".into(),
            allowed: vec!["U111".into()],
            secrets: [("bot_token".to_string(), "xoxb-test".to_string()), ("app_token".to_string(), "xapp-test".to_string())].into(),
            ..Default::default()
        })
        .unwrap();
    let ctx = e.context_create(ContextCreate { name: "Mario".into(), members: vec![ctx_member(&ch.channel.id, "U111")] }).unwrap();
    e.channel_patch(&ch.channel.id, ChannelPatch { enabled: Some(true), ..Default::default() }).unwrap();
    until("both answers", || fake.lock().unwrap().posts.len() == 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.lock().unwrap().posts.len(), 2, "the repeated event made no third run");
    let sessions = e.sessions().unwrap();
    let shared: Vec<_> = sessions.iter().filter(|s| s.channel.as_deref() == Some("shared")).collect();
    let own: Vec<_> = sessions.iter().filter(|s| s.channel.as_deref() == Some("slack")).collect();
    assert_eq!((shared.len(), own.len()), (1, 1), "{sessions:?}");
    assert_eq!(shared[0].id, ctx.context.session_id);
    let in_channel = backend.requests.lock().unwrap()[1].messages.iter().map(|m| m.text()).collect::<Vec<_>>().join("\n");
    assert!(!in_channel.contains("BLU-42"), "a mention in a channel does not read the private history");
    let posts = fake.lock().unwrap().posts.clone();
    assert_eq!(posts[1]["channel"], "C1", "the answer to the mention goes to the channel, not to the private chat");
}
