# Stato dell'app e come procedere

> 2026-11-03 · v0.4 · Basato su un audit del codice (non della documentazione) eseguito per layer, con
> verifica avversariale di ogni lacuna e riferimenti `file:riga`, aggiornato dopo i giri di verifica
> end to end, la revisione di sicurezza e la revisione dell'agent loop. Le voci **v0.4** indicano cosa
> è stato chiuso in questa versione.

## 1. Sintesi

AI Workspace Local è un core Rust solido (11 crate) con una UI React unica per desktop, mobile e web.
L'infrastruttura comune è reale e testata: AG-UI, policy e approvazioni, sandbox per OS, gateway
OpenAI/Anthropic, server MCP, A2A e mesh. Il divario era tra ciò che README e manifest dichiaravano
e ciò che il codice faceva davvero, soprattutto in tre punti:

1. **Inferenza**: 33 manifest di runtime, ma 20 passano tutti dallo stesso adapter OpenAI e 6
   (Windows ML, MLX Swift, Core AI, ExecuTorch, MLC, Phi Silica) erano solo JSON. Le feature
   Metal, CUDA e Vulkan del motore llama.cpp in-process **non lo registravano**, quindi su macOS e
   iOS il motore embedded non partiva.
2. **Modelli**: catalogo statico di 18 voci, download di un solo file da Hugging Face, nessuna
   ricerca, nessun altro provider e la voce MLX non scaricabile.
3. **Sicurezza dell'API locale**: CORS permissivo e richieste da loopback senza token permettevano
   a qualunque pagina web di registrare hook o server MCP stdio, cioè di eseguire comandi.

La v0.4 chiude questi tre punti e rifà la UI. Test Rust: da 82 a 357 (con la voce della 0.3). Test UI: da 0 a 629 (Vitest),
più uno smoke test Playwright (29 rotte × 2 larghezze × 2 temi).

Dopo la bozza sono entrati cinque giri di correzioni, descritti nel §2.8: difetti emersi dal giro
end to end di `aiw serve` con la UI reale, poi una revisione avversariale del codice Rust (sicurezza,
agent loop, inferenza, model hub). La revisione ha confermato 32 difetti su 46 segnalati; gli ultimi due (SEC-03 e SEC-05) sono
chiusi dal quinto giro.

## 2. Copertura per layer

Legenda: ✅ implementato end to end · 🟡 parziale · 📄 solo manifest o dichiarato · ❌ assente.

### 2.1 Layer applicativo e agentico (L5)

| Funzione | Stato | Note |
|---|---|---|
| Agenti (5 predefiniti: operator, coder, browser, desktop, planner) e loop con streaming AG-UI | ✅ | tool read-only in parallelo, max turni, self-check sugli errori dei tool; **budget per run** (token, turni del modello, secondi) condiviso con i subagenti, per agente (`budget`) o dalle impostazioni `agents.budget.*`: a limite esaurito il run si chiude con l'avviso `aiw.budget_exceeded`; **output strutturato**: un agente può avere `output_schema` (sottoinsieme di JSON Schema), la risposta finale viene estratta e validata con un tentativo di correzione, il JSON valido è in `RunFinished.result.json`; nessun `response_format` verso i runtime |
| Subagenti (tool `agent`, contesto isolato, profondità 2) | ✅ | modalità di approvazione, sandbox e tool del subagente non sono mai più ampi di quelli del padre; più chiamate `agent` nello stesso turno girano in parallelo (limite `agents.max_parallel_subagents`, default 3; attenzione a subagenti che scrivono negli stessi file) |
| Skill `SKILL.md`, memoria `AGENTS.md` | ✅ | nessuna UI per le skill |
| Hook (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, Stop, SubagentStart, SubagentStop, PreCompact) | ✅ | SessionStart parte al primo messaggio di una conversazione e aggiunge contesto al prompt di sistema; SubagentStart/SubagentStop usano il filtro sul nome dell'agente; PreCompact può saltare la compattazione; **v0.4** editor degli hook nella UI; un prompt bloccato da `UserPromptSubmit` resta visibile in chat ma non arriva al modello |
| Policy (plan/ask/auto-edit/auto/yolo), regole, approvazioni, audit, kill switch | ✅ | regole solo globali; **v0.4** editor delle regole, kill switch raggiungibile da telefono; una conferma in attesa viene ritirata quando il run smette di aspettarla e uno stop durante la conferma conta come annullamento, non come rifiuto; validazione di policy, hook e agenti prima del salvataggio |
| Code mode (Rhai), A2UI `render_ui`, worktree git per sessione | ✅ | |
| Messaggi dei run annullati o falliti | ✅ **v0.4** | prima andavano persi; l'errore resta nella cronologia (messaggio `aiw.run_error`, mai inviato al modello) |
| Esecuzione dei run | ✅ **v0.4** | un solo run per sessione (il secondo risponde 409); eliminare la sessione ferma il run; agente, profilo o runtime spariti → 409; i run rimasti `running` dopo un crash sono segnati falliti all'avvio; senza il profilo `default` il gateway ripiega sul profilo dell'agente predefinito |
| Checkpoint prima delle scritture e rewind | ✅ | `aiw rewind <run>` (`--dry-run`), `GET /api/runs/{id}/checkpoint`, `POST /api/runs/{id}/rewind` e «Ripristina i file» nella chat; i file fuori dal workspace sono solo elencati |
| Workflow (tool, agent, llm, approval, condition, foreach, parallel, sotto-workflow) e trigger (manuale, cron, intervallo, file, webhook) | ✅ | retry, timeout e `on_error` per passo, approvazioni con scadenza, segnaposto JSON, stato salvato a ogni passo e ripresa dopo il riavvio (passo in corso rieseguito: almeno una volta); **v0.4** editor visuale dei passi |
| A2A 1.0 server, server MCP, delega ai peer via MCP | 🟡 | A2A senza streaming (`SendStreamingMessage` non c'è: il client interroga `GetTask`); i task sono in SQLite (`a2a_tasks`) e scadono dopo 24 ore |
| Agenti remoti A2A | ✅ | registro `remote_agent` in Integrazioni (token mascherato come i segreti MCP) e dispositivi accoppiati, usabili come `remote:<id>` nei team e come subagenti; client A2A in `aiw-engine` (Agent Card, `SendMessage`, polling di `GetTask` con attesa crescente e timeout `a2a.timeout_seconds`, default 10 minuti, `CancelTask` se il run padre viene annullato); si parla solo con gli indirizzi registrati (l'Agent Card deve restare sulla stessa origine, nessun redirect) e ogni delega è nel registro delle attività |
| Tracce e valutazione (export delle traiettorie, feedback, suite di valutazione) | 🟡 | export NDJSON delle sessioni con segreti mascherati (`aiw traces export`, `GET /api/traces/export`); **feedback** pollice su/giù sulle risposte finali, con nota, esportato con le tracce; **suite di valutazione** (crate `aiw-eval`): task con file iniziali e grader `file_exists`, `file_contains`, `output_matches`, `json_schema`, `command`, `judge` (un modello valuta il lavoro), eseguiti in una cartella temporanea con `aiw eval run`, con ripetizioni, report salvato e confronto A/B (`aiw eval compare`, `--profile` ripetuto); pagina **Valutazioni** nella UI con editor delle suite, esecuzione in background, report e confronto |
| Team multi-agente | ✅ | team (`/api/teams`, pagina Team, sessione `team:<id>`) con strategie supervisor, parallelo e catena, budget e schema di risposta, 1-8 membri locali o remoti; **handoff** (tool `handoff`: la conversazione passa a un altro agente, al massimo 3 passaggi per messaggio, mai verso una modalità più permissiva), **stato condiviso** (tool `team_state`, 64 chiavi e 32 KB, visibile nell'Inspector) e traccia annidata dei subagenti in chat (`GET /api/runs/{id}/events.json`). Limite: dopo un ricaricamento la traccia annidata non c'è più (la cronologia mostra le schede dei tool); un team di soli agenti remoti non si apre in chat |
| Bot e canali (Telegram, Slack, email) | ❌ | |
| Documenti e RAG (knowledge base, embeddings) | ❌ | la capacità `embeddings` è solo un'etichetta nei manifest |
| Memoria a lungo termine | ❌ | |
| Voce (STT/TTS) | ✅ STT · 🟡 TTS | STT con `whisper-server` (whisper.cpp, `POST /inference`) o con un server OpenAI-compatibile (`/v1/audio/transcriptions`: LocalAI, Nexa, remoto, dispositivo accoppiato); scelta automatica del motore e del modello Whisper più grande che sta in memoria, impostazioni `voice.*`, modelli ggml nel catalogo e nella scansione, `GET /api/voice`, `POST /api/voice/transcribe` (audio grezzo o base64 per l'IPC), `POST /v1/audio/transcriptions` (mai inoltrato a un peer). In chat: dettatura dal microfono e conversazione a voce (ascolto con rilevamento della pausa, risposta letta con le voci locali del sistema, conferme annunciate, Esc termina). TTS solo con le voci di sistema della UI, nessun motore neurale (Kokoro/Piper); trascrizione nativa su mobile assente; nel browser il microfono richiede `localhost` o HTTPS |
| Allegati dell'utente (immagini, file) in chat | ❌ | |

### 2.2 Tool (L4) e interfacce (L6)

| Funzione | Stato | Note |
|---|---|---|
| File: read, write, edit, list, glob, grep, move | ✅ | **v0.4** confinamento anche rispetto ai symlink; radici di lettura che sono symlink ignorate; `read_file` limitato a 16 MB, `grep` controlla la dimensione prima di leggere |
| Shell in sandbox: bwrap (Linux), sandbox-exec (macOS), integrità bassa + job object (Windows) | 🟡 | rete della shell aperta di default; `~/.aiw` leggibile |
| `web_fetch` anti-SSRF | ✅ **v0.4** | redirect rivalidati a ogni salto, DNS fissato, IPv6 e range speciali bloccati; lettura limitata a 5 MB |
| Ricerca web | ❌ | nessun provider (SearXNG, Brave…) |
| Browser via CDP: navigate, snapshot, click, type, screenshot | 🟡 | mancano scroll, attese, tab, eval, download, albero di accessibilità; **v0.4** screenshot visibili nella chat |
| Computer-use: screenshot, mouse, tastiera | 🟡 | 5 azioni, solo build desktop Windows e macOS; **v0.4** screenshot visibili nella chat |
| Client MCP: stdio e streamable HTTP, `tools/list` e `tools/call` | 🟡 | mancano resources, prompts, sampling, elicitation, OAuth, list_changed e riconnessione; **v0.4** env e header dalla UI, validazione prima del salvataggio, segreti (header ed env) mascherati nelle risposte e conservati se la maschera viene rimandata |
| Gateway OpenAI `/v1/chat/completions` e `/v1/models`, gateway Anthropic `/v1/messages` | ✅ | mancano `/v1/embeddings` e `/v1/responses` |
| API locale: token, CORS, Origin/Host | ✅ **v0.4** | allowlist delle origini, protezione da DNS rebinding e CSRF; pairing con limite globale ai tentativi; header anti-framing (`X-Frame-Options`, CSP `frame-ancestors`, `nosniff`, `Referrer-Policy`); token legato all'origine che l'ha rilasciato (`?server=` non lo riceve) |
| Errori e rotte sconosciute dell'API | ✅ **v0.4** | 404 e 405 in JSON sotto `/api`, `/v1`, `/mcp`, `/a2a`, `/hooks`, `/.well-known`; rifiuti degli extractor come 400 JSON; errori solo in italiano, senza prefissi inglesi; `GET /api/tools` elenca anche i tool interni (todo, agent, skill, render_ui, code), non attivabili |
| Configurazione a strati del workspace | ✅ **v0.4** | lo strato nel repository può solo restringere `policy.mode` e `sandbox.mode`; non può impostare rete, server, percorsi, telemetria |
| Workflow del workspace (`<workspace>/.aiw/workflows`) | ✅ **v0.4** | importati disattivati; restano attivi solo se il file non cambia dopo l'abilitazione |

### 2.3 Inferenza: runtime (L2–L3)

| Gruppo | Runtime | Stato |
|---|---|---|
| Sidecar con adapter OpenAI | llama-server, LM Studio, vLLM, SGLang, TensorRT-LLM, mlx-lm, mlx-vlm, Foundry Local, Lemonade, OpenVINO, LocalAI, koboldcpp, llamafile, Nexa, mistral.rs, TabbyAPI, exo | ✅ avvio, health check e chat; il binario lo installa l'utente. **v0.4**: contesto del profilo inoltrato (`-c`, `--max-model-len`…), percorso reale dei modelli locali per MLX, vLLM, SGLang, TRT-LLM e mistral.rs |
| Ollama | `ollama` | ✅ **v0.4** elenco dei modelli installati da `/api/tags`, `OLLAMA_CONTEXT_LENGTH` e `OLLAMA_KEEP_ALIVE` |
| Distribuito | llama.cpp RPC worker, exo | ✅ |
| Embedded in-process | llama.cpp (FFI), ONNX Runtime GenAI (dlopen) | ✅ **v0.4** llama.cpp ora registrato anche con Metal/CUDA/Vulkan; ORT GenAI richiede la libreria installata |
| Di sistema | Apple Foundation Models (iOS e macOS), Gemini Nano e LiteRT-LM (Android) | 🟡 plugin Swift e Kotlin reali ma minimi; **macOS desktop** tramite il helper Swift `aiw-apple-fm` (processo, JSON per riga), da provare su un Mac con macOS 26 e Apple Intelligence |
| Solo manifest | Windows ML, MLX Swift, Core AI, ExecuTorch, MLC LLM, Phi Silica | 📄 la UI li mostra come "non ancora implementato" |
| Voce | whisper-server | ✅ trascrizione (sidecar `whisper-server` o già avviato, modelli ggml) |
| Remoti | OpenAI-compatibile, peer AIW | ✅ |

### 2.4 Acceleratori

| Acceleratore | Come viene usato | Stato |
|---|---|---|
| CPU (AVX2/AVX-512/NEON/dotprod/i8mm) | tutti i backend | ✅ |
| Metal | llama.cpp embedded (automatico su macOS/iOS), llama-server, mlx-lm | ✅ **v0.4** (prima il motore embedded non veniva registrato); non eseguito su hardware reale |
| CUDA | sidecar con `-ngl 999`, vLLM/SGLang/TRT-LLM, embedded con feature `llama-cuda` | 🟡 le release non abilitano CUDA |
| Vulkan | sidecar, embedded con `llama-vulkan` | 🟡 **v0.4** job di release opzionale `daemon-gpu` (Linux e Windows), non ancora eseguito |
| ROCm | solo sidecar | 🟡 **v0.4** rilevamento con `rocm-smi`/`rocminfo` |
| MLX | solo sidecar `mlx-lm` (Python) | 🟡 MLX Swift in-process assente; **v0.4** MLX riservato ai Mac Apple Silicon, repository MLX scaricabili |
| DirectML, QNN, OpenVINO, Core ML | tramite gli execution provider di ORT GenAI o i server che li gestiscono (Foundry, Lemonade, OVMS) | 🟡 AIW non li pilota direttamente |
| NPU (ANE, Hexagon, Ryzen AI, Intel NPU) | solo tramite modelli di sistema o server di terze parti | 🟡 |

### 2.5 Modelli e provider (v0.4)

| Provider | Ricerca | Varianti | Download |
|---|---|---|---|
| Consigliati (catalogo curato) | filtro locale | dalla voce | ✅ (anche la voce MLX, come repository intero) |
| Hugging Face | ✅ live, paginata | quantizzazioni GGUF con shard e `mmproj`, repository MLX/safetensors, cartelle ONNX GenAI | ✅ con token per i repo gated, mirror configurabile |
| ModelScope | 🟡 best effort (API del sito non documentata), nome esatto sempre accettato | ✅ | ✅ |
| Registry Ollama | indice locale di 48 modelli + nome esatto `name:tag` | ✅ GGUF + projector | ✅ |
| URL diretto | — | — | ✅ con sha256 opzionale |
| Modelli già presenti in Ollama, LM Studio, cache HF | scansione in sola lettura, nessuna copia | — | — |
| GitHub Releases, Kaggle | ❌ roadmap | | |

Downloader comune: coda con 2 job attivi, ripresa con Range, sha256 verificato in streaming,
controllo dello spazio disco, timeout di lettura, retry con backoff, annullamento ed eliminazione.
Il token di un provider viene inviato solo al suo host. La compatibilità modello → runtime è
calcolata dai manifest e dallo stato reale dei runtime su questo device.

Gli endpoint di HF, ModelScope e Ollama non erano raggiungibili dall'ambiente di sviluppo: i
provider sono testati con server HTTP locali che riproducono le risposte documentate. La ricerca di
ModelScope è quella con più incertezza.

Gli id logici, i sidecar `.aiw-model.json` e i percorsi dei file remoti usano sempre `/`; le
destinazioni di download sono costruite componente per componente e rifiutano `..`, percorsi
assoluti e prefissi di unità (correzione per Windows). Ogni URL diretto ha una cartella propria
(`url/<host>/<nome>-<hash>`), quindi due URL dello stesso host sono due modelli distinti.

### 2.6 UI (v0.4)

Struttura per dominio (`app/`, `shared/`, `features/`), TanStack Query con polling mirato, router
su hash con parametri (sessione, agente e workflow nell'URL), nuova architettura dell'informazione
(Panoramica, Capacità, Integrazioni, Sicurezza), palette comandi Ctrl/Cmd+K, design system
estratto, contrasto AA in entrambi i temi, layout corretti su telefono e tablet. Dettagli in
[UI-ARCHITECTURE.md](design/UI-ARCHITECTURE.md). La UI mostra l'errore di un run salvato come quello
dal vivo, raggruppa i tool interni, mostra i segreti MCP mascherati e riporta a `#/chat` se la
sessione aperta è stata eliminata altrove.

### 2.7 Piattaforme e build

| | Windows x64 | macOS arm64 | Linux x64 | Android arm64 | iOS |
|---|---|---|---|---|---|
| App Tauri + motore in-process | ✅ | ✅ | ✅ | ✅ APK non firmato | 🟡 solo simulatore in CI |
| llama.cpp embedded | CPU | Metal **v0.4** | CPU | CPU | Metal **v0.4** |
| Computer-use | ✅ | ✅ | ❌ escluso in CI | ❌ | ❌ |
| Firma e notarizzazione | ❌ | ❌ | — | ❌ | ❌ |

Non costruiti: Windows arm64, Linux arm64, Android x86_64/armv7.

### 2.8 Correzioni dopo la bozza

| Giro | Esito |
|---|---|
| Fix Windows | percorsi indipendenti dal separatore dell'OS negli id e nei sidecar dei modelli; `prune_empty` confronta le forme canoniche (`\\?\C:\...` contro i nomi 8.3) |
| Verifica end to end di `aiw serve` | download da URL diretto con cartella propria; errori dei run nella cronologia; 404/405 JSON; errori in italiano; validazione di policy, hook, agenti e server MCP; segreti MCP mascherati; tool interni elencati |
| Revisione di sicurezza | strato di configurazione del workspace ristretto; workflow del workspace disattivati; symlink nelle radici di lettura; limiti di 5 MB (`web_fetch`) e 16 MB (`read_file`); header anti-framing; token legato all'origine |
| Revisione dell'agent loop | prompt bloccati da hook non inviati al modello; conferme ritirate e stop distinto dal rifiuto; un run per sessione; eliminazione della sessione che ferma il run; 409 per agente o profilo mancanti; run interrotti segnati falliti all'avvio; profilo di ripiego del gateway |
| Revisione dell'inferenza | vLLM e SGLang servono il modello con l'id del profilo (`--served-model-name`) e il backend usa il nome che il sidecar elenca in `/v1/models` (mlx-lm con un percorso locale); un solo avvio per runtime anche con richieste concorrenti; lo stop termina l'intero gruppo di processi del sidecar; nessun riavvio per un contesto più grande mentre altre richieste sono in corso; budget di compattazione = minimo tra contesto del profilo e contesto servito; memoria unificata senza doppio conteggio della RAM; VRAM oltre 4 GB su Windows dal valore a 64 bit del registro |
| Revisione del model hub | nessuna eliminazione mentre un'altra variante della stessa cartella si scarica (409) e niente più `remove_dir_all`; pagine HTML rifiutate e magic byte GGUF/safetensors verificati; installazioni in corso o fallite non elencate come modelli; eliminazione dell'intero set di shard e pulizia dei `.part` orfani; ripresa con `If-Range` e validatori salvati; retry azzerati quando il download avanza; sidecar con lock per cartella; lettura degli header GGUF senza seek inutili; immagini degli eventi limitate a 1,5 MB e 4 per risultato |
| Chiusura dei limiti di rete (SEC-03, SEC-05) | la policy di rete del browser si applica a ogni richiesta (documenti, XHR/fetch, iframe, script, immagini, WebSocket, target figli) con `Fetch.enable`/`Fetch.requestPaused` e `failRequest BlockedByClient` (verdetto con timeout, mai richieste appese), oltre al controllo dopo la navigazione; dietro un proxy HTTP(S) (rispettando `NO_PROXY`) `web_fetch` blocca IP letterali e `localhost`, richiede indirizzi pubblici se il nome si risolve in locale e rifiuta i nomi non risolvibili salvo `network.allow_domains`. Resta la finestra del DNS rebinding tra verifica e connessione di Chrome |

### 2.9 Limiti noti

- La rete della shell è aperta di default e `~/.aiw` è leggibile dentro la sandbox.
- I server MCP stdio girano fuori dalla sandbox.
- Esiste un solo token API condiviso, senza revoca.
- Gli endpoint dei provider (HF, ModelScope, Ollama) non sono mai stati raggiunti dall'ambiente di
  sviluppo: i test usano server locali finti.
- Le build GPU (CUDA, Vulkan) e i percorsi nativi Apple e Android non sono stati eseguiti su
  hardware reale.

## 3. Come procedere

Ordine consigliato: prima ciò che sblocca l'uso reale su hardware, poi i layer agentici mancanti.

**P0 · Verifica su hardware reale**
1. Eseguire il job `daemon-gpu` (Vulkan) e aggiungere CUDA nella matrice di release desktop
   (Windows, Linux). Verificare llama.cpp Metal su un Mac Apple Silicon e su iPhone.
2. Provare i provider del model hub contro i servizi reali (HF, ModelScope, Ollama) e
   rigenerare l'indice Ollama da CI. Le build GPU e i percorsi nativi Apple e Android vanno
   provati su hardware reale (vedi §2.9).
3. Selezionare Xcode 26 nella CI iOS, così il ramo Foundation Models viene compilato davvero.
   Il build macOS desktop usa già Xcode 26 e fallisce se il helper Swift non si compila
   (`AIW_REQUIRE_APPLE_FM=1`): provare `aiw-apple-fm` su un Mac con macOS 26 e Apple Intelligence attiva.

**P1 · Inferenza**
4. Router automatico modello → runtime → acceleratore (oggi il profilo è statico) con fallback tra
   runtime e stima della KV cache anche per le varianti remote.
5. MLX in-process con MLX Swift (macOS/iOS) al posto del solo sidecar Python.
6. Embeddings end to end: `ChatBackend::embed`, `/v1/embeddings` nel gateway, client per i runtime
   che li offrono.
7. Provisioning dei binari dei runtime (llama-server con la build giusta per l'acceleratore).

**P2 · Layer agentici**
8. Knowledge base locale (documenti → embeddings → ricerca) come tool degli agenti, poi memoria a
   lungo termine.
9. Ricerca web (SearXNG locale o provider con chiave) e browser più ricco (scroll, attese, tab,
   download, albero di accessibilità, intercettazione CDP delle richieste per la policy di rete).
10. Client MCP completo: resources, prompts, sampling, OAuth, riconnessione.
11. Rewind dei checkpoint, workflow durevoli (ripresa, foreach, retry, timeout), approvazioni con
    timeout e notifiche desktop.
12. Canali (Telegram, Slack, email) e voce neurale locale
    (Kokoro/Piper al posto delle voci di sistema; la trascrizione con whisper.cpp c'è già).

**P3 · Sicurezza e distribuzione**
13. Rete della shell chiusa di default, `~/.aiw` fuori dalla sandbox, server MCP stdio in sandbox e
    con conferma, protezione SSRF anche dietro proxy, regole di policy sulla shell che non si aggirano con comandi concatenati.
14. Token per dispositivo con revoca (oggi un solo token condiviso) e TLS per la LAN.
15. Firma e notarizzazione delle release, pubblicazione su GitHub Releases, target arm64.
16. Font inclusi nell'app invece di Google Fonts, i18n it/en.
