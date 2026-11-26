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
    /// Controlla se Docker o Podman sono installati e attivi (sola lettura: non installa e non avvia nulla)
    Containers {
        /// Stampa il JSON invece del testo
        #[arg(long)]
        json: bool,
    },
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
        /// Adapter LoRA da applicare al modello (GGUF per llama.cpp, cartella per mlx-lm); ripetibile
        #[arg(long)]
        adapter: Vec<String>,
    },
    /// Esegue un compito con un agente e mostra la risposta in streaming
    ///
    /// Gli strumenti di gestione dell'Assistente (modelli, motori, impostazioni, connettori, segreti) funzionano solo dalla chat dell'app: da terminale restano i comandi `aiw models`, `aiw runtimes` e `aiw profile` per modelli e motori; il resto si gestisce dall'app.
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
    /// Ripristina i file di un run: rimette gli originali salvati ed elimina i file che il run ha creato
    Rewind {
        run_id: String,
        /// Mostra cosa cambierebbe senza toccare nulla
        #[arg(long)]
        dry_run: bool,
    },
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
    /// Fine-tuning locale con trainer esterni (mlx-lm, TRL)
    Train {
        #[command(subcommand)]
        cmd: TrainCmd,
    },
    /// Adapter LoRA addestrati da AIW
    Adapters {
        #[command(subcommand)]
        cmd: Option<AdaptersCmd>,
    },
    /// Lezioni proposte dagli agenti: le vedi, le approvi (vengono scritte) o le scarti
    Lessons {
        #[command(subcommand)]
        cmd: Option<LessonsCmd>,
    },
    /// Canali per parlare con l'assistente da Telegram, Slack o email
    Channels {
        #[command(subcommand)]
        cmd: Option<ChannelsCmd>,
    },
    /// Token di accesso all'API: per app di terzi e per i dispositivi accoppiati
    Tokens {
        #[command(subcommand)]
        cmd: Option<TokensCmd>,
    },
    /// Documenti locali che gli assistenti possono cercare (base di conoscenza)
    Kb {
        #[command(subcommand)]
        cmd: Option<KbCmd>,
    },
    /// Copia di sicurezza e ripristino dei dati (la cartella ~/.aiw): crea un archivio, lo controlla, lo ripristina
    Backup {
        #[command(subcommand)]
        cmd: BackupCmd,
    },
    /// Agenti tenuti separati: workspace dedicato, rete della shell chiusa, credenziali di ciascuno (docs/agenti-isolati.md)
    Agents {
        #[command(subcommand)]
        cmd: Option<AgentsCmd>,
    },
    /// Segreti cifrati sul disco: stato, ritorno in chiaro, cifratura, pulizia dei segreti che non si leggono più
    Secrets {
        #[command(subcommand)]
        cmd: SecretsCmd,
    },
    /// Esporta la diagnostica in un file JSON da allegare a una segnalazione: versione, sistema, hardware, runtime,
    /// impostazioni (segreti mascherati), schema del database, controlli di salute, ultime righe dei log.
    /// Non contiene chat, documenti o posta.
    Diagnostics {
        /// File di destinazione (di serie: stampa sul terminale)
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AgentsCmd {
    /// Elenca gli agenti con il loro isolamento e quante credenziali hanno (predefinito)
    List,
    /// Imposta l'isolamento di un agente. Senza opzioni mostra lo stato attuale. Solo il proprietario lo cambia: l'agente no
    Isolation {
        /// Id dell'agente
        id: String,
        /// `dedicated`: cartella di lavoro sua, che gli altri agenti non leggono; `shared`: il workspace condiviso, come sempre
        #[arg(long = "workspace-mode", value_parser = ["dedicated", "shared"])]
        workspace_mode: Option<String>,
        /// `closed`: la shell di questo agente non ha mai la rete; `inherit`: vale l'impostazione sandbox.shell_network
        #[arg(long, value_parser = ["closed", "inherit"])]
        shell_network: Option<String>,
    },
    /// Credenziali di un agente: valgono solo per lui, solo verso gli host indicati, e il modello non ne vede mai il valore
    Credential {
        #[command(subcommand)]
        cmd: CredentialCmd,
    },
}

#[derive(Subcommand)]
enum CredentialCmd {
    /// Elenca le credenziali di un agente: nome, host, intestazione, stato (mai il valore)
    List { agent: String },
    /// Crea o cambia una credenziale. Il valore si legge dallo standard input, mai dagli argomenti (resterebbero nella cronologia
    /// della shell e in `ps`): `printf %s "$TOKEN" | aiw agents credential set alfa github --host api.github.com`
    Set {
        agent: String,
        /// Nome con cui l'agente la chiama (lettere, cifre, - e _)
        name: String,
        /// Host verso cui può essere mandata (con i suoi sottodomini): almeno uno, ripetibile
        #[arg(long = "host", required = true)]
        hosts: Vec<String>,
        /// Intestazione della richiesta (predefinita: Authorization)
        #[arg(long)]
        header: Option<String>,
        /// Testo prima del valore (predefinito: «Bearer » per Authorization, niente per le altre)
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Toglie una credenziale: da quel momento nessuno la usa più
    Remove { agent: String, name: String },
}

#[derive(Subcommand)]
enum SecretsCmd {
    /// Mostra se i segreti sono cifrati, con quale portachiavi e quanti sono cifrati, in chiaro o illeggibili (mai i valori)
    Status,
    /// Cifra i segreti con una chiave custodita nel portachiavi del sistema (è lo stato di serie dove il portachiavi c'è).
    /// Solo con l'app chiusa e il daemon fermo
    Encrypt,
    /// Riporta i segreti in chiaro e toglie la chiave dal portachiavi (imposta security.secrets_at_rest = off).
    /// Solo con l'app chiusa e il daemon fermo
    Decrypt,
    /// Toglie i segreti cifrati che non si possono più aprire (la chiave non è nel portachiavi), così si reinseriscono; poi
    /// si crea una chiave nuova. È l'unico caso in cui un segreto cifrato viene scartato. Solo con l'app chiusa e il daemon fermo
    ForgetUnreadable {
        /// Non chiede conferma
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum BackupCmd {
    /// Crea un archivio .tar.gz dei dati con un manifesto (versioni, file, SHA-256). Funziona anche a daemon acceso.
    /// Esclusi sempre: modelli (non si includono mai), runtime, cache, log, browser, checkpoint. Esclusi di serie: chiavi
    /// e certificati TLS, token, e tutti i segreti (password IMAP, token dei canali, chiavi dei provider, header dei
    /// server MCP, chiavi API nelle impostazioni).
    Create {
        /// File di destinazione (di serie: aiw-backup-<data>.tar.gz nella cartella corrente; non si sovrascrive)
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Include anche i segreti: l'archivio diventa sensibile e va custodito come la cartella dei dati
        #[arg(long)]
        include_secrets: bool,
    },
    /// Ripristina un archivio. Solo con l'app chiusa e il daemon fermo. Prima controlla tutto (manifesto, SHA-256,
    /// percorsi, versione dello schema), poi salva una copia dei dati attuali in <dati>/backups/ e solo allora sostituisce.
    Restore {
        file: PathBuf,
        /// Non chiede conferma
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Controlla un archivio senza ripristinarlo e mostra cosa contiene
    Inspect { file: PathBuf },
}

#[derive(Subcommand)]
enum TokensCmd {
    /// Elenca i token con ultimo uso, scadenza e stato (predefinito)
    List,
    /// Crea un token; il valore si vede una volta sola
    Create {
        /// Nome (per esempio l'app che lo userà)
        name: String,
        /// gateway (app di terzi, solo /v1) o device (dispositivo accoppiato)
        #[arg(long, default_value = "gateway")]
        kind: String,
        /// Giorni di validità (di serie non scade)
        #[arg(long)]
        expires_in_days: Option<u32>,
        /// Ambiti in più per un token gateway: a2a, mcp
        #[arg(long)]
        scope: Vec<String>,
    },
    /// Revoca un token: smette di funzionare subito
    Revoke { id: String },
    /// Sostituisce un token con uno nuovo (il vecchio smette di funzionare subito)
    Rotate { id: String },
}

#[derive(Subcommand)]
enum ChannelsCmd {
    /// Elenca i canali con modalità, lista dei mittenti e conteggi (predefinito)
    List,
    /// Mostra i conteggi e il motivo dello stato di ogni canale (o di uno solo)
    Status {
        /// Nome o id del canale
        channel: Option<String>,
    },
}

#[derive(Subcommand)]
enum KbCmd {
    /// Elenca raccolte e documenti (predefinito)
    List,
    /// Aggiunge file o cartelle (testo, Markdown, codice, PDF) a una raccolta; la crea se non c'è
    Add {
        /// Nome o id della raccolta
        collection: String,
        /// File o cartelle del computer
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Cerca nei documenti
    Search {
        query: String,
        /// Solo in questa raccolta (nome o id)
        #[arg(long)]
        collection: Option<String>,
        /// Quanti passaggi mostrare (massimo 10)
        #[arg(long, short = 'n', default_value_t = 5)]
        max: usize,
    },
    /// Elimina un documento (id o nome)
    Rm { document: String },
}

#[derive(Subcommand)]
enum AdaptersCmd {
    /// Elenca gli adapter (predefinito)
    List,
    /// Elimina un adapter e i suoi file
    Rm { id: String },
}

/// What a fine-tuning needs, whatever the method.
#[derive(clap::Args)]
struct TrainArgs {
    /// Modello base: id o nome di un modello locale, oppure un repository Hugging Face
    #[arg(long)]
    base: String,
    /// Trainer da usare (default: il primo pronto che conosce il metodo)
    #[arg(long)]
    trainer: Option<String>,
    /// Dataset già creato (id); senza, ne creo uno dalle sessioni con i filtri qui sotto
    #[arg(long)]
    dataset: Option<String>,
    /// Nome del dataset creato e dell'adapter
    #[arg(long)]
    name: Option<String>,
    /// Solo le sessioni di questo agente
    #[arg(long)]
    agent: Option<String>,
    /// Solo le sessioni con attività da questa data (RFC 3339 o AAAA-MM-GG)
    #[arg(long)]
    since: Option<String>,
    /// Solo le sessioni con attività fino a questa data
    #[arg(long)]
    until: Option<String>,
    /// Al massimo questi esempi
    #[arg(long)]
    limit: Option<usize>,
    /// Da dove prendere gli esempi: feedback, succeeded, eval (ripetibile; default feedback)
    #[arg(long)]
    source: Vec<String>,
    /// Report di valutazione da cui prendere i task riusciti (ripetibile)
    #[arg(long)]
    eval_report: Vec<String>,
    /// Passi dell'ottimizzatore (ha la precedenza su --epochs)
    #[arg(long)]
    iters: Option<u32>,
    #[arg(long)]
    epochs: Option<f32>,
    /// Rango LoRA
    #[arg(long)]
    rank: Option<u32>,
    /// Learning rate
    #[arg(long)]
    lr: Option<f64>,
    #[arg(long)]
    batch: Option<u32>,
    /// Suite da eseguire, con e senza adapter, quando l'addestramento finisce (serve --eval-profile)
    #[arg(long)]
    eval: Option<String>,
    /// Profilo di inferenza da cui parte il confronto (il suo modello, con e senza adapter)
    #[arg(long)]
    eval_profile: Option<String>,
}

#[derive(Subcommand)]
enum TrainCmd {
    /// Mostra cosa si può addestrare su questo dispositivo e quali trainer sono installati
    Doctor,
    /// Installa un trainer nell'ambiente Python di AIW (~/.aiw/pyenv)
    Install { id: String },
    /// Addestra un adapter LoRA con SFT sulle conversazioni approvate (pollice in su, riuscite o task superati)
    Sft(Box<TrainArgs>),
    /// Addestra un adapter LoRA con DPO sulle coppie di preferenza (pollice in su e in giù sullo stesso messaggio, task di eval ripetuti)
    Dpo(Box<TrainArgs>),
    /// Addestra un adapter LoRA con GRPO: i prompt sono i task di una suite e la reward la danno i suoi grader (solo dispositivi di classe alta)
    Grpo(Box<GrpoArgs>),
    /// Elenca gli addestramenti
    Jobs,
    /// Mostra il log di un addestramento
    Logs {
        id: String,
        /// Quante righe (le ultime)
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    /// Interrompe un addestramento in corso
    Cancel { id: String },
    /// Elenca i dataset creati
    Datasets,
}

/// GRPO: the prompts and the rewards come from a suite.
#[derive(clap::Args)]
struct GrpoArgs {
    #[command(flatten)]
    common: TrainArgs,
    /// Suite i cui task danno i prompt e i cui grader (output_matches, json_schema, judge) danno le reward
    #[arg(long)]
    suite: String,
    /// Quante volte compare ogni prompt
    #[arg(long)]
    repeat: Option<u32>,
}

#[derive(Subcommand)]
enum LessonsCmd {
    /// Elenca le lezioni (di default quelle in attesa)
    List {
        /// pending, approved o rejected; "all" le mostra tutte
        #[arg(long, default_value = "pending")]
        status: String,
    },
    /// Approva una lezione: una memoria va in ~/.aiw/AGENTS.md, una skill in ~/.aiw/skills
    Approve {
        id: String,
        /// Nuovo titolo
        #[arg(long)]
        title: Option<String>,
        /// Nuovo contenuto
        #[arg(long)]
        content: Option<String>,
    },
    /// Scarta una lezione
    Reject { id: String },
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
    /// Prova varianti del system prompt di un agente sulla suite e propone la migliore. Il prompt dell'agente
    /// cambia solo con --apply (e una conferma)
    Optimize {
        /// File della suite oppure id di una suite salvata
        suite: String,
        /// Agente di cui ottimizzare il prompt
        #[arg(long)]
        agent: String,
        /// Quante varianti provare (da 1 a 5)
        #[arg(long, default_value_t = 3)]
        variants: u32,
        /// Profilo di inferenza dei task (e delle varianti, se `learning.profile` è vuoto)
        #[arg(long)]
        profile: Option<String>,
        /// Quante volte eseguire ogni task
        #[arg(long, default_value_t = 1)]
        repeat: u32,
        /// Applica all'agente la variante migliore, se batte quella attuale
        #[arg(long)]
        apply: bool,
        /// Con --apply: non chiede conferma (necessario fuori da un terminale)
        #[arg(long)]
        yes: bool,
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
    let filter = aiw_core::QUIET_LOG_TARGETS
        .iter()
        .fold(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,aiw=info".into()), |f, d| {
            f.add_directive(d.parse().expect("a valid log directive"))
        });
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
    let cli = Cli::parse();
    // Backup commands never start the engine: restoring must not have the database open, and inspecting needs no data.
    let cmd = match cli.cmd {
        Cmd::Backup { cmd } => return backup_cmd(cli.data.clone(), cmd),
        Cmd::Secrets { cmd } => return secrets_cmd(cli.data.clone(), cli.workspace.clone(), cmd).await,
        other => other,
    };
    let serve_mode = matches!(cmd, Cmd::Serve { .. });
    let engine = Engine::new(EngineOptions {
        data_dir: cli.data.clone(),
        workspace_root: cli.workspace.clone(),
        mode: if serve_mode { "daemon".into() } else { "embedded".into() },
        // A command that is not the server runs next to the app that may be open on the same data: the app owns the tasks.
        no_tasks: !serve_mode,
        keychain: aiw_engine::vault::Keychain::System,
        // Only what stays up (`aiw serve`) may make the key or migrate: a one-shot command can run next to the app, which holds
        // the key in memory and would write back what it has.
        secrets_read_only: !serve_mode,
        ..Default::default()
    })
    .await
    .context("avvio del motore")?;
    match cmd {
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
                match engine.tls_material() {
                    Ok(Some(m)) => eprintln!(
                        "TLS attivo ({}): impronta {} — il pairing la porta con sé; per un browser su un altro dispositivo installa il certificato da Sicurezza → Rete.",
                        engine.tls_mode().as_str(),
                        aiw_core::tlspin::display_fingerprint(&m.fingerprint)
                    ),
                    Ok(None) => eprintln!(
                        "ATTENZIONE: l'API è raggiungibile dalla rete in HTTP in chiaro (token e dati senza cifratura). Attiva la cifratura con server.tls = self_signed."
                    ),
                    Err(e) => eprintln!("ATTENZIONE: TLS non disponibile ({e}): il servizio non parte finché non lo correggi."),
                }
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
        Cmd::Backup { .. } | Cmd::Secrets { .. } => unreachable!("handled before the engine starts"),
        Cmd::Diagnostics { output } => {
            let doc = serde_json::to_string_pretty(&engine.diagnostics().await?)?;
            match output {
                Some(path) => {
                    if path.exists() {
                        bail!("{} esiste già: scegli un altro nome", path.display());
                    }
                    std::fs::write(&path, doc + "\n").with_context(|| format!("scrittura di {}", path.display()))?;
                    eprintln!(
                        "Diagnostica scritta in {}. Non contiene chat, documenti o posta; i segreti sono mascherati.",
                        path.display()
                    );
                }
                None => println!("{doc}"),
            }
        }
        Cmd::Doctor => doctor(&engine).await,
        Cmd::Containers { json } => {
            let st = engine.containers_status().await;
            if json {
                println!("{}", serde_json::to_string_pretty(&st)?);
            } else {
                for l in containers_lines(&st) {
                    println!("{l}");
                }
            }
        }
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
        Cmd::Profile { id, runtime, model, context, adapter } => {
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
                    thinking: None,
                    thinking_budget: None,
                    adapters: adapter.into_iter().map(|path| aiw_core::api::AdapterRef { path, scale: None }).collect(),
                    allow_remote: false,
                },
            })?;
            println!("Profilo {} → {} · {}", p.id, p.runtime_id, p.model);
        }
        Cmd::Traces { cmd: TracesCmd::Export { agent, since, until, status, limit, rated, out, no_redact, exclude_path } } => {
            let filter = aiw_engine::traces::TraceFilter {
                session: None,
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
        Cmd::Train { cmd } => train_cmd(&engine, cmd).await?,
        Cmd::Adapters { cmd } => match cmd.unwrap_or(AdaptersCmd::List) {
            AdaptersCmd::List => {
                for a in engine.adapters()? {
                    println!("{:<22} {:<5} {:<10} {:<28} {}", a.id, a.method, a.trainer, a.name, a.base_model);
                    println!("    {}", a.path);
                }
            }
            AdaptersCmd::Rm { id } => {
                engine.delete_adapter(&id)?;
                println!("Adapter {id} eliminato.");
            }
        },
        Cmd::Lessons { cmd } => lessons_cmd(&engine, cmd.unwrap_or(LessonsCmd::List { status: "pending".into() }))?,
        Cmd::Channels { cmd } => channels_cmd(&engine, cmd.unwrap_or(ChannelsCmd::List))?,
        Cmd::Tokens { cmd } => tokens_cmd(&engine, cmd.unwrap_or(TokensCmd::List))?,
        Cmd::Agents { cmd } => agents_cmd(&engine, cmd.unwrap_or(AgentsCmd::List))?,
        Cmd::Kb { cmd } => kb_cmd(&engine, cmd.unwrap_or(KbCmd::List)).await?,
        Cmd::Run { prompt, agent, yes, worktree } => run(&engine, &agent, prompt, yes, worktree).await?,
        Cmd::Rewind { run_id, dry_run } => {
            let info = engine.checkpoint_info(&run_id)?;
            let show = |label: &str, files: &[String]| files.iter().for_each(|f| println!("{label:<12} {f}"));
            if dry_run {
                println!("Il ripristino del run {run_id} cambierebbe:");
                show("ripristina", &info.restore);
                show("elimina", &info.delete);
                show("non toccato", &info.external);
            } else {
                let r = engine.rewind(&run_id)?;
                show("ripristinato", &r.restored);
                show("eliminato", &r.deleted);
                show("non toccato", &r.external);
                show("FALLITO", &r.failed);
                println!("Fatto: {} ripristinati, {} eliminati, {} falliti.", r.restored.len(), r.deleted.len(), r.failed.len());
                if !r.failed.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        Cmd::Workflow { id } => {
            let first = engine.run_workflow(&id, Default::default(), "user").await?;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                for a in engine.approvals() {
                    eprint!("\n[conferma] {} — consentire? [s/N] ", a.reason);
                    std::io::stderr().flush()?;
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    // The person at the terminal of this computer is the owner.
                    engine.respond_approval_as(
                        &a.id,
                        line.trim().eq_ignore_ascii_case("s") || line.trim().eq_ignore_ascii_case("y"),
                        false,
                        true,
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

/// Starts a fine-tuning (creating the dataset when none is given), follows it and registers the adapter.
async fn train_run(engine: &Engine, method: &str, a: TrainArgs, reward_suite: Option<String>, repeat: Option<u32>) -> Result<()> {
    use aiw_core::api::{DatasetSpec, TrainJobRequest, TrainParams};
    let dataset_id = match a.dataset {
        Some(id) => id,
        None => {
            let ds = engine.build_dataset(&DatasetSpec {
                name: a.name.clone(),
                method: method.into(),
                agent: a.agent,
                since: a.since,
                until: a.until,
                limit: a.limit,
                sources: a.source,
                eval_reports: a.eval_report,
                suite: reward_suite.clone(),
                repeat,
            })?;
            println!(
                "Dataset {} ({}): {} esempi ({} per l'addestramento, {} per la validazione), ~{} token",
                ds.id, ds.name, ds.examples, ds.train, ds.valid, ds.tokens_est
            );
            ds.id
        }
    };
    if reward_suite.is_some() && engine.local_url().is_none() {
        // The trainer asks this process for rewards: serve the API on a free local port while it runs.
        let (e, ui) = (engine.clone(), None);
        tokio::spawn(async move {
            if let Err(err) = aiw_server::serve(e, 0, false, ui).await {
                eprintln!("server delle reward: {err}");
            }
        });
        for _ in 0..50 {
            if engine.local_url().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    let job = engine
        .start_train_job(TrainJobRequest {
            method: method.into(),
            trainer: a.trainer,
            base_model: a.base,
            dataset_id,
            params: TrainParams { epochs: a.epochs, iters: a.iters, learning_rate: a.lr, rank: a.rank, batch_size: a.batch },
            name: a.name,
            eval_suite: a.eval,
            eval_profile: a.eval_profile,
            reward_suite,
        })
        .await?;
    println!("Addestramento {} avviato con {}. Ctrl+C lo interrompe.", job.id, job.trainer);
    let mut shown = 0usize;
    let mut cancelled = false;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(700)) => {}
            _ = tokio::signal::ctrl_c(), if !cancelled => {
                cancelled = true;
                let _ = engine.cancel_train_job(&job.id);
                eprintln!("Interruzione richiesta…");
            }
        }
        let log = engine.train_job_log(&job.id, 5000)?;
        for l in log.iter().skip(shown) {
            println!("{l}");
        }
        shown = log.len();
        let j = engine.train_job(&job.id)?;
        match j.status.as_str() {
            "queued" | "running" => {}
            "succeeded" => {
                if let Some(id) = &j.adapter_id {
                    let ad = engine.adapter(id)?;
                    println!("\nAdapter {} pronto: {}", ad.id, ad.path);
                    println!("Usalo con: aiw profile --adapter {}", ad.path);
                }
                if let Some(m) = &j.metrics {
                    println!("Metriche: {m}");
                }
                break;
            }
            "cancelled" => bail!("addestramento interrotto"),
            _ => bail!("{}", j.error.unwrap_or_else(|| "addestramento fallito".into())),
        }
    }
    Ok(())
}

async fn train_cmd(engine: &Engine, cmd: TrainCmd) -> Result<()> {
    match cmd {
        TrainCmd::Sft(a) => train_run(engine, "sft", *a, None, None).await?,
        TrainCmd::Dpo(a) => train_run(engine, "dpo", *a, None, None).await?,
        TrainCmd::Grpo(a) => {
            let GrpoArgs { common, suite, repeat } = *a;
            train_run(engine, "grpo", common, Some(suite), repeat).await?
        }
        TrainCmd::Jobs => {
            for j in engine.train_jobs()? {
                let pct = j.progress.map(|p| format!("{:.0}%", p * 100.0)).unwrap_or_default();
                println!("{:<22} {:<4} {:<10} {:<9} {:>4}  {}", j.id, j.method, j.trainer, j.status, pct, j.base_model);
            }
        }
        TrainCmd::Logs { id, lines } => {
            for l in engine.train_job_log(&id, lines)? {
                println!("{l}");
            }
        }
        TrainCmd::Cancel { id } => {
            engine.cancel_train_job(&id)?;
            println!("Interruzione richiesta per {id}.");
        }
        TrainCmd::Datasets => {
            for d in engine.datasets()? {
                println!("{:<22} {:<5} {:>5} esempi  {:<24} {}", d.id, d.method, d.examples, d.name, d.created_at);
            }
        }
        TrainCmd::Doctor => {
            let d = engine.train_doctor().await;
            let dev = &d.device;
            println!("Dispositivo: {} · {} {} · classe {}", dev.name, dev.os, dev.arch, dev.tier.to_uppercase());
            let mem = if dev.unified_memory { "memoria unificata" } else { "RAM" };
            print!("Memoria: {:.1} GB di {mem}", dev.memory_mb as f64 / 1024.0);
            if dev.vram_mb > 0 && !dev.unified_memory {
                print!(" · VRAM {:.1} GB", dev.vram_mb as f64 / 1024.0);
            }
            println!(" · acceleratori: {}", dev.accelerators.join(", "));
            println!("\nTrainer:");
            for t in &d.trainers {
                println!("  {:<12} {:<12} {}", t.id, t.state, t.reason.clone().unwrap_or_default());
                for m in &t.methods {
                    match (&m.max_params_b, &m.blocked) {
                        (Some(b), _) => println!("      {:<5} fino a ~{b}B", m.method),
                        (None, Some(why)) => println!("      {:<5} no: {why}", m.method),
                        _ => {}
                    }
                }
            }
            if d.can_train.is_empty() {
                println!("\nQui non si può addestrare.");
            } else {
                println!("\nSi può addestrare:");
                for c in &d.can_train {
                    println!("  · {c}");
                }
            }
            for r in &d.remote {
                println!("  · {r}");
            }
            for a in &d.advice {
                println!("\n{a}");
            }
        }
        TrainCmd::Install { id } => {
            engine.trainer_install_start(&id).await?;
            let mut shown = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let Some(job) = engine.trainer_install_status(&id) else { bail!("installazione non avviata") };
                for l in job.log.iter().skip(shown) {
                    println!("{l}");
                }
                shown = job.log.len();
                match job.state {
                    aiw_runtimes::install::InstallState::Running => {}
                    aiw_runtimes::install::InstallState::Done => break,
                    aiw_runtimes::install::InstallState::Failed => bail!("{}", job.error.unwrap_or_else(|| "installazione fallita".into())),
                }
            }
        }
    }
    Ok(())
}

fn percent(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn lessons_cmd(engine: &Engine, cmd: LessonsCmd) -> Result<()> {
    match cmd {
        LessonsCmd::List { status } => {
            let lessons = engine.lessons((status != "all").then_some(status.as_str()))?;
            if lessons.is_empty() {
                println!("Nessuna lezione.");
            }
            for l in lessons {
                println!("{:<24} {:<9} {:<7} {}  ({})", l.id, l.status, l.kind, l.title, l.agent_id);
                if l.status == "pending" {
                    println!("    {}", l.reason);
                }
            }
        }
        LessonsCmd::Approve { id, title, content } => {
            let l = engine.approve_lesson(&id, title, content)?;
            println!("Lezione «{}» approvata e scritta ({}).", l.title, if l.kind == "skill" { "nuova skill" } else { "AGENTS.md" });
        }
        LessonsCmd::Reject { id } => {
            let l = engine.reject_lesson(&id)?;
            println!("Lezione «{}» scartata.", l.title);
        }
    }
    Ok(())
}

fn tokens_cmd(engine: &Engine, cmd: TokensCmd) -> Result<()> {
    match cmd {
        TokensCmd::List => {
            for t in engine.api_tokens() {
                let state = if t.revoked_at.is_some() { "revocato" } else { "attivo" };
                println!(
                    "{:<22} {:<8} {:<9} {:<16} ultimo uso: {}{}  {}",
                    t.id,
                    t.kind,
                    state,
                    t.prefix,
                    t.last_used_at.as_deref().unwrap_or("mai"),
                    t.expires_at.as_deref().map(|e| format!("  scade: {e}")).unwrap_or_default(),
                    t.name
                );
            }
        }
        TokensCmd::Create { name, kind, expires_in_days, scope } => {
            let t = engine.create_api_token(&name, &kind, None, expires_in_days, Some(&scope))?;
            println!("{}", t.token);
            eprintln!("Token «{}» creato ({}). Copialo ora: non verrà mostrato di nuovo.", t.info.name, t.info.id);
        }
        TokensCmd::Revoke { id } => {
            engine.revoke_api_token(&id)?;
            println!("Token {id} revocato: da ora le nuove richieste con quel token sono rifiutate. I flussi già aperti si chiudono solo revocando dall'app o dall'API del servizio, oppure al suo riavvio.");
        }
        TokensCmd::Rotate { id } => {
            let t = engine.rotate_api_token(&id)?;
            println!("{}", t.token);
            eprintln!("Il token {id} è stato sostituito da {}: copia il nuovo valore ora, non verrà mostrato di nuovo.", t.info.id);
        }
    }
    Ok(())
}

fn agents_cmd(engine: &Engine, cmd: AgentsCmd) -> Result<()> {
    use aiw_core::api::{AgentCredentialInput, AgentIsolation};
    match cmd {
        AgentsCmd::List => {
            for a in engine.agents()? {
                let iso = a.isolation.as_ref();
                let credentials = engine.agent_credentials(&a.id).map(|c| c.len()).unwrap_or(0);
                println!(
                    "{:<18} {:<10} workspace: {:<10} shell: {:<8} credenziali: {credentials}  {}",
                    a.id,
                    a.sandbox,
                    if iso.is_some_and(|i| i.dedicated_workspace) { "dedicato" } else { "condiviso" },
                    if iso.is_some_and(|i| i.close_shell_network) { "senza rete" } else { "da impostazioni" },
                    a.name
                );
            }
        }
        AgentsCmd::Isolation { id, workspace_mode, shell_network } => {
            let mut agent = engine.agent(&id)?;
            if workspace_mode.is_some() || shell_network.is_some() {
                let mut iso = agent.isolation.clone().unwrap_or_default();
                match workspace_mode.as_deref() {
                    Some("dedicated") => iso.dedicated_workspace = true,
                    Some("shared") => iso.dedicated_workspace = false,
                    _ => {}
                }
                match shell_network.as_deref() {
                    Some("closed") => iso.close_shell_network = true,
                    Some("inherit") => iso.close_shell_network = false,
                    _ => {}
                }
                // Both flags off is how the owner says «no isolation»: the engine stores it as absent.
                agent.isolation = Some(iso);
                agent = engine.save_agent(agent)?;
            }
            let iso = agent.isolation.clone().unwrap_or(AgentIsolation::default());
            for w in engine.isolation_warnings(&agent) {
                eprintln!("ATTENZIONE: {w}");
            }
            println!(
                "{} ({}): workspace {}, rete della shell {}",
                agent.name,
                agent.id,
                if iso.dedicated_workspace { "dedicato" } else { "condiviso" },
                if iso.close_shell_network { "sempre chiusa" } else { "come da impostazioni" }
            );
            if iso.dedicated_workspace {
                println!("Cartella: {}", engine.data_dirs().agents().join(&agent.id).join("workspace").display());
            }
        }
        AgentsCmd::Credential { cmd } => match cmd {
            CredentialCmd::List { agent } => {
                let list = engine.agent_credentials(&agent)?;
                if list.is_empty() {
                    println!("L'agente {agent} non ha credenziali.");
                }
                for c in list {
                    println!(
                        "{:<20} host: {:<40} {}{}  {}",
                        c.name,
                        c.hosts.join(", "),
                        c.header,
                        if c.prefix.is_empty() { String::new() } else { format!(" ({}…)", c.prefix.trim_end()) },
                        if c.set { "valore salvato" } else { "SENZA VALORE leggibile" }
                    );
                }
            }
            CredentialCmd::Set { agent, name, hosts, header, prefix } => {
                use std::io::{IsTerminal, Read};
                // The value comes from a pipe: an argument would stay in the history of the shell and in `ps`.
                let value = if std::io::stdin().is_terminal() {
                    None
                } else {
                    let mut raw = String::new();
                    std::io::stdin().read_to_string(&mut raw).context("lettura del valore dallo standard input")?;
                    Some(raw.trim().to_string()).filter(|v| !v.is_empty())
                };
                if value.is_none() {
                    eprintln!("Nessun valore sullo standard input: se la credenziale esiste ne tengo il valore, altrimenti serve (`printf %s \"$TOKEN\" | aiw agents credential set …`).");
                }
                let info = engine.set_agent_credential(&agent, &name, AgentCredentialInput { hosts, header, prefix, value })?;
                println!(
                    "Credenziale «{}» dell'agente {agent}: host {}, intestazione {}. Il valore non viene mai mostrato.",
                    info.name,
                    info.hosts.join(", "),
                    info.header
                );
            }
            CredentialCmd::Remove { agent, name } => {
                engine.remove_agent_credential(&agent, &name)?;
                println!("Credenziale «{name}» dell'agente {agent} tolta: da ora nessuno la usa.");
            }
        },
    }
    Ok(())
}

fn channels_cmd(engine: &Engine, cmd: ChannelsCmd) -> Result<()> {
    let list = engine.channels()?;
    if list.is_empty() {
        println!("Nessun canale. Li configuri dall'app (Canali) o con l'API /api/channels; la guida è in docs/channels.md.");
        return Ok(());
    }
    let mode = |m: &str| if m == "ask" { "chiede conferma" } else { "sola lettura" };
    match cmd {
        ChannelsCmd::List => {
            for c in &list {
                let ch = &c.channel;
                println!(
                    "{:<26} {:<9} {:<18} {:<8} {:>2} mittenti  {}",
                    ch.id,
                    ch.kind,
                    mode(&ch.mode),
                    if ch.enabled { "acceso" } else { "spento" },
                    ch.allowed.len(),
                    ch.name
                );
            }
        }
        ChannelsCmd::Status { channel } => {
            let wanted = channel.map(|c| c.to_lowercase());
            let shown: Vec<_> = list
                .iter()
                .filter(|c| wanted.as_ref().is_none_or(|w| c.channel.id.to_lowercase() == *w || c.channel.name.to_lowercase() == *w))
                .collect();
            if shown.is_empty() {
                anyhow::bail!("canale non trovato");
            }
            for c in shown {
                let (ch, st) = (&c.channel, &c.status);
                println!("{} ({}, {})", ch.name, ch.kind, ch.id);
                println!("  stato: {}", if ch.enabled { "acceso nelle impostazioni" } else { "spento" });
                println!("  messaggi ignorati da sconosciuti: {}", st.ignored);
                println!("  messaggi passati all'assistente: {}", st.handled);
                println!("  conversazioni: {}", st.conversations);
            }
            println!("\nI canali si collegano solo mentre l'app o `aiw serve` è in esecuzione: lo stato dal vivo (connesso o errore) lo vedi nell'app.");
        }
    }
    Ok(())
}

async fn kb_cmd(engine: &Engine, cmd: KbCmd) -> Result<()> {
    use aiw_core::api::{KbAddBody, KbCollectionCreate};
    match cmd {
        KbCmd::List => {
            let cols = engine.kb_collections()?;
            if cols.is_empty() {
                println!("Nessuna raccolta. Aggiungi dei documenti con: aiw kb add <raccolta> <percorso>");
            }
            for c in cols {
                println!("{:<24} {:>3} documenti  {:>5} passaggi  {}", c.id, c.documents, c.chunks, c.name);
                for d in engine.kb_documents(&c.id)? {
                    let note = d.error.as_deref().map(|e| format!("  ({e})")).unwrap_or_default();
                    println!("    {:<24} {:<9} {:>4} passaggi  {}{note}", d.id, d.status, d.chunks, d.name);
                }
            }
            match engine.embedding_model_name() {
                Some(m) => println!("Ricerca anche per significato con il modello {m}."),
                None => println!("Ricerca per parole chiave: per trovare anche per significato serve un modello di embedding acceso."),
            }
        }
        KbCmd::Add { collection, paths } => {
            let col = match engine.kb_collection(&collection) {
                Ok(c) => c,
                Err(aiw_core::Error::NotFound(_)) => {
                    let c = engine.kb_create_collection(KbCollectionCreate { name: collection.clone(), description: None })?;
                    println!("Creata la raccolta «{}».", c.name);
                    c
                }
                Err(e) => return Err(e.into()),
            };
            for p in paths {
                let path = std::fs::canonicalize(&p).with_context(|| format!("il percorso {} non esiste", p.display()))?;
                let job =
                    engine.kb_add(&col.id, KbAddBody { files: None, path: Some(path.to_string_lossy().into_owned()) }, true).await?.job;
                let mut shown = usize::MAX;
                let done = loop {
                    let j = engine.kb_job(&job.id).unwrap_or_else(|| job.clone());
                    if j.done != shown && j.total > 0 {
                        shown = j.done;
                        eprint!("\r  {} / {}  {:<60}", j.done, j.total, j.current.as_deref().unwrap_or(""));
                    }
                    if j.state != "running" {
                        break j;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                };
                eprintln!();
                println!(
                    "{}: aggiunti {}, già presenti {}, sostituiti {}, falliti {}, esclusi {}.",
                    path.display(),
                    done.added,
                    done.unchanged,
                    done.replaced,
                    done.failed,
                    done.skipped_total
                );
                for s in &done.skipped {
                    println!("    escluso: {} ({})", s.name, s.reason);
                }
                if done.skipped_total > done.skipped.len() {
                    println!("    … e altri {} file esclusi.", done.skipped_total - done.skipped.len());
                }
            }
            engine.kb_wait_idle().await;
            if engine.embedding_model_name().is_some() {
                let n = engine.kb_embed_now().await?;
                if n > 0 {
                    println!("Calcolati {n} vettori per la ricerca per significato.");
                }
            } else {
                println!("Nessun modello di embedding acceso: la ricerca funziona per parole chiave.");
            }
        }
        KbCmd::Search { query, collection, max } => {
            let r = engine.kb_search(&query, collection.as_deref(), max).await?;
            if r.hits.is_empty() {
                println!("Nessun passaggio trovato.");
            }
            for (i, h) in r.hits.iter().enumerate() {
                println!("[{}] {} · parte {}  ({})", i + 1, h.document_name, h.part, h.collection_name);
                for line in h.text.lines().filter(|l| !l.trim().is_empty()).take(6) {
                    println!("    {}", line.trim());
                }
            }
            if !r.semantic {
                println!("(Ricerca per parole chiave: nessun modello di embedding acceso.)");
            }
        }
        KbCmd::Rm { document } => {
            let d = engine.kb_document(&document)?;
            engine.kb_delete_document(&d.id)?;
            println!("Documento «{}» eliminato.", d.name);
        }
    }
    Ok(())
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
        EvalCmd::Optimize { suite, agent, variants, profile, repeat, apply, yes } => {
            use std::io::IsTerminal;
            if apply && !yes && !std::io::stdin().is_terminal() {
                anyhow::bail!("--apply fuori da un terminale richiede anche --yes");
            }
            eprintln!("Ottimizzo il prompt di «{agent}» con {variants} varianti: la suite gira {} volte in tutto…", variants + 1);
            let opts = aiw_engine::optimize::OptimizeOptions { variants: Some(variants), profile, repeat: Some(repeat) };
            let o = engine.optimize_prompt(&suite, &agent, &opts).await?;
            println!("{:<12} {:>7} {:>10}  REPORT", "", "SCORE", "PASS RATE");
            println!(
                "{:<12} {:>7.2} {:>10}  {}",
                "attuale",
                o.baseline_score,
                percent(o.baseline_pass_rate),
                o.baseline_report_id.as_deref().unwrap_or("-")
            );
            for (i, v) in o.variants.iter().enumerate() {
                println!(
                    "{:<12} {:>7.2} {:>10}  {}  {}",
                    format!("variante {}{}", i + 1, if o.best == Some(i) { " *" } else { "" }),
                    v.score,
                    percent(v.pass_rate),
                    v.report_id.as_deref().unwrap_or("-"),
                    v.rationale
                );
            }
            let Some(best) = o.best else {
                println!("\nNessuna variante batte il prompt attuale: non c'è nulla da applicare.");
                return Ok(());
            };
            println!("\nLa migliore è la variante {} (*). Differenze rispetto al prompt attuale:\n{}", best + 1, o.variants[best].diff);
            if !apply {
                println!("Per applicarla: aiw eval optimize … --apply, oppure dalla pagina Valutazioni (ottimizzazione {}).", o.id);
                return Ok(());
            }
            if !yes {
                eprint!("Applicare la variante {} al prompt dell'agente «{agent}»? [s/N] ", best + 1);
                std::io::stderr().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !matches!(line.trim().to_lowercase().as_str(), "s" | "si" | "sì" | "y" | "yes") {
                    println!("Prompt non modificato.");
                    return Ok(());
                }
            }
            engine.apply_prompt_optimization(&o.id, best)?;
            println!("Prompt di «{agent}» aggiornato.");
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

fn backup_cmd(data: Option<PathBuf>, cmd: BackupCmd) -> Result<()> {
    use aiw_engine::backup::{self, BackupManifest, BackupOptions};
    let dirs = data.map(aiw_core::paths::DataDirs::at).unwrap_or_else(aiw_core::paths::DataDirs::resolve);
    let describe = |m: &BackupManifest| {
        println!("  Creato il {} da AI Workspace Local {} ({}, {})", m.created_at, m.app_version, m.os, m.arch);
        println!("  Schema del database: {} · {} file · {}", m.schema_version, m.files.len(), size(m.total_bytes()));
        if m.sensitive {
            println!("  SENSIBILE: contiene segreti (chiavi, token, certificati TLS). Conservalo come la cartella dei dati.");
        }
        for e in &m.excluded {
            println!("  Escluso: {e}");
        }
        for s in &m.skipped {
            println!("  Non salvato: {s}");
        }
        for n in &m.notes {
            println!("  Nota: {n}");
        }
    };
    match cmd {
        BackupCmd::Create { output, include_secrets } => {
            let out = output.unwrap_or_else(|| {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                PathBuf::from(format!("aiw-backup-{stamp}{}.tar.gz", if include_secrets { "-con-segreti" } else { "" }))
            });
            // What this computer keeps sealed is opened with the key of the keychain: with secrets the archive is portable (they go
            // in plain), without them the scrubbing sees an address with a password before it is sealed.
            let codec: Option<std::sync::Arc<dyn aiw_core::sealed::SecretCodec>> =
                Some(aiw_engine::vault::Vault::reader(&dirs, aiw_engine::vault::os_keystore()) as _);
            let m = backup::create(&dirs, &BackupOptions { include_secrets, codec }, &out)?;
            println!("Backup creato: {} ({})", out.display(), size(std::fs::metadata(&out)?.len()));
            describe(&m);
            if include_secrets {
                eprintln!("ATTENZIONE: l'archivio contiene i segreti in chiaro (anche quelli che su questo computer sono cifrati: così si ripristina su qualunque computer). Non caricarlo su servizi condivisi e non allegarlo a una segnalazione.");
            } else {
                eprintln!("I segreti non sono inclusi: dopo un ripristino vanno inseriti di nuovo (chiavi dei provider, password IMAP, token dei canali e dei server MCP).");
            }
        }
        BackupCmd::Inspect { file } => {
            let m = backup::inspect(&file)?;
            println!("{}: archivio valido (manifesto, percorsi, dimensioni e SHA-256 di ogni file controllati)", file.display());
            describe(&m);
            if m.schema_version > aiw_engine::store::SCHEMA_VERSION {
                println!(
                    "  NON ripristinabile qui: lo schema {} è più recente di quello che questa versione conosce ({}).",
                    m.schema_version,
                    aiw_engine::store::SCHEMA_VERSION
                );
            }
        }
        BackupCmd::Restore { file, yes } => {
            if backup::data_in_use(&dirs)? {
                bail!(
                    "i dati in {} sono in uso: chiudi l'app e ferma il daemon (`aiw serve`), poi riprova. Il ripristino non gira a programma acceso",
                    dirs.root.display()
                );
            }
            let m = backup::inspect(&file)?;
            println!("Ripristino di {} in {}", file.display(), dirs.root.display());
            describe(&m);
            if !yes {
                eprint!(
                    "Sostituire i dati con quelli del backup? Prima salvo una copia di quelli attuali in {}. [s/N] ",
                    dirs.root.join("backups").display()
                );
                std::io::stderr().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !(line.trim().eq_ignore_ascii_case("s") || line.trim().eq_ignore_ascii_case("y")) {
                    bail!("annullato: non ho toccato nulla");
                }
            }
            let r = backup::restore(&dirs, &file)?;
            match &r.safety_copy {
                Some(p) => println!(
                    "Copia di sicurezza dei dati di prima (con i segreti, cifrati con la chiave di questo computer dove c'è un portachiavi; trattala come sensibile): {}",
                    p.display()
                ),
                None => println!("Non c'erano dati da salvare prima."),
            }
            if r.schema_from != r.schema_to {
                println!("Database migrato dallo schema {} al {}.", r.schema_from, r.schema_to);
            }
            println!("Ripristinati {} file. Riavvia l'app.", r.restored.len());
            if !r.manifest.sensitive {
                println!("Il backup non conteneva i segreti: reinserisci chiavi dei provider, password IMAP, token dei canali e dei server MCP, e accoppia di nuovo i dispositivi.");
            } else {
                println!("I segreti del backup sono tornati com'erano nell'archivio (in chiaro): al prossimo avvio si cifrano con la chiave di questo computer, se c'è un portachiavi (controlla con `aiw secrets status`).");
            }
        }
    }
    Ok(())
}

/// What `aiw secrets status` and `aiw doctor` say about the secrets at rest: the state in words, then the counts.
fn secrets_lines(s: &aiw_core::api::SecretsAtRest) -> Vec<String> {
    let state = match s.state.as_str() {
        "active" => format!("cifrati con una chiave custodita nel portachiavi ({})", s.backend.as_deref().unwrap_or("del sistema")),
        "off" => "NON cifrati: la cifratura è spenta (security.secrets_at_rest = off)".to_string(),
        "unavailable" => format!(
            "NON cifrati: il portachiavi non è utilizzabile ora ({}). Restano in chiaro; non si perde nulla e al prossimo avvio con il portachiavi si cifrano",
            s.detail.as_deref().unwrap_or("motivo sconosciuto")
        ),
        "key_lost" => "ATTENZIONE: la chiave non è più nel portachiavi: i segreti cifrati non si leggono. Reinseriscili dopo `aiw secrets forget-unreadable`".to_string(),
        _ => format!("NON cifrati: {}", s.detail.as_deref().unwrap_or("questa copia di AIW non usa il portachiavi")),
    };
    vec![
        format!("Segreti: {state}"),
        format!("  valori cifrati {} · in chiaro {} · illeggibili {}", s.encrypted, s.plain, s.unreadable),
        "  La cifratura protegge da una copia del disco o della cartella dei dati; non da un programma che gira con il tuo stesso utente."
            .to_string(),
    ]
}

/// `aiw secrets encrypt|decrypt` write the global layer; a value set in the device layer wins over it, and the command would
/// then report a success that changed nothing.
fn warn_if_overridden(engine: &Engine, wanted: &str) {
    let now = engine.config().effective.get("security.secrets_at_rest").and_then(|v| v.as_str().map(String::from));
    if now.as_deref().unwrap_or("auto") != wanted {
        eprintln!(
            "ATTENZIONE: l'impostazione security.secrets_at_rest vale «{}» perché la decide un altro livello (dispositivo o cartella di lavoro): cambiala lì, il comando non ha effetto.",
            now.unwrap_or_default()
        );
    }
}

/// `aiw secrets`: these never start the whole app (no network, no background loops) and the ones that change something refuse to
/// run while another process has the data open: that process holds the secrets in memory and would write them back.
async fn secrets_cmd(data: Option<PathBuf>, workspace: Option<PathBuf>, cmd: SecretsCmd) -> Result<()> {
    let dirs = data.clone().map(aiw_core::paths::DataDirs::at).unwrap_or_else(aiw_core::paths::DataDirs::resolve);
    let changes = !matches!(cmd, SecretsCmd::Status);
    if changes && aiw_engine::backup::data_in_use(&dirs)? {
        bail!("i dati in {} sono in uso: chiudi l'app e ferma il daemon (`aiw serve`), poi riprova", dirs.root.display());
    }
    let engine = Engine::new(EngineOptions {
        data_dir: data,
        workspace_root: workspace,
        mode: "embedded".into(),
        offline: true,
        keychain: aiw_engine::vault::Keychain::System,
        // `status` only reads; the others are the explicit requests that may change the vault (and refused above if the data are in use).
        secrets_read_only: !changes,
        ..Default::default()
    })
    .await
    .context("avvio del motore")?;
    let show = |engine: &Engine| {
        for l in secrets_lines(&engine.secrets_at_rest()) {
            println!("{l}");
        }
    };
    match cmd {
        SecretsCmd::Status => show(&engine),
        SecretsCmd::Encrypt => {
            engine.set_config("global", "security.secrets_at_rest", serde_json::json!("auto"))?;
            warn_if_overridden(&engine, "auto");
            let m = engine.secrets_reconcile();
            println!("Cifrati {} documenti e {} impostazioni.", m.sealed_docs, m.sealed_settings);
            show(&engine);
        }
        SecretsCmd::Decrypt => {
            engine.set_config("global", "security.secrets_at_rest", serde_json::json!("off"))?;
            warn_if_overridden(&engine, "off");
            let m = engine.secrets_reconcile();
            println!("Riportati in chiaro {} documenti e {} impostazioni.", m.unsealed_docs, m.unsealed_settings);
            show(&engine);
        }
        SecretsCmd::ForgetUnreadable { yes } => {
            let before = engine.secrets_at_rest();
            if before.unreadable == 0 {
                println!("Nessun segreto illeggibile: non c'è nulla da togliere.");
                show(&engine);
                return Ok(());
            }
            if !yes {
                eprint!("Tolgo {} segreti cifrati che non si possono più aprire (vanno reinseriti a mano). [s/N] ", before.unreadable);
                std::io::stderr().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !(line.trim().eq_ignore_ascii_case("s") || line.trim().eq_ignore_ascii_case("y")) {
                    bail!("annullato: non ho toccato nulla");
                }
            }
            let n = engine.secrets_forget_unreadable().map_err(|e| anyhow::anyhow!(e))?;
            println!("Tolti {n} segreti illeggibili: reinseriscili dall'app.");
            show(&engine);
        }
    }
    Ok(())
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
    println!();
    for l in secrets_lines(&engine.secrets_at_rest()) {
        println!("{l}");
    }
    println!("\nModelli locali: {}", engine.models().len());
    println!("Workspace: {}", engine.workspace_root().display());
}

/// The text of `aiw containers`: one line per engine and the next step in words.
fn containers_lines(st: &aiw_core::api::ContainerStatus) -> Vec<String> {
    let mut out = vec![];
    for e in &st.engines {
        let state = match (e.installed, e.running) {
            (false, _) => "non installato".to_string(),
            (true, false) => "installato, non attivo".to_string(),
            (true, true) => format!(
                "attivo{}{}",
                if e.compose { ", con compose" } else { ", senza compose" },
                match (e.cpus, e.memory_mb) {
                    (Some(c), Some(m)) => format!(" · {c} CPU · {:.1} GB", m as f64 / 1024.0),
                    _ => String::new(),
                }
            ),
        };
        out.push(format!("{:<7} {}{}", e.engine, e.version.as_deref().map(|v| format!("{v} · ")).unwrap_or_default(), state));
    }
    out.push(match st.hint.as_str() {
        "none" if st.state == "ready" => format!("Pronto: si usa {}.", st.engine.as_deref().unwrap_or("?")),
        "none" => "Nessun contenitore su questo dispositivo.".into(),
        "start_docker" => "Avvia Docker Desktop (o la macchina di Podman) e riprova.".into(),
        "install_compose" => "Manca «compose»: installa il plugin Docker Compose.".into(),
        _ => "Docker non c'è: installa Docker Desktop (macOS/Windows) o Docker Engine/Podman (Linux).".into(),
    });
    out
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
                    engine.respond_approval_as(&id, allow, false, true)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// A flag of a subcommand must not collide with a global one (`--workspace` is global: the first version of
    /// `aiw agents isolation` reused the name and clap panicked when the command was run).
    #[test]
    fn the_agents_commands_parse_next_to_the_global_options() {
        Cli::command().debug_assert();
        let parsed = Cli::try_parse_from([
            "aiw",
            "--workspace",
            "/tmp/ws",
            "agents",
            "isolation",
            "alfa",
            "--workspace-mode",
            "dedicated",
            "--shell-network",
            "closed",
        ])
        .unwrap();
        assert_eq!(parsed.workspace.as_deref(), Some(std::path::Path::new("/tmp/ws")));
        match parsed.cmd {
            Cmd::Agents { cmd: Some(AgentsCmd::Isolation { id, workspace_mode, shell_network }) } => {
                assert_eq!((id.as_str(), workspace_mode.as_deref(), shell_network.as_deref()), ("alfa", Some("dedicated"), Some("closed")));
            }
            _ => panic!("not the isolation command"),
        }
        let set = Cli::try_parse_from([
            "aiw",
            "agents",
            "credential",
            "set",
            "alfa",
            "github",
            "--host",
            "api.example.org",
            "--host",
            "git.example.org",
        ])
        .unwrap();
        match set.cmd {
            Cmd::Agents { cmd: Some(AgentsCmd::Credential { cmd: CredentialCmd::Set { hosts, header, prefix, .. } }) } => {
                assert_eq!(hosts, ["api.example.org", "git.example.org"]);
                assert!(header.is_none() && prefix.is_none());
            }
            _ => panic!("not the credential command"),
        }
        // The value is never an argument: there is no option for it.
        assert!(Cli::try_parse_from(["aiw", "agents", "credential", "set", "alfa", "github", "--host", "a.example.org", "--value", "x"])
            .is_err());
        // A host is required.
        assert!(Cli::try_parse_from(["aiw", "agents", "credential", "set", "alfa", "github"]).is_err());
        assert!(Cli::try_parse_from(["aiw", "agents", "isolation", "alfa", "--workspace-mode", "boh"]).is_err());
    }
}
