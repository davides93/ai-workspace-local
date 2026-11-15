//! The live view of the controlled browser and the human takeover (issue #73): the controls of the person
//! (`pause`, `take`, `give_back`, the input, the answer to a window of the site) and `browser_handover`, the tool with
//! which the agent asks the person to log in or to type a verification code.
//!
//! **The credentials never go through the model.** The person types them in the page, in the live view or in the
//! real window; the agent is not acting meanwhile (`aiw_tools::browser::Control`), its next snapshot hides the value
//! of password and code fields and the query of addresses, and nothing of the input is logged, put in an event, saved
//! in a message or written in the audit: the audit holds the *transitions* (who took the browser, when, on which host),
//! never a key, a frame or a title.
//!
//! These controls are the owner's: the routes need the `full` scope, so a paired device (`device` token) neither sees
//! the page nor drives it (see `scopes.rs` and `SECURITY.md`).

use crate::{Engine, Inner};
use aiw_core::api::{Question, QuestionAnswer, QuestionOutcome, QuestionReply};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, Error, Result};
pub use aiw_tools::browser::{Frame, HumanInput, LiveState};
use aiw_tools::browser::{Handover, Mode};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// The least the agent waits for the person after `browser_handover`: logging in, finding the phone and typing a
/// code takes longer than answering a question.
const HANDOVER_MIN_WAIT: Duration = Duration::from_secs(600);

/// How long the handover waits: at least [`HANDOVER_MIN_WAIT`] (the tests keep the wait of the settings, to be able to end it).
fn handover_wait(configured: Duration) -> Duration {
    if cfg!(test) {
        configured
    } else {
        configured.max(HANDOVER_MIN_WAIT)
    }
}

pub(crate) fn tools(e: &Engine) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(BrowserHandover(Arc::downgrade(&e.0)))]
}

struct BrowserHandover(Weak<Inner>);

fn answer(v: Value) -> ToolOutput {
    ToolOutput::text(v.to_string())
}

#[async_trait::async_trait]
impl Tool for BrowserHandover {
    fn name(&self) -> &str {
        "browser_handover"
    }

    fn description(&self) -> &str {
        "Chiede all'utente di fare lui un passo nel browser controllato: accedere a un sito, la verifica in due passaggi (2FA, codice ricevuto), un CAPTCHA. Mostra la vista del browser nella chat, mette il browser in mano all'utente e aspetta (fino a 10 minuti) che lo restituisca; intanto tu non puoi agire. Non chiedere mai le credenziali all'utente a parole e non provare a leggerle: le scrive lui nella pagina e tu non le vedrai (nello snapshot successivo le password e i codici risultano «(nascosto)»). `reason` dice cosa deve fare, in una frase. Prima apri tu la pagina di accesso con browser_navigate; dopo la restituzione rifai browser_snapshot. Se risponde `unsupported`, di' all'utente che l'accesso va fatto dall'app su questo computer."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["reason"],
            "properties": {
                "reason": {"type": "string", "maxLength": 300, "description": "Cosa deve fare l'utente, per esempio «Accedi a example.com e completa la verifica in due passaggi»."}
            }
        })
    }

    fn risk(&self) -> Risk {
        // It asks the person and gives the browser to them: nothing happens that they do not do themselves.
        Risk::Read
    }

    fn available(&self) -> std::result::Result<(), String> {
        match self.0.upgrade() {
            Some(inner) => inner.browser.available(),
            None => Err("motore in arresto".into()),
        }
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = self.0.upgrade().map(Engine).ok_or_else(|| Error::Conflict("motore in arresto".into()))?;
        // Only a person at this computer can be asked: the live view needs the owner, and a run from a phone, a channel
        // or the command line has nobody to show it to (it would wait for nothing).
        let origin = engine.run_origin(&ctx.run_id).filter(|o| o.owner && o.questions);
        if origin.is_none() {
            return Ok(answer(
                json!({"outcome": "unsupported", "hint": "L'accesso va fatto dall'app su questo computer: di' all'utente di aprire la chat da lì e di accedere al sito (o di fornirti un altro modo)."}),
            ));
        }
        let reason = args.get("reason").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty());
        let Some(reason) = reason else { return Err(Error::Invalid("`reason`: manca o è vuoto".into())) };
        if reason.chars().count() > 300 {
            return Err(Error::Invalid("`reason`: al massimo 300 caratteri".into()));
        }
        let browser = engine.0.browser.clone();
        if !browser.live_state().running {
            return Ok(answer(
                json!({"outcome": "not_open", "hint": "Il browser non è aperto: apri prima la pagina di accesso con browser_navigate, poi richiama browser_handover."}),
            ));
        }
        let wait = handover_wait(engine.interaction_wait());
        let question = Question {
            id: new_id("ask"),
            run_id: ctx.run_id.clone(),
            kind: "handover".into(),
            title: Some("Serve il tuo intervento nel browser".into()),
            message: reason.to_string(),
            options: vec![],
            multiple: false,
            allow_other: false,
            fields: vec![],
            confirm_label: Some("Ho finito: restituisci il controllo".into()),
            deny_label: Some("Non posso ora".into()),
            expires_at: None,
        };
        browser.control().begin_handover(Handover {
            question_id: question.id.clone(),
            run_id: ctx.run_id.clone(),
            reason: reason.to_string(),
        });
        let host = engine.browser_host();
        engine.audit_system("browser.handover", &format!("l'agente chiede all'utente di intervenire nel browser{host}"));
        engine.emit_browser(Some(&ctx.run_id));
        let (outcome, reply) = engine.ask_for(ctx, question, wait, "browser_handover").await;
        // Whatever ended the wait, the browser goes back to the agent (idempotent if the person already gave it back).
        browser.control().give_back();
        engine.audit_system("browser.handover_end", &format!("esito: {}{}", outcome.as_str(), engine.browser_host()));
        engine.emit_browser(Some(&ctx.run_id));
        let confirmed = reply.as_ref().and_then(|a| a.confirmed).unwrap_or(false);
        let result = match (outcome, confirmed) {
            (QuestionOutcome::Answered, true) => json!({
                "outcome": "returned",
                "hint": "L'utente ha restituito il controllo. Rifai browser_snapshot per vedere dov'è arrivata la pagina: non conosci ciò che ha scritto e non devi chiederlo."
            }),
            (QuestionOutcome::Answered, false) | (QuestionOutcome::Declined, _) => json!({
                "outcome": "declined",
                "hint": "L'utente non può intervenire ora. Spiega cosa serve e aspetta un suo messaggio."
            }),
            (QuestionOutcome::Timeout, _) => json!({
                "outcome": "timeout",
                "hint": "L'utente non ha restituito il controllo in tempo: il browser è tornato a te. Chiedi nel messaggio se vuole riprovare."
            }),
            (other, _) => json!({"outcome": other.as_str()}),
        };
        Ok(answer(result))
    }
}

impl Engine {
    /// ` (host)` of the page in front, or nothing: what the audit says about where (never an address or a title).
    fn browser_host(&self) -> String {
        self.0.browser.live_state().page.map(|p| format!(" ({})", p.host)).unwrap_or_default()
    }

    /// Tells the run the browser was last used by that who drives it now (mode only: no address, no title).
    fn emit_browser(&self, run_id: Option<&str>) {
        let Some(run_id) = run_id.map(String::from).or_else(|| self.0.browser.last_run()) else { return };
        let live = self.0.runs.lock().unwrap().get(&run_id).is_some_and(|h| !h.done);
        if !live {
            return;
        }
        let s = self.0.browser.control().state();
        self.emit(
            &run_id,
            AgEvent::Custom { name: "aiw.browser".into(), value: json!({"mode": s.mode, "handover": s.handover.is_some()}) },
        );
    }

    /// What the live view shows: whether a browser runs, who drives it, the page in front (title and host only) and a
    /// window of the site that waits.
    pub async fn browser_state(&self) -> LiveState {
        self.0.browser.live_state_with_focus().await
    }

    /// A frame of the page, `None` when no browser runs. Never kept.
    pub async fn browser_frame(&self) -> Result<Option<Frame>> {
        self.0.browser.frame().await
    }

    /// The person's controls: `pause`, `resume`, `take`, `give_back`. Each transition is one audit row (the host, nothing else).
    pub async fn browser_control(&self, action: &str) -> Result<LiveState> {
        let browser = self.0.browser.clone();
        let before = browser.control().mode();
        let say = match action {
            "pause" => {
                browser.pause();
                "l'utente mette in pausa l'agente"
            }
            "resume" => {
                if before == Mode::Human {
                    return Err(Error::Conflict("il browser è in mano all'utente: restituiscilo con «Restituisci all'agente»".into()));
                }
                browser.resume();
                "l'utente lascia riprendere l'agente"
            }
            "take" => {
                browser.take_control().await?;
                "l'utente prende il controllo del browser"
            }
            "give_back" => {
                // A handover waits for its question: ending it is what returns the browser (and wakes the tool).
                match browser.control().handover() {
                    Some(h) => {
                        let reply = QuestionReply {
                            action: "answer".into(),
                            answer: QuestionAnswer { confirmed: Some(true), ..Default::default() },
                        };
                        let _ = self.respond_question(&h.question_id, reply);
                        // At once, so the answer to this very request already shows the agent has it back.
                        browser.give_back();
                    }
                    None => {
                        browser.give_back();
                    }
                }
                "l'utente restituisce il browser all'agente"
            }
            other => return Err(Error::Invalid(format!("azione `{other}` non valida: usa pause, resume, take o give_back"))),
        };
        if browser.control().mode() != before {
            self.audit_user("browser.control", &format!("{say}{}", self.browser_host()));
            self.emit_browser(None);
        }
        Ok(browser.live_state_with_focus().await)
    }

    /// The person clicks, scrolls or types in the page. Nothing of it is logged or kept.
    pub async fn browser_input(&self, input: HumanInput) -> Result<()> {
        self.0.browser.human_input(input).await
    }

    /// The person answers a window of the site (`confirm`, `prompt`).
    pub async fn browser_dialog(&self, accept: bool, text: Option<String>) -> Result<()> {
        self.0.browser.answer_dialog(accept, text).await
    }
}

impl Engine {
    /// Applies `browser.attach_url` (the Chrome the person wants the agent to use; empty: the one of our own). Read when the
    /// browser starts, so a browser that is running keeps its own until the kill switch closes it.
    pub(crate) fn sync_browser(&self) {
        let url = self.0.config.get_str("browser.attach_url").filter(|u| aiw_core::config::is_loopback_endpoint(u));
        if self.0.browser.set_attach(url.clone()) {
            self.audit_system(
                "browser.attach",
                &if url.is_some() {
                    "il browser dell'agente è il Chrome indicato (solo su questo computer)".to_string()
                } else {
                    "il browser dell'agente è il suo browser separato".to_string()
                },
            );
        }
    }
}

pub(crate) fn register(engine: &Engine) {
    for tool in tools(engine) {
        engine.0.tools.register(tool);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::test_ctx;
    use crate::tests::engine;
    use crate::RunOrigin;
    use aiw_tools::browser::fake::FakeCdp;

    const OWNER: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };
    const PHONE: RunOrigin = RunOrigin { owner: false, questions: true, secrets: false };
    const BARE: RunOrigin = RunOrigin { owner: true, questions: false, secrets: false };
    const REASON: &str = "Accedi a example.com e completa la verifica in due passaggi";

    /// An engine whose browser is a fake Chrome of the person's (`browser.attach_url`), already running: a tool of the
    /// browser has opened its tab.
    async fn with_browser() -> (Engine, FakeCdp, ToolContext, tempfile::TempDir, tempfile::TempDir) {
        let (e, a, b) = engine(vec![]).await;
        let fake = FakeCdp::start(|_, _| None).await;
        e.0.browser.set_attach(Some(fake.url.clone()));
        let ctx = test_ctx(&e, Some(OWNER));
        let out = e.tool_registry().get("browser_snapshot").unwrap().call(json!({}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        assert!(e.browser_state().await.running);
        (e, fake, ctx, a, b)
    }

    fn handover(e: &Engine, ctx: &ToolContext) -> tokio::task::JoinHandle<Result<ToolOutput>> {
        let tool = e.tool_registry().get("browser_handover").expect("browser_handover is registered");
        let (args, ctx) = (json!({"reason": REASON}), ctx.clone());
        tokio::spawn(async move { tool.call(args, &ctx).await })
    }

    async fn finished(task: tokio::task::JoinHandle<Result<ToolOutput>>) -> Value {
        let out = tokio::time::timeout(Duration::from_secs(10), task).await.expect("the tool ends").unwrap().unwrap();
        serde_json::from_str(&out.content).unwrap_or_else(|_| panic!("the result is JSON: {}", out.content))
    }

    async fn next_question(e: &Engine) -> Question {
        for _ in 0..300 {
            if let Some(q) = e.questions().into_iter().next() {
                return q;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no question");
    }

    fn audit_details(e: &Engine, action: &str) -> Vec<String> {
        e.audit().into_iter().filter(|a| a.action == action).map(|a| a.detail).collect()
    }

    #[tokio::test]
    async fn a_handover_nobody_can_complete_is_refused_at_once_and_touches_nothing() {
        let (e, _fake, _ctx, _a, _b) = with_browser().await;
        for origin in [None, Some(PHONE), Some(BARE)] {
            let ctx = test_ctx(&e, origin);
            let tool = e.tool_registry().get("browser_handover").unwrap();
            let out = tokio::time::timeout(Duration::from_secs(2), tool.call(json!({"reason": REASON}), &ctx))
                .await
                .expect("it does not wait")
                .unwrap();
            let v: Value = serde_json::from_str(&out.content).unwrap();
            assert_eq!(v["outcome"], "unsupported", "{origin:?}: {v}");
            assert!(v["hint"].as_str().unwrap().contains("su questo computer"), "{v}");
        }
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
        assert!(e.questions().is_empty());
    }

    #[tokio::test]
    async fn a_handover_needs_a_browser_that_is_open_and_a_reason() {
        let (e, _a, _b) = engine(vec![]).await;
        let tool = e.tool_registry().get("browser_handover").unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let v: Value = serde_json::from_str(&tool.call(json!({"reason": REASON}), &ctx).await.unwrap().content).unwrap();
        assert_eq!(v["outcome"], "not_open");
        assert!(tool.call(json!({}), &ctx).await.is_err());
        assert!(tool.call(json!({"reason": "x".repeat(301)}), &ctx).await.is_err());
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
    }

    #[tokio::test]
    async fn the_person_logs_in_while_the_agent_waits_and_gives_the_browser_back() {
        let (e, fake, ctx, _a, _b) = with_browser().await;
        let task = handover(&e, &ctx);
        let q = next_question(&e).await;
        assert_eq!((q.kind.as_str(), q.message.as_str()), ("handover", REASON));
        assert_eq!(q.confirm_label.as_deref(), Some("Ho finito: restituisci il controllo"));
        // The browser is the person's: the state says so and carries the question that ends it.
        let state = e.browser_state().await;
        assert_eq!(state.control.mode, Mode::Human);
        assert_eq!(state.control.handover.as_ref().map(|h| h.question_id.as_str()), Some(q.id.as_str()));
        // An action of the agent (another tool call of the same run) waits and does not reach the browser.
        let before = fake.commands("Runtime.evaluate").len();
        let action = {
            let (tool, ctx) = (e.tool_registry().get("browser_click").unwrap(), ctx.clone());
            tokio::spawn(async move { tool.call(json!({"ref": "1"}), &ctx).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!action.is_finished());
        assert_eq!(fake.commands("Runtime.evaluate").len(), before);
        // «Restituisci all'agente» in the live view ends the handover too.
        let state = e.browser_control("give_back").await.unwrap();
        assert_eq!(state.control.mode, Mode::Agent);
        let v = finished(task).await;
        assert_eq!(v["outcome"], "returned", "{v}");
        assert!(!v.to_string().contains(REASON), "the result tells the model nothing of what was typed or asked");
        // The action goes on once the browser is back, but not on the numbers of before the person drove: it has to look again.
        let late = action.await.unwrap().unwrap();
        assert!(late.is_error && late.content.contains("Rifai browser_snapshot"), "{}", late.content);
        assert!(e.browser_state().await.control.stale);
        // The audit: the transitions, with no title, no address, no reason.
        assert_eq!(audit_details(&e, "browser.handover").len(), 1);
        assert_eq!(audit_details(&e, "browser.handover_end").len(), 1);
        assert_eq!(audit_details(&e, "browser.control").len(), 1, "{:?}", audit_details(&e, "browser.control"));
        for a in e.audit() {
            assert!(!a.detail.contains(REASON) && !a.detail.contains("example.com"), "{a:?}");
        }
    }

    #[tokio::test]
    async fn the_person_can_answer_the_card_instead_and_can_decline() {
        let (e, _fake, ctx, _a, _b) = with_browser().await;
        let task = handover(&e, &ctx);
        let q = next_question(&e).await;
        e.respond_question(
            &q.id,
            QuestionReply { action: "answer".into(), answer: QuestionAnswer { confirmed: Some(true), ..Default::default() } },
        )
        .unwrap();
        assert_eq!(finished(task).await["outcome"], "returned");
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
        // Declined: the browser goes back to the agent all the same, and the model is told to wait for a message.
        let task = handover(&e, &ctx);
        let q = next_question(&e).await;
        e.respond_question(&q.id, QuestionReply { action: "decline".into(), answer: QuestionAnswer::default() }).unwrap();
        let v = finished(task).await;
        assert_eq!(v["outcome"], "declined");
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
    }

    #[tokio::test]
    async fn the_stop_of_the_run_and_the_kill_switch_end_the_handover() {
        let (e, _fake, ctx, _a, _b) = with_browser().await;
        let task = handover(&e, &ctx);
        next_question(&e).await;
        ctx.cancel.cancel();
        assert_eq!(finished(task).await["outcome"], "cancelled");
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
        // The kill switch: the question closes, the browser closes with it and the person's turn with it.
        let ctx = test_ctx(&e, Some(OWNER));
        let task = handover(&e, &ctx);
        next_question(&e).await;
        assert_eq!(e.browser_state().await.control.mode, Mode::Human);
        e.kill().await;
        assert_eq!(finished(task).await["outcome"], "cancelled");
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
        assert!(!e.browser_state().await.running);
    }

    #[tokio::test]
    async fn a_handover_that_nobody_answers_ends_with_a_timeout_and_the_browser_goes_back() {
        let (e, _fake, ctx, _a, _b) = with_browser().await;
        e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
        let v = finished(handover(&e, &ctx)).await;
        assert_eq!(v["outcome"], "timeout", "{v}");
        assert_eq!(e.browser_state().await.control.mode, Mode::Agent);
    }

    #[tokio::test]
    async fn the_controls_of_the_person_are_audited_and_checked() {
        let (e, _fake, _ctx, _a, _b) = with_browser().await;
        assert_eq!(e.browser_control("pause").await.unwrap().control.mode, Mode::Paused);
        assert_eq!(e.browser_control("resume").await.unwrap().control.mode, Mode::Agent);
        assert_eq!(e.browser_control("take").await.unwrap().control.mode, Mode::Human);
        assert!(matches!(e.browser_control("resume").await, Err(Error::Conflict(_))), "from the person's turn only give_back");
        assert_eq!(e.browser_control("give_back").await.unwrap().control.mode, Mode::Agent);
        assert!(matches!(e.browser_control("vola").await, Err(Error::Invalid(_))));
        let rows = audit_details(&e, "browser.control");
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(rows.iter().all(|r| r.starts_with("l'utente ")), "{rows:?}");
    }

    #[tokio::test]
    async fn the_controls_need_a_browser_that_is_running_and_the_input_is_not_logged() {
        let (e, _a, _b) = engine(vec![]).await;
        assert!(matches!(e.browser_control("take").await, Err(Error::Conflict(_))), "nothing to take");
        assert!(!e.browser_state().await.running);
        assert!(e.browser_frame().await.unwrap().is_none());
        let typed = "PASSWORD-DI-PROVA";
        let err = e.browser_input(HumanInput::Text { text: typed.into() }).await.unwrap_err().to_string();
        assert!(!err.contains(typed), "{err}");
        for a in e.audit() {
            assert!(!a.detail.contains(typed), "{a:?}");
        }
    }

    #[tokio::test]
    async fn the_chrome_to_attach_is_the_persons_choice_and_only_on_this_computer() {
        let (e, _a, _b) = engine(vec![]).await;
        assert!(e.set_config("global", "browser.attach_url", json!("http://192.168.1.5:9222")).is_err());
        assert_eq!(e.0.browser.attach(), None);
        e.set_config("global", "browser.attach_url", json!("http://127.0.0.1:9222")).unwrap();
        assert_eq!(e.0.browser.attach().as_deref(), Some("http://127.0.0.1:9222"));
        e.set_config("global", "browser.attach_url", json!("")).unwrap();
        assert_eq!(e.0.browser.attach(), None);
        assert_eq!(audit_details(&e, "browser.attach").len(), 2);
    }
}
