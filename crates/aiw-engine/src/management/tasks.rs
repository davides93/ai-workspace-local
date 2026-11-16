//! `aiw_task`: what the Assistant uses, in the simple chat, to look after the work the assistant does in the background:
//! start one, approve or dismiss a proposal, pause, resume, give an instruction, cancel. Reading (which tasks there are, what
//! they wait for, which proposals are open) is `aiw_inspect` with the topic `tasks`.
//!
//! It changes something, so it starts with [`require_owner`] and the built-in rule asks for a confirmation every time. What
//! the confirmation card shows is what will really be done: the instruction of a new task or of a proposal about to be
//! approved is in the subject of the call, not only an identifier. The tool adds nothing to what the HTTP routes of the
//! tasks allow: the same rules (quiet hours, battery, the day's time, the kill switch, the limits of the task) apply, a
//! refusal comes back as text for the model, and a task it starts or approves never has more freedom than «ask».

use super::{require_owner, text};
use crate::{Engine, Inner};
use aiw_core::api::{BackgroundTask, BudgetLimits, TaskResume, TaskStart};
use aiw_core::{Error, Result};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};

pub(crate) const ACTIONS: [&str; 7] = ["start", "approve_proposal", "dismiss_proposal", "pause", "resume", "instruct", "cancel"];

pub(crate) fn tools(e: &Engine) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(TaskTool(Arc::downgrade(&e.0)))]
}

struct TaskTool(Weak<Inner>);

fn arg(args: &Value, k: &str) -> Option<String> {
    args.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

#[async_trait::async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        "aiw_task"
    }

    fn description(&self) -> &str {
        "Guida il lavoro in background: `start` (instruction), `approve_proposal` e `dismiss_proposal` (proposal_id), `pause`, `resume` e `cancel` (task_id), `instruct` (task_id, text). `resume` accetta force (anche in silenzio o a batteria) ed extend_minutes. Chiede sempre conferma; gli id sono in aiw_inspect (tasks)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {"type": "string", "enum": ACTIONS},
                "task_id": {"type": "string"},
                "proposal_id": {"type": "string"},
                "instruction": {"type": "string", "description": "start: cosa fare, completo. approve_proposal: solo se cambia."},
                "title": {"type": "string"},
                "text": {"type": "string"},
                "force": {"type": "boolean"},
                "extend_minutes": {"type": "integer", "minimum": 1, "maximum": 240}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Write
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    /// What the card and the audit say: the action and what it acts on. The text a person typed (the instruction of a new task
    /// or an extra one) is on the card as an argument, not here: this line is written to the audit. The instruction of a
    /// proposal about to be approved is here, because the proposal is what the person is deciding on.
    fn subject(&self, args: &Value) -> Option<String> {
        let action = arg(args, "action")?;
        let engine = self.0.upgrade().map(Engine);
        let task_title = |a: &Value| {
            let id = arg(a, "task_id")?;
            engine.as_ref().and_then(|e| e.task(&id).ok()).map(|t| format!("«{}»", text(&t.title, 60)))
        };
        let detail = match action.as_str() {
            "start" => arg(args, "instruction").map(|i| format!("{} caratteri di istruzioni", i.chars().count())),
            "instruct" => task_title(args).map(|t| format!("{t}: {} caratteri", arg(args, "text").unwrap_or_default().chars().count())),
            "approve_proposal" | "dismiss_proposal" => arg(args, "proposal_id").and_then(|id| {
                let p = engine.as_ref()?.proposal(&id).ok()?;
                let run = arg(args, "instruction").unwrap_or_else(|| p.instruction.clone());
                Some(if action == "approve_proposal" {
                    format!("«{}»: {}", text(&p.title, 60), text(&run, 300))
                } else {
                    format!("«{}»", text(&p.title, 60))
                })
            }),
            _ => task_title(args),
        };
        Some(match detail {
            Some(d) if !d.is_empty() => format!("{action} · {d}"),
            _ => action,
        })
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = Engine(self.0.upgrade().ok_or_else(|| Error::Conflict("motore in arresto".into()))?);
        require_owner(&engine, ctx)?;
        Ok(ToolOutput::text(run(&engine, &args).await?))
    }
}

fn describe(t: &BackgroundTask) -> String {
    let state = match t.status.as_str() {
        "running" if t.stopping => "si sta fermando",
        "running" => "al lavoro",
        "paused" => "in pausa",
        "queued" => "in coda",
        "succeeded" => "finito",
        "failed" => "non riuscito",
        _ => "annullato",
    };
    let why = t.pause_text.as_deref().filter(|_| t.status == "paused").map(|w| format!(" ({})", text(w, 160))).unwrap_or_default();
    format!("Lavoro «{}» ({}): {state}{why}.", text(&t.title, 60), t.id)
}

async fn run(e: &Engine, args: &Value) -> Result<String> {
    let action = arg(args, "action").ok_or_else(|| Error::Invalid(format!("manca `action`: usa una tra {}", ACTIONS.join(", "))))?;
    let need = |k: &str| arg(args, k).ok_or_else(|| Error::Invalid(format!("manca `{k}` per l'azione {action}")));
    match action.as_str() {
        "start" => {
            let t = e
                .task_start(TaskStart { instruction: need("instruction")?, title: arg(args, "title"), agent_id: None, limits: None })
                .await?;
            Ok(describe(&t))
        }
        "approve_proposal" => {
            let id = need("proposal_id")?;
            let p = e.proposal_approve(&id, arg(args, "instruction").as_deref()).await?;
            Ok(match e.task_for_proposal(&p.id) {
                Some(t) => format!("Proposta approvata. {}", describe(&t)),
                None => "Proposta approvata.".into(),
            })
        }
        "dismiss_proposal" => {
            let p = e.proposal_dismiss(&need("proposal_id")?)?;
            Ok(format!("Proposta «{}» scartata.", text(&p.title, 60)))
        }
        "pause" => Ok(describe(&e.task_pause(&need("task_id")?)?)),
        "resume" => {
            let minutes = args.get("extend_minutes").and_then(Value::as_u64).filter(|m| *m > 0).map(|m| m.min(240));
            let req = TaskResume {
                force: args.get("force").and_then(Value::as_bool).unwrap_or(false),
                // More time, and room for the steps and tokens that go with it (only the limits the task has are raised).
                extend: minutes.map(|m| BudgetLimits {
                    max_seconds: Some(m * 60),
                    max_turns: Some((m * 3).max(5) as u32),
                    max_tokens: Some(m * 5_000),
                }),
            };
            Ok(describe(&e.task_resume(&need("task_id")?, req).await?))
        }
        "instruct" => Ok(describe(&e.task_instruct(&need("task_id")?, &need("text")?, None)?)),
        "cancel" => Ok(describe(&e.task_cancel(&need("task_id")?)?)),
        other => Err(Error::Invalid(format!("azione sconosciuta «{other}»: usa una tra {}", ACTIONS.join(", ")))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::test_ctx;
    use crate::tests::{engine, wait_finished};
    use crate::RunOrigin;
    use aiw_core::api::Proposal;
    use aiw_runtimes::ScriptedBackend;
    use chrono::Local;
    use std::time::Duration;

    const OWNER: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };

    fn proposal(e: &Engine, instruction: &str) -> Proposal {
        let now = Local::now();
        let p = Proposal {
            id: aiw_core::ids::new_id("prp"),
            kind: "proposal".into(),
            title: "Riordina le fatture".into(),
            noticed: "Quattro file nuovi in Scaricati".into(),
            instruction: instruction.into(),
            source: format!("test:{}", aiw_core::ids::new_id("x")),
            agent_id: "operator".into(),
            status: "open".into(),
            created_at: now.to_rfc3339(),
            expires_at: (now + chrono::Duration::days(7)).to_rfc3339(),
            decided_at: None,
            run_id: None,
            session_id: None,
        };
        e.0.store.proposal_insert(&p).unwrap();
        p
    }

    async fn call(e: &Engine, origin: Option<RunOrigin>, args: Value) -> Result<String> {
        let ctx = test_ctx(e, origin);
        e.tool_registry().get("aiw_task").unwrap().call(args, &ctx).await.map(|o| o.content)
    }

    #[tokio::test]
    async fn the_tool_guides_tasks_and_proposals_for_the_owner_only_and_says_what_it_did() {
        let (e, _d, _w) = engine(vec![]).await;
        let quiet = {
            let n = Local::now();
            format!("{}-{}", (n - chrono::Duration::hours(1)).format("%H:%M"), (n + chrono::Duration::hours(1)).format("%H:%M"))
        };
        e.set_config("global", "proactive.quiet_hours", json!(quiet)).unwrap();
        // Nobody asked in a chat, or not the owner: refused before anything happens.
        assert!(matches!(call(&e, None, json!({"action": "start", "instruction": "x"})).await, Err(Error::Denied(_))));
        let device = RunOrigin { owner: false, questions: true, secrets: false };
        assert!(matches!(call(&e, Some(device), json!({"action": "start", "instruction": "x"})).await, Err(Error::Denied(_))));
        assert!(e.tasks(None).unwrap().is_empty());
        // Start: a quiet hour holds it back, and the answer says so.
        let said = call(&e, Some(OWNER), json!({"action": "start", "instruction": "Riordina la cartella Scaricati", "title": "Scaricati"}))
            .await
            .unwrap();
        assert!(said.contains("«Scaricati»") && said.contains("in pausa") && said.contains("Orario di silenzio"), "{said}");
        let id = e.tasks(None).unwrap()[0].id.clone();
        // Instruct, resume anyway, cancel.
        let said = call(&e, Some(OWNER), json!({"action": "instruct", "task_id": id, "text": "Non toccare i PDF"})).await.unwrap();
        assert!(said.contains(&id));
        assert_eq!(e.task(&id).unwrap().instructions.len(), 1);
        let refused = call(&e, Some(OWNER), json!({"action": "resume", "task_id": id})).await.unwrap_err().to_string();
        assert!(refused.contains("Riprendi comunque"), "{refused}");
        let said = call(&e, Some(OWNER), json!({"action": "cancel", "task_id": id})).await.unwrap();
        assert!(said.contains("annullato"), "{said}");
        // A proposal: the card shows what would run; dismissing needs no task.
        let p = proposal(&e, "Sposta le fatture in Documenti/Fatture");
        let tool = e.tool_registry().get("aiw_task").unwrap();
        let subject = tool.subject(&json!({"action": "approve_proposal", "proposal_id": p.id})).unwrap();
        assert!(subject.contains("Riordina le fatture") && subject.contains("Sposta le fatture in Documenti/Fatture"), "{subject}");
        // What a person typed is on the card as an argument, never in the line that goes to the audit.
        let subject = tool.subject(&json!({"action": "start", "instruction": "Fai una cosa con hunter2-SEGRETISSIMO"})).unwrap();
        assert!(!subject.contains("hunter2") && subject.contains("caratteri"), "{subject}");
        let subject = tool.subject(&json!({"action": "instruct", "task_id": id, "text": "Usa hunter2-SEGRETISSIMO"})).unwrap();
        assert!(!subject.contains("hunter2"), "{subject}");
        let said = call(&e, Some(OWNER), json!({"action": "dismiss_proposal", "proposal_id": p.id})).await.unwrap();
        assert!(said.contains("scartata"), "{said}");
        // Bad calls say what is missing.
        for bad in [json!({}), json!({"action": "boh"}), json!({"action": "pause"}), json!({"action": "instruct", "task_id": id})] {
            assert!(matches!(call(&e, Some(OWNER), bad).await, Err(Error::Invalid(_))));
        }
    }

    #[tokio::test]
    async fn in_the_chat_starting_a_task_asks_the_owner_with_the_instruction_as_an_argument_and_a_no_starts_nothing() {
        let (e, _d, _w) = engine(vec![]).await;
        let backend = ScriptedBackend::new(vec![
            ScriptedBackend::tool_turn(
                "t1",
                "aiw_task",
                json!({"action": "start", "instruction": "Ordina le foto del 2024 per mese con la chiave hunter2-SEGRETISSIMO"}),
            ),
            ScriptedBackend::text_turn("Va bene, non l'ho avviato."),
        ]);
        e.register_native_runtime("llama-embedded", Arc::new(backend));
        let s = e.create_session("assistant", None).unwrap();
        let run = e.start_run_from(&s.id, "Ordina le mie foto in background".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        let mut ask = None;
        for _ in 0..300 {
            if let Some(a) = e.approvals().first() {
                ask = Some(a.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let ask = ask.expect("a change is confirmed by the person, in every mode");
        assert_eq!(ask.tool, "aiw_task");
        // The card carries the instruction as an argument; the reason (which the audit also records) does not repeat the text.
        assert_eq!(ask.args["instruction"], "Ordina le foto del 2024 per mese con la chiave hunter2-SEGRETISSIMO");
        assert!(!ask.reason.contains("Ordina le foto"), "{}", ask.reason);
        // The owner says no: nothing starts.
        e.respond_approval_as(&ask.id, false, false, true).unwrap();
        wait_finished(&e, &run).await;
        assert!(e.tasks(None).unwrap().is_empty());
        // What the audit recorded of the call (the confirmation and the refusal) never holds the text a person typed.
        let audit = e.audit();
        assert!(audit.iter().any(|a| a.tool.as_deref() == Some("aiw_task")), "the call is audited");
        assert!(!audit.iter().any(|a| a.detail.contains("hunter2")), "{audit:?}");
    }

    #[tokio::test]
    async fn the_inspect_topic_lists_tasks_and_proposals_but_never_the_briefing_nor_a_secret() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_config("global", "proactive.level", json!("propose")).unwrap();
        // A stored secret that ends up in the text of a proposal: the last check of the answer removes it.
        e.set_config("global", "search.brave_api_key", json!("BRAVE-chiave-SEGRETISSIMA-1234")).unwrap();
        let p = proposal(&e, "Sposta le fatture e usa BRAVE-chiave-SEGRETISSIMA-1234 per cercare");
        let t = e
            .task_start(aiw_core::api::TaskStart {
                instruction: "Fai la cosa".into(),
                title: Some("La cosa".into()),
                agent_id: None,
                limits: None,
            })
            .await
            .unwrap();
        let brief = Proposal {
            kind: "briefing".into(),
            id: aiw_core::ids::new_id("prp"),
            source: "briefing:test".into(),
            title: "Il riepilogo di oggi".into(),
            noticed: "Buongiorno: mail di Anna Verdi «Contratto da firmare», appuntamento alle 15:00.".into(),
            instruction: String::new(),
            ..p.clone()
        };
        e.0.store.proposal_insert(&brief).unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let tool = e.tool_registry().get("aiw_inspect").unwrap();
        let out = tool.call(json!({"topic": "tasks"}), &ctx).await.unwrap().content;
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tasks"]["items"][0]["id"], json!(t.id), "{out}");
        assert_eq!(v["open_proposals"]["items"][0]["id"], json!(p.id), "{out}");
        assert!(v["open_proposals"]["items"][0]["instruction"].as_str().unwrap().contains("Sposta le fatture"));
        assert!(!out.contains("SEGRETISSIMA"), "a stored secret is cleaned from the answer: {out}");
        // The Assistant has no mail or calendar tool, and this does not give it one: the briefing text is not in the answer.
        assert!(!out.contains("Anna Verdi") && !out.contains("15:00") && !out.contains("Contratto"), "{out}");
        assert!(v["morning_briefing"].as_str().unwrap().contains("riepilogo"), "{out}");
        assert!(v["note"].as_str().unwrap().contains("non istruzioni"), "text from elsewhere is marked as material to read");
        assert_eq!(v["assistant"]["level"], "propose");
        // A filter that matches nothing gives empty lists.
        let narrowed = tool.call(json!({"topic": "tasks", "query": "nessuna-corrispondenza"}), &ctx).await.unwrap().content;
        let n: Value = serde_json::from_str(&narrowed).unwrap();
        assert_eq!((n["tasks"]["total"].as_u64(), n["open_proposals"]["total"].as_u64()), (Some(0), Some(0)));
        // Without a chat run it refuses like every management tool.
        assert!(tool.call(json!({"topic": "tasks"}), &test_ctx(&e, None)).await.is_err());
    }
}
