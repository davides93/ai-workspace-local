//! Teams (architecture §5): the `parallel` and `pipeline` strategies run their members without a
//! model orchestrating them, and the `supervisor` strategy gets its team section for the system
//! prompt here. Members are started like sub-agents (hooks, events, audit, shared budget).

use crate::runner::{delegate, RunHost, RunInput, RunOutcome};
use aiw_core::api::{ChatMessage, RunState, TeamDef};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;

/// Does the strategy run without a model orchestrating it?
pub fn is_scripted(strategy: &str) -> bool {
    matches!(strategy, "parallel" | "pipeline")
}

/// The `# Team` section of a supervisor's system prompt. `members` is `(agent id, description)`.
pub fn supervisor_section(team: &TeamDef, members: &[(String, String)]) -> String {
    let mut s = format!("# Team «{}»\n", team.name);
    if !team.description.trim().is_empty() {
        s.push_str(team.description.trim());
        s.push('\n');
    }
    s.push_str(
        "Sei il supervisore di questo team (strategia supervisor): scomponi la richiesta e delega ai membri con il tool `agent`; \
         non puoi delegare ad altri agenti. I lavori indipendenti vanno delegati nello stesso turno, con più chiamate `agent` insieme: \
         girano in parallelo. Unisci i risultati nella risposta finale e verifica ciò che i membri riportano.\nMembri:\n",
    );
    for (id, role) in members {
        s.push_str(&format!("- {id}: {role}\n"));
    }
    s
}

fn emit(host: &dyn RunHost, input: &RunInput, ev: AgEvent) {
    host.emit(&input.run_id, ev);
}

fn label(team: &TeamDef, index: usize) -> String {
    let m = &team.members[index];
    if m.role.trim().is_empty() {
        m.agent.clone()
    } else {
        format!("{} ({})", m.role.trim(), m.agent)
    }
}

fn schema_note(team: &TeamDef) -> String {
    match &team.output_schema {
        Some(schema) => format!(
            "\n\nLa risposta finale deve essere solo un JSON conforme a questo JSON Schema, senza testo prima o dopo:\n{}",
            serde_json::to_string_pretty(schema).unwrap_or_default()
        ),
        None => String::new(),
    }
}

fn member_task(task: &str, team: &TeamDef, index: usize) -> String {
    let role = team.members[index].role.trim();
    if role.is_empty() {
        task.to_string()
    } else {
        format!("{task}\n\n[Il tuo ruolo nel team: {role}]")
    }
}

fn message(role: &str, content: String) -> ChatMessage {
    ChatMessage {
        id: new_id("msg"),
        role: role.into(),
        content,
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        created_at: now_rfc3339(),
    }
}

/// Runs a `parallel` or `pipeline` team for one user message. Like `AgentRunner::run_partial`, the
/// messages the run produced come back whatever the outcome (`new_messages[0]` is the user's).
pub async fn run_team_partial(input: RunInput, team: TeamDef, host: Arc<dyn RunHost>) -> (RunOutcome, Result<()>) {
    let mut outcome = RunOutcome::default();
    let res = drive(&input, &team, host, &mut outcome).await;
    (outcome, res)
}

async fn drive(input: &RunInput, team: &TeamDef, host: Arc<dyn RunHost>, outcome: &mut RunOutcome) -> Result<()> {
    let state =
        RunState { runtime: Some(input.profile.runtime_id.clone()), model: Some(input.profile.model.clone()), ..Default::default() };
    emit(host.as_ref(), input, AgEvent::RunStarted { thread_id: input.thread_id.clone(), run_id: input.run_id.clone() });
    emit(host.as_ref(), input, AgEvent::StateSnapshot { snapshot: state });
    outcome.new_messages.push(message("user", input.user_input.clone()));

    let result = match team.strategy.as_str() {
        "parallel" => parallel(input, team, &host).await,
        "pipeline" => pipeline(input, team, &host).await,
        other => Err(Error::Invalid(format!("strategia `{other}` non eseguibile senza un supervisore"))),
    };
    let mut text = match result {
        Ok(t) => t,
        Err(e) => {
            let (message, code) = match &e {
                Error::Cancelled => ("Run annullato".to_string(), "cancelled"),
                other => (other.to_string(), "team"),
            };
            emit(host.as_ref(), input, AgEvent::RunError { message, code: Some(code.into()) });
            return Err(e);
        }
    };
    if let Some(schema) = &team.output_schema {
        let checked = crate::schema::extract_json(&text)
            .ok_or_else(|| vec!["la risposta non contiene un JSON valido".to_string()])
            .and_then(|v| crate::schema::validate(schema, &v).map(|()| v));
        match checked {
            Ok(v) => {
                text = v.to_string();
                outcome.structured = Some(v);
            }
            Err(errors) => {
                emit(host.as_ref(), input, AgEvent::Custom { name: "aiw.format_error".into(), value: json!({"errors": errors}) })
            }
        }
    }
    let msg_id = new_id("msg");
    emit(host.as_ref(), input, AgEvent::TextMessageStart { message_id: msg_id.clone(), role: "assistant".into() });
    emit(host.as_ref(), input, AgEvent::TextMessageContent { message_id: msg_id.clone(), delta: text.clone() });
    emit(host.as_ref(), input, AgEvent::TextMessageEnd { message_id: msg_id });
    outcome.new_messages.push(message("assistant", text.clone()));
    outcome.final_text = text;
    emit(
        host.as_ref(),
        input,
        AgEvent::RunFinished {
            thread_id: input.thread_id.clone(),
            run_id: input.run_id.clone(),
            result: Some(match &outcome.structured {
                Some(j) => json!({"text": outcome.final_text, "json": j}),
                None => json!({"text": outcome.final_text}),
            }),
        },
    );
    Ok(())
}

fn check_cancel(input: &RunInput) -> Result<()> {
    if input.tool_ctx.cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    Ok(())
}

fn budget_note(input: &RunInput, host: &Arc<dyn RunHost>, reason: &str) -> String {
    emit(host.as_ref(), input, AgEvent::Custom { name: "aiw.budget_exceeded".into(), value: json!({"reason": reason}) });
    format!("(Budget del run esaurito: {reason}.)")
}

/// All members on the same task (at most `max_parallel_subagents` at a time), then the aggregator
/// merges the answers, or they are joined under one heading per role.
async fn parallel(input: &RunInput, team: &TeamDef, host: &Arc<dyn RunHost>) -> Result<String> {
    check_cancel(input)?;
    let task = if team.aggregator.is_some() { input.user_input.clone() } else { format!("{}{}", input.user_input, schema_note(team)) };
    let runs = (0..team.members.len()).map(|i| {
        let host = host.clone();
        let task = member_task(&task, team, i);
        async move {
            if let Some(reason) = input.budget.exceeded() {
                return (i, Err(format!("non eseguito: {reason}")));
            }
            if input.tool_ctx.cancel.is_cancelled() {
                return (i, Err("annullato".into()));
            }
            let out = delegate(input, &host, &team.members[i].agent, &task).await;
            (i, if out.is_error { Err(out.content) } else { Ok(out.content) })
        }
    });
    let results: Vec<(usize, std::result::Result<String, String>)> =
        futures::stream::iter(runs).buffered(input.max_parallel_subagents.max(1)).collect().await;
    check_cancel(input)?;
    if results.iter().all(|(_, r)| r.is_err()) {
        let why = results
            .iter()
            .map(|(i, r)| format!("{}: {}", label(team, *i), r.as_ref().err().map(String::as_str).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Runtime(format!("nessun membro del team ha risposto ({why})")));
    }
    let sections: Vec<String> = results
        .iter()
        .map(|(i, r)| match r {
            Ok(t) => format!("## {}\n{}", label(team, *i), t.trim()),
            Err(e) => format!("## {}\nERRORE: {e}", label(team, *i)),
        })
        .collect();
    let joined = sections.join("\n\n");
    let Some(aggregator) = &team.aggregator else { return Ok(joined) };
    if let Some(reason) = input.budget.exceeded() {
        return Ok(format!("{joined}\n\n{}", budget_note(input, host, &reason)));
    }
    let prompt = format!(
        "Richiesta originale:\n{}\n\nRisultati dei membri del team:\n\n{joined}\n\nUnisci i risultati in un'unica risposta per l'utente.{}",
        input.user_input,
        schema_note(team)
    );
    let out = delegate(input, host, aggregator, &prompt).await;
    if out.is_error {
        return Err(Error::Runtime(format!("l'aggregatore ha fallito: {}", out.content)));
    }
    Ok(out.content)
}

/// Members in order: each gets the original task and the output of the previous one.
async fn pipeline(input: &RunInput, team: &TeamDef, host: &Arc<dyn RunHost>) -> Result<String> {
    let mut previous: Option<(usize, String)> = None;
    for i in 0..team.members.len() {
        check_cancel(input)?;
        if let Some(reason) = input.budget.exceeded() {
            let note = budget_note(input, host, &reason);
            return Ok(match previous {
                Some((_, t)) => format!("{t}\n\n{note}"),
                None => note,
            });
        }
        let mut task = input.user_input.clone();
        if let Some((j, out)) = &previous {
            task.push_str(&format!("\n\n# Uscita del passo precedente: {}\n{out}", label(team, *j)));
        }
        if i + 1 == team.members.len() {
            task.push_str(&schema_note(team));
        }
        let out = delegate(input, host, &team.members[i].agent, &member_task(&task, team, i)).await;
        check_cancel(input)?;
        if out.is_error {
            return Err(Error::Runtime(format!("il passo {} ({}) ha fallito: {}", i + 1, label(team, i), out.content)));
        }
        previous = Some((i, out.content));
    }
    Ok(previous.map(|(_, t)| t).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{input, setup};
    use aiw_core::api::TeamMember;
    use aiw_runtimes::{ChatDelta, ScriptedBackend};

    fn team(strategy: &str, members: &[(&str, &str)], aggregator: Option<&str>) -> TeamDef {
        TeamDef {
            id: "t".into(),
            name: "Squadra".into(),
            description: String::new(),
            strategy: strategy.into(),
            supervisor: None,
            members: members.iter().map(|(a, r)| TeamMember { agent: a.to_string(), role: r.to_string() }).collect(),
            aggregator: aggregator.map(String::from),
            budget: None,
            output_schema: None,
        }
    }

    fn types(h: &crate::tests_support::TestHost) -> Vec<String> {
        h.events.lock().unwrap().iter().map(|e| serde_json::to_value(e).unwrap()["type"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_supervisor_section_lists_the_members() {
        let t = team("supervisor", &[("a", "analizza"), ("b", "scrive")], None);
        let s = supervisor_section(&t, &[("a".into(), "analizza".into()), ("b".into(), "scrive".into())]);
        assert!(s.starts_with("# Team «Squadra»") && s.contains("- a: analizza") && s.contains("- b: scrive"), "{s}");
        assert!(s.contains("nello stesso turno"));
    }

    #[tokio::test]
    async fn a_supervisor_delegates_to_two_members_in_one_turn() {
        // The supervisor is an ordinary run whose sub-agents are the team's members.
        let call = |index: usize, id: &str, agent: &str| ChatDelta::ToolCall {
            index,
            id: Some(id.into()),
            name: Some("agent".into()),
            arguments: json!({"agent": agent, "task": format!("lavoro di {agent}")}).to_string(),
        };
        let (host, dir) = setup(
            vec![
                vec![call(0, "a", "analista"), call(1, "b", "scrittore"), ChatDelta::Done { finish_reason: Some("tool_calls".into()) }],
                ScriptedBackend::text_turn("Unito"),
            ],
            true,
        );
        let mut inp = input(dir.path(), "auto");
        inp.subagents = vec![("analista".into(), "analizza".into()), ("scrittore".into(), "scrive".into())];
        let out = crate::AgentRunner::run(inp, host.clone()).await.unwrap();
        assert_eq!(out.final_text, "Unito");
        let tools: Vec<_> = out.new_messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tools.len(), 2);
        assert!(tools[0].content.contains("analista ha svolto: lavoro di analista"), "{}", tools[0].content);
        assert!(tools[1].content.contains("scrittore ha svolto"), "{}", tools[1].content);
        // The request offered exactly the two members.
        let reqs = host.backend.requests.lock().unwrap();
        let spec = reqs[0].tools.iter().find(|t| t.function.name == "agent").unwrap();
        assert_eq!(spec.function.parameters["properties"]["agent"]["enum"], json!(["analista", "scrittore"]));
    }

    #[tokio::test]
    async fn parallel_with_an_aggregator_merges_the_labelled_results() {
        let (host, dir) = setup(vec![], true);
        let t = team("parallel", &[("a", "analisi"), ("b", "bozza")], Some("capo"));
        let (out, res) = run_team_partial(input(dir.path(), "auto"), t, host.clone()).await;
        res.unwrap();
        // The aggregator (the last delegation) saw both results, labelled by role.
        assert!(out.final_text.starts_with("capo ha svolto:"), "{}", out.final_text);
        assert!(out.final_text.contains("## analisi (a)\na ha svolto: Fai il lavoro"), "{}", out.final_text);
        assert!(out.final_text.contains("## bozza (b)\nb ha svolto"), "{}", out.final_text);
        assert!(out.final_text.contains("[Il tuo ruolo nel team: analisi]"));
        assert_eq!(out.new_messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), ["user", "assistant"]);
        let t = types(&host);
        assert_eq!(t.first().unwrap(), "RUN_STARTED");
        assert_eq!(t.last().unwrap(), "RUN_FINISHED");
        assert_eq!(t.iter().filter(|x| *x == "SUBAGENT_STARTED").count(), 3);
        assert_eq!(t.iter().filter(|x| *x == "SUBAGENT_FINISHED").count(), 3);
    }

    #[tokio::test]
    async fn parallel_without_an_aggregator_joins_the_results_by_role() {
        let (host, dir) = setup(vec![], true);
        let t = team("parallel", &[("a", "analisi"), ("b", "")], None);
        let (out, res) = run_team_partial(input(dir.path(), "auto"), t, host).await;
        res.unwrap();
        assert!(out.final_text.starts_with("## analisi (a)\n"), "{}", out.final_text);
        assert!(out.final_text.contains("\n\n## b\nb ha svolto: Fai il lavoro"), "{}", out.final_text);
    }

    #[tokio::test]
    async fn pipeline_passes_each_output_to_the_next_member() {
        let (host, dir) = setup(vec![], true);
        let t = team("pipeline", &[("a", "prima"), ("b", "seconda")], None);
        let (out, res) = run_team_partial(input(dir.path(), "auto"), t, host.clone()).await;
        res.unwrap();
        // b received the original task plus a's output; the answer is b's.
        assert!(out.final_text.starts_with("b ha svolto: Fai il lavoro"), "{}", out.final_text);
        assert!(out.final_text.contains("# Uscita del passo precedente: prima (a)\na ha svolto: Fai il lavoro"), "{}", out.final_text);
        let t = types(&host);
        assert_eq!(t.iter().filter(|x| *x == "SUBAGENT_STARTED").count(), 2);
    }

    #[tokio::test]
    async fn a_cancelled_team_run_stops_with_an_error() {
        let (host, dir) = setup(vec![], true);
        let inp = input(dir.path(), "auto");
        inp.tool_ctx.cancel.cancel();
        let (out, res) = run_team_partial(inp, team("pipeline", &[("a", "x")], None), host.clone()).await;
        assert!(matches!(res, Err(Error::Cancelled)));
        assert_eq!(out.new_messages.len(), 1, "only the user message");
        assert_eq!(types(&host).last().unwrap(), "RUN_ERROR");
    }

    #[tokio::test]
    async fn an_exhausted_budget_stops_the_pipeline_with_what_it_has() {
        let (host, dir) = setup(vec![], true);
        let mut inp = input(dir.path(), "auto");
        inp.budget = Arc::new(crate::RunBudget::new(None, Some(1), None));
        inp.budget.record_turn();
        let (out, res) = run_team_partial(inp, team("pipeline", &[("a", "x"), ("b", "y")], None), host.clone()).await;
        res.unwrap();
        assert!(out.final_text.contains("Budget del run esaurito"), "{}", out.final_text);
        assert!(!types(&host).contains(&"SUBAGENT_STARTED".to_string()));
    }

    #[tokio::test]
    async fn the_team_schema_checks_the_final_answer() {
        let (host, dir) = setup(vec![], true);
        let mut t = team("pipeline", &[("a", "x")], None);
        t.output_schema = Some(json!({"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"]}));
        let (out, res) = run_team_partial(input(dir.path(), "auto"), t, host.clone()).await;
        res.unwrap();
        // The scripted member answers in prose: kept as it is, flagged.
        assert!(out.structured.is_none());
        assert!(host.events.lock().unwrap().iter().any(|e| matches!(e, AgEvent::Custom { name, .. } if name == "aiw.format_error")));
    }
}
