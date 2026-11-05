//! Agentic layer (architecture §5): the agent loop and its policy, hooks, skills and helpers.

pub mod a2ui;
pub mod code;
pub mod fallback;
pub mod hooks;
pub mod internal;
pub mod narrow;
pub mod policy;
pub mod runner;
pub mod skills;
pub mod think;

pub use hooks::HookConfig;
pub use internal::{internal_tools, InternalTool};
pub use narrow::{narrow_sandbox, narrow_tools};
pub use policy::{Decision, Policy, PolicyRule};
pub use runner::{to_msgs, AgentRunner, RunHost, RunInput, RunOutcome};
pub use skills::Skill;

#[cfg(test)]
pub(crate) mod tests_support {
    pub use super::tests::TestHost;
    pub use super::tests::{input, setup};
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::api::{AgentProfile, ApprovalRequest, AuditEntry, ChatMessage, InferenceParams, InferenceProfile};
    use aiw_core::events::AgEvent;
    use aiw_core::Result;
    use aiw_runtimes::{ChatBackend, ChatDelta, ScriptedBackend};
    use aiw_tools::{ToolContext, ToolRegistry};
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    pub struct TestHost {
        pub(crate) backend: ScriptedBackend,
        tools: Arc<ToolRegistry>,
        pub(crate) events: Mutex<Vec<AgEvent>>,
        /// Pause inside `subagent`, to tell parallel runs from sequential ones.
        pub(crate) subagent_delay_ms: std::sync::atomic::AtomicU64,
        approve: bool,
        audits: Mutex<Vec<AuditEntry>>,
        /// Tests that need a backend which can fail put it here instead of the scripted one.
        replacement: Mutex<Option<Arc<dyn ChatBackend>>>,
    }

    #[async_trait]
    impl RunHost for TestHost {
        fn emit(&self, _run_id: &str, ev: AgEvent) {
            self.events.lock().unwrap().push(ev);
        }
        async fn approve(&self, _req: ApprovalRequest) -> bool {
            self.approve
        }
        fn audit(&self, e: AuditEntry) {
            self.audits.lock().unwrap().push(e);
        }
        async fn backend(&self, _p: &InferenceProfile) -> Result<Arc<dyn ChatBackend>> {
            if let Some(b) = self.replacement.lock().unwrap().clone() {
                return Ok(b);
            }
            Ok(Arc::new(self.backend.clone()))
        }
        fn tools(&self) -> Arc<ToolRegistry> {
            self.tools.clone()
        }
        fn skills(&self) -> Vec<Skill> {
            vec![]
        }
        async fn subagent(&self, _p: &RunInput, agent: &str, task: &str) -> Result<String> {
            let delay = self.subagent_delay_ms.load(std::sync::atomic::Ordering::Relaxed);
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            Ok(format!("{agent} ha svolto: {task}"))
        }
    }

    pub fn setup(turns: Vec<Vec<ChatDelta>>, approve: bool) -> (Arc<TestHost>, tempfile::TempDir) {
        let reg = ToolRegistry::default();
        for t in aiw_tools::fs::tools() {
            reg.register(t);
        }
        reg.register(Arc::new(aiw_tools::shell::ShellTool));
        let host = Arc::new(TestHost {
            backend: ScriptedBackend::new(turns),
            tools: Arc::new(reg),
            events: Mutex::new(vec![]),
            subagent_delay_ms: Default::default(),
            approve,
            audits: Mutex::new(vec![]),
            replacement: Mutex::new(None),
        });
        (host, tempfile::tempdir().unwrap())
    }

    pub fn input(dir: &std::path::Path, mode: &str) -> RunInput {
        RunInput {
            run_id: "run_1".into(),
            thread_id: "ses_1".into(),
            agent: AgentProfile {
                id: "op".into(),
                name: "Operatore".into(),
                description: String::new(),
                inference_profile: "p".into(),
                system_prompt: String::new(),
                tools: vec!["*".into()],
                mode: mode.into(),
                sandbox: "workspace-write".into(),
                max_turns: 8,
            },
            profile: InferenceProfile {
                id: "p".into(),
                name: "p".into(),
                description: None,
                runtime_id: "scripted".into(),
                model: "scripted".into(),
                params: InferenceParams { context: Some(8192), ..Default::default() },
            },
            history: vec![],
            user_input: "Fai il lavoro".into(),
            system_extra: vec![],
            tool_ctx: ToolContext::new("run_1", dir.to_path_buf()),
            policy: Policy { mode: mode.into(), rules: vec![], sandbox_available: true },
            hooks: vec![],
            compact_at: 0.8,
            depth: 0,
            session_allow: Arc::new(Mutex::new(HashSet::new())),
            subagents: vec![("planner".into(), "pianifica".into())],
            max_parallel_subagents: 3,
        }
    }

    fn types(h: &TestHost) -> Vec<String> {
        h.events.lock().unwrap().iter().map(|e| serde_json::to_value(e).unwrap()["type"].as_str().unwrap().to_string()).collect()
    }

    #[tokio::test]
    async fn read_tool_then_answer() {
        let (host, dir) = setup(
            vec![ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})), ScriptedBackend::text_turn("La cartella è vuota.")],
            true,
        );
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let out = AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        assert_eq!(out.final_text, "La cartella è vuota.");
        assert_eq!(out.new_messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), vec!["user", "assistant", "tool", "assistant"]);
        assert!(out.new_messages[2].content.contains("a.txt"));
        let t = types(&host);
        assert_eq!(t.first().unwrap(), "RUN_STARTED");
        assert_eq!(t.last().unwrap(), "RUN_FINISHED");
        for needed in [
            "TOOL_CALL_START",
            "TOOL_CALL_ARGS",
            "TOOL_CALL_END",
            "TOOL_CALL_RESULT",
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "TEXT_MESSAGE_END",
            "STATE_SNAPSHOT",
        ] {
            assert!(t.contains(&needed.to_string()), "missing {needed}: {t:?}");
        }
        // the tool definitions were sent to the model
        let reqs = host.backend.requests.lock().unwrap();
        assert!(reqs[0].tools.iter().any(|t| t.function.name == "list_dir"));
        assert!(reqs[1].messages.iter().any(|m| m.role == "tool"));
    }

    #[tokio::test]
    async fn writes_need_approval_in_ask_mode() {
        let turns = || {
            vec![
                ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "out.txt", "content": "ciao"})),
                ScriptedBackend::text_turn("ok"),
            ]
        };
        let (host, dir) = setup(turns(), false);
        AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        assert!(!dir.path().join("out.txt").exists(), "denied write must not happen");
        assert!(types(&host).contains(&"CUSTOM".to_string()), "approval request emitted");
        assert!(host.audits.lock().unwrap().iter().any(|a| a.decision.as_deref() == Some("rejected")));

        let (host, dir) = setup(turns(), true);
        AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("out.txt")).unwrap(), "ciao");
    }

    #[tokio::test]
    async fn plan_mode_denies_side_effects_without_asking() {
        let (host, dir) = setup(
            vec![ScriptedBackend::tool_turn("c1", "shell", json!({"command": "touch x"})), ScriptedBackend::text_turn("Ecco il piano")],
            true,
        );
        let out = AgentRunner::run(input(dir.path(), "plan"), host.clone()).await.unwrap();
        assert!(out.new_messages[2].content.contains("Negato dalla policy"));
        assert!(!dir.path().join("x").exists());
        assert!(!types(&host).contains(&"CUSTOM".to_string()));
    }

    #[tokio::test]
    async fn todo_and_subagent_and_text_fallback() {
        let turns = vec![
            ScriptedBackend::tool_turn("c1", "todo", json!({"todos": [{"content": "Leggere", "status": "in_progress"}]})),
            ScriptedBackend::tool_turn("c2", "agent", json!({"agent": "planner", "task": "fai un piano"})),
            vec![
                ChatDelta::Text(
                    "<think>uso un tool</think>Controllo.<tool_call>{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}</tool_call>"
                        .into(),
                ),
                ChatDelta::Done { finish_reason: Some("stop".into()) },
            ],
            ScriptedBackend::text_turn("Finito"),
        ];
        let (host, dir) = setup(turns, true);
        let out = AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        assert_eq!(out.final_text, "Finito");
        let evs = host.events.lock().unwrap();
        assert!(evs
            .iter()
            .any(|e| matches!(e, AgEvent::StateSnapshot { snapshot } if snapshot.todos.as_ref().map(|t| t.len()) == Some(1))));
        assert!(evs.iter().any(|e| matches!(e, AgEvent::SubagentFinished { result: Some(r), .. } if r.contains("planner ha svolto"))));
        assert!(evs.iter().any(|e| matches!(e, AgEvent::ReasoningMessageContent { delta, .. } if delta.contains("uso un tool"))));
        assert!(out.new_messages.iter().any(|m| m.role == "tool" && m.name.as_deref() == Some("list_dir")), "text tool call executed");
    }

    #[tokio::test]
    async fn render_ui_emits_a2ui_messages() {
        let ui = json!({"surfaceId": "scelta", "components": [
            {"id": "root", "component": "Column", "children": ["q", "ok"]},
            {"id": "q", "component": "ChoicePicker", "label": "Formato", "options": [{"label": "PDF", "value": "pdf"}], "value": {"path": "/fmt"}},
            {"id": "ok", "component": "Button", "label": "Avanti", "action": {"event": {"name": "scegli", "context": {"fmt": {"path": "/fmt"}}}}}
        ], "data": {"fmt": ["pdf"]}});
        let turns = vec![
            ScriptedBackend::tool_turn("c1", "render_ui", json!({"components": [{"id": "x", "component": "Text", "text": "no root"}]})),
            ScriptedBackend::tool_turn("c2", "render_ui", ui),
            ScriptedBackend::text_turn("Scegli il formato."),
        ];
        let (host, dir) = setup(turns, true);
        let out = AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        assert_eq!(out.final_text, "Scegli il formato.");
        let tools: Vec<&ChatMessage> = out.new_messages.iter().filter(|m| m.role == "tool").collect();
        assert!(tools[0].content.starts_with("ERRORE") && tools[0].content.contains("root"), "{}", tools[0].content);
        assert!(tools[1].content.contains("mostrata all'utente"));
        let evs = host.events.lock().unwrap();
        let surface = evs.iter().find_map(|e| match e {
            AgEvent::Custom { name, value } if name == a2ui::EVENT => Some(value.clone()),
            _ => None,
        });
        let surface = surface.expect("a2ui event");
        assert_eq!(surface["surfaceId"], "scelta");
        assert_eq!(surface["messages"].as_array().unwrap().len(), 3);
        // the tool is offered and documented in the system prompt
        let reqs = host.backend.requests.lock().unwrap();
        assert!(reqs[0].tools.iter().any(|t| t.function.name == "render_ui"));
        assert!(reqs[0].messages[0].text().contains("A2UI"));
    }

    #[tokio::test]
    async fn code_mode_runs_tools_through_policy() {
        let script = r#"
            let listing = tool("list_dir", #{path: "."});
            let n = listing.split("\n").filter(|l| l.contains(".txt")).len();
            let w = "ok";
            try { tool("write_file", #{path: "x.txt", content: "no"}); } catch (e) { w = `${e}`; }
            `txt: ${n}; scrittura: ${w}`
        "#;
        let turns = vec![ScriptedBackend::tool_turn("c1", "code", json!({"script": script})), ScriptedBackend::text_turn("Fatto")];
        let (host, dir) = setup(turns, true);
        std::fs::write(dir.path().join("a.txt"), "1").unwrap();
        std::fs::write(dir.path().join("b.txt"), "2").unwrap();
        // Plan mode: reads are allowed, the write inside the script is denied by policy.
        let out = AgentRunner::run(input(dir.path(), "plan"), host.clone()).await.unwrap();
        let result = out.new_messages.iter().find(|m| m.role == "tool").unwrap();
        assert!(result.content.contains("txt: 2"), "{}", result.content);
        assert!(result.content.contains("Negato dalla policy"), "{}", result.content);
        assert!(!dir.path().join("x.txt").exists());
        let evs = host.events.lock().unwrap();
        let nested: Vec<String> = evs
            .iter()
            .filter_map(|e| match e {
                AgEvent::ToolCallStart { tool_call_name, .. } => Some(tool_call_name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(nested, vec!["code", "list_dir", "write_file"]);
    }

    #[tokio::test]
    async fn self_check_hands_back_unretried_errors() {
        let turns = vec![
            ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "- nuova"})),
            ScriptedBackend::text_turn("Fatto, ho aggiunto la riga."),
            ScriptedBackend::tool_turn("c2", "write_file", json!({"path": "note.md", "content": "- nuova", "append": true})),
            ScriptedBackend::text_turn("Ora la riga è in fondo al file."),
        ];
        let (host, dir) = setup(turns, true);
        std::fs::write(dir.path().join("note.md"), "Note:\n").unwrap();
        let mut inp = input(dir.path(), "auto");
        inp.tool_ctx.require_read_before_overwrite = true;
        let out = AgentRunner::run(inp, host.clone()).await.unwrap();
        assert_eq!(out.final_text, "Ora la riga è in fondo al file.");
        assert_eq!(std::fs::read_to_string(dir.path().join("note.md")).unwrap(), "Note:\n- nuova");
        let note = out.new_messages.iter().find(|m| m.name.as_deref() == Some(runner::SELF_CHECK_NAME)).expect("self-check note");
        assert_eq!(note.role, "user");
        assert!(note.content.contains("write_file"));
        assert!(host.events.lock().unwrap().iter().any(|e| matches!(e, AgEvent::Custom { name, .. } if name == "aiw.self_check")));
    }

    /// Returns images like `browser_screenshot` and the `computer_*` tools do.
    struct FakeShot;
    #[async_trait]
    impl aiw_tools::Tool for FakeShot {
        fn name(&self) -> &str {
            "fake_screenshot"
        }
        fn description(&self) -> &str {
            "screenshot finto"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        fn risk(&self) -> aiw_tools::Risk {
            aiw_tools::Risk::Read
        }
        async fn call(&self, _args: serde_json::Value, _ctx: &ToolContext) -> Result<aiw_tools::ToolOutput> {
            Ok(aiw_tools::ToolOutput {
                content: "Screenshot allegato.".into(),
                images: vec![
                    "data:image/png;base64,iVBORw0KGgo=".into(),
                    "data:image/jpeg;base64,/9j/4AAQ".into(),
                    "https://example.org/not-a-data-url.png".into(),
                ],
                is_error: false,
            })
        }
    }

    /// Cancels the run, as the kill switch or the stop button would, while it executes.
    struct Canceller;
    #[async_trait]
    impl aiw_tools::Tool for Canceller {
        fn name(&self) -> &str {
            "stopper"
        }
        fn description(&self) -> &str {
            "annulla il run"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        fn risk(&self) -> aiw_tools::Risk {
            aiw_tools::Risk::Write
        }
        async fn call(&self, _args: serde_json::Value, ctx: &ToolContext) -> Result<aiw_tools::ToolOutput> {
            ctx.cancel.cancel();
            Ok(aiw_tools::ToolOutput::text("fatto"))
        }
    }

    /// A backend whose turns may break half way.
    struct Flaky(Mutex<std::collections::VecDeque<Vec<Result<ChatDelta>>>>);
    #[async_trait]
    impl ChatBackend for Flaky {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec!["flaky".into()])
        }
        async fn chat(
            &self,
            _req: aiw_runtimes::ChatRequest,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<aiw_runtimes::DeltaStream> {
            use futures::StreamExt;
            let turn = self.0.lock().unwrap().pop_front().unwrap_or_default();
            Ok(futures::stream::iter(turn).boxed())
        }
    }

    fn image_values(h: &TestHost) -> Vec<(usize, serde_json::Value)> {
        h.events
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                AgEvent::Custom { name, value } if name == "aiw.image" => Some((i, value.clone())),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn tool_images_reach_the_ui_as_aiw_image_events() {
        let (host, dir) = setup(
            vec![ScriptedBackend::tool_turn("c1", "fake_screenshot", json!({})), ScriptedBackend::text_turn("Vedo la pagina.")],
            true,
        );
        host.tools.register(Arc::new(FakeShot));
        AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();

        let imgs = image_values(&host);
        assert_eq!(imgs.len(), 2, "one event per data URL image, the plain URL is skipped: {imgs:?}");
        assert_eq!(imgs[0].1, json!({"toolCallId": "c1", "mime": "image/png", "data": "iVBORw0KGgo="}));
        assert_eq!(imgs[1].1, json!({"toolCallId": "c1", "mime": "image/jpeg", "data": "/9j/4AAQ"}));
        // Right after the TOOL_CALL_RESULT of that call.
        let evs = host.events.lock().unwrap();
        let result_at = evs.iter().position(|e| matches!(e, AgEvent::ToolCallResult { tool_call_id, .. } if tool_call_id == "c1")).unwrap();
        assert_eq!((imgs[0].0, imgs[1].0), (result_at + 1, result_at + 2));
        // The model still gets the images as a message of its own.
        let reqs = host.backend.requests.lock().unwrap();
        let last = reqs[1].messages.last().unwrap();
        assert!(matches!(&last.content, Some(aiw_runtimes::MsgContent::Parts(p)) if p.len() >= 3), "{:?}", last.content);
    }

    #[tokio::test]
    async fn images_from_code_mode_calls_are_emitted_for_the_nested_call() {
        let script = r#"let r = tool("fake_screenshot", #{}); r"#;
        let (host, dir) =
            setup(vec![ScriptedBackend::tool_turn("c1", "code", json!({"script": script})), ScriptedBackend::text_turn("Ok")], true);
        host.tools.register(Arc::new(FakeShot));
        AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.unwrap();
        let nested_id = host
            .events
            .lock()
            .unwrap()
            .iter()
            .find_map(|e| match e {
                AgEvent::ToolCallStart { tool_call_id, tool_call_name, .. } if tool_call_name == "fake_screenshot" => {
                    Some(tool_call_id.clone())
                }
                _ => None,
            })
            .expect("nested call");
        let imgs = image_values(&host);
        assert_eq!(imgs.len(), 2, "{imgs:?}");
        assert!(imgs.iter().all(|(_, v)| v["toolCallId"] == nested_id.as_str()));
    }

    #[tokio::test]
    async fn a_failed_run_hands_back_what_it_did() {
        let (host, dir) = setup(vec![], true);
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        *host.replacement.lock().unwrap() = Some(Arc::new(Flaky(Mutex::new(
            vec![
                ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})).into_iter().map(Ok).collect(),
                // The second turn dies after a few words.
                vec![
                    Ok(ChatDelta::Text("Ho letto la cartella e sto per".into())),
                    Err(aiw_core::Error::Runtime("connessione persa".into())),
                ],
            ]
            .into(),
        ))));
        let (out, res) = AgentRunner::run_partial(input(dir.path(), "ask"), host.clone()).await;
        assert!(matches!(res, Err(aiw_core::Error::Runtime(_))), "{res:?}");
        let roles: Vec<&str> = out.new_messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "tool", "assistant"], "{:?}", out.new_messages);
        assert!(out.new_messages[2].content.contains("a.txt"));
        assert_eq!(out.new_messages[3].content, "Ho letto la cartella e sto per");
        assert!(out.new_messages[3].tool_calls.is_none());
        {
            let evs = host.events.lock().unwrap();
            assert!(matches!(evs.last(), Some(AgEvent::RunError { code: Some(c), .. }) if c == "runtime"), "{:?}", evs.last());
        }
        // `run` keeps its contract: just the error.
        *host.replacement.lock().unwrap() = Some(Arc::new(Flaky(Mutex::new(vec![vec![Err(aiw_core::Error::Runtime("x".into()))]].into()))));
        assert!(AgentRunner::run(input(dir.path(), "ask"), host.clone()).await.is_err());
    }

    #[tokio::test]
    async fn a_cancelled_run_answers_every_tool_call_it_left_open() {
        let (host, dir) = setup(vec![], true);
        host.tools.register(Arc::new(Canceller));
        let both = vec![
            ChatDelta::ToolCall { index: 0, id: Some("s".into()), name: Some("stopper".into()), arguments: "{}".into() },
            ChatDelta::ToolCall { index: 1, id: Some("l".into()), name: Some("list_dir".into()), arguments: "{}".into() },
            ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
        ];
        *host.replacement.lock().unwrap() = Some(Arc::new(Flaky(Mutex::new(vec![both.into_iter().map(Ok).collect()].into()))));
        let (out, res) = AgentRunner::run_partial(input(dir.path(), "auto"), host.clone()).await;
        assert!(matches!(res, Err(aiw_core::Error::Cancelled)), "{res:?}");
        // The assistant asked for two calls: the first ran (and cancelled), the second never did.
        let asked: Vec<String> = out.new_messages[1].tool_calls.as_ref().unwrap().iter().map(|c| c.id.clone()).collect();
        assert_eq!(asked, vec!["s", "l"]);
        let answers: Vec<(String, String)> =
            out.new_messages.iter().filter(|m| m.role == "tool").map(|m| (m.tool_call_id.clone().unwrap(), m.content.clone())).collect();
        assert_eq!(answers.len(), 2, "{answers:?}");
        assert!(answers.iter().any(|(id, c)| id == "s" && c == "fatto"));
        assert!(answers.iter().any(|(id, c)| id == "l" && c.starts_with("ERRORE") && c.contains("interrotto")), "{answers:?}");
        let evs = host.events.lock().unwrap();
        assert!(matches!(evs.last(), Some(AgEvent::RunError { code: Some(c), .. }) if c == "cancelled"));
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use aiw_runtimes::{ChatDelta, ScriptedBackend};
    use serde_json::json;

    /// A write followed by a read in the same turn must be executed in that order.
    #[tokio::test]
    async fn write_then_read_in_one_turn_keeps_order() {
        let turn = vec![
            ChatDelta::ToolCall {
                index: 0,
                id: Some("w".into()),
                name: Some("write_file".into()),
                arguments: json!({"path": "x.txt", "content": "1"}).to_string(),
            },
            ChatDelta::ToolCall {
                index: 1,
                id: Some("l".into()),
                name: Some("list_dir".into()),
                arguments: json!({"path": "."}).to_string(),
            },
            ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
        ];
        let (host, dir) = tests_support::setup(vec![turn, ScriptedBackend::text_turn("ok")], true);
        let out = AgentRunner::run(tests_support::input(dir.path(), "auto-edit"), host).await.unwrap();
        let listing = out.new_messages.iter().find(|m| m.name.as_deref() == Some("list_dir")).unwrap();
        assert!(listing.content.contains("x.txt"), "{}", listing.content);
    }
}

#[cfg(all(test, unix))]
mod hook_event_tests {
    use super::*;
    use aiw_core::events::AgEvent;
    use aiw_runtimes::{Msg, ScriptedBackend};
    use serde_json::json;
    use std::sync::Arc;

    fn hook(event: &str, command: String) -> HookConfig {
        HookConfig { event: event.into(), matcher: None, command, timeout_s: 10 }
    }

    fn log_hook(event: &str, file: &std::path::Path) -> HookConfig {
        hook(event, format!("echo {event} >> '{}'", file.display()))
    }

    fn logged(file: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(file).unwrap_or_default().lines().map(String::from).collect()
    }

    fn delegate_turns() -> Vec<Vec<aiw_runtimes::ChatDelta>> {
        vec![
            ScriptedBackend::tool_turn("c1", "agent", json!({"agent": "planner", "task": "fai un piano"})),
            ScriptedBackend::text_turn("Fine"),
        ]
    }

    #[tokio::test]
    async fn session_and_subagent_hooks_fire_once() {
        let (host, dir) = tests_support::setup(delegate_turns(), true);
        let log = dir.path().join("events.log");
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.hooks = ["SessionStart", "SubagentStart", "SubagentStop"].iter().map(|e| log_hook(e, &log)).collect();
        inp.hooks.push(hook("SessionStart", "echo 'branch main'".into()));
        AgentRunner::run(inp, host.clone()).await.unwrap();
        assert_eq!(logged(&log), vec!["SessionStart", "SubagentStart", "SubagentStop"]);
        // The stdout of a SessionStart hook reaches the system prompt.
        let reqs = host.backend.requests.lock().unwrap();
        let system = reqs[0].messages[0].text();
        assert!(system.contains("# Contesto di avvio sessione") && system.contains("branch main"), "{system}");
    }

    #[tokio::test]
    async fn session_start_does_not_fire_on_a_continued_conversation() {
        let (host, dir) = tests_support::setup(delegate_turns(), true);
        let log = dir.path().join("events.log");
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.hooks = vec![log_hook("SessionStart", &log), log_hook("SubagentStart", &log)];
        inp.history = vec![Msg::user("ciao"), Msg::assistant("ciao a te")];
        AgentRunner::run(inp, host).await.unwrap();
        assert_eq!(logged(&log), vec!["SubagentStart"]);
    }

    #[tokio::test]
    async fn blocking_subagent_start_keeps_the_subagent_from_running() {
        let (host, dir) = tests_support::setup(delegate_turns(), true);
        let log = dir.path().join("events.log");
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.hooks = vec![hook("SubagentStart", "echo vietato >&2; exit 2".into()), log_hook("SubagentStop", &log)];
        let out = AgentRunner::run(inp, host.clone()).await.unwrap();
        let tool = out.new_messages.iter().find(|m| m.role == "tool").unwrap();
        assert!(tool.content.contains("Negato da un hook") && tool.content.contains("vietato"), "{}", tool.content);
        assert!(logged(&log).is_empty());
        let evs = host.events.lock().unwrap();
        assert!(!evs.iter().any(|e| matches!(e, AgEvent::SubagentStarted { .. })));
    }

    #[tokio::test]
    async fn subagent_start_can_rewrite_the_task_and_stop_adds_context() {
        let (host, dir) = tests_support::setup(delegate_turns(), true);
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.hooks = vec![
            hook("SubagentStart", r#"echo '{"updatedInput":{"task":"nuovo compito"},"additionalContext":"extra"}'"#.into()),
            hook("SubagentStop", r#"echo '{"additionalContext":"controllato"}'"#.into()),
        ];
        let out = AgentRunner::run(inp, host).await.unwrap();
        let tool = out.new_messages.iter().find(|m| m.role == "tool").unwrap();
        assert!(
            tool.content.contains("nuovo compito") && tool.content.contains("[contesto aggiunto da un hook]\nextra"),
            "{}",
            tool.content
        );
        assert!(tool.content.ends_with("[hook] controllato"), "{}", tool.content);
    }

    fn long_history() -> Vec<Msg> {
        (0..8).map(|i| if i % 2 == 0 { Msg::user(format!("domanda {i}")) } else { Msg::assistant(format!("risposta {i}")) }).collect()
    }

    #[tokio::test]
    async fn pre_compact_runs_and_can_block_the_compaction() {
        // Without a blocking hook the history is summarised (one extra model call).
        let (host, dir) = tests_support::setup(vec![ScriptedBackend::text_turn("riassunto"), ScriptedBackend::text_turn("ok")], true);
        let log = dir.path().join("events.log");
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.history = long_history();
        inp.compact_at = 0.0001;
        inp.hooks = vec![log_hook("PreCompact", &log)];
        AgentRunner::run(inp, host.clone()).await.unwrap();
        assert_eq!(logged(&log), vec!["PreCompact"]);
        let has = |h: &Arc<tests_support::TestHost>| {
            h.events.lock().unwrap().iter().any(|e| matches!(e, AgEvent::Custom { name, .. } if name == "aiw.compacted"))
        };
        assert!(has(&host));

        // A blocking hook skips it.
        let (host, dir) = tests_support::setup(vec![ScriptedBackend::text_turn("ok")], true);
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.history = long_history();
        inp.compact_at = 0.0001;
        inp.hooks = vec![hook("PreCompact", "exit 2".into())];
        let out = AgentRunner::run(inp, host.clone()).await.unwrap();
        assert_eq!(out.final_text, "ok");
        assert!(!has(&host));
    }
}

#[cfg(test)]
mod parallel_subagent_tests {
    use super::*;
    use aiw_runtimes::{ChatDelta, ScriptedBackend};
    use serde_json::json;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    fn two_agents() -> Vec<ChatDelta> {
        let call = |index: usize, id: &str, task: &str| ChatDelta::ToolCall {
            index,
            id: Some(id.into()),
            name: Some("agent".into()),
            arguments: json!({"agent": "planner", "task": task}).to_string(),
        };
        vec![call(0, "a", "primo"), call(1, "b", "secondo"), ChatDelta::Done { finish_reason: Some("tool_calls".into()) }]
    }

    const DELAY_MS: u64 = 400;

    async fn run_with_limit(limit: usize) -> (Duration, Vec<String>) {
        let (host, dir) = tests_support::setup(vec![two_agents(), ScriptedBackend::text_turn("Fatto")], true);
        host.subagent_delay_ms.store(DELAY_MS, Ordering::Relaxed);
        let mut inp = tests_support::input(dir.path(), "auto");
        inp.max_parallel_subagents = limit;
        let start = Instant::now();
        let out = AgentRunner::run(inp, host).await.unwrap();
        let tools = out.new_messages.iter().filter(|m| m.role == "tool").map(|m| m.content.clone()).collect();
        (start.elapsed(), tools)
    }

    #[tokio::test]
    async fn consecutive_agent_calls_run_in_parallel_and_keep_their_order() {
        let (elapsed, results) = run_with_limit(3).await;
        assert!(elapsed < Duration::from_millis(DELAY_MS * 8 / 5), "{elapsed:?}");
        assert_eq!(results, vec!["planner ha svolto: primo", "planner ha svolto: secondo"]);
    }

    #[tokio::test]
    async fn the_limit_serialises_subagents() {
        let (elapsed, results) = run_with_limit(1).await;
        assert!(elapsed >= Duration::from_millis(DELAY_MS * 2), "{elapsed:?}");
        assert_eq!(results, vec!["planner ha svolto: primo", "planner ha svolto: secondo"]);
    }
}
