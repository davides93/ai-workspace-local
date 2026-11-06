# Architettura · AI Workspace Local (AIW)

> Versione 0.1 · 2026-11-01 · Input: [ricerca REA](../research/01-reverse-engineering.md) (pattern P01–P24) e [panorama di frontiera](../research/02-frontier-landscape.md)

## 1. Obiettivi e vincoli

1. **Locale prima di tutto**: tutto funziona offline. Il cloud non serve e non si contatta mai
   senza un consenso esplicito.
2. **Ovunque**: Windows, macOS, Linux, Android, iOS (e browser come client remoto) con lo stesso
   core e la stessa UI.
3. **Tutti i metodi di inferenza**: embedded, sidecar, modelli di sistema, endpoint LAN, sotto
   un'unica API.
4. **Ogni layer personalizzabile dall'app**: ogni componente (runtime, modello, tool, agente,
   workflow, policy) è descritto da un *manifest* con uno *schema di impostazioni*; la UI genera i
   form dagli schemi e la configurazione è a strati, con override ispezionabili.
5. **Agentico oltre il coding**: file, shell, browser, computer-use, MCP, workflow schedulati, subagenti.
6. **Sicuro di default**: policy esplicita, sandbox del sistema operativo, audit trail e kill switch.

## 2. Vista a layer

```mermaid
flowchart TB
  subgraph Clients["L7 · Client"]
    UIdesk["App desktop (Tauri)"]:::c
    UImob["App mobile (Tauri iOS/Android)"]:::c
    Web["Browser in LAN"]:::c
    CLI["CLI aiw"]:::c
    Ext["App esterne: Claude Code, Codex, OpenCode, CopilotKit…"]:::c
  end
  subgraph API["L6 · Interfacce"]
    IPC["IPC Tauri"]
    HTTP["REST + SSE (eventi AG-UI)"]
    GW["Gateway compatibile OpenAI /v1"]
    MCPS["Server MCP e A2A"]
  end
  subgraph Agentic["L5 · Layer agentico"]
    WF["Motore workflow: trigger, step, approvazioni"]
    AG["Agent loop: subagenti, hook, compattazione, skill, memoria"]
    POL["Policy engine + audit + kill switch"]
  end
  subgraph Tools["L4 · Tool"]
    FS["fs / edit / search"]
    SH["shell (sandbox)"]
    BR["browser (CDP)"]
    CU["computer-use"]
    WEB["web_fetch"]
    MCPC["client MCP"]
  end
  subgraph Infer["L3 · Inferenza unificata"]
    ROUTER["Router: profilo → runtime → modello"]
    REG["Runtime Registry (manifest)"]
    SUP["Supervisor dei sidecar"]
  end
  subgraph Runtimes["L2 · Runtime"]
    EMB["Embedded: llama.cpp, ORT GenAI, MLX, LiteRT-LM, ExecuTorch"]
    SIDE["Sidecar: llama-server, Ollama, vLLM, SGLang, mlx-lm, Foundry Local, Lemonade…"]
    SYS["Di sistema: Apple Foundation Models, Gemini Nano, Phi Silica"]
    LAN["Remoti LAN: altri device AIW, RPC llama.cpp, exo"]
  end
  subgraph HW["L1 · Hardware e sistema operativo"]
    PROBE["Probe: CPU/RAM/GPU/NPU/OS → DeviceProfile"]
    MODELS["Model store: GGUF, MLX, ONNX, .litertlm, .pte"]
  end
  Clients --> API --> Agentic --> Tools
  Agentic --> Infer --> Runtimes --> HW
  POL -. vaglia .-> Tools
  classDef c fill:#eef,stroke:#88a
```

| Layer | Crate / package | Responsabilità | Pattern |
|---|---|---|---|
| L1 | `aiw-hw`, `aiw-models` | Rilevamento hardware, catalogo e store dei modelli, parser GGUF, download con ripresa | — |
| L2–L3 | `aiw-runtimes` | Manifest dei runtime, rilevamento disponibilità, adapter, supervisor, router | P20 |
| L4 | `aiw-tools` | Tool builtin, client MCP, browser CDP, computer-use | P12, P14, P18, P19 |
| L5 | `aiw-agent`, `aiw-workflow` | Loop agentico, policy, hook, contesto, skill, subagenti, workflow | P04–P11, P13, P15, P17, P22 |
| L6 | `aiw-server`, `apps/desktop/src-tauri` | API HTTP/SSE, gateway OpenAI, IPC | P01–P03 |
| L7 | `apps/ui`, `aiw-cli` | UI React unica (desktop, mobile, web) e CLI | — |
| Collante | `aiw-core`, `aiw-engine` | Tipi, configurazione a strati, eventi AG-UI, storage; facciata `Workspace` | — |

## 3. Topologie di deployment

```mermaid
flowchart LR
  subgraph PC["PC / Mac (GPU)"]
    D["App desktop Tauri\n(core in-process)"]
    S1["llama-server / vLLM / Ollama"]
    D --> S1
  end
  subgraph Phone["Telefono"]
    M["App mobile Tauri\n(core in-process)"]
    N["Plugin nativo: Foundation Models / Gemini Nano / LiteRT-LM / llama.cpp"]
    M --> N
  end
  Browser["Browser di un altro device"] -- "HTTP+SSE, token di pairing" --> D
  M -- "runtime remoto LAN (OpenAI API)" --> D
```

- **Standalone**: il core gira dentro l'app (desktop o mobile).
- **Daemon**: `aiw serve` su un PC o server headless; i client si collegano via HTTP.
- **Mesh**: i device si accoppiano. Il telefono usa i modelli grandi del PC come runtime `remote`
  e il PC delega al telefono i task che servono lì (A2A 1.0 e server MCP del peer).

## 4. Inferenza unificata (L2–L3)

### 4.1 Runtime manifest

Ogni runtime è un file JSON dichiarativo (vedi `crates/aiw-runtimes/manifests/`):

```jsonc
{
  "id": "llama-server",
  "name": "llama.cpp server",
  "kind": "sidecar",                 // embedded | sidecar | system | remote
  "platforms": ["windows","macos","linux"],
  "accelerators": ["cpu","cuda","metal","vulkan","rocm","sycl"],
  "formats": ["gguf"],
  "api": "openai",                   // openai | ollama | native
  "capabilities": ["chat","tools","vision","embeddings","grammar"],
  "detect": { "binaries": ["llama-server"] },
  "launch": {
    "command": "llama-server",
    "args": ["-m","{model_path}","--host","127.0.0.1","--port","{port}","-c","{ctx_size}","-ngl","{gpu_layers}","--jinja"],
    "health": "/health",
    "ready_timeout_s": 120
  },
  "settings": {                      // JSON Schema (sottoinsieme) → form generato nella UI
    "type": "object",
    "properties": {
      "ctx_size":   { "type": "integer", "default": 8192, "minimum": 512, "title": "Context" },
      "gpu_layers": { "type": "integer", "default": 999, "title": "GPU layers" },
      "extra_args": { "type": "array", "items": {"type": "string"}, "title": "Argomenti extra" }
    }
  },
  "install": { "hint": "winget install llama.cpp | brew install llama.cpp | nix/apt" }
}
```

- **Rilevamento**: binari nel `PATH` o in percorsi noti, endpoint raggiungibile, piattaforma
  compatibile, bridge nativo registrato. Lo stato è uno di `available`, `installable`, `running`,
  `unsupported`.
- **Adapter**: `openai` copre la maggior parte dei sidecar (llama-server, vLLM, SGLang, mlx-lm,
  TRT-LLM, Foundry Local, Lemonade, LM Studio, LocalAI…); `ollama` usa lo stesso adapter OpenAI e vi aggiunge solo la
  parte nativa minima (elenco dei modelli **installati** da `GET /api/tags`; `num_ctx` e
  `keep_alive` non passano da `/v1`, quindi vanno nell'ambiente di `ollama serve`:
  `OLLAMA_CONTEXT_LENGTH` e `OLLAMA_KEEP_ALIVE`); `native` instrada verso un *bridge* fornito dalla
  shell (plugin Swift o Kotlin, embedded FFI).
- **Supervisor**: avvia il processo, assegna la porta, sostituisce i placeholder (`{model_path}`,
  `{model}`, `{model_id}`, `{port}`, impostazioni), attende l'health check, raccoglie i log,
  riavvia secondo la policy e ferma tutto all'uscita. `{model}` è il percorso locale (file o
  cartella) quando il modello del profilo è un modello locale, altrimenti l'id così com'è (repo
  Hugging Face che il runtime scarica da solo); `{model_id}` è sempre l'id. Le variabili `env`
  senza valore non vengono esportate.
- **Contesto del profilo**: la finestra di contesto del profilo (`InferenceParams.context`) ha la
  precedenza sul setting di contesto del runtime (`ctx_size`, `max_model_len`, oppure il setting
  indicato da `launch.ctx_setting`) per il processo avviato per quel profilo, e per le generazioni
  di un motore in-process. Un sidecar già avviato da AIW con un contesto più piccolo viene
  riavviato; uno più grande viene riusato. Un server avviato fuori da AIW non si modifica.
- **Architettura**: `arch` (opzionale, es. `["aarch64"]`) limita un runtime a una CPU oltre che al
  sistema operativo: MLX e Apple Foundation Models non sono offerti su un Mac Intel.
- **Installazione automatica**: un manifest può avere un campo `installer` per OS
  (`{"macos": {"kind": "pip", "packages": ["mlx-lm"]}}` oppure `{"kind": "brew", "formula": "llama.cpp"}`).
  Un clic nella UI installa il runtime senza altre domande: i pacchetti `pip` vanno in un ambiente Python
  isolato di AIW (`~/.aiw/pyenv`, creato con un Python 3.10+ del sistema o, se manca, installato con Homebrew),
  le formule con `brew`. Solo gestori di pacchetti, mai `curl | sh`; i nomi dei pacchetti sono validati.
  API: `GET /api/runtimes/{id}/install-plan`, `POST` e `GET /api/runtimes/{id}/install` (avanzamento e output).
  I runtime senza ricetta mostrano solo le istruzioni manuali.
- **Manifest utente**: ogni JSON in `~/.aiw/runtimes/*.json` si aggiunge al catalogo. Così si
  integra qualsiasi motore futuro senza ricompilare.

### Aggiornamenti

`aiw-engine::update` cerca nuove versioni nelle release GitHub del repository pubblico. Il **canale** si ricava
dall'etichetta della versione nel tag: nessuna etichetta = `stable` (produzione), `-rc.N`, `-beta.N`, `-alpha.N`
(o `-dev`, `-nightly`). Un canale riceve anche le release più stabili e vince la versione più alta; `auto`
(default) segue la versione installata, quindi una beta resta sulle beta. Chiavi: `update.channel`,
`update.auto_check` (all'avvio e ogni 6 ore), `update.auto_download`; il livello *workspace* le ignora, perché
un repository non deve scegliere da dove si aggiorna l'app.

L'installer del sistema (`.dmg`, `.msi`, `.AppImage`) viene scaricato solo da GitHub e **verificato con
`SHA256SUMS.txt`** della stessa release prima di essere aperto; un file che non corrisponde viene eliminato.
L'installazione è un passaggio di consegne al sistema (apre il dmg, avvia l'MSI, sostituisce l'AppImage in
esecuzione): le release non sono ancora firmate, quindi il sistema chiede la sua solita conferma.
API: `GET /api/update`, `POST /api/update/{check,download,install}`.

### 4.2 Interfaccia comune

```rust
#[async_trait]
pub trait ChatBackend: Send + Sync {
    async fn list_models(&self) -> Result<Vec<RemoteModel>>;
    async fn chat(&self, req: ChatRequest) -> Result<BoxStream<'static, Result<ChatDelta>>>;
}
```

`ChatRequest` usa messaggi e tool in formato OpenAI. `ChatDelta` normalizza testo, *reasoning* e
tool call parziali. Per i modelli senza tool calling nativo c'è un **fallback di parsing**
(`<tool_call>{…}</tool_call>` in stile Hermes/Qwen e blocchi JSON) con system prompt dedicato.

### 4.3 Router e profili

Un **profilo di inferenza** sceglie runtime + modello + parametri (temperature, contesto…) ed è
assegnabile per agente, per workflow step o per sessione. Il router, a partire dal `DeviceProfile`
(RAM, VRAM, acceleratori), propone il runtime migliore per un modello e stima se ci sta in memoria
(pesi + KV cache). L'utente può sempre forzare un'altra scelta.

## 5. Layer agentico (L5)

### 5.1 Ciclo di un run

```mermaid
sequenceDiagram
  participant UI
  participant Engine
  participant Agent
  participant Policy
  participant Tool
  participant LLM as Runtime
  UI->>Engine: POST /sessions/{id}/runs {input}
  Engine-->>UI: SSE RUN_STARTED
  loop fino a risposta finale o max_turns
    Agent->>LLM: chat(messages, tools) stream
    LLM-->>Agent: delta testo / tool_calls
    Agent-->>UI: TEXT_MESSAGE_CONTENT / TOOL_CALL_START/ARGS/END
    Agent->>Policy: evaluate(tool, args)
    alt ask
      Policy-->>UI: CUSTOM approval_request
      UI->>Engine: POST /approvals/{id} {allow|deny}
    end
    Agent->>Tool: run (sandbox)
    Tool-->>Agent: risultato
    Agent-->>UI: TOOL_CALL_RESULT
    Agent->>Agent: hook PostToolUse, compattazione se oltre il budget
  end
  Agent-->>UI: RUN_FINISHED
```

- **Eventi**: AG-UI 1.0 (`RUN_STARTED`, `TEXT_MESSAGE_*`, `REASONING_*`, `TOOL_CALL_*`,
  `STATE_SNAPSHOT`, `SUBAGENT_*`, `RUN_FINISHED`/`RUN_ERROR`) più `CUSTOM` con nome
  `aiw.approval_request`, `aiw.todo`, `aiw.usage`. Ogni evento viene salvato (P17), così i run
  sono riproducibili.
- **Tool paralleli**: le tool call read-only dello stesso turno vengono eseguite in parallelo; le chiamate `agent`
  consecutive formano un gruppo a sé, eseguito in parallelo fino a `agents.max_parallel_subagents` (default 3).
- **Subagenti** (P09): il tool `agent` avvia un run figlio con un profilo diverso e contesto
  isolato; al padre torna solo il risultato. Modalità di approvazione, sandbox e tool del figlio
  non sono mai più ampi di quelli del padre.
- **Budget del run**: `RunBudget` (token, turni del modello, tempo) è creato dall'engine per il run di primo livello
  dai limiti dell'agente (`budget`) o dalle impostazioni `agents.budget.*` (0 = nessun limite) ed è condiviso con tutti i
  subagenti. All'inizio di ogni turno, se è esaurito, il run termina normalmente con l'evento `aiw.budget_exceeded` e
  una risposta che riporta il motivo; ogni chiamata al modello conta un turno e prompt + completion contano come token.
- **Output strutturato**: se il profilo dell'agente ha `output_schema` (sottoinsieme di JSON Schema: `type`,
  `properties`, `required`, `items`, `enum`, `additionalProperties: false`, `minItems`/`maxItems`), il prompt di
  sistema lo riporta e la risposta finale viene estratta e validata. Una risposta non conforme torna al modello una
  sola volta (nota `aiw.format_check`); se resta errata il testo si conserva e si emette `aiw.format_error`. Il JSON
  valido finisce in `RunFinished.result.json`. Lo schema non viene passato ai runtime come `response_format`.
- **Contesto** (P10): un budget di token per profilo; oltre la soglia i turni più vecchi vengono
  riassunti con il modello attivo o con uno dedicato.
- **Memoria e skill** (P11): `AGENTS.md` del workspace entra nel system prompt; le skill
  (`SKILL.md` con frontmatter `name`/`description`) sono elencate solo con la descrizione e il
  modello le carica su richiesta con il tool `skill`.
- **Hook** (P08): `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `SubagentStart`, `SubagentStop`,
  `PreCompact`. `SessionStart` parte solo al primo messaggio di una conversazione e il suo output entra nel prompt di sistema;
  per gli eventi dei subagenti il filtro (`matcher`) è un glob sull'id dell'agente; `PreCompact` può saltare la compattazione.
  Un hook è un comando che riceve JSON su stdin e può bloccare o modificare l'azione, oppure un hook builtin.
- **Checkpoint** (P15): prima di ogni scrittura il file originale viene salvato in
  `checkpoints/<run_id>/<percorso>` (una volta per run). Accanto, `.aiw-run.json` registra la radice
  del workspace e i file che il run ha *creato* (`write_file`, `edit_file` con `old_string` vuoto,
  destinazione di `move_file`, che salva anche l'originale della sorgente); un file creato dal run non
  ha un originale da salvare. I file fuori dal workspace stanno in `_esterni/`. Il **rewind**
  (`aiw rewind <run_id> [--dry-run]`, `GET /api/runs/{id}/checkpoint`, `POST /api/runs/{id}/rewind`,
  «Ripristina i file» nel pannello della chat) rimette gli originali ed elimina i file creati se
  esistono ancora ed entro la radice registrata; i file esterni e quelli di un checkpoint senza radice
  (precedente al rewind) sono solo elencati. È rifiutato (409) mentre il run è in corso, scrive in
  audit `checkpoint.rewind`, e toglie dal checkpoint ciò che ha ripristinato, così un secondo rewind non
  cancella le modifiche successive. Le cartelle spostate con `move_file` e i run dei sotto-agenti
  (id propri) non sono coperti.
  Il rewind (ripristino dei file di un run) non è ancora implementato, né dalla UI né dalla CLI.

### 5.2 Policy, sandbox, audit

| Elemento | Default | Personalizzabile |
|---|---|---|
| Modalità | `ask` (chiede conferma per gli effetti collaterali) | `plan` (solo lettura), `auto-edit` (modifica file senza chiedere, comandi su conferma), `auto` (tutto entro la sandbox), `yolo` (tutto, sconsigliato) |
| Regole | allow: read, list, search, web_fetch su host pubblici · ask: write, edit, shell, browser, computer · deny: percorsi fuori dal workspace | Regole `tool(pattern)` per agente o workspace, es. `shell(git status*)` → allow |
| Sandbox OS (P07) | `workspace-write`: Linux bwrap, macOS sandbox-exec, Windows token a integrità Low + job object + desktop dedicato (`aiw-winsandbox`) | `read-only`, `workspace-write`, `off` |
| Rete dei tool | Consentita solo per i tool web e browser | Allowlist di domini |
| Audit (P22) | Ogni decisione di policy ed esecuzione viene registrata con l'ID del run | Esportabile in JSONL |
| Kill switch | `POST /kill` e scorciatoia globale: ferma run, sidecar avviati per i run e browser | — |

### 5.3 Workflow (task operativi)

Definizione YAML o JSON (editabile dalla UI):

```yaml
id: daily-inbox-digest
trigger: { cron: "0 8 * * 1-5" }          # manual | cron | interval | file | webhook
inputs: { folder: "~/Downloads" }
steps:
  - id: scan
    tool: list_dir
    args: { path: "{{inputs.folder}}" }
  - id: triage
    agent: { profile: "fast-local", prompt: "Classifica questi file: {{steps.scan.output}}" }
  - id: confirm
    approval: { message: "Sposto i file come proposto?" }
  - id: act
    agent: { profile: "operator", prompt: "Esegui il piano: {{steps.triage.output}}", tools: ["move_file"] }
```

Tipi di step: `tool`, `agent`, `llm`, `approval`, `condition` (`if`), `foreach`, `workflow`
(sotto-workflow). Ogni run di workflow è un run normale, quindi ha eventi, audit e possibilità di
ripresa dopo un'approvazione.

**Segnaposto.** `{{inputs.x}}`, `{{now}}`, `{{steps.<id>.output}}` (anche `{{steps.<id>}}`),
`{{steps.<id>.error}}` per un passo andato avanti con `on_error: continue`. L'output di un passo si
legge come JSON con `{{steps.<id>.json}}` (riformattato compatto), `{{steps.<id>.json.a.b}}` e
`{{steps.<id>.json.items.0.nome}}` (un numero indicizza un array): vale se l'output è JSON per
intero o contiene un blocco ```json, altrimenti il segnaposto resta vuoto. Un valore stringa si
inserisce senza virgolette, gli altri come JSON. Dentro un `foreach` ci sono anche `{{item}}`,
`{{item.campo}}` e `{{index}}`. Le `condition` vedono il testo già sostituito.

**Opzioni dei passi.** `tool`, `agent` e `llm` accettano `retry` (0–5 nuovi tentativi),
`retry_delay_s` (attesa, default 2 s), `timeout_s` (limite per tentativo) e `on_error: continue`
(il passo resta `failed`, il run prosegue e l'errore è in `{{steps.<id>.error}}`). Il timeout di un
passo `agent` annulla anche il run dell'agente.

**`foreach` e `parallel`.** `foreach` (`items`, `do`, `parallel` 1–8) esegue un passo inline
`tool`/`agent`/`llm` per ogni voce di `items`: un array JSON (anche in un blocco ```json) oppure un
testo con una voce per riga, al massimo 200. Nel passo valgono `{{item}}`, `{{item.campo}}` e
`{{index}}`; l'output è l'array JSON degli output nell'ordine delle voci (`null` per una voce
fallita con `on_error: continue`). Con `on_error: fail` il primo errore ferma il ciclo e annulla le
voci ancora in corso. `parallel` (`steps`) esegue più passi inline insieme; l'output è l'oggetto
`{id_interno: output}` e ogni output è leggibile anche come `{{steps.<id_interno>.output}}`. I passi
interni possono essere solo `tool`, `agent` o `llm` (niente annidamento, `approval` o `condition`) e
i loro id contano nell'unicità degli id del workflow. Gli agenti eseguiti insieme usano sessioni
diverse. Durante l'esecuzione il passo mostra «3/5 completati».

**Sotto-workflow.** `{kind: workflow, workflow: <id>, inputs: {…}}` avvia il workflow figlio (i
valori di `inputs` accettano i segnaposto) e ne aspetta la fine. L'output del passo è l'output
dell'ultimo passo `done` del figlio che non sia una `condition`; un figlio fallito o annullato è un
errore del passo. Il figlio è un run normale, con `parent_run_id` che punta al padre, e può essere
un workflow disattivato (l'attivazione riguarda solo i trigger). Profondità massima 3: un
workflow che richiama se stesso è rifiutato dalla validazione, i cicli indiretti si fermano al
limite. Se il padre smette di aspettare (timeout del passo) il figlio risulta annullato; dopo un
riavvio un figlio non riprende da solo: il padre riparte dal passo e ne avvia uno nuovo.

**Approvazioni con scadenza.** Il passo `approval` accetta `timeout_s` e `default` (`deny` o `allow`,
default `deny`): allo scadere vale il default, l'output è «scaduto: approvato» oppure il passo
fallisce con «scaduto: rifiutato» (e il run risulta annullato), l'audit registra `decision: "timeout"`.
Per le richieste dei tool degli agenti vale l'impostazione globale `approvals.timeout_s` (0 = nessun
limite, il default): allo scadere la richiesta è rifiutata e l'agente riceve un messaggio che spiega
il timeout. Il livello *workspace* non può impostarla.

**Durabilità.** Il motore tiene lo stato del run (`WorkflowState`: run, definizione, input, output,
rami abilitati, passo corrente) e lo salva a ogni cambiamento nella tabella `workflow_state`; il run
stesso è aggiornato in `workflow_runs`. All'avvio l'engine riprende i run `running` e
`waiting_approval` dal passo salvato: quel passo ricomincia da capo (semantica *almeno una volta*,
quindi un passo con effetti collaterali dovrebbe poter essere ripetuto), un'approvazione in attesa
viene richiesta di nuovo, un run il cui workflow non esiste più diventa `failed`. L'audit registra
`workflow.resume`.

### 5.4 Valutazione e tracce

Ogni sessione è già una traiettoria completa (messaggi, eventi AG-UI, run, audit). Il motore la
esporta e la usa per misurare gli agenti:

- **Export** (`aiw traces export`, `GET /api/traces/export`): una riga JSON (NDJSON) per sessione,
  con i messaggi in formato OpenAI, i run, le decisioni sulle approvazioni (`signals`) e i
  feedback. Le note interne (controlli automatici, errori del run, prompt bloccati) restano fuori
  dai messaggi e sono contate in `notes`. I segreti sono mascherati di default (`[SEGRETO]`: chiavi
  `sk-…`, token GitHub, `AKIA…`, `xox…`, `Bearer …`, chiavi private e i valori dei segreti MCP
  salvati); `--no-redact` lo disattiva, `--exclude-path` sostituisce con `[escluso]` il contenuto
  dei messaggi dei tool che citano i percorsi indicati.
- **Feedback**: pollice su/giù (valore 1 o -1) con nota facoltativa (fino a 2000 caratteri) su un
  messaggio dell'assistente, uno per messaggio (tabella `feedback`, sostituito a ogni nuova
  valutazione). Il `run_id` è il run in corso quando il messaggio è stato scritto. Entra nel campo
  `feedback` dei record dell'export e `--rated up|down|any` seleziona le sessioni valutate.
- **Suite di valutazione** (crate `aiw-eval`, esempio in `docs/evals/esempio.yaml`): un file YAML o
  JSON con i task da dare a un agente e i grader che dicono se ciascuno è riuscito.

  ```yaml
  id: base
  name: Suite di base
  agent: operator            # facoltativo, anche per task
  profile: default           # facoltativo, sostituisce il profilo dell'agente
  tasks:
    - id: crea-nota
      prompt: "Crea note.md con scritto ciao"
      files: { "input.txt": "contenuto iniziale" }   # scritti nella cartella di lavoro prima del run
      timeout_s: 300
      graders:
        - { kind: file_exists, path: note.md }
        - { kind: file_contains, path: note.md, pattern: "(?i)ciao" }
        - { kind: output_matches, pattern: "note\\.md" }
        - { kind: json_schema, schema: { type: object, required: [ok] } }   # sulla risposta finale
        - { kind: command, run: "test -s note.md", expect_exit: 0, timeout_s: 30 }
        - { kind: judge, criteria: "La nota è cortese e breve", profile: default, threshold: 0.7 }
  ```

  Ogni task gira in una cartella temporanea sotto `~/.aiw/eval-work/` (cancellata alla fine, salvo
  `--keep`) che è la radice del workspace dell'agente: modalità di policy `auto`, sandbox
  `workspace-write`, nessuna conferma possibile (se il run ne chiedesse una, viene rifiutata), il
  timeout del task come budget di tempo. Il run non appartiene a nessuna sessione; i suoi eventi
  sono salvati come gli altri. Il punteggio di un task è la media dei suoi grader, e il task riesce
  solo se tutti passano e il run non è finito in errore (o per limite di tempo). Con `--repeat N`
  ogni task gira N volte; il report ha `summary` con riusciti, totale, score medio e pass rate.
  I report sono nella tabella `eval_runs` e si aggiornano a ogni task.

  Il grader `command` esegue un comando con `sh -c` (PowerShell su Windows) nella cartella del
  task, con timeout, **fuori dalla sandbox dell'agente e con i permessi dell'utente**: la suite la
  scrive sempre l'utente. Per questo le suite si leggono solo da `~/.aiw/evals` (e dal database);
  quelle della cartella del workspace (`.aiw/evals`) **non** vengono caricate, perché un repository
  sconosciuto non deve poter eseguire comandi. Dalla CLI si può comunque lanciare un file qualsiasi,
  sapendo cosa contiene: `aiw eval run <file|id> [--agent] [--profile] [--repeat N] [--out report.json] [--keep]`,
  `aiw eval suites`, `aiw eval list`.

  Il grader `judge` chiede a un modello (il profilo `profile`, altrimenti `default`) di valutare il
  lavoro rispetto ai `criteria`: gli mostra il prompt del task, la risposta finale e i file che il run
  ha creato, modificato o eliminato, e si aspetta `{"score": 0..1, "reason": "…"}` (il JSON è letto
  con tolleranza, anche dentro un blocco di codice). Passa se `score >= threshold` (default 0,7); lo
  score del grader è quello del giudice. Una risposta senza JSON, senza punteggio o con un punteggio
  fuori da 0–1 fa fallire il grader con la spiegazione. Il giudice non ha strumenti.

  Confronto A/B: `aiw eval compare <report_a> <report_b>` (id salvati o file scritti da `--out`;
  `--json` per la struttura) mostra per ogni task riusciti, pass rate e score di A e B e la differenza
  B − A, più i totali, i token e la durata medi. `aiw eval run <suite> --profile a --profile b` esegue la
  suite con ciascun profilo e stampa il confronto dei successivi con il primo (con `--out report.json`
  scrive `report.json.a.json`, `report.json.b.json`). Nel motore: `Engine::compare_eval_runs(a, b)`.

  Dall'API (`/api/evals`, `/api/eval-runs`) e dalla pagina **Valutazioni** della UI (`#/evals`: suite,
  esecuzione, report, confronto) una suite parte in background (`Engine::start_eval`): la risposta è
  il report `running`, che il motore aggiorna a ogni task e la UI legge a intervalli finché non è
  `completed` o `failed`. I report ancora `running` all'avvio del motore sono di un run interrotto e
  diventano `failed`. Poiché il grader `command` esegue comandi con i permessi dell'utente senza
  alcuna conferma, l'API rifiuta di salvare suite che ne contengono: si scrivono solo nei file di
  `~/.aiw/evals` (le suite già salvate si possono comunque eseguire) e il confronto via API legge solo
  report salvati, mai percorsi di file.

## 6. Tool (L4)

| Tool | Piattaforme | Note |
|---|---|---|
| `read_file`, `write_file`, `edit_file` (sostituzione esatta), `list_dir`, `glob`, `grep` | tutte | Confinati alle radici del workspace |
| `shell` | desktop | Timeout, cwd nel workspace, sandbox OS, output troncato |
| `web_fetch` | tutte | HTML → testo, limiti di dimensione |
| `browser_*` (navigate, snapshot, click, type, screenshot) | desktop (Chrome/Edge/Chromium via CDP) | Snapshot testuale con riferimenti numerati per i modelli solo testo; screenshot per i VLM |
| `computer_*` (screenshot, click, type, key, scroll) | desktop (feature `computer-use`) · Android via AccessibilityService (plugin) | Modalità *vision* oppure *accessibility* |
| `todo`, `agent` (subagente), `skill` | tutte | — |
| `mcp__<server>__<tool>` | tutte (stdio solo desktop) | Client MCP: stdio e streamable HTTP |

## 7. Configurazione a strati

`defaults (manifest)` → `globale (~/.aiw/config.json)` → `device` → `workspace (.aiw/config.json)`
→ `agente/profilo` → `sessione`. La UI mostra per ogni valore **da quale strato arriva**
(`GET /config/explain?key=…`). Ogni componente espone uno `settings` schema; i form sono generati
(oggetti, enum, numeri con min/max, booleani, liste, segreti).

## 8. API (estratto)

| Metodo | Percorso | Descrizione |
|---|---|---|
| GET | `/api/system` | DeviceProfile + versione + stato |
| GET | `/api/runtimes` · POST `/api/runtimes/{id}/start` · `/stop` · PUT `/settings` | Catalogo e ciclo di vita dei runtime |
| GET | `/api/models` · POST `/api/models/pull` · GET `/api/models/catalog` | Modelli locali, download, catalogo consigliato |
| GET/PUT | `/api/profiles` | Profili di inferenza |
| GET/POST | `/api/sessions` · POST `/api/sessions/{id}/runs` | Sessioni e run |
| GET | `/api/runs/{id}/events` (SSE) | Stream AG-UI |
| POST | `/api/approvals/{id}` | Risposta a una richiesta di approvazione |
| GET/POST | `/api/workflows` · POST `/api/workflows/{id}/run` | Workflow |
| PUT/DELETE | `/api/sessions/{id}/messages/{mid}/feedback` · GET `/api/sessions/{id}/feedback` | Feedback sulle risposte |
| GET/PUT/DELETE | `/api/evals[/{id}]` | Suite di valutazione (PUT: JSON della suite o `{"text": yaml|json}`; i grader `command` non si scrivono via API) |
| POST | `/api/evals/{id}/runs` | Avvia la suite in background (`{agent?, profile?, repeat?}`); risponde con il report `running` |
| GET | `/api/eval-runs` · `/api/eval-runs/{id}` · `/api/eval-runs/compare?a=&b=` | Report (aggiornati a ogni task), confronto A/B tra due report salvati |
| GET | `/api/traces/export` | Traiettorie delle sessioni in NDJSON (filtri `agent`, `since`, `until`, `status`, `limit`) |
| GET/POST | `/api/mcp/servers` | Server MCP |
| GET/PUT | `/api/config` · GET `/api/config/explain` | Configurazione a strati |
| POST | `/api/kill` | Kill switch |
| — | `/v1/models`, `/v1/chat/completions` | Gateway OpenAI-compatibile verso i runtime locali |

Autenticazione: in loopback (e nel canale in-process della shell Tauri) non serve; per l'accesso
dalla LAN serve un **token di pairing** (QR mostrato nell'app) inviato come `Authorization: Bearer`.
Una pagina web non deve poter pilotare l'API locale (CSRF, DNS rebinding), quindi senza token:
- da loopback l'header `Host` deve essere `localhost`, `127.0.0.1` o `[::1]` (qualsiasi porta), altrimenti `403`;
- un `Origin` fuori dalla allowlist (`tauri://localhost`, `http(s)://tauri.localhost`,
  `http(s)://localhost|127.0.0.1|[::1]:*` e l'origine LAN del dispositivo stesso) o un fetch
  `Sec-Fetch-Site: cross-site` senza `Origin` ammesso danno `403`;
- il CORS risponde solo alla allowlist e non imposta mai `Access-Control-Allow-Credentials`;
  `/hooks/{id}` accetta qualsiasi origine perché il suo segreto è la credenziale.

`POST /api/pair` è l'unico endpoint LAN senza token: i tentativi falliti sono limitati
(10 ogni 10 minuti in totale, 5 per indirizzo); al superamento il codice viene rinnovato e si
risponde `429` con `Retry-After`, con pausa crescente. `GET /api/runs/{id}/events` riprende da
`Last-Event-ID` (o `?after=<seq>`) e rimanda solo gli eventi successivi.

## 9. Storage

- `~/.aiw/` (desktop) o la sandbox dell'app (mobile): `config.json`, `aiw.db` (SQLite:
  sessioni, messaggi, eventi, run, workflow, audit, feedback, report di valutazione), `models/`,
  `runtimes/` (manifest utente), `skills/`, `checkpoints/`, `evals/` (suite di valutazione),
  `eval-work/` (cartelle temporanee dei task).
- Workspace utente: `AGENTS.md`, `.aiw/config.json`, `.aiw/skills/`, `.aiw/workflows/`.

## 10. Piattaforme: cosa gira dove

| Capacità | Win | macOS | Linux | Android | iOS |
|---|---|---|---|---|---|
| Core + UI (Tauri 2) | ● | ● | ● | ● | ● |
| Sidecar (llama-server, vLLM, Ollama…) | ● | ● | ● | — | — |
| llama.cpp embedded (FFI) | ● | ● | ● | ● | ● |
| Modello di sistema | Phi Silica (Copilot+) | Foundation Models | — | Gemini Nano (ML Kit) | Foundation Models |
| Altri embedded | ORT GenAI / Windows ML | MLX, Core ML / Core AI | ORT, ExecuTorch | LiteRT-LM, ExecuTorch, ORT (QNN) | MLX Swift, LiteRT-LM (preview), Core AI |
| shell / browser / computer-use | ● | ● | ● | ◐ (AccessibilityService) | — (App Intents / Shortcuts) |
| Runtime remoto LAN | ● | ● | ● | ● | ● |

## 11. Decisioni (ADR)

| ADR | Decisione | Motivazione | Alternative scartate |
|---|---|---|---|
| [0001](adr/0001-tauri-rust-core.md) | **Tauri 2 + core Rust** per tutte le piattaforme | Un solo core nativo per desktop e mobile, binari piccoli, llama.cpp e ORT facili da integrare in Rust, plugin mobili in Swift e Kotlin | Electron (niente mobile, pesante), Flutter (FFI da duplicare, ecosistema agentico più povero), React Native (desktop immaturo), KMP |
| [0002](adr/0002-headless-core-protocol.md) | **Core headless** con la stessa API su IPC e su HTTP/SSE | P01: UI, CLI, browser e app terze sono tutti client | API diverse per ogni shell |
| [0003](adr/0003-manifest-driven-runtimes.md) | **Runtime come manifest dichiarativi** con schema delle impostazioni | Integrare "tutto" senza codice per ogni motore e generare la UI dagli schemi | Un adapter scritto a mano per ciascun motore |
| [0004](adr/0004-ag-ui-events.md) | **AG-UI 1.0** come formato degli eventi | Interoperabilità con CopilotKit e A2UI, già pensato per streaming, tool e stato | Formato proprietario |
| [0005](adr/0005-policy-sandbox.md) | **Policy separata dalla sandbox** e `ask` di default | P05–P07 osservati in tutti gli agenti maturi | Sola conferma dell'utente |

## 12. Roadmap tecnica

1. v0.1 (questo repository): core, registry con circa 30 manifest (oggi 33), adapter OpenAI e Ollama,
   supervisor, catalogo modelli e download, agent loop con policy, hook, subagenti e skill, tool
   (fs, shell, web, browser, computer-use, MCP), workflow con trigger manual/interval/cron, server
   e gateway, UI completa, shell Tauri, plugin nativi mobili (Foundation Models, ML Kit GenAI,
   LiteRT-LM).
2. v0.2 (fatto, tranne MLX Swift): llama.cpp in-process su tutte le piattaforme, ORT GenAI caricato
   a runtime (anche DirectML/QNN, la base di Windows ML), gateway Anthropic Messages, server MCP,
   renderer A2UI, git worktree per i task paralleli, sandbox Windows a integrità Low.
3. v0.3 (fatto): device mesh (mDNS, pairing via QR, A2A 1.0, peer come runtime e server MCP), worker
   RPC di llama.cpp ed exo per l'inferenza distribuita, voice mode (whisper.cpp o server
   OpenAI-compatibili o peer, voci di sistema locali), code mode (Rhai ermetico, P13), trigger webhook.
4. v0.4 (fatto): UI riorganizzata per domini con data layer TanStack Query, model hub multi-provider
   (Hugging Face, ModelScope, registry Ollama, URL diretto, catalogo con i modelli Whisper), API locale
   protetta (token, CORS ristretto, controllo di Origin/Host), correzioni di agent loop e runtime.
   Il voice mode della 0.3 è nella nuova struttura (`features/chat/voice/`).
5. Prossimi passi: MLX Swift su iOS (template Xcode con mlx-swift-lm e bridge `@_cdecl`), Kokoro come voce
   neurale locale, trascrizione nativa on-device su mobile (SpeechAnalyzer, SpeechRecognizer)
   quando manca whisper, catalogo degli execution provider di Windows ML, HTTPS in LAN (serve al
   microfono nei browser), firma e notarizzazione dei pacchetti.
