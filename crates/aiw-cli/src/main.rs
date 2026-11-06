//! `aiw` — run AI Workspace Local headless (daemon / CLI).

use aiw_core::api::{InferenceParams, InferenceProfile, PullRequest};
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
        /// Lavora in un git worktree separato (branch aiw/<id>) invece che nel workspace
        #[arg(long)]
        worktree: bool,
    },
    /// Esegue un workflow
    Workflow { id: String },
    /// Esporta le traiettorie delle sessioni (per valutazione e fine-tuning)
    Traces {
        #[command(subcommand)]
        cmd: TracesCmd,
    },
    /// Valuta gli agenti con suite di task e grader
    Eval {
        #[command(subcommand)]
        cmd: EvalCmd,
    },
}

#[derive(Subcommand)]
enum EvalCmd {
    /// Esegue una suite (file YAML/JSON o id di una suite salvata) e mostra il riepilogo.
    /// I grader `command` eseguono comandi della suite con i tuoi permessi: lancia solo suite di cui ti fidi.
    Run {
        /// File della suite oppure id di una suite salvata
        suite: String,
        /// Agente per tutti i task (sostituisce quello della suite)
        #[arg(long)]
        agent: Option<String>,
        /// Profilo di inferenza per tutti i task (sostituisce quello dell'agente). Ripetuto, esegue la
        /// suite con ciascun profilo e confronta gli altri con il primo
        #[arg(long)]
        profile: Vec<String>,
        /// Quante volte eseguire ogni task
        #[arg(long, default_value_t = 1)]
        repeat: u32,
        /// Scrive il report in questo file (JSON)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Non cancella le cartelle di lavoro dei task
        #[arg(long)]
        keep: bool,
    },
    /// Confronta due report (id salvati o file JSON): le differenze sono B meno A
    Compare {
        a: String,
        b: String,
        /// Scrive il confronto come JSON
        #[arg(long)]
        json: bool,
    },
    /// Elenca le suite salvate
    Suites,
    /// Elenca i report salvati
    List,
}

#[derive(Subcommand)]
enum TracesCmd {
    /// Scrive una riga JSON per sessione (JSONL), con i segreti mascherati
    Export {
        /// Solo le sessioni di questo agente
        #[arg(long)]
        agent: Option<String>,
        /// Attività dal giorno (AAAA-MM-GG) o dall'istante (RFC 3339)
        #[arg(long)]
        since: Option<String>,
        /// Attività fino al giorno (AAAA-MM-GG, incluso) o all'istante (RFC 3339)
        #[arg(long)]
        until: Option<String>,
        /// Ultimo stato della sessione: succeeded, failed, cancelled, running
        #[arg(long)]
        status: Option<String>,
        /// Numero massimo di sessioni (le più recenti)
        #[arg(long)]
        limit: Option<usize>,
        /// Solo le sessioni con un feedback: up (pollice su), down (pollice giù) o any
        #[arg(long, value_parser = ["up", "down", "any"])]
        rated: Option<String>,
        /// File di destinazione (default: standard output)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Non maschera i segreti: l'export può contenere chiavi e token
        #[arg(long)]
        no_redact: bool,
        /// Glob dei percorsi da escludere: il contenuto dei messaggi dei tool che li citano diventa [escluso]
        #[arg(long = "exclude-path")]
        exclude_path: Vec<String>,
    },
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
    /// Elenca i modelli installati (anche quelli di Ollama, LM Studio e della cache Hugging Face)
    List,
    /// Elenca i modelli consigliati con la compatibilità per questo dispositivo
    Catalog,
    /// Cerca modelli su un provider (catalog, hf, modelscope, ollama)
    Search {
        /// Testo da cercare
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value = "hf")]
        provider: String,
        /// gguf, mlx, onnx, safetensors…
        #[arg(long)]
        format: Option<String>,
        /// downloads, likes o updated
        #[arg(long)]
        sort: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Mostra le varianti scaricabili di un modello, con fit e runtime compatibili
    Show {
        provider: String,
        model: String,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Scarica un modello: un id del catalogo, --provider/--model/--variant oppure --url
    Pull {
        /// Id di un modello del catalogo
        id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Variante (es. Q4_K_M); se manca si usa la consigliata
        #[arg(long)]
        variant: Option<String>,
        #[arg(long)]
        revision: Option<String>,
        /// Scarica un singolo file da un indirizzo diretto
        #[arg(long)]
        url: Option<String>,
        /// Nome da dare al file scaricato da --url
        #[arg(long)]
        file_name: Option<String>,
        /// sha256 atteso del file scaricato da --url
        #[arg(long)]
        sha256: Option<String>,
        /// Hugging Face: repository (con --file), come nella v0.3
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        file: Option<String>,
    },
    /// Elimina un modello installato in AIW (mai quelli di altre app)
    Rm { id: String },
    /// Elenca i provider di modelli
    Providers,
    /// Imposta il token o l'endpoint di un provider
    Auth {
        provider: String,
        /// Token di accesso (per i repository gated di Hugging Face)
        #[arg(long)]
        token: Option<String>,
        /// Endpoint alternativo (mirror); vuoto per tornare a quello predefinito
        #[arg(long)]
        endpoint: Option<String>,
        /// Rimuove il token memorizzato
        #[arg(long)]
        clear: bool,
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
        Cmd::Models { cmd } => models_cmd(&engine, cmd.unwrap_or(ModelCmd::List)).await?,
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
        Cmd::Traces { cmd: TracesCmd::Export { agent, since, until, status, limit, rated, out, no_redact, exclude_path } } => {
            let filter = aiw_engine::traces::TraceFilter {
                agent,
                since,
                until,
                status,
                limit,
                rated,
                redact: !no_redact,
                exclude_paths: exclude_path,
            };
            let records = engine.export_traces(&filter)?;
            let jsonl = aiw_engine::traces::to_jsonl(&records)?;
            match out {
                Some(path) => {
                    std::fs::write(&path, jsonl).with_context(|| format!("scrittura di {}", path.display()))?;
                    eprintln!("{} sessioni esportate in {}", records.len(), path.display());
                }
                None => {
                    print!("{jsonl}");
                    eprintln!("{} sessioni esportate", records.len());
                }
            }
            if no_redact {
                eprintln!("Attenzione: i segreti non sono stati mascherati.");
            }
        }
        Cmd::Eval { cmd } => eval_cmd(&engine, cmd).await?,
        Cmd::Run { prompt, agent, yes, worktree } => run(&engine, &agent, prompt, yes, worktree).await?,
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

fn percent(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

async fn eval_cmd(engine: &Engine, cmd: EvalCmd) -> Result<()> {
    use aiw_engine::eval::EvalRequest;
    match cmd {
        EvalCmd::Suites => {
            for s in engine.eval_suites()? {
                println!("{:<24} {:>3} task  {}", s.id, s.tasks.len(), s.name);
            }
        }
        EvalCmd::List => {
            for r in engine.eval_reports()? {
                println!(
                    "{:<26} {:<20} {:<10} {:>3}/{:<3} pass rate {:>4}  score {:.2}  {}",
                    r.id,
                    r.suite_id,
                    r.status,
                    r.summary.passed,
                    r.summary.total,
                    percent(r.summary.pass_rate),
                    r.summary.score,
                    r.started_at
                );
            }
        }
        EvalCmd::Compare { a, b, json } => {
            let comparison = engine.compare_eval_runs(&a, &b)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&comparison)?);
            } else {
                print!("{}", comparison.to_text());
            }
        }
        EvalCmd::Run { suite, agent, profile, repeat, out, keep } => {
            let suite = engine.resolve_eval_suite(&suite)?;
            eprintln!("Suite «{}»: {} task × {} (i grader `command` girano con i tuoi permessi)", suite.name, suite.tasks.len(), repeat);
            // With several profiles the suite runs once per profile and the others are compared to the first.
            let profiles: Vec<Option<String>> = if profile.is_empty() { vec![None] } else { profile.iter().cloned().map(Some).collect() };
            let mut reports = vec![];
            for p in &profiles {
                if profiles.len() > 1 {
                    eprintln!("\n== profilo {} ==", p.as_deref().unwrap_or_default());
                }
                let report = eval_run_one(
                    engine,
                    &suite,
                    EvalRequest { agent: agent.clone(), profile: p.clone(), repeat: Some(repeat), keep, ..Default::default() },
                )
                .await?;
                if let Some(path) = &out {
                    let path = match (profiles.len(), p) {
                        (n, Some(p)) if n > 1 => {
                            let mut name = path.clone().into_os_string();
                            name.push(format!(".{p}.json"));
                            PathBuf::from(name)
                        }
                        _ => path.clone(),
                    };
                    std::fs::write(&path, serde_json::to_string_pretty(&report)?)
                        .with_context(|| format!("scrittura di {}", path.display()))?;
                    println!("Report scritto in {}", path.display());
                }
                reports.push(report);
            }
            for other in reports.iter().skip(1) {
                println!("\nConfronto tra i profili:\n{}", aiw_eval::compare(&reports[0], other).to_text());
            }
        }
    }
    Ok(())
}

/// Runs the suite once, telling how it goes, and prints the summary table.
async fn eval_run_one(engine: &Engine, suite: &aiw_eval::Suite, req: aiw_engine::eval::EvalRequest) -> Result<aiw_eval::EvalReport> {
    use aiw_eval::EvalEvent;
    let repeat = req.repeat.unwrap_or(1);
    {
        {
            let mut started = 1;
            let report = engine
                .run_eval(suite, &req, &mut |ev| match ev {
                    EvalEvent::Started(_) => {}
                    EvalEvent::TaskStarted { task_id, attempt, attempts } => {
                        eprint!(
                            "[{}/{attempts}] {task_id}{} … ",
                            started,
                            if repeat > 1 { format!(" (tentativo {attempt})") } else { String::new() }
                        );
                        started += 1;
                    }
                    EvalEvent::TaskFinished { result, .. } => {
                        eprintln!(
                            "{} ({:.1} s, {} token, {} turni)",
                            if result.passed { "ok" } else { "FALLITO" },
                            result.duration_ms as f64 / 1000.0,
                            result.tokens,
                            result.turns
                        );
                        if let Some(e) = &result.error {
                            eprintln!("    errore: {e}");
                        }
                        for g in result.graders.iter().filter(|g| !g.passed) {
                            eprintln!("    ✗ {}: {}", g.kind, g.detail);
                        }
                        if let Some(dir) = &result.workdir {
                            eprintln!("    cartella: {dir}");
                        }
                    }
                })
                .await?;
            println!("\n{:<24} {:>8} {:>7} {:>7} {:>8}", "TASK", "RIUSCITI", "SCORE", "TOKEN", "SECONDI");
            for t in &suite.tasks {
                let runs: Vec<_> = report.tasks.iter().filter(|r| r.task_id == t.id).collect();
                let ok = runs.iter().filter(|r| r.passed).count();
                let n = runs.len().max(1) as f64;
                println!(
                    "{:<24} {:>5}/{:<2} {:>7.2} {:>7} {:>8.1}",
                    t.id,
                    ok,
                    runs.len(),
                    runs.iter().map(|r| r.score).sum::<f64>() / n,
                    runs.iter().map(|r| r.tokens).sum::<u64>() / runs.len().max(1) as u64,
                    runs.iter().map(|r| r.duration_ms).sum::<u64>() as f64 / 1000.0 / n
                );
            }
            let s = &report.summary;
            println!("\nTotale: {}/{} riusciti (pass rate {}), score medio {:.2}", s.passed, s.total, percent(s.pass_rate), s.score);
            println!("Report: {}", report.id);
            Ok(report)
        }
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1e9)
    } else {
        format!("{:.1} MB", bytes as f64 / 1e6)
    }
}

async fn models_cmd(engine: &Engine, cmd: ModelCmd) -> Result<()> {
    match cmd {
        ModelCmd::List => {
            for m in engine.models() {
                println!(
                    "{:<44} {:<9} {:<6} {:<8} {:>9}  {:<5} {}",
                    m.id,
                    m.origin,
                    m.format,
                    m.quant.unwrap_or_default(),
                    size(m.size_bytes),
                    m.fit,
                    m.path.unwrap_or_default()
                );
            }
        }
        ModelCmd::Catalog => {
            for c in engine.catalog() {
                println!(
                    "{:<24} {:<7} {:<6} {:>9}  {:<6} {}{}",
                    c.id,
                    c.format,
                    c.quant,
                    size(c.size_bytes),
                    c.fit,
                    c.fit_detail,
                    if c.installed { "  [installato]" } else { "" }
                );
            }
        }
        ModelCmd::Providers => {
            for p in engine.model_providers() {
                println!("{:<10} {:<22} {:<9} {:<9} {}", p.id, p.name, p.auth, if p.auth_configured { "token ok" } else { "" }, p.endpoint);
            }
        }
        ModelCmd::Search { query, provider, format, sort, limit } => {
            let page =
                engine.search_models(&provider, aiw_models::hub::SearchQuery { q: query, format, sort, limit, cursor: None }).await?;
            if page.items.is_empty() {
                println!("Nessun risultato su {provider}.");
            }
            for m in &page.items {
                println!(
                    "{:<46} {:>12} {:>8}  {:<14} {}",
                    m.id,
                    m.downloads.map(|d| format!("{d} scaric.")).unwrap_or_default(),
                    m.likes.map(|l| format!("{l} ♥")).unwrap_or_default(),
                    m.formats.join(","),
                    m.capabilities.join(",")
                );
            }
            if page.next_cursor.is_some() {
                eprintln!("(altri risultati: aumenta --limit)");
            }
        }
        ModelCmd::Show { provider, model, revision } => {
            let d = engine.remote_model(&provider, &model, revision.as_deref()).await?;
            let m = &d.model;
            println!("{} · {} · revisione {}", m.id, m.publisher, d.revision);
            if let Some(desc) = &m.description {
                println!("{desc}");
            }
            println!(
                "Formati: {}  Capacità: {}  Licenza: {}{}",
                m.formats.join(","),
                m.capabilities.join(","),
                m.license.clone().unwrap_or_else(|| "?".into()),
                if m.gated { "  (gated: serve un token)" } else { "" }
            );
            println!();
            println!("{:<34} {:<8} {:>10}  {:<6} {:<4} {:<4} RUNTIME DISPONIBILI", "VARIANTE", "QUANT", "DIMENSIONE", "FIT", "FILE", "OK");
            for v in &d.variants {
                let rts: Vec<&str> = v.runtimes.iter().filter(|r| r.available).map(|r| r.id.as_str()).collect();
                println!(
                    "{:<34} {:<8} {:>10}  {:<6} {:<4} {:<4} {}{}",
                    v.id,
                    v.quant.clone().unwrap_or_default(),
                    size(v.size_bytes),
                    v.fit,
                    v.files.len(),
                    if v.installed { "sì" } else { "" },
                    if rts.is_empty() { "nessuno (vedi `aiw runtimes`)".to_string() } else { rts.join(",") },
                    if v.recommended { "  ★ consigliata" } else { "" }
                );
            }
            if d.variants.is_empty() {
                println!("Nessuna variante scaricabile.");
            }
        }
        ModelCmd::Rm { id } => {
            engine.delete_model(&id)?;
            println!("Eliminato: {id}");
        }
        ModelCmd::Auth { provider, token, endpoint, clear } => {
            let token = if clear { Some(None) } else { token.map(Some) };
            if token.is_none() && endpoint.is_none() {
                bail!("indica --token, --endpoint o --clear");
            }
            let p = engine.set_provider_auth(&provider, token, endpoint).await?;
            println!("{} → {}{}", p.name, p.endpoint, if p.auth_configured { " (token impostato)" } else { "" });
        }
        ModelCmd::Pull { id, provider, model, variant, revision, url, file_name, sha256, repo, file } => {
            let req = match (id, provider, model, url, repo, file) {
                (Some(id), None, None, None, None, None) => PullRequest::Catalog { id },
                (None, Some(provider), Some(model), None, None, None) => {
                    PullRequest::Hub { provider, model, variant: variant.unwrap_or_default(), revision }
                }
                (None, None, None, Some(url), None, None) => PullRequest::Url { url, file_name, sha256 },
                (None, None, None, None, Some(repo), Some(file)) => PullRequest::HfFile { repo, file },
                _ => bail!("indica un id del catalogo, oppure --provider e --model (con --variant), oppure --url, oppure --repo e --file"),
            };
            let d = engine.pull_model(req).await?;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    _ = tokio::signal::ctrl_c() => {
                        engine.cancel_download(&d.id)?;
                        bail!("download annullato (il file parziale resta e si riprende col prossimo pull)");
                    }
                }
                let cur = engine.downloads().into_iter().find(|x| x.id == d.id).context("download perso")?;
                let pct = if cur.total > 0 { cur.received as f64 * 100.0 / cur.total as f64 } else { 0.0 };
                eprint!(
                    "\r[{}/{}] {}  {:.1}%  ({}/{})  {:.1} MB/s  {}   ",
                    cur.files_done.min(cur.files_total),
                    cur.files_total,
                    cur.file,
                    pct,
                    size(cur.received),
                    size(cur.total),
                    cur.speed_bps as f64 / 1e6,
                    if cur.status == "queued" { "in coda" } else { "" }
                );
                match cur.status.as_str() {
                    "done" => {
                        eprintln!("\nCompletato: {}", cur.dest);
                        break;
                    }
                    "error" => bail!("download fallito: {}", cur.error.unwrap_or_default()),
                    "cancelled" => bail!("download annullato"),
                    _ => {}
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

async fn run(engine: &Engine, agent: &str, prompt: String, yes: bool, worktree: bool) -> Result<()> {
    let session = engine.create_session_with(agent, None, worktree.then_some("worktree"))?;
    if let Some(wt) = &session.worktree {
        eprintln!("Worktree isolato: {} (branch {})", wt.path, wt.branch);
    }
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
    if session.worktree.is_some() {
        if let Ok(st) = engine.worktree_status(&session.id) {
            if st.files.is_empty() {
                eprintln!("\nNessuna modifica nel worktree.");
            } else {
                eprintln!("\nModifiche nel worktree {} (branch {}):\n{}", st.info.path, st.info.branch, st.diff_stat.trim_end());
                eprintln!("Applicale dalla UI (Chat → Applica) o con: git -C {} diff {}", st.info.path, st.info.base);
            }
        }
    }
    Ok(())
}
