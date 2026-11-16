//! What the engine reads for itself from the shared workspace (`AGENTS.md`, `.aiw/skills`) and puts in a prompt: a link the
//! agent makes there (or a repository ships) must not carry in a file the workspace does not hold, such as the token of the owner
//! (`aiw_core::confined`). A link that stays inside the workspace is still read.
//!
//! The ones that fail without the rule are those that make a link or a hard link out of the workspace, a link to the data folder
//! of an engine whose data lives inside the workspace, and the skills that lead out. The others (a plain file, a link inside, a
//! folder named `AGENTS.md`, the user's own skills) say what must keep working.

use super::*;
use aiw_runtimes::ScriptedBackend;

async fn engine_at(data: &Path, ws: &Path) -> Engine {
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
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
    e
}

/// The system prompt the next message of a chat session would carry.
fn prompt_of(e: &Engine) -> String {
    let s = e.create_session("operator", None).unwrap();
    let input = e.build_input_at(
        "r",
        &s.id,
        e.agent("operator").unwrap(),
        e.profile("default").unwrap(),
        vec![],
        String::new(),
        CancellationToken::new(),
        0,
        Default::default(),
        None,
    );
    input.system_extra.join("\n")
}

fn skill_file(dir: &Path, name: &str, desc: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n")).unwrap();
}

#[test]
fn a_plain_agents_md_and_the_global_one_reach_the_prompt() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        assert_eq!(e.agents_md(), "");
        std::fs::write(data.path().join("AGENTS.md"), "Regole globali").unwrap();
        std::fs::write(ws.path().join("AGENTS.md"), "Regole del progetto").unwrap();
        let text = e.agents_md();
        assert!(text.contains("# Istruzioni globali (AGENTS.md)\nRegole globali"), "{text}");
        assert!(text.contains("# Istruzioni del workspace (AGENTS.md)\nRegole del progetto"), "{text}");
        assert!(prompt_of(&e).contains("Regole del progetto"));
        // A file longer than the limit is cut, as before.
        std::fs::write(ws.path().join("AGENTS.md"), "x".repeat(30_000)).unwrap();
        assert_eq!(e.agents_md().matches('x').count(), 20_000);
    });
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn a_link_to_the_token_or_to_a_file_outside_the_workspace_does_not_reach_the_prompt() {
        let (data, ws, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        // The file `token` of the data folder is the real one, written by the engine itself.
        assert_eq!(std::fs::read_to_string(data.path().join("token")).unwrap().trim(), e.token());
        std::fs::write(other.path().join("chiave"), "-----BEGIN PRIVATE KEY-----").unwrap();

        // What the agent does from a shell it can write the workspace with.
        symlink(data.path().join("token"), ws.path().join("AGENTS.md")).unwrap();
        assert_eq!(e.agents_md(), "");
        assert!(!prompt_of(&e).contains(e.token()), "the token of the owner is in the prompt");

        // The same with a file of the account that is not the token, and with a link made of relative steps.
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        symlink(other.path().join("chiave"), ws.path().join("AGENTS.md")).unwrap();
        assert_eq!(e.agents_md(), "");
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        symlink("../../../../../../../../../../../../etc/hosts", ws.path().join("AGENTS.md")).unwrap();
        assert_eq!(e.agents_md(), "");
        assert!(!prompt_of(&e).contains("localhost"));
    }

    #[tokio::test]
    async fn a_link_to_a_file_inside_the_workspace_is_read() {
        let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        std::fs::write(ws.path().join("CLAUDE.md"), "Istruzioni di Claude").unwrap();
        symlink("CLAUDE.md", ws.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("# Istruzioni del workspace (AGENTS.md)\nIstruzioni di Claude"));
        assert!(prompt_of(&e).contains("Istruzioni di Claude"));
        // Also a file in a folder of the workspace, and a hard link between two files of it.
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        std::fs::create_dir_all(ws.path().join("docs")).unwrap();
        std::fs::write(ws.path().join("docs/agenti.md"), "Nella cartella docs").unwrap();
        symlink("docs/agenti.md", ws.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("Nella cartella docs"));
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        std::fs::hard_link(ws.path().join("CLAUDE.md"), ws.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("Istruzioni di Claude"), "a file with two names, both in the workspace folder");
    }

    #[tokio::test]
    async fn a_hard_link_to_the_token_does_not_reach_the_prompt() {
        let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        std::fs::hard_link(data.path().join("token"), ws.path().join("AGENTS.md")).unwrap();
        assert_eq!(e.agents_md(), "");
        assert!(!prompt_of(&e).contains(e.token()));
    }

    #[tokio::test]
    async fn a_folder_called_agents_md_gives_nothing() {
        let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        std::fs::create_dir_all(ws.path().join("AGENTS.md")).unwrap();
        std::fs::write(ws.path().join("AGENTS.md").join("note.md"), "x").unwrap();
        assert_eq!(e.agents_md(), "");
        let _ = prompt_of(&e);
    }

    #[tokio::test]
    async fn the_global_agents_md_is_read_as_it_always_was_links_included() {
        let (data, ws, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        // The data folder is out of the reach of the agents: a link there is the person's own.
        std::fs::write(other.path().join("mie-regole.md"), "Le mie regole altrove").unwrap();
        symlink(other.path().join("mie-regole.md"), data.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("# Istruzioni globali (AGENTS.md)\nLe mie regole altrove"));
    }

    /// The data folder inside the workspace (the workspace is the home folder, the data `~/.aiw`): a link to the token resolves
    /// to a path inside the workspace, and only the hidden places keep it out.
    #[tokio::test]
    async fn a_link_to_the_data_folder_does_not_reach_the_prompt_when_the_data_lives_inside_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let data = ws.path().join(".aiw");
        let e = engine_at(&data, ws.path()).await;
        assert!(data.join("token").is_file());
        symlink(".aiw/token", ws.path().join("AGENTS.md")).unwrap();
        assert_eq!(e.agents_md(), "");
        assert!(!prompt_of(&e).contains(e.token()));
        // A place of `sandbox.deny_read` inside the workspace is kept out as well (the default entries are in the home folder, which a
        // workspace can be; here the entry is made absolute).
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        std::fs::create_dir_all(ws.path().join(".ssh")).unwrap();
        std::fs::write(ws.path().join(".ssh/id_ed25519"), "CHIAVE-SSH-PRIVATA").unwrap();
        symlink(".ssh/id_ed25519", ws.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("CHIAVE-SSH-PRIVATA"), "not hidden yet: a file of the workspace");
        e.set_config("global", "sandbox.deny_read", json!([ws.path().join(".ssh").display().to_string()])).unwrap();
        assert_eq!(e.agents_md(), "");
        assert!(!prompt_of(&e).contains("CHIAVE-SSH-PRIVATA"));
        // And the same link from a plain workspace is still a file of the workspace: only the places made hidden are refused.
        std::fs::remove_file(ws.path().join("AGENTS.md")).unwrap();
        std::fs::write(ws.path().join("regole.md"), "Regole normali").unwrap();
        symlink("regole.md", ws.path().join("AGENTS.md")).unwrap();
        assert!(e.agents_md().contains("Regole normali"));
    }

    #[tokio::test]
    async fn the_skills_of_the_workspace_cannot_lead_out_of_it() {
        let (data, ws, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        let skills = ws.path().join(".aiw/skills");
        skill_file(&skills.join("buona"), "buona", "una skill del progetto");
        // A file of the account that starts like a skill, and a folder with files in it.
        skill_file(&other.path().join("riservata"), "riservata", "non deve entrare nel prompt");
        std::fs::write(other.path().join("riservata").join("chiavi.txt"), "x").unwrap();
        std::fs::create_dir_all(skills.join("file")).unwrap();
        symlink(other.path().join("riservata/SKILL.md"), skills.join("file/SKILL.md")).unwrap();
        symlink(other.path().join("riservata"), skills.join("cartella")).unwrap();
        let names: Vec<String> = e.skills().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["buona"], "{names:?}");
        assert!(!prompt_of(&e).contains("non deve entrare"));

        // The folder of the skills itself leads out.
        std::fs::remove_dir_all(&skills).unwrap();
        symlink(other.path(), &skills).unwrap();
        assert!(e.skills().is_empty());
        // And `.aiw` as a link.
        std::fs::remove_file(&skills).unwrap();
        std::fs::remove_dir(ws.path().join(".aiw")).unwrap();
        std::fs::create_dir_all(other.path().join("skills")).unwrap();
        skill_file(&other.path().join("skills/altra"), "altra", "di un'altra cartella");
        symlink(other.path(), ws.path().join(".aiw")).unwrap();
        assert!(e.skills().is_empty());
    }

    #[tokio::test]
    async fn the_skills_of_the_user_stay_as_they_are_and_a_link_inside_the_workspace_skills_works() {
        let (data, ws, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        // The person puts a skill kept elsewhere in `<data>/skills`, by a link: it stays.
        skill_file(&other.path().join("mia"), "mia", "la skill dell'utente");
        std::fs::create_dir_all(data.path().join("skills")).unwrap();
        symlink(other.path().join("mia"), data.path().join("skills/mia")).unwrap();
        // A skill of the workspace, with a second name for the same folder, and one that overrides the user's.
        skill_file(&ws.path().join(".aiw/skills/progetto"), "progetto", "del progetto");
        symlink("progetto", ws.path().join(".aiw/skills/alias")).unwrap();
        skill_file(&ws.path().join(".aiw/skills/mia"), "mia", "versione del workspace");
        let skills = e.skills();
        let by_name = |n: &str| skills.iter().find(|s| s.name == n).unwrap().description.clone();
        assert_eq!(by_name("progetto"), "del progetto");
        assert_eq!(by_name("mia"), "versione del workspace");
        assert_eq!(skills.len(), 2);
        // The user's skill by itself (the one of the workspace is removed).
        std::fs::remove_dir_all(ws.path().join(".aiw/skills/mia")).unwrap();
        let skills = e.skills();
        let mia = skills.iter().find(|s| s.name == "mia").unwrap();
        assert_eq!(mia.description, "la skill dell'utente");
        assert!(aiw_agent::skills::load(mia).contains("# mia"));
    }

    /// With the data folder inside the workspace, `<workspace>/.aiw/skills` is the folder of the user's skills: kept out as a
    /// place of the workspace, the skills are all the same found as the user's own.
    #[tokio::test]
    async fn the_user_skills_are_still_found_when_the_data_folder_is_inside_the_workspace() {
        let ws = tempfile::tempdir().unwrap();
        let data = ws.path().join(".aiw");
        let e = engine_at(&data, ws.path()).await;
        skill_file(&data.join("skills/mia"), "mia", "della cartella dei dati");
        let skills = e.skills();
        assert_eq!(skills.len(), 1);
        assert!(skills[0].confined_to.is_none());
    }
}
