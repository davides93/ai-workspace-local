//! Agents kept apart (issue #71): what the acceptance of the issue asks, shown with the engine and not with words. An agent with
//! a folder of its own works there and nowhere else; no other agent reads or writes it, by any path; it survives a restart; it
//! is revoked when the agent is removed; the boundary holds for sub-agents, team members, handoffs and background runs.

use crate::host::EngineHost;
use crate::{Engine, EngineOptions};
use aiw_agent::{RunHost, RunInput};
use aiw_core::api::*;
use aiw_core::Error;
use aiw_runtimes::ScriptedBackend;
use aiw_tools::ToolOutput;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

// Recognisable fakes: nothing here is real.
pub(crate) const ALFA_FILE: &str = "CONTENUTO-RISERVATO-DI-ALFA";
pub(crate) const BETA_FILE: &str = "CONTENUTO-RISERVATO-DI-BETA";
const SHARED_FILE: &str = "CONTENUTO-DEL-WORKSPACE-CONDIVISO";

/// Data folders of the engines whose runs may reach a stand-in server on loopback (`build_input_at` reads it in tests only).
static PRIVATE_NETWORK: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn allow_private_network_for(data: &Path) {
    PRIVATE_NETWORK.lock().unwrap().push(data.to_path_buf());
}

pub(crate) fn private_network_allowed(data: &Path) -> bool {
    PRIVATE_NETWORK.lock().unwrap().iter().any(|p| p == data)
}

pub(crate) async fn engine_at(data: &Path, ws: &Path) -> Engine {
    engine_with(data, ws, crate::vault::Keychain::None).await
}

pub(crate) async fn engine_with(data: &Path, ws: &Path, keychain: crate::vault::Keychain) -> Engine {
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        keychain,
        ..Default::default()
    })
    .await
    .unwrap();
    profile_default(&e);
    e
}

pub(crate) fn profile_default(e: &Engine) {
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(vec![])));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
}

/// An agent in `auto` mode with a sandbox, that works in a folder of its own.
pub(crate) fn apart(id: &str, tools: &[&str]) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        name: id.into(),
        inference_profile: "default".into(),
        tools: tools.iter().map(|s| s.to_string()).collect(),
        mode: "auto".into(),
        sandbox: "workspace-write".into(),
        max_turns: 8,
        isolation: Some(AgentIsolation { dedicated_workspace: true, close_shell_network: false }),
        ..crate::defaults::agents().into_iter().find(|a| a.id == "planner").unwrap()
    }
}

/// The same agent in the shared workspace.
pub(crate) fn shared(id: &str, tools: &[&str]) -> AgentProfile {
    AgentProfile { isolation: None, ..apart(id, tools) }
}

/// The input of a run of `agent_id` in a new session.
pub(crate) fn input_of(e: &Engine, agent_id: &str) -> RunInput {
    let s = e.create_session(agent_id, None).unwrap();
    e.build_input_at(
        &aiw_core::ids::new_id("run"),
        &s.id,
        e.agent(agent_id).unwrap(),
        e.profile("default").unwrap(),
        vec![],
        String::new(),
        CancellationToken::new(),
        0,
        Default::default(),
        None,
    )
}

pub(crate) async fn call(e: &Engine, input: &RunInput, tool: &str, args: Value) -> aiw_core::Result<ToolOutput> {
    e.tool_registry().get(tool).unwrap_or_else(|| panic!("tool {tool}")).call(args, &input.tool_ctx).await
}

pub(crate) fn denied(r: aiw_core::Result<ToolOutput>, what: &str) {
    match r {
        Err(Error::Denied(_)) => {}
        other => panic!("{what}: expected a refusal, got {other:?}"),
    }
}

fn folder(data: &Path, id: &str) -> PathBuf {
    data.join("agents").join(id).join("workspace")
}

const FILE_TOOLS: [&str; 5] = ["read_file", "write_file", "list_dir", "grep", "glob"];

#[tokio::test]
async fn an_agent_with_a_folder_of_its_own_works_there_and_no_other_agent_reaches_it() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    std::fs::write(ws.path().join("condiviso.txt"), SHARED_FILE).unwrap();
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    e.save_agent(apart("beta", &FILE_TOOLS)).unwrap();
    e.save_agent(shared("comune", &FILE_TOOLS)).unwrap();
    let (alfa, beta, comune) = (input_of(&e, "alfa"), input_of(&e, "beta"), input_of(&e, "comune"));
    let (fa, fb) = (folder(data.path(), "alfa"), folder(data.path(), "beta"));
    assert_eq!((&alfa.tool_ctx.workspace_root, &beta.tool_ctx.workspace_root), (&fa, &fb));
    assert_eq!(comune.tool_ctx.workspace_root, e.workspace_root(), "an agent that is not apart keeps working where it always did");

    // Each writes in its own folder: not in the shared workspace, not in the other's.
    call(&e, &alfa, "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    call(&e, &beta, "write_file", json!({"path": "nota.txt", "content": BETA_FILE})).await.unwrap();
    assert_eq!(std::fs::read_to_string(fa.join("nota.txt")).unwrap(), ALFA_FILE);
    assert_eq!(std::fs::read_to_string(fb.join("nota.txt")).unwrap(), BETA_FILE);
    assert!(!ws.path().join("nota.txt").exists());
    // ...and reads it back.
    assert!(call(&e, &alfa, "read_file", json!({"path": "nota.txt"})).await.unwrap().content.contains(ALFA_FILE));

    // beta cannot reach the folder of alfa, whatever the way to name it.
    let alfa_file = fa.join("nota.txt").display().to_string();
    for path in [
        alfa_file.as_str(),
        "../../alfa/workspace/nota.txt",
        "../../alfa/workspace/../workspace/nota.txt",
        "~/../../alfa/workspace/nota.txt",
    ] {
        denied(call(&e, &beta, "read_file", json!({"path": path})).await, path);
    }
    denied(call(&e, &beta, "write_file", json!({"path": alfa_file, "content": "rotto"})).await, "write into the folder of alfa");
    for dir in [fa.parent().unwrap(), fa.as_path(), data.path().join("agents").as_path(), data.path()] {
        let d = dir.display().to_string();
        denied(call(&e, &beta, "list_dir", json!({"path": d})).await, &d);
        denied(call(&e, &beta, "grep", json!({"pattern": "RISERVATO", "path": d})).await, &d);
        denied(call(&e, &beta, "glob", json!({"pattern": "**/*.txt", "path": d})).await, &d);
    }
    assert_eq!(std::fs::read_to_string(fa.join("nota.txt")).unwrap(), ALFA_FILE, "nothing was changed");

    // An agent apart does not see the shared workspace either, and the shared agent does not see the folders apart.
    let shared_file = ws.path().join("condiviso.txt").display().to_string();
    denied(call(&e, &alfa, "read_file", json!({"path": shared_file})).await, "shared workspace from alfa");
    denied(call(&e, &alfa, "read_file", json!({"path": "../../../../condiviso.txt"})).await, "shared workspace by a relative path");
    assert!(call(&e, &comune, "read_file", json!({"path": "condiviso.txt"})).await.unwrap().content.contains(SHARED_FILE));
    denied(call(&e, &comune, "read_file", json!({"path": alfa_file})).await, "alfa from the shared agent");
    denied(call(&e, &comune, "write_file", json!({"path": alfa_file, "content": "rotto"})).await, "write from the shared agent");
    denied(call(&e, &comune, "list_dir", json!({"path": data.path().join("agents").display().to_string()})).await, "agents folder");

    // A link inside the folder that points into another folder does not open it.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fb.join("nota.txt"), fa.join("scorciatoia.txt")).unwrap();
        denied(call(&e, &alfa, "read_file", json!({"path": "scorciatoia.txt"})).await, "symlink into the folder of beta");
        std::os::unix::fs::symlink(ws.path().join("condiviso.txt"), fa.join("verso-condiviso.txt")).unwrap();
        denied(call(&e, &alfa, "read_file", json!({"path": "verso-condiviso.txt"})).await, "symlink into the shared workspace");
    }
}

#[tokio::test]
async fn the_shell_of_an_agent_apart_runs_in_its_own_folder_and_cannot_read_another() {
    // A real sandboxed command: only where this machine can contain one (bubblewrap, Seatbelt). On Windows the shell cannot hide
    // folders from reading (`docs/agenti-isolati.md`); there the file tools are what keeps the agents apart.
    if !aiw_tools::sandbox::can_wrap_process() {
        eprintln!("skip: no sandbox that can contain a command on this machine");
        return;
    }
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    std::fs::write(ws.path().join("condiviso.txt"), SHARED_FILE).unwrap();
    e.save_agent(apart("alfa", &["shell", "write_file"])).unwrap();
    e.save_agent(apart("beta", &["shell", "write_file"])).unwrap();
    e.save_agent(shared("comune", &["shell"])).unwrap();
    let (alfa, beta, comune) = (input_of(&e, "alfa"), input_of(&e, "beta"), input_of(&e, "comune"));
    call(&e, &alfa, "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    call(&e, &beta, "write_file", json!({"path": "nota.txt", "content": BETA_FILE})).await.unwrap();
    let shell = |ctx: aiw_tools::ToolContext, command: String| {
        let e = e.clone();
        async move { e.tool_registry().get("shell").unwrap().call(json!({"command": command, "timeout_s": 20}), &ctx).await.unwrap() }
    };
    // Its own folder works: the command starts there and reads and writes there.
    let own = shell(alfa.tool_ctx.clone(), "cat nota.txt && echo scritto > prodotto.txt".into()).await;
    if own.is_error && !own.content.contains(ALFA_FILE) {
        // A machine that is already inside a sandbox cannot start a second one: nothing to prove here.
        eprintln!("skip: the sandbox cannot start on this machine: {}", own.content);
        return;
    }
    assert!(own.content.contains(ALFA_FILE), "{}", own.content);
    assert!(folder(data.path(), "alfa").join("prodotto.txt").is_file());
    // The folder of another agent, the data folder and the shared workspace are closed to it.
    let beta_file = folder(data.path(), "beta").join("nota.txt");
    for (who, input) in [("alfa", alfa.tool_ctx.clone()), ("comune", comune.tool_ctx.clone())] {
        let out = shell(input, format!("cat '{}'", beta_file.display())).await;
        assert!(out.is_error && !out.content.contains(BETA_FILE), "{who} read the file of beta: {}", out.content);
    }
    let out = shell(
        alfa.tool_ctx.clone(),
        format!("ls '{}'; cat '{}'", data.path().join("agents").display(), ws.path().join("condiviso.txt").display()),
    )
    .await;
    assert!(!out.content.contains("beta") && !out.content.contains(SHARED_FILE), "{}", out.content);
    // Nor can it write there.
    let out = shell(alfa.tool_ctx.clone(), format!("echo rotto > '{}'", beta_file.display())).await;
    assert!(out.is_error, "{}", out.content);
    assert_eq!(std::fs::read_to_string(&beta_file).unwrap(), BETA_FILE);
    // The shared agent still works in the shared workspace.
    let out = shell(comune.tool_ctx.clone(), "cat condiviso.txt".into()).await;
    assert!(out.content.contains(SHARED_FILE), "{}", out.content);
}

#[tokio::test]
async fn an_agent_with_the_sandbox_off_is_not_kept_out_and_so_it_cannot_be_apart() {
    // The honest limit: `off` means no sandbox, and nothing is kept apart without one. It is documented, it is tested, and an
    // agent that asks for a folder of its own with the sandbox off is refused when it is saved.
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    call(&e, &input_of(&e, "alfa"), "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    let mut libero = shared("libero", &FILE_TOOLS);
    libero.sandbox = "off".into();
    e.save_agent(libero).unwrap();
    let path = folder(data.path(), "alfa").join("nota.txt").display().to_string();
    let out = call(&e, &input_of(&e, "libero"), "read_file", json!({"path": path})).await.unwrap();
    assert!(out.content.contains(ALFA_FILE), "an agent with the sandbox off reads whatever the account can");
    let mut both = apart("entrambi", &FILE_TOOLS);
    both.sandbox = "off".into();
    let err = e.save_agent(both).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("sandbox"), "{err}");
}

#[tokio::test]
async fn save_agent_accepts_a_folder_only_where_it_holds_and_never_loses_it_by_accident() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    // Ids that cannot be a folder.
    for bad in ["a/b", "..", "con spazio", "è", ".nascosto"] {
        let err = e.save_agent(AgentProfile { id: bad.into(), ..apart("x", &["todo"]) }).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{bad}: {err}");
    }
    // Two ids that differ only by case would share a folder on macOS and Windows.
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    let err = e.save_agent(apart("ALFA", &["todo"])).unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    // A save that does not mention the isolation (an editor that does not know the field) keeps it.
    let without = AgentProfile { isolation: None, description: "ritoccato".into(), ..e.agent("alfa").unwrap() };
    let saved = e.save_agent(without).unwrap();
    assert_eq!(saved.description, "ritoccato");
    assert!(saved.has_dedicated_workspace() && e.agent("alfa").unwrap().has_dedicated_workspace());
    // Only the owner, saying it, switches it off: both flags false. It is then stored as absent and stays absent.
    let off = AgentProfile { isolation: Some(AgentIsolation::default()), ..e.agent("alfa").unwrap() };
    assert!(e.save_agent(off).unwrap().isolation.is_none());
    assert!(!e.agent("alfa").unwrap().has_dedicated_workspace());
    let later = e.save_agent(AgentProfile { isolation: None, ..e.agent("alfa").unwrap() }).unwrap();
    assert!(later.isolation.is_none());
    // The network of the shell can be closed for one agent and is stored.
    let closed = AgentProfile { isolation: Some(AgentIsolation { dedicated_workspace: false, close_shell_network: true }), ..later };
    assert!(e.save_agent(closed).unwrap().isolation.unwrap().close_shell_network);
    // A session in a git worktree is not offered to an agent apart.
    e.save_agent(apart("gamma", &["todo"])).unwrap();
    let err = e.create_session_with("gamma", None, Some("worktree")).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("worktree"), "{err}");
}

#[tokio::test]
async fn the_network_of_the_shell_can_be_closed_for_one_agent_and_never_opened() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.set_config("global", "sandbox.shell_network", json!("on")).unwrap();
    e.save_agent(AgentProfile {
        isolation: Some(AgentIsolation { dedicated_workspace: false, close_shell_network: true }),
        ..shared("chiuso", &["shell"])
    })
    .unwrap();
    e.save_agent(shared("aperto", &["shell"])).unwrap();
    assert!(input_of(&e, "aperto").tool_ctx.shell.network, "the setting of the machine is what it was");
    assert!(!input_of(&e, "chiuso").tool_ctx.shell.network, "this agent closes it for itself");
    e.set_config("global", "sandbox.shell_network", json!("off")).unwrap();
    assert!(!input_of(&e, "aperto").tool_ctx.shell.network, "an agent cannot open what the setting closed");
    assert!(!input_of(&e, "chiuso").tool_ctx.shell.network);
}

#[tokio::test]
async fn the_instructions_and_the_skills_of_the_shared_workspace_do_not_reach_an_agent_apart() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    std::fs::write(ws.path().join("AGENTS.md"), "ISTRUZIONE-DEL-WORKSPACE-CONDIVISO").unwrap();
    let skill_dir = ws.path().join(".aiw/skills/condivisa");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "---\nname: condivisa\ndescription: solo del workspace condiviso\n---\ncorpo").unwrap();
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    e.save_agent(shared("comune", &["todo"])).unwrap();
    let (alfa, comune) = (input_of(&e, "alfa"), input_of(&e, "comune"));
    let prompt = |i: &RunInput| i.system_extra.join("\n");
    assert!(prompt(&comune).contains("ISTRUZIONE-DEL-WORKSPACE-CONDIVISO"));
    assert!(!prompt(&alfa).contains("ISTRUZIONE-DEL-WORKSPACE-CONDIVISO"), "{}", prompt(&alfa));
    // Its own AGENTS.md and its own skills do reach it.
    let own = folder(data.path(), "alfa");
    std::fs::write(own.join("AGENTS.md"), "ISTRUZIONE-DI-ALFA").unwrap();
    let own_skill = own.join(".aiw/skills/mia");
    std::fs::create_dir_all(&own_skill).unwrap();
    std::fs::write(own_skill.join("SKILL.md"), "---\nname: mia\ndescription: solo di alfa\n---\ncorpo").unwrap();
    let alfa = input_of(&e, "alfa");
    assert!(prompt(&alfa).contains("ISTRUZIONE-DI-ALFA"));
    let host = EngineHost::new(e.clone(), None, None);
    let names = |i: &RunInput| host.skills_for(i).into_iter().map(|s| s.name).collect::<Vec<_>>();
    assert_eq!(names(&alfa), ["mia"]);
    assert_eq!(names(&comune), ["condivisa"]);
}

/// A bridge that grants one folder, as the macOS panel would.
struct OneFolder(PathBuf);

impl aiw_core::folders::FolderBridge for OneFolder {
    fn pick(&self, _m: &str, _p: &str) -> aiw_core::Result<Vec<aiw_core::folders::PickedFolder>> {
        Ok(vec![aiw_core::folders::PickedFolder { bookmark: format!("bm1:{}", self.0.display()), path: self.0.clone() }])
    }
    fn open(&self, bookmark: &str) -> aiw_core::Result<aiw_core::folders::OpenedFolder> {
        let path = PathBuf::from(bookmark.split_once(':').map(|(_, p)| p).unwrap_or(""));
        Ok(aiw_core::folders::OpenedFolder { path, renewed: None })
    }
    fn release(&self, _path: &Path) {}
}

#[tokio::test]
async fn the_folders_the_person_granted_are_for_the_shared_agents_only() {
    let (data, ws, granted) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(granted.path().join("fattura.txt"), "totale 42").unwrap();
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(Arc::new(OneFolder(granted.path().to_path_buf())));
    e.add_folders("m", "p").unwrap();
    assert_eq!(e.folders().folders.len(), 1);
    e.save_agent(apart("alfa", &["read_file"])).unwrap();
    e.save_agent(shared("comune", &["read_file"])).unwrap();
    let file = std::fs::canonicalize(granted.path()).unwrap().join("fattura.txt").display().to_string();
    let (alfa, comune) = (input_of(&e, "alfa"), input_of(&e, "comune"));
    assert!(call(&e, &comune, "read_file", json!({"path": file})).await.unwrap().content.contains("totale 42"));
    assert!(comune.system_extra[0].contains("Cartelle del computer"));
    // The agent apart neither reads the folder nor is told about it.
    assert!(alfa.tool_ctx.write_roots.is_empty() && alfa.tool_ctx.outside_hint.is_none());
    denied(call(&e, &alfa, "read_file", json!({"path": file})).await, "granted folder from an agent apart");
    assert!(!alfa.system_extra[0].contains("Cartelle del computer"), "{}", alfa.system_extra[0]);
}

#[tokio::test]
async fn a_run_apart_goes_through_the_whole_loop_and_leaves_its_file_in_its_own_folder() {
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "risultato.txt", "content": ALFA_FILE})),
        ScriptedBackend::text_turn("Fatto."),
    ]);
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend));
    e.save_agent(apart("alfa", &["write_file"])).unwrap();
    let s = e.create_session("alfa", None).unwrap();
    let run = e.start_run(&s.id, "scrivi".into()).await.unwrap();
    crate::tests::wait_finished(&e, &run).await;
    assert_eq!(std::fs::read_to_string(folder(data.path(), "alfa").join("risultato.txt")).unwrap(), ALFA_FILE);
    assert!(!ws.path().join("risultato.txt").exists());
    assert_eq!(e.sessions().unwrap().into_iter().find(|x| x.id == s.id).unwrap().last_status.as_deref(), Some("succeeded"));
}

struct Delegation {
    e: Engine,
    data: tempfile::TempDir,
    ws: tempfile::TempDir,
}

/// `parent` calls the `agent` tool for `child`, which writes `figlio.txt`.
async fn delegate(parent: AgentProfile, child: AgentProfile) -> Delegation {
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("d1", "agent", json!({"agent": child.id, "task": "scrivi il file"})),
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "figlio.txt", "content": "dal figlio"})),
        ScriptedBackend::text_turn("fatto dal figlio"),
        ScriptedBackend::text_turn("tutto a posto"),
    ]);
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend));
    e.save_agent(child).unwrap();
    e.save_agent(parent.clone()).unwrap();
    let s = e.create_session(&parent.id, None).unwrap();
    let run = e.start_run(&s.id, "delega".into()).await.unwrap();
    crate::tests::wait_finished(&e, &run).await;
    Delegation { e, data, ws }
}

#[tokio::test]
async fn a_sub_agent_works_in_the_folder_of_its_parent_when_either_one_is_apart() {
    // Parent apart, child in the shared workspace: the child writes in the folder of the parent, not in the shared workspace.
    let d = delegate(apart("capo", &["agent", "write_file"]), shared("figlio", &["write_file"])).await;
    assert_eq!(std::fs::read_to_string(folder(d.data.path(), "capo").join("figlio.txt")).unwrap(), "dal figlio");
    assert!(!d.ws.path().join("figlio.txt").exists(), "an agent apart is not a way to write in the shared workspace");
    // Parent in the shared workspace, child apart: the child works where the parent does. The parent never reads what the child
    // keeps apart, and the child's own folder stays empty.
    let d = delegate(shared("capo", &["agent", "write_file"]), apart("figlio", &["write_file"])).await;
    assert_eq!(std::fs::read_to_string(d.ws.path().join("figlio.txt")).unwrap(), "dal figlio");
    assert!(!folder(d.data.path(), "figlio").join("figlio.txt").exists());
    // Both apart: the child works in the folder of the parent, never in its own.
    let d = delegate(apart("capo", &["agent", "write_file"]), apart("figlio", &["write_file"])).await;
    assert_eq!(std::fs::read_to_string(folder(d.data.path(), "capo").join("figlio.txt")).unwrap(), "dal figlio");
    assert!(!folder(d.data.path(), "figlio").join("figlio.txt").exists());
    assert!(!d.ws.path().join("figlio.txt").exists());
    // Neither apart: nothing changed, the shared workspace as always.
    let d = delegate(shared("capo", &["agent", "write_file"]), shared("figlio", &["write_file"])).await;
    assert_eq!(std::fs::read_to_string(d.ws.path().join("figlio.txt")).unwrap(), "dal figlio");
    assert!(d.e.agents().unwrap().iter().any(|a| a.id == "figlio"));
}

#[tokio::test]
async fn a_conversation_does_not_pass_to_or_from_an_agent_apart() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    e.save_agent(shared("comune", &["todo"])).unwrap();
    e.save_agent(shared("altro", &["todo"])).unwrap();
    let host = EngineHost::new(e.clone(), None, None);
    let (from_apart, from_shared) = (input_of(&e, "alfa"), input_of(&e, "comune"));
    let to_apart = host.handoff_target(&from_shared, "alfa").unwrap_err();
    assert!(matches!(to_apart, Error::Denied(_)) && to_apart.to_string().contains("isolato"), "{to_apart}");
    let from_apart_err = host.handoff_target(&from_apart, "comune").unwrap_err();
    assert!(matches!(from_apart_err, Error::Denied(_)), "{from_apart_err}");
    assert_eq!(host.handoff_target(&from_shared, "altro").unwrap(), "altro", "between agents that are not apart nothing changed");
}

#[tokio::test]
async fn a_background_or_channel_run_of_an_agent_apart_uses_the_folder_of_that_agent() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &["read_file"])).unwrap();
    for derived in [e.proactive_agent("alfa").unwrap(), e.channel_agent(crate::channels::RunKind::Ask, "alfa").unwrap()] {
        let id = derived.id.clone();
        assert_ne!(id, "alfa");
        let s = e.create_session(&id, None).unwrap();
        let input = e.build_input_at(
            "r",
            &s.id,
            derived,
            e.profile("default").unwrap(),
            vec![],
            String::new(),
            CancellationToken::new(),
            0,
            Default::default(),
            None,
        );
        assert_eq!(input.tool_ctx.workspace_root, folder(data.path(), "alfa"), "{id}");
    }
    // A session in a git worktree is refused for the derived id too.
    let err = e.create_session_with("proactive:alfa", None, Some("worktree")).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
}

#[tokio::test]
async fn removing_an_agent_moves_its_folder_aside_so_that_an_agent_with_the_same_id_inherits_nothing() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    call(&e, &input_of(&e, "alfa"), "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    e.delete_agent("alfa").unwrap();
    assert!(e.agent("alfa").is_err());
    assert!(!data.path().join("agents/alfa").exists(), "the folder is out of the way");
    let removed: Vec<PathBuf> = std::fs::read_dir(data.path().join("agents/.removed")).unwrap().flatten().map(|f| f.path()).collect();
    assert_eq!(removed.len(), 1, "{removed:?}");
    assert_eq!(std::fs::read_to_string(removed[0].join("workspace/nota.txt")).unwrap(), ALFA_FILE, "nothing was deleted");
    assert!(e.audit().iter().any(|a| a.action == "agent.workspace_archived"));
    // An agent made later with the same id starts from an empty folder, and no agent reaches the archived one.
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    let nuovo = input_of(&e, "alfa");
    denied(
        call(&e, &nuovo, "read_file", json!({"path": removed[0].join("workspace/nota.txt").display().to_string()})).await,
        "archived folder",
    );
    assert!(
        !call(&e, &nuovo, "list_dir", json!({})).await.unwrap().content.contains("nota.txt"),
        "the new agent starts from an empty folder"
    );
    e.save_agent(shared("comune", &FILE_TOOLS)).unwrap();
    denied(
        call(&e, &input_of(&e, "comune"), "read_file", json!({"path": removed[0].join("workspace/nota.txt").display().to_string()})).await,
        "archived folder from the shared agent",
    );
}

#[tokio::test]
async fn the_folder_and_the_isolation_are_still_there_after_a_restart() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    {
        let e = engine_at(data.path(), ws.path()).await;
        e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
        e.save_agent(apart("beta", &FILE_TOOLS)).unwrap();
        call(&e, &input_of(&e, "alfa"), "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
        call(&e, &input_of(&e, "beta"), "write_file", json!({"path": "nota.txt", "content": BETA_FILE})).await.unwrap();
    }
    let e = engine_at(data.path(), ws.path()).await;
    // Role, permissions and isolation of the agent come back as saved.
    let alfa = e.agent("alfa").unwrap();
    assert!(alfa.has_dedicated_workspace() && alfa.mode == "auto" && alfa.sandbox == "workspace-write" && alfa.tools == FILE_TOOLS);
    // Its files are there, and it still cannot read the ones of the other.
    let input = input_of(&e, "alfa");
    assert!(call(&e, &input, "read_file", json!({"path": "nota.txt"})).await.unwrap().content.contains(ALFA_FILE));
    let beta_file = folder(data.path(), "beta").join("nota.txt").display().to_string();
    denied(call(&e, &input, "read_file", json!({"path": beta_file})).await, "beta after a restart");
}
