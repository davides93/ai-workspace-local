//! Folders of the computer handed to the agents (Mac App Store build): the list, what is stored, what
//! reaches the file tools, and what the agents cannot do to widen it. The open panel and the bookmarks
//! are a fake here: their real behaviour is the shell's.

use super::*;
use aiw_core::folders::{FolderBridge, OpenedFolder, PickedFolder};
use aiw_runtimes::ScriptedBackend;

#[derive(Default)]
struct FakeBridge {
    /// What the next panels return (one entry per `pick`).
    picks: Mutex<Vec<Vec<PathBuf>>>,
    /// Bookmarks that come back stale: `open` hands a fresh one.
    stale: Mutex<HashSet<String>>,
    released: Mutex<Vec<PathBuf>>,
    refused: Mutex<Vec<PathBuf>>,
    home: Mutex<Option<PathBuf>>,
    opened: Mutex<Vec<String>>,
}

impl FakeBridge {
    fn will_pick(&self, dirs: Vec<PathBuf>) {
        self.picks.lock().unwrap().push(dirs);
    }
}

impl FolderBridge for FakeBridge {
    fn pick(&self, _message: &str, _prompt: &str) -> Result<Vec<PickedFolder>> {
        let dirs = self.picks.lock().unwrap().remove(0);
        Ok(dirs.into_iter().map(|p| PickedFolder { bookmark: format!("bm1:{}", p.display()), path: p }).collect())
    }
    fn open(&self, bookmark: &str) -> Result<OpenedFolder> {
        self.opened.lock().unwrap().push(bookmark.to_string());
        let path = PathBuf::from(bookmark.split_once(':').map(|(_, p)| p).unwrap_or(""));
        if !path.exists() {
            return Err(Error::NotFound("il permesso non si riapre: la cartella non c'è più".into()));
        }
        let renewed = self.stale.lock().unwrap().remove(bookmark).then(|| format!("bm2:{}", path.display()));
        Ok(OpenedFolder { path, renewed })
    }
    fn release(&self, path: &Path) {
        self.released.lock().unwrap().push(path.to_path_buf());
    }
    fn refuse(&self, path: &Path) -> Option<String> {
        self.refused.lock().unwrap().iter().any(|r| path.starts_with(r)).then(|| "è una cartella di sistema".into())
    }
    fn real_home(&self) -> Option<PathBuf> {
        self.home.lock().unwrap().clone()
    }
}

async fn engine_at(data: &Path, ws: &Path) -> Engine {
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        sandboxed: true,
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

fn input_of(e: &Engine) -> aiw_agent::RunInput {
    let s = e.create_session("operator", None).unwrap();
    e.build_input_at(
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
    )
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap()
}

#[tokio::test]
async fn without_a_bridge_nothing_is_granted_and_the_tools_see_no_extra_folder() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    assert!(!e.folders().supported && e.folders().folders.is_empty());
    assert!(!e.system(false).folder_access);
    assert!(matches!(e.add_folders("m", "p"), Err(Error::Unsupported(_))));
    let input = input_of(&e);
    assert!(input.tool_ctx.write_roots.is_empty() && input.tool_ctx.outside_hint.is_none());
    assert!(!input.system_extra[0].contains("Cartelle del computer"));
}

#[tokio::test]
async fn a_folder_the_person_picks_reaches_the_file_tools_and_the_model() {
    let (data, ws, downloads) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(downloads.path().join("fattura.txt"), "totale 42").unwrap();
    let e = engine_at(data.path(), ws.path()).await;
    let bridge = Arc::new(FakeBridge::default());
    e.set_folder_bridge(bridge.clone());
    assert!(e.folders().supported && e.system(false).folder_access);
    // Before the pick the folder is outside, and the refusal says where the person can change it.
    let before = input_of(&e);
    let g = canon(downloads.path());
    let err = before.tool_ctx.resolve(&g.join("fattura.txt").display().to_string(), false).unwrap_err().to_string();
    assert!(err.contains("fuori dal workspace") && err.contains("Impostazioni → Cartelle consentite"), "{err}");
    assert!(before.system_extra[0].contains("nessuna"), "{}", before.system_extra[0]);

    bridge.will_pick(vec![downloads.path().to_path_buf()]);
    let added = e.add_folders("Scegli", "Consenti").unwrap();
    assert_eq!((added.added, added.refused.len()), (1, 0));
    let f = &added.info.folders[0];
    assert!(f.available && f.problem.is_none());
    assert_eq!(f.path, g.display().to_string());
    assert_eq!(f.name, g.file_name().unwrap().to_string_lossy());

    let input = input_of(&e);
    assert_eq!(input.tool_ctx.write_roots, vec![g.clone()]);
    assert!(input.system_extra[0].contains(&g.display().to_string()), "{}", input.system_extra[0]);
    assert!(input.system_extra[0].contains("`~` è la cartella dell'app"), "{}", input.system_extra[0]);
    assert!(input.tool_ctx.resolve(&g.join("fattura.txt").display().to_string(), false).is_ok());
    assert!(input.tool_ctx.resolve(&g.join("nuovo.txt").display().to_string(), true).is_ok());
    // Not what is around it, and the workspace is still the working folder.
    assert!(input.tool_ctx.resolve(&g.parent().unwrap().display().to_string(), false).is_err());
    assert_eq!(input.tool_ctx.workspace_root, canon(ws.path()));
    // The tools run by workflows and the API get them too.
    let out = e.execute_tool("run_x", "api", "test", "list_dir", json!({"path": g.display().to_string()})).await.unwrap();
    assert!(out.content.contains("fattura.txt"), "{}", out.content);
}

#[tokio::test]
async fn the_folders_come_back_at_the_next_launch_and_a_stale_permission_is_renewed() {
    let (data, ws, docs) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let g = canon(docs.path());
    let first = Arc::new(FakeBridge::default());
    {
        let e = engine_at(data.path(), ws.path()).await;
        e.set_folder_bridge(first.clone());
        first.will_pick(vec![docs.path().to_path_buf()]);
        e.add_folders("m", "p").unwrap();
    }
    // The next launch: the permission is reopened before anything else, and it came back stale.
    let second = Arc::new(FakeBridge::default());
    second.stale.lock().unwrap().insert(format!("bm1:{}", docs.path().display()));
    let e = engine_at(data.path(), ws.path()).await;
    assert!(e.folders().folders.is_empty(), "nothing is granted until the shell registers its bridge");
    e.set_folder_bridge(second.clone());
    let list = e.folders().folders;
    assert_eq!(list.len(), 1);
    assert!(list[0].available, "{:?}", list[0]);
    assert_eq!(list[0].path, g.display().to_string());
    assert_eq!(input_of(&e).tool_ctx.write_roots, vec![g]);
    // The fresh permission was stored: the launch after that opens it, not the old one.
    let third = Arc::new(FakeBridge::default());
    let e2 = engine_at(data.path(), ws.path()).await;
    e2.set_folder_bridge(third.clone());
    assert_eq!(third.opened.lock().unwrap().clone(), vec![format!("bm2:{}", docs.path().display())]);
}

#[tokio::test]
async fn a_folder_that_cannot_be_reopened_stays_listed_but_the_agents_do_not_see_it() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let gone = tempfile::tempdir().unwrap();
    let gone_path = gone.path().to_path_buf();
    let bridge = Arc::new(FakeBridge::default());
    {
        let e = engine_at(data.path(), ws.path()).await;
        e.set_folder_bridge(bridge.clone());
        bridge.will_pick(vec![gone_path.clone()]);
        e.add_folders("m", "p").unwrap();
    }
    drop(gone); // the folder is deleted
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(Arc::new(FakeBridge::default()));
    let list = e.folders().folders;
    assert_eq!(list.len(), 1);
    assert!(!list[0].available && list[0].path.is_empty() && list[0].problem.as_deref().unwrap().contains("non c'è più"), "{:?}", list[0]);
    assert!(input_of(&e).tool_ctx.write_roots.is_empty());
    // It can be taken off the list.
    let id = list[0].id.clone();
    assert!(e.remove_folder(&id).unwrap().folders.is_empty());
    assert!(matches!(e.remove_folder(&id), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn removing_a_folder_gives_up_its_permission_and_the_tools_lose_it() {
    let (data, ws, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let g = canon(dir.path());
    let bridge = Arc::new(FakeBridge::default());
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(bridge.clone());
    bridge.will_pick(vec![dir.path().to_path_buf()]);
    let id = e.add_folders("m", "p").unwrap().info.folders[0].id.clone();
    assert!(input_of(&e).tool_ctx.resolve(&g.join("a.txt").display().to_string(), true).is_ok());
    assert!(e.remove_folder(&id).unwrap().folders.is_empty());
    assert_eq!(bridge.released.lock().unwrap().clone(), vec![g.clone()]);
    assert!(input_of(&e).tool_ctx.resolve(&g.join("a.txt").display().to_string(), true).is_err());
    // And it stays gone at the next launch.
    let e2 = engine_at(data.path(), ws.path()).await;
    e2.set_folder_bridge(Arc::new(FakeBridge::default()));
    assert!(e2.folders().folders.is_empty());
}

#[tokio::test]
async fn what_cannot_be_granted_is_refused_with_a_reason_and_duplicates_are_ignored() {
    let (data, ws, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let g = canon(dir.path());
    std::fs::create_dir_all(g.join("sotto")).unwrap();
    let bridge = Arc::new(FakeBridge::default());
    bridge.refused.lock().unwrap().push(g.join("sistema"));
    std::fs::create_dir_all(g.join("sistema")).unwrap();
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(bridge.clone());
    let data_c = canon(data.path());

    // The whole disk, the data folder, one that holds it, one the platform refuses.
    bridge.will_pick(vec![PathBuf::from("/"), data_c.clone(), data_c.parent().unwrap().to_path_buf(), g.join("sistema")]);
    let r = e.add_folders("m", "p").unwrap();
    assert_eq!(r.added, 0);
    assert_eq!(r.refused.len(), 4, "{:?}", r.refused);
    assert!(r.refused[0].contains("tutto il disco"), "{:?}", r.refused);
    assert!(r.refused[1].contains("dati dell'app") && r.refused[2].contains("dati dell'app"), "{:?}", r.refused);
    assert!(r.refused[3].contains("sistema"), "{:?}", r.refused);
    assert!(e.folders().folders.is_empty());
    assert!(bridge.released.lock().unwrap().len() >= 4, "the permission of a refused pick is given up");

    // The same folder twice, and a folder inside one already there.
    bridge.will_pick(vec![g.clone(), g.clone()]);
    let r = e.add_folders("m", "p").unwrap();
    assert_eq!((r.added, r.info.folders.len()), (1, 1));
    bridge.will_pick(vec![g.join("sotto")]);
    let r = e.add_folders("m", "p").unwrap();
    assert_eq!((r.added, r.info.folders.len()), (0, 1));
    assert!(r.refused[0].contains("è già dentro"), "{:?}", r.refused);
    assert!(bridge.released.lock().unwrap().contains(&g.join("sotto")), "the permission opened for the inner folder is given up");
    assert!(!bridge.released.lock().unwrap().iter().rev().take(1).any(|p| *p == g), "the one of the folder on the list stays");
    // A cancelled panel changes nothing.
    bridge.will_pick(vec![]);
    let r = e.add_folders("m", "p").unwrap();
    assert_eq!((r.added, r.refused.len(), r.info.folders.len()), (0, 0, 1));
}

#[tokio::test]
async fn settings_cannot_add_a_folder_or_change_the_workspace_of_a_sandboxed_build() {
    let (data, ws, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(Arc::new(FakeBridge::default()));
    // The workspace is refused, and the reason points at the person's own way.
    let err = e.set_config("global", "workspace.root", json!(dir.path().display().to_string())).unwrap_err().to_string();
    assert!(err.contains("Cartelle consentite"), "{err}");
    // No setting holds folders: whatever key is written, nothing reads it as a folder to grant (the layers of
    // the workspace and of the device are written the same way).
    for key in ["folders", "folder_grants", "workspace.folders", "workspace.extra_roots", "sandbox.write_roots", "folders.granted"] {
        for layer in ["global", "device", "workspace"] {
            let _ = e.set_config(layer, key, json!([dir.path().display().to_string()]));
        }
    }
    assert!(e.folders().folders.is_empty());
    assert!(input_of(&e).tool_ctx.write_roots.is_empty());
}

#[tokio::test]
async fn the_secret_locations_are_hidden_at_the_real_home_too() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let home = tempfile::tempdir().unwrap();
    let h = canon(home.path());
    std::fs::create_dir_all(h.join(".ssh")).unwrap();
    std::fs::write(h.join(".ssh/id_ed25519"), "chiave").unwrap();
    std::fs::create_dir_all(h.join("Documenti")).unwrap();
    std::fs::write(h.join("Documenti/a.txt"), "x").unwrap();
    let bridge = Arc::new(FakeBridge::default());
    *bridge.home.lock().unwrap() = Some(h.clone());
    let e = engine_at(data.path(), ws.path()).await;
    e.set_folder_bridge(bridge.clone());
    // The person grants a folder that holds the secret one (the platform did not refuse it here).
    bridge.will_pick(vec![h.clone()]);
    assert_eq!(e.add_folders("m", "p").unwrap().added, 1);
    let ctx = input_of(&e).tool_ctx;
    assert!(ctx.resolve(&h.join("Documenti/a.txt").display().to_string(), false).is_ok());
    let err = ctx.resolve(&h.join(".ssh/id_ed25519").display().to_string(), false).unwrap_err().to_string();
    assert!(err.contains("nascosto"), "{err}");
    assert!(ctx.resolve(&h.join(".ssh/nuova").display().to_string(), true).is_err());
}
