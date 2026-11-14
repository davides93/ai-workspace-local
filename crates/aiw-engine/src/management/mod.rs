//! The management tools: what the Assistant uses to look after the app from the chat (models,
//! runtimes, connectors, settings, agents, memories) and to ask the person something.
//!
//! They are ordinary tools of the registry, so they go through the same policy, hooks, audit and
//! confirmations as any other, plus three protections that hold wherever the tool is called from:
//!
//! 1. a rule built into the app asks for a confirmation for every tool that changes something
//!    ([`builtin_rules`], added to the person's rules by `Engine::run_rules`), in every mode and
//!    even after «Consenti per la sessione»;
//! 2. the tool works only in a top-level run the server started for a person ([`RunOrigin`]), and
//!    a tool that changes something only when that person is the owner (the checks of
//!    [`require_origin`] and [`require_owner`] are inside each tool, not in the policy);
//! 3. the confirmation is valid only when the owner gives it (`Engine::respond_approval_as`).
//!
//! Each file of this folder holds the tools of one area and exports `tools`, which `register`
//! calls; the contract with the other work items is in the plan of the simple harness.

use crate::{Engine, RunOrigin};
use aiw_agent::PolicyRule;
use aiw_core::{Error, Result};
use aiw_tools::ToolContext;

mod admin;
mod inspect;
mod interaction;
mod models;

/// Every management tool but `ask_user` has a name that starts with this.
pub const PREFIX: &str = "aiw_";
/// The tool that asks the person a question.
pub const ASK_USER: &str = "ask_user";

/// The management tools by name, as the plan fixes them. An agent profile may list them even
/// where a tool is not registered (yet): see [`is_known`].
pub const ALL: [&str; 18] = [
    "aiw_inspect",
    "aiw_model_search",
    "aiw_model_download",
    "aiw_model_use",
    "aiw_model_delete",
    "aiw_runtime_control",
    "aiw_runtime_install",
    "aiw_setting_set",
    "aiw_setting_set_protected",
    "aiw_connector_save",
    "aiw_connector_remove",
    "aiw_agent_save",
    "aiw_agent_delete",
    "aiw_memory_review",
    "aiw_memory_delete",
    "aiw_secret_request",
    "aiw_secret_clear",
    ASK_USER,
];

/// The tools that change something (risk `Write`): always confirmed by the owner.
pub const MODIFYING: [&str; 7] = [
    "aiw_model_download",
    "aiw_model_use",
    "aiw_runtime_control",
    "aiw_setting_set",
    "aiw_agent_save",
    "aiw_memory_review",
    "aiw_secret_clear",
];

/// The tools that change something with a heavier effect (risk `Exec`: they delete, install, run
/// commands, touch machine settings): always confirmed by the owner, shown as a high risk.
pub const STRONG: [&str; 7] = [
    "aiw_model_delete",
    "aiw_runtime_install",
    "aiw_setting_set_protected",
    "aiw_connector_save",
    "aiw_connector_remove",
    "aiw_agent_delete",
    "aiw_memory_delete",
];

/// What a management tool answers outside a chat run the server started for a person.
pub const NOT_IN_A_CHAT: &str = "Strumento di gestione: disponibile solo all'Assistente in una chat.";

/// Is this a management tool? True for the names that start with `aiw_` and for `ask_user`. It
/// holds for names that are not (or not yet) registered, and it never looks at the registry.
pub fn is_management(name: &str) -> bool {
    name.starts_with(PREFIX) || name == ASK_USER
}

/// Is this one of the management tools of the plan? An agent profile may list them (the Assistant
/// does) even when this build has not registered a tool of that name yet.
pub fn is_known(name: &str) -> bool {
    ALL.contains(&name)
}

/// The rules built into the app: a confirmation (`ask`) for each tool of [`MODIFYING`] and
/// [`STRONG`]. `Engine::run_rules` adds them to the person's own rules wherever a policy is built.
/// An `ask` rule is looked at before «Consenti per la sessione» and before the mode, so it holds in
/// `auto` and `yolo` too; only a rule the person wrote in Security can still allow or deny.
pub fn builtin_rules() -> Vec<PolicyRule> {
    MODIFYING.iter().chain(STRONG.iter()).map(|name| PolicyRule { tool: (*name).into(), pattern: None, action: "ask".into() }).collect()
}

/// The origin of the run a management tool is called from. Refuses (without looking at the
/// policy) a call that is not in a top-level run the server started for a person: a workflow, the
/// MCP server, a channel, the proactive assistant, a sub-agent.
pub(crate) fn require_origin(engine: &Engine, ctx: &ToolContext) -> Result<RunOrigin> {
    engine.run_origin(&ctx.run_id).ok_or_else(|| Error::Denied(NOT_IN_A_CHAT.into()))
}

/// [`require_origin`], and the person who asked must be the owner: every tool that changes
/// something starts with this.
pub(crate) fn require_owner(engine: &Engine, ctx: &ToolContext) -> Result<RunOrigin> {
    let origin = require_origin(engine, ctx)?;
    if !origin.owner {
        return Err(Error::Denied(
            "Questa azione richiede l'accesso completo di chi usa l'app su questo computer: da questo dispositivo non è consentita.".into(),
        ));
    }
    Ok(origin)
}

/// Registers the management tools of every area. Called once, when the engine starts.
pub(crate) fn register(engine: &Engine) {
    for tool in
        inspect::tools(engine).into_iter().chain(models::tools(engine)).chain(admin::tools(engine)).chain(interaction::tools(engine))
    {
        engine.0.tools.register(tool);
    }
}

/// A [`ToolContext`] for the tests of the tools: it registers a run with the given origin (`None`:
/// a run without one) and returns the context a tool of that run would get. The run never ends,
/// like a chat run waiting for the person.
#[cfg(test)]
pub(crate) fn test_ctx(engine: &Engine, origin: Option<RunOrigin>) -> ToolContext {
    use tokio_util::sync::CancellationToken;
    let run_id = aiw_core::ids::new_id("run");
    engine.0.runs.lock().unwrap().insert(
        run_id.clone(),
        crate::RunHandle { session_id: None, cancel: CancellationToken::new(), events: vec![], done: false, origin },
    );
    ToolContext::new(&run_id, engine.workspace_root())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{engine, wait_finished};
    use crate::{Inner, RunOrigin};
    use aiw_agent::{Decision, Policy};
    use aiw_core::api::{AgentProfile, ApprovalRequest};
    use aiw_runtimes::ScriptedBackend;
    use aiw_tools::{Risk, Tool, ToolOutput};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Weak};
    use std::time::Duration;

    const OWNER: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };

    /// A management tool that changes "something": it checks the owner like the real ones do and counts the calls.
    struct FakeChange {
        engine: Weak<Inner>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Tool for FakeChange {
        fn name(&self) -> &str {
            "aiw_setting_set"
        }
        fn description(&self) -> &str {
            "Strumento di prova che modifica qualcosa."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {"key": {"type": "string"}}})
        }
        fn risk(&self) -> Risk {
            Risk::Write
        }
        fn explicit_grant(&self) -> bool {
            true
        }
        async fn call(&self, _args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
            let engine = Engine(self.engine.upgrade().expect("engine alive"));
            require_owner(&engine, ctx)?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::text("fatto"))
        }
    }

    /// The engine with the fake tool registered in place of `aiw_setting_set`, and its call counter.
    async fn with_fake(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, Arc<AtomicUsize>, tempfile::TempDir, tempfile::TempDir) {
        let (e, d, w) = engine(turns).await;
        let calls = Arc::new(AtomicUsize::new(0));
        e.tool_registry().register(Arc::new(FakeChange { engine: Arc::downgrade(&e.0), calls: calls.clone() }));
        (e, calls, d, w)
    }

    /// A copy of the planner with the given id, mode and tools.
    fn agent(e: &Engine, id: &str, mode: &str, tools: &[&str]) -> AgentProfile {
        let a = AgentProfile {
            id: id.into(),
            name: id.into(),
            mode: mode.into(),
            tools: tools.iter().map(|t| t.to_string()).collect(),
            ..e.agent("planner").unwrap()
        };
        e.save_agent(a).unwrap()
    }

    async fn next_approval(e: &Engine) -> ApprovalRequest {
        for _ in 0..250 {
            if let Some(a) = e.approvals().into_iter().next() {
                return a;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no confirmation was asked");
    }

    fn change() -> Vec<aiw_runtimes::ChatDelta> {
        ScriptedBackend::tool_turn("c1", "aiw_setting_set", json!({"key": "x"}))
    }

    #[test]
    fn the_names_of_the_contract_are_all_management_tools_and_nothing_else_is() {
        assert_eq!(ALL.len(), 18);
        assert!(ALL.iter().all(|n| is_management(n) && is_known(n)));
        assert!(is_management("aiw_something_new") && !is_known("aiw_something_new"));
        for plain in ["read_file", "shell", "agent", "todo", "memory_save", "channel_send", "mcp__x__aiw_inspect", "ask", "ask_users"] {
            assert!(!is_management(plain), "{plain}");
        }
    }

    #[test]
    fn modifying_and_strong_tools_are_disjoint_known_names_and_get_an_ask_rule_each() {
        let mut names: Vec<&str> = MODIFYING.iter().chain(STRONG.iter()).copied().collect();
        assert_eq!(names.len(), 14);
        assert!(names.iter().all(|n| is_known(n)));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 14, "a tool is either modifying or strong");
        // The tools that need no confirmation are exactly the four of the plan.
        let free: Vec<&str> = ALL.iter().copied().filter(|n| !names.contains(n)).collect();
        assert_eq!(free, ["aiw_inspect", "aiw_model_search", "aiw_secret_request", "ask_user"]);
        let rules = builtin_rules();
        assert_eq!(rules.len(), 14);
        assert!(rules.iter().all(|r| r.action == "ask" && r.pattern.is_none()));
        for name in &names {
            assert!(rules.iter().any(|r| r.tool == *name), "{name}");
        }
    }

    #[test]
    fn builtin_rules_ask_in_every_mode_and_after_session_allow_but_the_persons_own_rules_still_win() {
        let rules = builtin_rules();
        let policy = |mode: &str, extra: Vec<PolicyRule>| Policy {
            mode: mode.into(),
            rules: extra.into_iter().chain(rules.clone()).collect(),
            sandbox_available: true,
        };
        for name in MODIFYING.iter().chain(STRONG.iter()) {
            for mode in ["ask", "auto-edit", "auto", "yolo"] {
                for session_allowed in [false, true] {
                    assert!(
                        matches!(policy(mode, vec![]).evaluate(name, Risk::Write, None, session_allowed), Decision::Ask(_)),
                        "{name} in {mode} (session {session_allowed})"
                    );
                }
            }
        }
        // The reading tools are not asked about in `yolo`, as ever; a lookalike name is not caught by a rule.
        assert_eq!(policy("yolo", vec![]).evaluate("aiw_inspect", Risk::Read, None, false), Decision::Allow);
        assert_eq!(policy("yolo", vec![]).evaluate("aiw_setting_set_extra", Risk::Write, None, false), Decision::Allow);
        // A rule the person wrote in Security keeps its place: `deny` closes the tool, `allow` is their explicit choice.
        let rule = |tool: &str, action: &str| PolicyRule { tool: tool.into(), pattern: None, action: action.into() };
        assert!(matches!(
            policy("yolo", vec![rule("aiw_model_delete", "deny")]).evaluate("aiw_model_delete", Risk::Exec, None, false),
            Decision::Deny(_)
        ));
        assert_eq!(
            policy("ask", vec![rule("aiw_model_use", "allow")]).evaluate("aiw_model_use", Risk::Write, None, false),
            Decision::Allow
        );
    }

    #[tokio::test]
    async fn the_built_in_rules_are_in_every_policy_the_engine_builds_but_not_in_the_persons_rules() {
        let (e, _d, _w) = engine(vec![]).await;
        assert!(e.policy_rules().is_empty(), "the person's own rules are what the policy page shows");
        let rules = e.run_rules();
        assert_eq!(rules, builtin_rules());
        // The person's rules come along, and the built-in ones are not stored as if they were the person's.
        e.set_policy_rules(vec![PolicyRule { tool: "shell".into(), pattern: Some("rm *".into()), action: "deny".into() }]).unwrap();
        assert_eq!(e.policy_rules().len(), 1);
        assert_eq!(e.run_rules().len(), 1 + builtin_rules().len());
        // The input of a run is built with them.
        let agent = e.agent("planner").unwrap();
        let profile = e.profile(&agent.inference_profile).unwrap_or_else(|_| e.profiles().unwrap().remove(0));
        let input = e.build_input(
            "run_x",
            "thread_x",
            agent,
            profile,
            vec![],
            "ciao".into(),
            tokio_util::sync::CancellationToken::new(),
            0,
            Default::default(),
        );
        assert_eq!(input.policy.rules, e.run_rules());
    }

    #[tokio::test]
    async fn a_change_asks_in_yolo_and_again_after_allow_for_the_session_and_only_the_owner_counts() {
        let (e, calls, _d, _w) = with_fake(vec![change(), change(), ScriptedBackend::text_turn("Fatto, due volte.")]).await;
        agent(&e, "gestore", "yolo", &["aiw_setting_set"]);
        let s = e.create_session("gestore", None).unwrap();
        let run = e.start_run_from(&s.id, "cambia".into(), None, None, vec![], Some(OWNER)).await.unwrap();

        // First call: yolo mode would run anything, but the built-in rule asks. The owner allows it for the session.
        let first = next_approval(&e).await;
        assert_eq!(first.tool, "aiw_setting_set");
        assert_eq!(first.risk, "medium");
        e.respond_approval_as(&first.id, true, true, true).unwrap();
        // Second call: «Consenti per la sessione» did not stick, it asks again.
        let second = loop {
            let a = next_approval(&e).await;
            if a.id != first.id {
                break a;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(!e.session_allow(&s.id).lock().unwrap().contains("aiw_setting_set"), "a management tool is never allowed for the session");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        e.respond_approval_as(&second.id, true, false, true).unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(e.messages(&s.id).unwrap().last().unwrap().content, "Fatto, due volte.");
        // The audit trail has both decisions, once each.
        let approved = e
            .audit()
            .into_iter()
            .filter(|a| a.tool.as_deref() == Some("aiw_setting_set") && a.decision.as_deref() == Some("approved"))
            .count();
        assert!(approved >= 2, "{approved}");
    }

    #[tokio::test]
    async fn a_confirmation_of_a_management_tool_from_a_non_owner_is_refused_and_stays_open() {
        let (e, calls, _d, _w) = with_fake(vec![change(), ScriptedBackend::text_turn("ok")]).await;
        agent(&e, "gestore", "ask", &["aiw_setting_set"]);
        let s = e.create_session("gestore", None).unwrap();
        let run = e.start_run_from(&s.id, "cambia".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        let req = next_approval(&e).await;
        // A channel or a paired phone answers through `respond_approval`: allow or deny, it is refused.
        for allow in [true, false] {
            let err = e.respond_approval(&req.id, allow, false).unwrap_err();
            assert!(matches!(err, Error::Denied(_)) && err.status() == 403, "{err:?}");
            let err = e.respond_approval_as(&req.id, allow, true, false).unwrap_err();
            assert!(matches!(err, Error::Denied(_)), "{err:?}");
        }
        assert_eq!(e.approvals().len(), 1, "the request is still open");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!e.run_done(&run));
        // The owner decides.
        e.respond_approval_as(&req.id, true, false, true).unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // An unknown request is still a 404, whoever asks.
        assert!(matches!(e.respond_approval(&req.id, true, false), Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn the_confirmations_of_ordinary_tools_still_work_for_anyone() {
        let turns =
            vec![ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "a.md", "content": "x"})), ScriptedBackend::text_turn("ok")];
        let (e, _d, ws) = engine(turns).await;
        let s = e.create_session("operator", None).unwrap();
        let run = e.start_run_from(&s.id, "scrivi".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        let req = next_approval(&e).await;
        e.respond_approval(&req.id, true, false).unwrap();
        wait_finished(&e, &run).await;
        assert!(ws.path().join("a.md").exists());
    }

    #[tokio::test]
    async fn without_an_origin_or_without_ownership_a_management_tool_refuses() {
        let (e, calls, _d, _w) = with_fake(vec![]).await;
        let tool = e.tool_registry().get("aiw_setting_set").unwrap();
        // No origin: the run is not one a person started through the server.
        let err = tool.call(json!({}), &test_ctx(&e, None)).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("disponibile solo all'Assistente in una chat"), "{err}");
        // A run nobody registered (a made-up id) has none either.
        let err = tool.call(json!({}), &ToolContext::new("run_inventato", e.workspace_root())).await.unwrap_err();
        assert!(err.to_string().contains("disponibile solo all'Assistente in una chat"), "{err}");
        // An origin without ownership (a paired phone): tools that change something refuse, with their own words.
        let not_owner = RunOrigin { owner: false, questions: true, secrets: false };
        let err = tool.call(json!({}), &test_ctx(&e, Some(not_owner))).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("accesso completo"), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // The owner's run goes through.
        assert_eq!(tool.call(json!({}), &test_ctx(&e, Some(OWNER))).await.unwrap().content, "fatto");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_tool_refuses_a_management_tool_without_asking_anybody() {
        let (e, calls, _d, _w) = with_fake(vec![]).await;
        // Workflows and MCP clients come through here with a run id of their own: no origin.
        for run_id in ["mcp_1", "wfr_1"] {
            let call = e.execute_tool(run_id, "agent", "Client MCP", "aiw_setting_set", json!({"key": "x"}));
            let err = tokio::time::timeout(Duration::from_secs(5), call).await.expect("must not wait for a confirmation").unwrap_err();
            assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("disponibile solo all'Assistente in una chat"), "{err}");
        }
        assert!(e.approvals().is_empty(), "nobody is asked to confirm what cannot work");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // Not even with the id of a run that has an origin... of a run that ended.
        let ctx = test_ctx(&e, Some(OWNER));
        e.emit(&ctx.run_id, aiw_core::events::AgEvent::RunFinished { thread_id: "t".into(), run_id: ctx.run_id.clone(), result: None });
        assert!(e.run_origin(&ctx.run_id).is_none(), "a finished run has no origin any more");
        let err = e.execute_tool(&ctx.run_id, "agent", "x", "aiw_setting_set", json!({})).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_run_has_an_origin_only_when_the_caller_gave_one_and_a_sub_agent_never_has() {
        let (e, calls, _d, _w) = with_fake(vec![]).await;
        let backend = ScriptedBackend::new(vec![
            ScriptedBackend::tool_turn("d1", "agent", json!({"agent": "figlio", "task": "prova"})),
            // The child tries the management tool anyway (it lists it, but a sub-agent is never offered one).
            ScriptedBackend::tool_turn("f1", "aiw_setting_set", json!({"key": "x"})),
            ScriptedBackend::text_turn("fatto dal figlio"),
            ScriptedBackend::text_turn("fine"),
        ]);
        e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
        let mut capo = agent(&e, "capo", "ask", &["agent", "aiw_setting_set"]);
        capo.delegate_tools = vec!["*".into()];
        e.save_agent(capo).unwrap();
        agent(&e, "figlio", "ask", &["aiw_setting_set", "todo"]);
        let s = e.create_session("capo", None).unwrap();
        let run = e.start_run_from(&s.id, "delega".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        assert_eq!(e.run_origin(&run), Some(OWNER));
        wait_finished(&e, &run).await;
        assert_eq!(e.run_origin(&run), None, "an origin is for a run that is going");
        let offered =
            |i: usize| -> Vec<String> { backend.requests.lock().unwrap()[i].tools.iter().map(|t| t.function.name.clone()).collect() };
        assert!(offered(0).contains(&"aiw_setting_set".to_string()), "the parent is offered its management tool");
        assert!(!offered(1).contains(&"aiw_setting_set".to_string()), "{:?}: a sub-agent is never offered one", offered(1));
        assert!(offered(1).contains(&"todo".to_string()));
        // The call the child made anyway found no such tool for it (the model was told so), and nothing changed.
        let told = backend.requests.lock().unwrap()[2].messages.last().map(|m| m.text()).unwrap_or_default();
        assert!(told.contains("non è abilitato per questo agente"), "{told}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(e.audit().iter().any(|a| a.tool.as_deref() == Some("aiw_setting_set") && a.decision.as_deref() == Some("deny")));

        // A run started without an origin (the CLI, a workflow, a channel) has none.
        let s2 = e.create_session("planner", None).unwrap();
        let run2 = e.start_run(&s2.id, "ciao".into()).await.unwrap();
        assert_eq!(e.run_origin(&run2), None);
        wait_finished(&e, &run2).await;
    }

    #[tokio::test]
    async fn a_run_without_an_origin_asks_and_then_the_tool_refuses_so_nothing_changes() {
        let (e, calls, _d, _w) = with_fake(vec![change(), ScriptedBackend::text_turn("non ho potuto")]).await;
        agent(&e, "gestore", "ask", &["aiw_setting_set"]);
        let s = e.create_session("gestore", None).unwrap();
        let run = e.start_run(&s.id, "cambia".into()).await.unwrap();
        let req = next_approval(&e).await;
        e.respond_approval_as(&req.id, true, false, true).unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0, "the owner said yes, but the run has no origin");
        let tool_result = e.messages(&s.id).unwrap().into_iter().find(|m| m.role == "tool").unwrap().content;
        assert!(tool_result.contains("disponibile solo all'Assistente in una chat"), "{tool_result}");
    }

    #[tokio::test]
    async fn an_agent_may_list_the_management_tools_even_before_they_are_registered() {
        let (e, _d, _w) = engine(vec![]).await;
        // `aiw_model_delete` is not registered by this item, `ask_user` neither: the profile is valid all the same.
        let tools = ["aiw_model_delete", "ask_user", "aiw_secret_request", "aiw_inspect"];
        agent(&e, "gestore", "ask", &tools);
        // Names that are not in the contract are as unknown as ever.
        let a = AgentProfile { tools: vec!["aiw_inventato".into()], ..e.agent("gestore").unwrap() };
        assert!(e.save_agent(a).unwrap_err().to_string().contains("aiw_inventato"));
    }
}
