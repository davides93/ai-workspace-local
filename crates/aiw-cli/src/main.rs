//! `aiw` — run AI Workspace Local headless (daemon / CLI).

use aiw_core::api::{InferenceParams, InferenceProfile};
use aiw_core::events::AgEvent;
use aiw_engine::{Engine, EngineOptions};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "aiw", version, about = "AI Workspace Local: workspace AI locale, headless")]
struct Cli {
    /// Cartella dati (default: $AIW_HOME o ~/.aiw)
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    /// Cartella di lavoro degli agenti (default: impostazione workspace.root o ~/AIW-Workspace)
    #[arg(long, short = 'w', global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Avvia il daemon: API REST/SSE, gateway OpenAI /v1 e UI web
    Serve {
        #[arg(long, default_value_t = 7777)]
        port: u16,
        /// Espone l'API in LAN (richiede il token di pairing)
        #[arg(long)]
        lan: bool,
        /// Cartella della UI compilata (apps/ui/dist)
        #[arg(long)]
        ui: Option<PathBuf>,
    },
    /// Mostra hardware rilevato e stato dei runtime
    Doctor,
    /// Gestisce i runtime di inferenza
    Runtimes {
        #[command(subcommand)]
        cmd: Option<RtCmd>,
    },
    /// Gestisce i modelli
    Models {
        #[command(subcommand)]
        cmd: Option<ModelCmd>,
    },
    /// Imposta il profilo di inferenza predefinito
    Profile {
        #[arg(long, default_value = "default")]
        id: String,
        #[arg(long)]
        runtime: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        context: Option<u32>,
    },
    /// Esegue un compito con un agente e mostra la risposta in streaming
    Run {
        prompt: String,
        #[arg(long, default_value = "operator")]
        agent: String,
        /// Approva automaticamente tutte le richieste di conferma
        #[arg(long)]
        yes: bool,
    },
    /// Esegue un workflow
    Workflow { id: String },
}

#[derive(Subcommand)]
enum RtCmd {
    List,
    Start {
        id: String,
        #[arg(long)]
        model: Option<String>,
    },
    Stop {
        id: String,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    List,
    Catalog,
    /// Scarica un modello del catalogo (id) oppure un file Hugging Face (--repo/--file)
    Pull {
        id: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        file: Option<String>,
    },
}

fn ui_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit
        .or_else(|| std::env::var("AIW_UI_DIR").ok().map(PathBuf::from))
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("ui"))).filter(|p| p.join("index.html").exists()))
        .or_else(|| Some(PathBuf::from("apps/ui/dist")).filter(|p| p.join("index.html").exists()))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,aiw=info".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let serve_mode = matches!(cli.cmd, Cmd::Serve { .. });
    let engine = Engine::new(EngineOptions {
        data_dir: cli.data.clone(),
        workspace_root: cli.workspace.clone(),
        mode: if serve_mode { "daemon".into() } else { "embedded".into() },
        ..Default::default()
    })
    .await
    .context("avvio del motore")?;
    match cli.cmd {
        Cmd::Serve { port, lan, ui } => {
            let lan = lan || engine.config().effective.get("server.lan").and_then(|v| v.as_bool()).unwrap_or(false);
            let ui = ui_dir(ui);
            eprintln!(
                "AI Workspace Local {} — http://127.0.0.1:{port}  (workspace: {})",
                aiw_core::VERSION,
                engine.workspace_root().display()
            );
            if lan {
                eprintln!("Accesso LAN attivo: codice di pairing {}", engine.pairing_code());
            }
            if ui.is_none() {
                eprintln!("UI web non trovata: compila apps/ui (npm run build) o passa --ui <cartella>");
            }
            let e2 = engine.clone();
            tokio::select! {
                r = aiw_server::serve(engine, port, lan, ui) => r?,
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nArresto: fermo i runtime avviati…");
                    e2.kill().await;
                }
            }
        }
        Cmd::Doctor => doctor(&engine).await,
        Cmd::Runtimes { cmd } => match cmd.unwrap_or(RtCmd::List) {
            RtCmd::List => {
                for r in engine.runtimes() {
                    println!("{:<16} {:<9} {:<12} {}", r.id, r.kind, r.status, r.status_detail.unwrap_or_default());
                }
            }
            RtCmd::Start { id, model } => {
                let r = engine.runtime_action(&id, "start", model).await?;
                println!("{} → {} {}", r.name, r.status, r.endpoint.unwrap_or_default());
                eprintln!("Il runtime resta attivo finché il processo è in esecuzione (Ctrl+C per fermarlo).");
                tokio::signal::ctrl_c().await?;
                engine.kill().await;
            }
            RtCmd::Stop { id } => {
                engine.runtime_action(&id, "stop", None).await?;
            }
        },
        Cmd::Models { cmd } => match cmd.unwrap_or(ModelCmd::List) {
            ModelCmd::List => {
                for m in engine.models() {
                    println!(
                        "{:<40} {:<6} {:<8} {:>8.2} GB  {}",
                        m.id,
                        m.format,
                        m.quant.unwrap_or_default(),
                        m.size_bytes as f64 / 1e9,
                        m.path.unwrap_or_default()
                    );
                }
            }
            ModelCmd::Catalog => {
                for c in engine.catalog() {
                    println!(
                        "{:<24} {:<7} {:<6} {:>7.1} GB  {:<6} {}",
                        c.id,
                        c.format,
                        c.quant,
                        c.size_bytes as f64 / 1e9,
                        c.fit,
                        c.fit_detail
                    );
                }
            }
            ModelCmd::Pull { id, repo, file } => {
                let req = match (id, repo, file) {
                    (Some(id), _, _) => serde_json::json!({"id": id}),
                    (None, Some(r), Some(f)) => serde_json::json!({"repo": r, "file": f}),
                    _ => bail!("indica un id del catalogo oppure --repo e --file"),
                };
                let d = engine.pull_model(&req)?;
                loop {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let cur = engine.downloads().into_iter().find(|x| x.id == d.id).context("download perso")?;
                    let pct = if cur.total > 0 { cur.received as f64 * 100.0 / cur.total as f64 } else { 0.0 };
                    eprint!("\r{}  {:.1}%  ({:.2}/{:.2} GB)   ", cur.file, pct, cur.received as f64 / 1e9, cur.total as f64 / 1e9);
                    match cur.status.as_str() {
                        "done" => {
                            eprintln!("\nCompletato.");
                            break;
                        }
                        "error" => bail!("download fallito: {}", cur.error.unwrap_or_default()),
                        "cancelled" => bail!("download annullato"),
                        _ => {}
                    }
                }
            }
        },
        Cmd::Profile { id, runtime, model, context } => {
            let p = engine.save_profile(InferenceProfile {
                id: id.clone(),
                name: if id == "default" { "Predefinito".into() } else { id.clone() },
                description: None,
                runtime_id: runtime,
                model,
                params: InferenceParams {
                    temperature: Some(0.6),
                    top_p: Some(0.95),
                    max_tokens: Some(4096),
                    context: Some(context.unwrap_or(8192)),
                },
            })?;
            println!("Profilo {} → {} · {}", p.id, p.runtime_id, p.model);
        }
        Cmd::Run { prompt, agent, yes } => run(&engine, &agent, prompt, yes).await?,
        Cmd::Workflow { id } => {
            let first = engine.run_workflow(&id, Default::default(), "user").await?;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                for a in engine.approvals() {
                    eprint!("\n[conferma] {} — consentire? [s/N] ", a.reason);
                    std::io::stderr().flush()?;
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    engine.respond_approval(
                        &a.id,
                        line.trim().eq_ignore_ascii_case("s") || line.trim().eq_ignore_ascii_case("y"),
                        false,
                    )?;
                }
                if let Some(r) = engine.workflow_runs().into_iter().find(|r| r.id == first.id) {
                    if r.finished_at.is_some() {
                        for s in &r.steps {
                            println!(
                                "{:<12} {:<8} {}",
                                s.id,
                                s.status,
                                s.output.clone().or(s.error.clone()).unwrap_or_default().lines().next().unwrap_or("")
                            );
                        }
                        println!("Esito: {}", r.status);
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn doctor(engine: &Engine) {
    let d = engine.device();
    println!("Dispositivo: {} · {} {} · {} · classe {}", d.name, d.os, d.os_version, d.arch, d.tier.to_uppercase());
    println!("CPU: {} ({} core / {} thread) [{}]", d.cpu.brand, d.cpu.cores, d.cpu.threads, d.cpu.features.join(" "));
    println!("Memoria: {:.1} GB (libera {:.1} GB)", d.memory_mb as f64 / 1024.0, d.available_memory_mb as f64 / 1024.0);
    for a in &d.accelerators {
        println!("  · {:<9} {} {}", a.kind, a.name, a.memory_mb.map(|m| format!("{:.1} GB", m as f64 / 1024.0)).unwrap_or_default());
    }
    println!("Sandbox shell: {}", aiw_engine_sandbox());
    println!("\nRuntime:");
    for r in engine.runtimes() {
        if r.status != "unsupported" {
            println!("  {:<16} {:<12} {}", r.id, r.status, r.status_detail.unwrap_or_default());
        }
    }
    println!("\nModelli locali: {}", engine.models().len());
    println!("Workspace: {}", engine.workspace_root().display());
}

fn aiw_engine_sandbox() -> &'static str {
    // Re-exported through the engine's dependencies.
    if cfg!(target_os = "linux") && std::path::Path::new("/usr/bin/bwrap").exists() {
        "bubblewrap"
    } else if cfg!(target_os = "macos") {
        "sandbox-exec"
    } else {
        "non disponibile (le esecuzioni chiedono sempre conferma)"
    }
}

async fn run(engine: &Engine, agent: &str, prompt: String, yes: bool) -> Result<()> {
    let session = engine.create_session(agent, None)?;
    let mut rx = engine.subscribe();
    let run_id = engine.start_run(&session.id, prompt).await?;
    let mut out = std::io::stdout();
    loop {
        let msg = match tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
            Ok(Ok(m)) => Some(m),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => None,
            Ok(Err(_)) => break,
            Err(_) => None,
        };
        if let Some(m) = msg.filter(|m| m.run_id == run_id) {
            match &m.event.event {
                AgEvent::TextMessageContent { delta, .. } => {
                    print!("{delta}");
                    out.flush()?;
                }
                AgEvent::TextMessageEnd { .. } => println!(),
                AgEvent::ToolCallStart { tool_call_name, .. } => eprintln!("\n▸ {tool_call_name}"),
                AgEvent::ToolCallResult { content, .. } => {
                    eprintln!("  ↳ {}", content.lines().next().unwrap_or("").chars().take(160).collect::<String>())
                }
                AgEvent::Custom { name, value } if name == "aiw.approval_request" => {
                    let id = value["id"].as_str().unwrap_or_default().to_string();
                    let allow = if yes {
                        true
                    } else {
                        eprint!("\n[conferma] {} {} — consentire? [s/N] ", value["tool"].as_str().unwrap_or(""), value["args"]);
                        std::io::stderr().flush()?;
                        let mut line = String::new();
                        std::io::stdin().read_line(&mut line)?;
                        matches!(line.trim().to_lowercase().as_str(), "s" | "si" | "sì" | "y" | "yes")
                    };
                    engine.respond_approval(&id, allow, false)?;
                }
                AgEvent::RunError { message, .. } => {
                    eprintln!("\nErrore: {message}");
                    break;
                }
                AgEvent::RunFinished { .. } => break,
                _ => {}
            }
        }
        if engine.run_done(&run_id) && rx.is_empty() {
            break;
        }
    }
    Ok(())
}
