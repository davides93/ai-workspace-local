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

/// The origin of a run the owner started from the chat.
pub(crate) const OWNER: crate::RunOrigin = crate::RunOrigin { owner: true, questions: true, secrets: true };

/// Registers a run as the server does for a chat message, with `origin`, and returns its id.
pub(crate) fn register_run(e: &Engine, origin: Option<crate::RunOrigin>) -> String {
    let run_id = aiw_core::ids::new_id("run");
    e.0.runs.lock().unwrap().insert(
        run_id.clone(),
        crate::RunHandle { session_id: None, cancel: CancellationToken::new(), events: vec![], done: false, origin },
    );
    run_id
}

/// The input of a run of `agent_id` in a new session, a run the owner started from the chat.
pub(crate) fn input_of(e: &Engine, agent_id: &str) -> RunInput {
    input_of_run(e, agent_id, Some(OWNER))
}

/// The same with the origin you choose (`None`: a run nobody started through the server, like a workflow or an A2A client).
pub(crate) fn input_of_run(e: &Engine, agent_id: &str, origin: Option<crate::RunOrigin>) -> RunInput {
    let s = e.create_session(agent_id, None).unwrap();
    let run_id = register_run(e, origin);
    e.build_input_at(
        &run_id,
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
    // A control first, in the shared workspace: if even that cannot run, this machine cannot start a sandbox (one that is already inside
    // a sandbox cannot start a second one) and there is nothing to prove. From here on a failure is a failure, not a skip.
    let control = shell(comune.tool_ctx.clone(), "cat condiviso.txt".into()).await;
    if control.is_error || !control.content.contains(SHARED_FILE) {
        eprintln!("skip: the sandbox cannot start on this machine: {}", control.content);
        return;
    }
    // Its own folder works: the command starts there and reads and writes there.
    let own = shell(alfa.tool_ctx.clone(), "cat nota.txt && echo scritto > prodotto.txt".into()).await;
    assert!(
        !own.is_error && own.content.contains(ALFA_FILE),
        "the exception for a workspace inside a hidden folder is broken: {}",
        own.content
    );
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
    // A hard link made from its own shell: the sandbox may or may not stop `ln` (Seatbelt denies reads and writes on the hidden path, bubblewrap
    // puts a different file system over it), and the engine reads `AGENTS.md` itself, outside the sandbox. Whatever `ln` does, nothing of
    // another agent, and not the token of the owner, may get in the prompt of the next run.
    let (token_path, beta_note) = (data.path().join("token"), folder(data.path(), "beta").join("nota.txt"));
    for target in [&beta_note, &token_path] {
        let _ = std::fs::remove_file(folder(data.path(), "alfa").join("AGENTS.md"));
        let out = shell(alfa.tool_ctx.clone(), format!("ln '{}' AGENTS.md", target.display())).await;
        eprintln!("ln {} from the shell of alfa: {}", target.display(), if out.is_error { "refused" } else { "worked" });
        let prompt = input_of(&e, "alfa").system_extra.join("\n");
        assert!(!prompt.contains(BETA_FILE) && !prompt.contains(e.token()), "a hard link carried a file in the prompt of alfa");
    }
    let _ = std::fs::remove_file(folder(data.path(), "alfa").join("AGENTS.md"));
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
    let global = data.path().join("skills/globale");
    std::fs::create_dir_all(&global).unwrap();
    std::fs::write(global.join("SKILL.md"), "---\nname: globale\ndescription: per tutti\n---\ncorpo").unwrap();
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    e.save_agent(shared("comune", &["todo"])).unwrap();
    let (alfa, comune) = (input_of(&e, "alfa"), input_of(&e, "comune"));
    let prompt = |i: &RunInput| i.system_extra.join("\n");
    assert!(prompt(&comune).contains("ISTRUZIONE-DEL-WORKSPACE-CONDIVISO"));
    assert!(!prompt(&alfa).contains("ISTRUZIONE-DEL-WORKSPACE-CONDIVISO"), "{}", prompt(&alfa));
    // Its own AGENTS.md reaches it (a regular file in its own folder).
    let own = folder(data.path(), "alfa");
    std::fs::write(own.join("AGENTS.md"), "ISTRUZIONE-DI-ALFA").unwrap();
    let own_skill = own.join(".aiw/skills/mia");
    std::fs::create_dir_all(&own_skill).unwrap();
    std::fs::write(own_skill.join("SKILL.md"), "---\nname: mia\ndescription: solo di alfa\n---\ncorpo").unwrap();
    let alfa = input_of(&e, "alfa");
    assert!(prompt(&alfa).contains("ISTRUZIONE-DI-ALFA"));
    let host = EngineHost::new(e.clone(), None, None);
    let names = |i: &RunInput| host.skills_for(i).into_iter().map(|s| s.name).collect::<Vec<_>>();
    // Skills: the global ones only. Not the shared workspace's, and not the ones in its own folder either: the engine reads a skill
    // outside the sandbox when it is loaded, and a folder the agent writes in could hold a link to somebody else's file.
    assert_eq!(names(&alfa), ["globale"]);
    assert_eq!(names(&comune), ["condivisa", "globale"]);
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

#[cfg(unix)]
#[tokio::test]
async fn a_link_in_the_folder_of_an_agent_apart_does_not_carry_another_file_into_its_prompt() {
    // The engine reads `AGENTS.md` itself, outside the sandbox, and puts it in the prompt: a link that the agent makes in its own
    // folder (its shell can) must not bring in the file of another agent, the token of the owner or anything else.
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    e.save_agent(apart("beta", &FILE_TOOLS)).unwrap();
    call(&e, &input_of(&e, "beta"), "write_file", json!({"path": "nota.txt", "content": BETA_FILE})).await.unwrap();
    let (fa, fb) = (folder(data.path(), "alfa"), folder(data.path(), "beta"));
    let prompt = |agent: &str| input_of(&e, agent).system_extra.join("\n");
    std::fs::create_dir_all(&fa).unwrap();
    let agents_md = fa.join("AGENTS.md");
    for target in [fb.join("nota.txt"), data.path().join("token"), ws.path().to_path_buf()] {
        let _ = std::fs::remove_file(&agents_md);
        std::os::unix::fs::symlink(&target, &agents_md).unwrap();
        let p = prompt("alfa");
        assert!(!p.contains(BETA_FILE) && !p.contains(e.token()), "a link to {} carried something in: {p}", target.display());
    }
    // A hard link to a file of its own folder is the file itself: it is read. A folder called AGENTS.md is not a file.
    let _ = std::fs::remove_file(&agents_md);
    std::fs::create_dir(&agents_md).unwrap();
    assert!(!prompt("alfa").contains("# Istruzioni del workspace"));
    std::fs::remove_dir(&agents_md).unwrap();
    std::fs::write(&agents_md, "ISTRUZIONE-DI-ALFA").unwrap();
    assert!(prompt("alfa").contains("ISTRUZIONE-DI-ALFA"));
    // The skills of an agent apart are the global ones: a link in its own skills folder leads nowhere.
    let skills = fa.join(".aiw/skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::os::unix::fs::symlink(fb.parent().unwrap(), skills.join("verso-beta")).unwrap();
    let host = EngineHost::new(e.clone(), None, None);
    assert!(host.skills_for(&input_of(&e, "alfa")).is_empty());
}

#[tokio::test]
async fn removing_an_agent_stops_what_runs_for_it_and_nothing_recreates_its_folder_or_leaves_anything_to_its_namesake() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    let running = input_of(&e, "alfa"); // a run that is going on
    call(&e, &running, "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    e.delete_agent("alfa").unwrap();
    // The tools of the run that was going cannot work any more, and the folder is not created again by a write.
    for tool_args in
        [("write_file", json!({"path": "nuovo.txt", "content": "x"})), ("list_dir", json!({})), ("read_file", json!({"path": "nota.txt"}))]
    {
        let r = call(&e, &running, tool_args.0, tool_args.1).await;
        assert!(matches!(&r, Err(Error::Denied(m)) if m.contains("non esiste più")), "{}: {r:?}", tool_args.0);
    }
    assert!(!data.path().join("agents/alfa").exists(), "the folder was created again");
    // An agent made later with the same id starts from nothing, even if a credential or a folder was left behind by a race.
    std::fs::create_dir_all(folder(data.path(), "alfa")).unwrap();
    std::fs::write(folder(data.path(), "alfa").join("rimasto.txt"), ALFA_FILE).unwrap();
    e.0.store
        .put_doc(
            "agent_credential",
            "alfa:orfana",
            &json!({"agent": "alfa", "name": "orfana", "hosts": ["example.org"], "header": "Authorization", "prefix": "Bearer ", "secret": "TESTONLY-ORFANA-S3CR3T", "created_at": "t", "updated_at": "t"}),
        )
        .unwrap();
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    assert!(e.agent_credentials("alfa").unwrap().is_empty(), "an orphan credential reached the agent that took the id");
    let fresh = input_of(&e, "alfa");
    let listing = call(&e, &fresh, "list_dir", json!({})).await.unwrap().content;
    assert!(!listing.contains("rimasto.txt") && !listing.contains("nota.txt"), "{listing}");
    let archived: Vec<String> = std::fs::read_dir(data.path().join("agents/.removed"))
        .unwrap()
        .flatten()
        .map(|f| f.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(archived.len(), 2, "the folder of the removed agent and the stray one are both put away: {archived:?}");
}

#[tokio::test]
async fn a_run_waiting_for_a_confirmation_is_stopped_when_its_agent_is_removed() {
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "risultato.txt", "content": ALFA_FILE})),
        ScriptedBackend::text_turn("Fatto."),
    ]);
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend));
    e.save_agent(AgentProfile { mode: "ask".into(), ..apart("alfa", &["write_file"]) }).unwrap();
    let s = e.create_session("alfa", None).unwrap();
    let run = e.start_run(&s.id, "scrivi".into()).await.unwrap();
    for _ in 0..200 {
        if !e.approvals().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(e.approvals().len(), 1, "the run asks before writing");
    e.delete_agent("alfa").unwrap();
    crate::tests::wait_finished(&e, &run).await;
    assert!(e.approvals().is_empty());
    let status = e.sessions().unwrap().into_iter().find(|x| x.id == s.id).unwrap().last_status;
    assert_ne!(status.as_deref(), Some("succeeded"), "the run went on after its agent was removed");
    assert!(!data.path().join("agents/alfa").exists() && !ws.path().join("risultato.txt").exists());
}

#[tokio::test]
async fn a_new_agent_whose_id_differs_only_by_case_is_refused_and_a_removal_moves_only_the_folder_with_its_exact_name() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &FILE_TOOLS)).unwrap();
    call(&e, &input_of(&e, "alfa"), "write_file", json!({"path": "nota.txt", "content": ALFA_FILE})).await.unwrap();
    // Whatever it asks for, a new agent that would share the folder of another on macOS and Windows is refused.
    let err = e.save_agent(shared("ALFA", &["todo"])).unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    assert!(e.agent("ALFA").is_err());
    // A folder called ALFA is not the folder of the agent alfa, even where the file system would find one for the other.
    std::fs::create_dir_all(data.path().join("agents/ALFA/workspace")).unwrap();
    assert!(e.archive_agent_workspace("zzz").unwrap().is_none());
    let moved = e.archive_agent_workspace("alfa").unwrap();
    assert!(moved.is_some() && !folder(data.path(), "alfa").join("nota.txt").exists());
    // Run again with nothing under the exact name: nothing is moved.
    e.delete_agent("alfa").unwrap();
    assert!(e.archive_agent_workspace("alfa").unwrap().is_none_or(|p| p.to_string_lossy().contains(".removed")));
}

#[tokio::test]
async fn a_team_member_that_is_apart_works_in_the_folder_of_the_team_run_and_never_in_its_own() {
    use aiw_core::api::{TeamDef, TeamMember};
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "dal-membro.txt", "content": "dal membro"})),
        ScriptedBackend::text_turn("fatto dal primo"),
        ScriptedBackend::text_turn("fatto dal secondo"),
    ]);
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend));
    e.save_agent(apart("primo", &["write_file"])).unwrap();
    e.save_agent(shared("secondo", &["write_file"])).unwrap();
    e.save_team(TeamDef {
        id: "squadra".into(),
        name: "Squadra".into(),
        description: String::new(),
        strategy: "pipeline".into(),
        supervisor: None,
        members: ["primo", "secondo"].iter().map(|a| TeamMember { agent: a.to_string(), role: format!("ruolo di {a}") }).collect(),
        aggregator: None,
        budget: None,
        output_schema: None,
    })
    .unwrap();
    let s = e.create_session("team:squadra", None).unwrap();
    let run = e.start_run(&s.id, "lavorate insieme".into()).await.unwrap();
    crate::tests::wait_finished(&e, &run).await;
    assert_eq!(
        std::fs::read_to_string(ws.path().join("dal-membro.txt")).unwrap(),
        "dal membro",
        "the member worked where the team run does"
    );
    assert!(
        !folder(data.path(), "primo").join("dal-membro.txt").exists(),
        "an agent apart never leaves its files in its own folder from a team"
    );
}

#[tokio::test]
async fn the_boundary_is_checked_again_when_a_handoff_really_moves_the_conversation() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    e.save_agent(shared("comune", &["todo"])).unwrap();
    e.save_agent(shared("altro", &["todo"])).unwrap();
    let agent_of = |id: &str| e.sessions().unwrap().into_iter().find(|x| x.id == id).unwrap().agent_id;
    // To an agent that became apart between the tool call and the move.
    let s = e.create_session("comune", None).unwrap();
    e.continue_handoff(&s.id, "comune", "auto", "alfa", "passa", 0, None).await;
    assert_eq!(agent_of(&s.id), "comune", "the conversation moved to an agent apart");
    assert!(e.messages(&s.id).unwrap().iter().any(|m| m.content.contains("rifiutato") && m.content.contains("isolato")));
    // From an agent apart.
    let s2 = e.create_session("alfa", None).unwrap();
    e.continue_handoff(&s2.id, "alfa", "auto", "comune", "passa", 0, None).await;
    assert_eq!(agent_of(&s2.id), "alfa", "the conversation left an agent apart");
}

#[tokio::test]
async fn the_warnings_say_where_a_shell_cannot_keep_an_agent_apart() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    let with_shell = apart("alfa", &["shell", "read_file"]);
    let without = apart("beta", &["read_file"]);
    let warned = !e.isolation_warnings(&with_shell).is_empty();
    assert_eq!(
        warned,
        !aiw_tools::sandbox::can_wrap_process(),
        "a shell on a machine that cannot contain a command is said not to keep the agent apart"
    );
    assert!(e.isolation_warnings(&without).is_empty(), "an agent with no shell is as apart as its file tools make it");
    assert!(e.isolation_warnings(&shared("comune", &["shell"])).is_empty());
    if warned {
        assert!(e.isolation_warnings(&with_shell)[0].contains("shell"));
    }
}

#[cfg(unix)]
#[test]
fn only_a_regular_file_is_read_as_the_instructions_of_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("fuori.txt"), "FUORI").unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "dentro").unwrap();
    assert_eq!(crate::isolation::read_regular_text(dir.path(), "AGENTS.md", 100).as_deref(), Some("dentro"));
    assert_eq!(crate::isolation::read_regular_text(dir.path(), "AGENTS.md", 3).as_deref(), Some("den"), "cut to the limit");
    assert!(crate::isolation::read_regular_text(dir.path(), "manca.md", 100).is_none());
    std::fs::remove_file(dir.path().join("AGENTS.md")).unwrap();
    std::os::unix::fs::symlink(other.path().join("fuori.txt"), dir.path().join("AGENTS.md")).unwrap();
    assert!(crate::isolation::read_regular_text(dir.path(), "AGENTS.md", 100).is_none(), "a link is not followed");
    std::fs::remove_file(dir.path().join("AGENTS.md")).unwrap();
    // A hard link to a file somewhere else is a regular file with the same inode as its target: it is not read either.
    std::fs::hard_link(other.path().join("fuori.txt"), dir.path().join("AGENTS.md")).unwrap();
    assert!(crate::isolation::read_regular_text(dir.path(), "AGENTS.md", 100).is_none(), "a hard link carried a file in");
    std::fs::remove_file(dir.path().join("AGENTS.md")).unwrap();
    std::fs::create_dir(dir.path().join("AGENTS.md")).unwrap();
    assert!(crate::isolation::read_regular_text(dir.path(), "AGENTS.md", 100).is_none());
}

#[tokio::test]
async fn a_workflow_step_of_another_agent_does_not_run_as_the_agent_of_the_first_step_when_one_of_them_is_apart() {
    let backend =
        ScriptedBackend::new(vec![ScriptedBackend::text_turn("risposta del primo"), ScriptedBackend::text_turn("risposta del secondo")]);
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    e.save_agent(apart("alfa", &["todo"])).unwrap();
    e.save_agent(shared("comune", &["todo"])).unwrap();
    e.save_agent(shared("altro", &["todo"])).unwrap();
    let workflow = |id: &str, first: &str, second: &str| -> aiw_core::api::WorkflowDef {
        serde_json::from_value(json!({
            "id": id, "name": id, "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [
                {"id": "uno", "kind": "agent", "agent": first, "prompt": "primo passo"},
                {"id": "due", "kind": "agent", "agent": second, "prompt": "secondo passo"}
            ]
        }))
        .unwrap()
    };
    let wait = |id: String| {
        let e = e.clone();
        async move {
            for _ in 0..300 {
                if e.workflow_runs().iter().any(|r| r.id == id && r.status == "succeeded") {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("the workflow did not finish");
        }
    };
    let agents_of_the_sessions = |title: &str| -> Vec<String> {
        let mut v: Vec<String> = e.sessions().unwrap().into_iter().filter(|s| s.title.contains(title)).map(|s| s.agent_id).collect();
        v.sort();
        v
    };
    // [alfa apart, comune]: the second step runs as comune, in a session of its own.
    e.save_workflow(workflow("misto", "alfa", "comune")).unwrap();
    let run = e.run_workflow("misto", serde_json::Map::new(), "test").await.unwrap();
    wait(run.id).await;
    assert_eq!(agents_of_the_sessions("misto"), ["alfa", "comune"], "the second step ran as the agent of the first");
    // [comune, alfa apart]: the other way round.
    backend.requests.lock().unwrap().clear();
    let backend2 = ScriptedBackend::new(vec![ScriptedBackend::text_turn("a"), ScriptedBackend::text_turn("b")]);
    e.register_native_runtime("llama-embedded", Arc::new(backend2));
    e.save_workflow(workflow("rovescio", "comune", "alfa")).unwrap();
    let run = e.run_workflow("rovescio", serde_json::Map::new(), "test").await.unwrap();
    wait(run.id).await;
    assert_eq!(agents_of_the_sessions("rovescio"), ["alfa", "comune"]);
    // Between agents that are not apart nothing changed: one session for the whole workflow, as before.
    let backend3 = ScriptedBackend::new(vec![ScriptedBackend::text_turn("a"), ScriptedBackend::text_turn("b")]);
    e.register_native_runtime("llama-embedded", Arc::new(backend3));
    e.save_workflow(workflow("semplice", "comune", "altro")).unwrap();
    let run = e.run_workflow("semplice", serde_json::Map::new(), "test").await.unwrap();
    wait(run.id).await;
    assert_eq!(agents_of_the_sessions("semplice"), ["comune"], "the session of the workflow is still shared between ordinary agents");
}
