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
| Workflow (tool, agent, team, llm, approval, condition, foreach, parallel, sotto-workflow) e trigger (manuale, cron, intervallo, file, webhook) | ✅ | retry, timeout e `on_error` per passo, approvazioni con scadenza, segnaposto JSON, stato salvato a ogni passo e ripresa dopo il riavvio (passo in corso rieseguito: almeno una volta); **v0.4** editor visuale dei passi |
| A2A 1.0 server, server MCP, delega ai peer via MCP | 🟡 | Streaming: `SendStreamingMessage`/`message/stream` e `SubscribeToTask`/`tasks/resubscribe` in SSE (`TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent` dal bus degli eventi del run), `capabilities.streaming: true`. Provato solo tra due istanze di questo stesso programma e con server finti, mai con client A2A di terzi; niente notifiche push, niente ripresa dal punto di interruzione (una nuova sottoscrizione riparte dallo stato attuale); i task sono in SQLite (`a2a_tasks`) e scadono dopo 24 ore |
| Agenti remoti A2A | ✅ | registro `remote_agent` in Integrazioni (token mascherato come i segreti MCP) e dispositivi accoppiati, usabili come `remote:<id>` nei team e come subagenti; client A2A in `aiw-engine` (Agent Card, streaming con `SendStreamingMessage` se la card lo annuncia — ripiego su `SendMessage` e polling di `GetTask` — e avanzamenti nella traccia del run, polling di `GetTask` con attesa crescente e timeout `a2a.timeout_seconds`, default 10 minuti, `CancelTask` se il run padre viene annullato); si parla solo con gli indirizzi registrati (l'Agent Card deve restare sulla stessa origine, nessun redirect) e ogni delega è nel registro delle attività |
| Tracce e valutazione (export delle traiettorie, feedback, suite di valutazione) | 🟡 | export NDJSON delle sessioni con segreti mascherati (`aiw traces export`, `GET /api/traces/export`); **feedback** pollice su/giù sulle risposte finali, con nota, esportato con le tracce; **suite di valutazione** (crate `aiw-eval`): task con file iniziali e grader `file_exists`, `file_contains`, `output_matches`, `json_schema`, `command`, `judge` (un modello valuta il lavoro), eseguiti in una cartella temporanea con `aiw eval run`, con ripetizioni, report salvato e confronto A/B (`aiw eval compare`, `--profile` ripetuto); pagina **Valutazioni** nella UI con editor delle suite, esecuzione in background, report e confronto |
| Apprendimento senza pesi (lezioni proposte dall'agente) | 🟡 | dopo un pollice giù (o un run fallito, secondo `learning.reflect`) l'agente propone una **lezione**: una memoria per `~/.aiw/AGENTS.md` o una skill per `~/.aiw/skills`. Resta in attesa finché l'utente non la approva (anche modificata) o la scarta, e solo allora viene scritta, mai nella cartella del workspace; `aiw lessons`, `GET /api/lessons`, `POST /api/lessons/{id}/approve|reject`, `POST /api/sessions/{id}/reflect`; pannello «Lezioni da rivedere» nella Panoramica e voce «Rifletti su questa conversazione» nell'elenco delle sessioni. Una lezione approvata non si può ritirare dalla UI: si modifica il file. **Ottimizzazione del prompt guidata dalla valutazione**: `aiw eval optimize <suite> --agent X` (o «Ottimizza il prompt» nella pagina Valutazioni) esegue la suite col prompt attuale, chiede al modello delle varianti guardando i task falliti, le prova e propone la migliore solo se batte l'attuale, con il diff del prompt; applicarla all'agente è sempre una scelta esplicita (`--apply` con conferma, pulsante «Applica all'agente») |
| Team multi-agente | ✅ | team (`/api/teams`, pagina Team, sessione `team:<id>`) con strategie supervisor, parallelo e catena, budget e schema di risposta, 1-8 membri locali o remoti; **handoff** (tool `handoff`: la conversazione passa a un altro agente, al massimo 3 passaggi per messaggio, mai verso una modalità più permissiva), **stato condiviso** (tool `team_state`, 64 chiavi e 32 KB, visibile nell'Inspector) e traccia annidata dei subagenti in chat (`GET /api/runs/{id}/events.json`). Limite: dopo un ricaricamento la traccia annidata non c'è più (la cronologia mostra le schede dei tool); un team di soli agenti remoti non si apre in chat |
| Addestramento (fine-tuning locale con trainer esterni) | 🟡 | trainer come manifest (`trainers.json`: mlx-lm su Apple Silicon, TRL/PEFT e Unsloth su CUDA) con rilevamento nel venv di AIW e installazione `pip`; `aiw train doctor` (classe del dispositivo, memoria, cosa si può addestrare, dispositivi accoppiati che potrebbero). SFT LoRA dalle traiettorie: dataset da feedback, sessioni riuscite e task superati (segreti mascherati), job in background con log, avanzamento e annullamento, registro degli adapter, confronto opzionale con una suite; `aiw train sft`, API e pagina Addestramento (`#/training`). DPO dalle coppie di preferenza (pollice su/giù sulla stessa domanda, tentativi di eval riusciti/falliti): `aiw train dpo`, solo TRL/Unsloth. GRPO con reward dai grader di una suite (`aiw train grpo`, `POST /api/evals/{id}/reward`), solo TRL su dispositivi di classe L o superiore. Router a bandit tra i profili candidati di un agente (`routing.bandit`, UCB1 su pollici e task di valutazione; `GET`/`DELETE /api/agents/{id}/routing`, tabella nell'editor dell'agente). Mai provato con un trainer vero (mlx-lm, TRL, Unsloth; DPO e GRPO compresi) né col formato dati reale di mlx-lm: i test usano trainer finti. Guida: `docs/training.md` |
| Bot e canali (Telegram, Slack, email) | 🟡 | `aiw-engine/src/channels/`: Telegram (long polling, abbinamento con codice, allegati, pulsanti), Slack (Socket Mode, pulsanti), email (IMAP + SMTP, solo mittenti autenticati), elenco dei mittenti, modalità `read_only`/`ask` (mai yolo), strumento `channel_send`, riepilogo del mattino sui canali, pagina «Canali», `aiw channels`. Spenti di serie. Provato solo contro server finti (Telegram e Slack in-process, IMAP/SMTP finti con TLS autofirmato), **mai contro i servizi veri**. Mancano WhatsApp, Discord, Matrix, webhook, allegati su Slack; i canali girano solo con l'app o il servizio accesi (sul telefono no); nei gruppi Telegram basta stare nel gruppo messo in elenco |
| Documenti e RAG (knowledge base) | 🟡 | raccolte, documenti e passaggi in SQLite (`kb_*` + FTS5); caricamento dall'app o da cartella (solo da questo computer; fino a 2.000 file da 10 MB), sha256 per non duplicare, sostituzione per percorso, passaggi da ~1.200 caratteri con 150 di sovrapposizione, lavoro in background interrompibile con avanzamento; ricerca ibrida parole chiave + coseno (RRF); strumento `kb_search` e `AgentProfile.knowledge`; API `/api/kb/*`, `aiw kb`, pagina «Documenti». **Mai provato con un vero modello di embedding** (i test usano un servizio finto): senza un runtime di embedding già avviato cerca per parole chiave; il llama.cpp incorporato non offre ancora embedding; niente docx, xlsx, OCR |
| Memoria a lungo termine | 🟡 | tabella `memories` (tipi, ambito per agente, «ricorda sempre», proposte da confermare, eliminazione morbida annullabile); ricerca ibrida FTS5 + coseno sugli embedding con rank fusion; strumenti `memory_save/search/update/forget` senza richiesta di conferma per i salvataggi espliciti; iniezione nel prompt (fissi ~1.500 token + 5 richiamati); `memory.enabled`; `GET/POST/PATCH/DELETE /api/memories`. Embedding solo via `/v1/embeddings` (runtime già avviato); il motore llama.cpp interno non li calcola; senza modello di embedding resta la ricerca per parole chiave. Mai provato con un modello di embedding vero (i test usano un finto). Pagina «Insegna» (`#/teach`) con l'agente `teacher` e il pannello «Cosa so»; scheda «Vuoi che lo ricordi?» nelle altre chat. UI provata solo con la demo (trasporto simulato), non con un modello vero |
| Ruoli dei modelli in un run | 🟡 | `AgentProfile.roles` (`"auto"` o `{plan, act, check, summarize}`): modello per chiamata nello stesso ciclo (plan alla prima chiamata e dopo un fallimento, act dopo risultati riusciti, check = verifica PASS/FAIL prima di concludere solo dopo strumenti che agiscono, al massimo un giro in più, summarize per la compattazione); decisione `auto` dalla classe del dispositivo in una funzione pura testata (`adviser::plan_auto_roles`); riduzione a un solo modello se un runtime ne regge uno solo per volta; etichette nel run e sezione «Ruoli dei modelli» solo in vista Sviluppatore. Provato solo con backend scriptati (test del ciclo e dell'engine) e con la demo, mai con due modelli veri; i titoli delle sessioni non usano un modello (sono il testo del primo messaggio), quindi `summarize` riguarda la sola compattazione; il dispositivo accoppiato come `plan` non è mai stato provato con un dispositivo vero |
| Assistente proattivo | 🟡 | spento di serie (`proactive.level`); battito (intervallo, ora del riepilogo, promemoria scaduti, `POST /api/proactive/tick`) con orario di silenzio, budget giornaliero, solo a corrente dove il sistema lo dice, kill switch, un controllo alla volta; filtro a una domanda JSON al modello più piccolo (mai chiamato a contesto vuoto), proposta preparata in sola lettura, tabella `proposals`, approvazione = run normale in una sessione nuova con la policy abituale; riepilogo del mattino senza modello; `GET /api/proposals`, `/api/proactive/status`; pagina «Proposte» (`#/inbox`), pannello in Panoramica, sezione nelle Impostazioni; suite `docs/evals/proattivo-filtro.yaml`. Provato con modello finto e orologio finto, mai con un modello vero né a lungo termine; la notifica del desktop (`apps/desktop/src-tauri/src/desktop.rs`) non è stata compilata. Il livello `act_readonly` aggiunge solo la rete in sola lettura: nessuna azione che scrive parte senza approvazione |
| Connettori locali (calendario e posta) | 🟡 | strumenti incorporati di sola lettura `calendar_today`, `calendar_upcoming`, `mail_recent`, `mail_search` (`aiw-tools/src/connectors`), spenti di serie, elencati solo con `connectors.*.enabled`; calendario da `.ics`, cartelle e sottoscrizioni (ricorrenze DAILY/WEEKLY/MONTHLY/YEARLY con INTERVAL, COUNT, UNTIL, BYDAY settimanale; fusi da una tabella interna, non il database IANA completo), posta da Maildir, `.mbox` e `.eml`; eventi di oggi e posta nuova nel riepilogo e nel battito. Provato su file di prova e con orologio finto, non su cassette vere né su sottoscrizioni reali. IMAP in sola lettura (`EXAMINE`/`BODY.PEEK`, solo TLS, password nell'archivio e mascherata nella API) provato solo su un server IMAP finto, mai su una casella vera. Mancano Google/Microsoft via OAuth (serve una «password per le app»), regole `BYSETPOS`/`BYMONTHDAY` e ordinali in `BYDAY` |
| Assistente proattivo in background (telefono) | 🟡 | motore e API provati con test (`proactive_background_tick`, tetto 25 s, filtro delegato a un dispositivo accoppiato o saltato con motivo registrato, `POST /api/proactive/background-tick`) e interfaccia con la demo. **Non compilati né provati su un telefono**: Android (WorkManager, canale, `POST_NOTIFICATIONS`), iOS (BGTaskScheduler, notifiche locali, App Intents) e i comandi della shell Tauri; da verificare in CI e a mano. Il sistema decide quando il lavoro parte (iOS soprattutto): l'ora precisa del riepilogo su iPhone passa da un Comando. Vedi `docs/mobile/BACKGROUND.md` |
| Voce (STT/TTS) | ✅ STT · 🟡 TTS | STT con `whisper-server` (whisper.cpp, `POST /inference`) o con un server OpenAI-compatibile (`/v1/audio/transcriptions`: LocalAI, Nexa, remoto, dispositivo accoppiato); scelta automatica del motore e del modello Whisper più grande che sta in memoria, impostazioni `voice.*`, modelli ggml nel catalogo e nella scansione, `GET /api/voice`, `POST /api/voice/transcribe` (audio grezzo o base64 per l'IPC), `POST /v1/audio/transcriptions` (mai inoltrato a un peer). In chat: dettatura dal microfono e conversazione a voce (ascolto con rilevamento della pausa, conferme annunciate, Esc termina). **TTS neurale** (`aiw-engine/src/tts.rs`): Piper (eseguibile + voce `.onnx`, WAV) o un server OpenAI-compatibile (`POST /v1/audio/speech`, per Kokoro-FastAPI), `voice.tts.engine` = auto \| system \| piper \| openai, voce per lingua, velocità, testo diviso in frasi (tetto 4000 caratteri), una sintesi alla volta e annullabile; `POST /api/voice/speak` (JSON con WAV in base64), gateway `POST /v1/audio/speech` (mai inoltrato); 4 voci Piper nel catalogo (tipo di modello `piper`, scaricabili e riconosciute dalla scansione); in chat «Leggi ad alta voce» su ogni risposta e voce neurale nella conversazione, frase per frase, con ripiego sulle voci di sistema. **Mai provato con Piper o Kokoro veri** (solo con un eseguibile e un server finti); **le dimensioni delle voci nel catalogo sono indicative e non verificate**; Piper parte una volta per frase (non resta caldo); trascrizione nativa su mobile assente; nel browser il microfono richiede `localhost` o HTTPS |
| Allegati dell'utente (immagini, file) in chat | 🟡 | Fino a 8 file per messaggio (10 MB ciascuno, 25 MB in tutto) con graffetta, trascinamento e incolla di immagini; `POST /api/sessions/{id}/runs` accetta `attachments: [{name, mime, data}]` (base64, per essere uguale su HTTP e IPC) e `GET /api/sessions/{id}/attachments/{id}` li restituisce. Salvati in `~/.aiw/attachments/<sessione>/` (mai nel workspace; spariscono con la sessione). Il tipo si decide dal contenuto: immagini (png, jpeg, webp, gif), testo UTF-8 e PDF con testo (estratto con `pdf-extract`, in un thread a parte con tetto di 30 s) finiscono nel messaggio al modello (fino a 40.000 caratteri per file e 120.000 per messaggio, poi un avviso «troncato» con il percorso da leggere); gli altri file e i PDF senza testo sono solo descritti, con il percorso leggibile dall'agente (`read_file`, `grep`, in sola lettura). Modello con `vision` noto: le immagini partono come `image_url`; senza visione una riga lo dice e la chat mostra un avviso; modello sconosciuto: partono e vale il ripiego «riprova senza immagini». Nei turni successivi il testo degli allegati resta, le immagini si rimandano solo per gli ultimi 2 messaggi che ne hanno. **Mai provato con un modello con visione vero né su un telefono** (solo con modello finto e interfaccia demo). Niente OCR per i PDF scansionati, niente estrazione da docx/xlsx (restano «file»), niente allegati via A2A, gateway OpenAI/Anthropic, workflow, voce o assistente proattivo (solo testo). Le tracce e i dataset non esportano gli allegati (né il contenuto né i metadati). Con una squadra a copione e con i sotto-agenti le immagini non arrivano: il testo degli allegati e i percorsi sì |

### 2.2 Tool (L4) e interfacce (L6)

| Funzione | Stato | Note |
|---|---|---|
| File: read, write, edit, list, glob, grep, move | ✅ | **v0.4** confinamento anche rispetto ai symlink; radici di lettura che sono symlink ignorate; `read_file` limitato a 16 MB, `grep` controlla la dimensione prima di leggere |
| Shell in sandbox: bwrap (Linux), sandbox-exec (macOS), integrità bassa + job object (Windows) | 🟡 | Linux: rete chiusa di default (`sandbox.shell_network`), cartella dei dati e credenziali (`sandbox.deny_read`) nascoste, variabili segrete tolte: **provato con bwrap vero**. macOS: stessa cosa con un profilo Seatbelt, **solo il testo del profilo è provato**, mai eseguito su un Mac. Windows: **rete non chiudibile e letture non nascondibili** (integrità bassa limita solo le scritture); le variabili segrete sono tolte. Le regole di policy leggono ogni comando di una riga composta |
| `web_fetch` anti-SSRF | ✅ **v0.4** | redirect rivalidati a ogni salto, DNS fissato, IPv6 e range speciali bloccati; lettura limitata a 5 MB |
| Ricerca web | 🟡 | strumento `web_search` (`aiw-tools/src/search.rs`) con `search.provider` = `auto` \| `duckduckgo` \| `searxng` \| `brave` \| `off`; chiave di Brave mascherata dall'API e solo a livello di macchina, indirizzo di SearXNG validato; sezione «Ricerca web» nelle Impostazioni; l'assistente proattivo la usa solo a `act_readonly`. **Mai provata contro i provider veri da questo ambiente**: i test usano un server locale con risposte scritte a mano nella forma attesa. Il parser di DuckDuckGo dipende dall'HTML della pagina `html.duckduckgo.com` e può smettere di funzionare se cambia; DuckDuckGo può chiedere una verifica anti-bot (l'errore lo dice). Gli agenti predefiniti già salvati non ricevono `web_search` da soli (solo le nuove installazioni) |
| Browser via CDP: navigate, snapshot (elementi, albero di accessibilità, testo), click, type, scroll, back, press, wait, screenshot, schede, select, hover, upload, download, console, eval | 🟡 | `browser.rs` + `browser/cdp.rs`: connessione al browser intero con auto-attach in pausa, quindi la policy di rete (`Fetch.requestPaused`) copre anche le schede aperte dal sito; limiti: 8 schede, 10 download per run da 200 MB, console da 100 voci; download solo con permesso di scrittura, in `<workspace>/downloads/<run>/`; `browser_eval` è `exec` con permesso esplicito. **Provato solo con un CDP finto (test sempre attivi) e con Chromium headless 153 su pagine di un server locale (test `#[ignore]`); mai su siti reali.** Mancano PDF della pagina, accettare le finestre di dialogo su richiesta, iframe nell'albero di accessibilità. Sul telefono (WebView) solo snapshot elementi/testo, select, hover, eval, attese su testo/selettore: **mai compilato per mobile né provato**; **v0.4** screenshot visibili nella chat |
| Computer-use: screenshot, mouse, tastiera | 🟡 | 5 azioni, solo build desktop Windows e macOS; **v0.4** screenshot visibili nella chat |
| Client MCP: stdio e streamable HTTP, strumenti, risorse, prompt, campionamento, domande del server, OAuth, `list_changed`, riconnessione | 🟡 | Provato **solo con server e server di autorizzazione finti in locale** (script Python su stdio, axum su HTTP): mai con server MCP di terzi né con provider OAuth veri, e il ritorno OAuth su indirizzo locale non è stato verificato nell'app Tauri né sul telefono. Mancano: trasporto HTTP+SSE della revisione 2024-11-05, sottoscrizioni alle risorse, ripresa dei flussi SSE, OAuth con endpoint su un'altra origine dell'issuer (rifiutato di proposito) e senza metadati della risorsa (server 2025-03-26), campionamento con immagini o strumenti, `roots` (risposta sempre vuota), una prova di salute periodica dei server HTTP; i server stdio girano ancora fuori dalla sandbox. Le richieste del server (campionamento, domande) sono attribuite al run con una chiamata in corso sul server (se ce ne sono più di uno, all'ultimo). **v0.4** env e header dalla UI, validazione prima del salvataggio, segreti (header ed env) mascherati nelle risposte e conservati se la maschera viene rimandata |
| Gateway OpenAI `/v1/chat/completions`, `/v1/models` e `/v1/embeddings`, gateway Anthropic `/v1/messages` | ✅ | `/v1/embeddings` usa un runtime di embedding già in esecuzione (503 in italiano altrimenti) ed è provato solo con un servizio finto; manca `/v1/responses` |
| API locale: token, CORS, Origin/Host | ✅ **v0.4** · 🟡 **token e TLS** | allowlist delle origini, protezione da DNS rebinding e CSRF; pairing con limite globale ai tentativi; header anti-framing; token legato all'origine che l'ha rilasciato. **Token per dispositivo e per app** con ambiti (`full`, `device`, `gateway`, `a2a`, `mcp`), revoca immediata (chiude gli SSE aperti), rotazione, scadenza, registro delle attività e limite ai token errati per indirizzo; `aiw tokens`, Sicurezza → Accessi. **TLS per la LAN** (`server.tls`: `self_signed` o `files`, rustls) con impronta nel pairing e fissaggio del certificato nei client Rust; Sicurezza → Rete. Provato con test su un listener TLS vero e con handshake in memoria; **mai provato su due computer reali né da un telefono**. Un browser su un altro dispositivo non può fissare il certificato: serve installare l'autorità (avviso altrimenti). Il token `owner` usato da un'altra macchina senza TLS vale solo come dispositivo |
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
| Di sistema | Apple Foundation Models (iOS e macOS), Gemini Nano e LiteRT-LM (Android) | 🟡 plugin Swift e Kotlin reali ma minimi; **macOS desktop** tramite il helper Swift `aiw-apple-fm` (processo, JSON per riga); strumenti e cronologia nativi (`Tool` e `Transcript` di Foundation Models, codice Swift condiviso con iOS in `AppleFMCore.swift`), provati su un Mac con macOS 27 e Apple Intelligence; il percorso iOS compila ma non è stato eseguito su un dispositivo |
| Solo manifest | Windows ML, MLX Swift, Core AI, ExecuTorch, MLC LLM, Phi Silica | 📄 la UI li mostra come "non ancora implementato" |
| Voce | whisper-server, piper | ✅ trascrizione (sidecar `whisper-server` o già avviato, modelli ggml) · 🟡 `piper` (manifest sidecar senza server: l'eseguibile parte per frase, voci `.onnx`; mai provato con un Piper vero) |
| Router automatico e fallback | `runtime_id: "auto"` | 🟡 `aiw-engine/src/router.rs`: ordinamento puro (`rank`) con tabella di preferenze documentata, risoluzione all'avvio del run (anche per i profili `auto:*`), evento `aiw.runtime`, fallback a catena (al massimo 2, mai a metà risposta, memoria dei fallimenti per 10 minuti, contesto ridotto sugli errori di memoria), stima della memoria anche per i dispositivi accoppiati (saltata con nota se manca il dato), `GET /api/route`, opzione «Runtime: Automatico» nei profili. Provato solo con runtime e dispositivi finti; **mai su GPU vere**; `gpu_layers` riportato ma non applicato |
| Remoti | OpenAI-compatibile, peer AIW | ✅ |

### 2.4 Acceleratori

| Acceleratore | Come viene usato | Stato |
|---|---|---|
| CPU (AVX2/AVX-512/NEON/dotprod/i8mm) | tutti i backend | ✅ |
| Metal | llama.cpp embedded (automatico su macOS/iOS), llama-server, mlx-lm | ✅ **v0.4** (prima il motore embedded non veniva registrato); non eseguito su hardware reale |
| CUDA | sidecar con `-ngl 999`, vLLM/SGLang/TRT-LLM, embedded con feature `llama-cuda` | 🟡 il lavoro facoltativo `cuda` di `release.yml` (daemon, Linux e Windows x64, CUDA 12.8) è scritto ma mai eseguito; nessun binario CUDA provato su una GPU; le release di default non lo includono |
| Vulkan | sidecar, embedded con `llama-vulkan` | 🟡 **v0.4** il job `daemon-gpu` è stato tolto dalla release con il commit 3cb23ae (recuperabile dalla storia); oggi nessun flusso costruisce Vulkan |
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
sessione aperta è stata eliminata altrove. Il profilo ha `thinking`/`thinking_budget` (Ragionamento spento, automatico o approfondito, risolto a ogni chiamata del ciclo agente e traducibile per runtime dal campo `thinking` del manifest; override per run nella richiesta di avvio) e la UI semplice parla di «Stile» e «Ragionamento». L'impostazione `ui.mode` (default `simple`) riduce le
sezioni a cinque e la prima esecuzione a un solo pulsante; `developer` (vista «Sviluppatore») mostra tutto; in modalità semplice la Panoramica mostra solo «Nuova chat», «Scarica un modello», «Il tuo assistente», approvazioni, conversazioni recenti e lezioni. Il consigliere dei modelli (`aiw-engine::adviser`, `GET /api/recommend`) sceglie per compito il modello installato e quello da scaricare; i profili `auto:chat|agent|reasoning|vision` (usati dagli agenti nuovi) sono risolti da lui all'avvio del run, e la UI semplice nasconde i modelli che non girano sul dispositivo (interruttore «Mostra tutti»). La shell desktop (`apps/desktop/src-tauri/src/desktop.rs`, plugin ufficiali tray-icon, notification, autostart e single-instance) tiene l'app viva: icona nel tray, chiusura nel tray, avvio all'accesso, avvisi per approvazioni, run finiti e download (logica in `aiw_engine::notices`, testata); da verificare in CI e a mano perché non compilata in locale.

### 2.7 Piattaforme e build

| | Windows x64 | macOS arm64 | Linux x64 | Android arm64 | iOS |
|---|---|---|---|---|---|
| App Tauri + motore in-process | ✅ | ✅ | ✅ | ✅ APK non firmato | 🟡 solo simulatore in CI |
| llama.cpp embedded | CPU | Metal **v0.4** | CPU | CPU | Metal **v0.4** |
| Computer-use | ✅ | ✅ | ❌ escluso in CI | ❌ | ❌ |
| Firma e notarizzazione | 🟡 flusso pronto, attivo solo con i segreti (PFX o Azure), mai eseguito | 🟡 flusso pronto (Developer ID + notarizzazione), attivo solo con i segreti, mai eseguito | — | ❌ | ❌ (lo store firma a parte, `store.yml`) |

Facoltativi, scritti ma **mai eseguiti su GitHub** (`release.yml`, target `arm64` o `all`): app Linux arm64 (deb/AppImage/rpm) e Windows arm64 (NSIS, solo CPU, senza computer-use), daemon arm64 e daemon CUDA 12; `release-check.yml` compila arm64 sulle pull request. Non costruiti: Android x86_64/armv7 (`store.yml` fa solo aarch64). Dalla release non escono più l'app Linux x64 né i daemon x64/macOS (commit 3cb23ae).

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

- Backup, diagnostica e migrazioni (pacchetto P3): **scritti e provati con test automatici** (archivio, manomissioni, percorsi pericolosi, schema più nuovo, migrazioni da schemi storici congelati, ambiti dei token, segreti finti nella diagnostica) e con un `aiw serve` e una CLI veri su Linux (il ripristino con il demone acceso viene rifiutato). **Non provati**: Windows e macOS (il rifiuto «dati in uso» si basa sul lock esclusivo di SQLite, verificato solo su Linux), l'interfaccia in un browser vero oltre allo smoke test su dati finti, il download dell'archivio dall'app Tauri (viaggia come base64 in JSON, tetto 64 MiB), backup di cartelle di dati grandi (tutto l'archivio passa in memoria per l'API). Il ripristino è **solo CLI con il demone fermo**: non c'è un ripristino via API. I modelli non sono mai nel backup (si riscaricano). Lo schema ha ora un numero di versione (`user_version` = 6); le migrazioni future dopo il 6 devono essere idempotenti.
- Canale `auto`: provato da un test (vedi `docs/release/RELEASE.md` §6), mai contro le release vere su GitHub. `Channel::parse` accetta `rc`, `production` e `nightly` che l'impostazione non ammette; `-beta8` conta come alpha.
- Sandbox della shell, dopo questo pacchetto:
  - **Linux**: rete chiusa di default (`sandbox.shell_network` = `off`), cartella dei dati di AIW e cartelle di credenziali (`sandbox.deny_read`) nascoste, `--unshare-pid` (senza, `/proc/<pid del demone>/root` aggirerebbe il nascondiglio), variabili segrete tolte. Provato con bubblewrap vero (test che saltano da soli se bwrap o i namespace utente mancano).
  - **macOS**: stesse regole con `sandbox-exec` (`(deny network*)`, rifiuto di lettura sui percorsi nascosti). Mai eseguito su un Mac: sono provati solo il testo del profilo e gli argomenti.
  - **Windows**: la rete dei comandi **non si può chiudere** e le cartelle **non si possono nascondere in lettura** (un token a integrità bassa limita le scritture, non le letture; servirebbero un AppContainer o regole del firewall, non implementati). Restano valide le scritture limitate al workspace e le variabili segrete tolte. Mai eseguito qui: compilato solo per `x86_64-pc-windows-gnu`.
  - Con la sandbox «Nessun contenimento», su Linux senza bwrap e su Android i comandi girano coi privilegi dell'utente (si tolgono comunque le variabili segrete).
  - Non coperti: socket dell'agente SSH e `/run/user` (su Linux restano visibili se l'utente li ha); wrapper che nascondono il comando vero (`ssh host «…»`, `docker run …`, interpreti con script propri) non vengono aperti dall'analisi della riga e una regola «Consenti» non li copre; hook, valutatori degli eval e browser controllato non passano dalla sandbox.
  - Nessuna impostazione di rete per singolo agente: i profili agente hanno solo `sandbox`; l'argomento `network` del tool può soltanto chiudere ancora di più.
- Server MCP stdio: ora hanno un campo `sandbox` (`off` | `read-only` | `workspace-write`) e chiedono conferma al primo avvio di un comando nuovo o cambiato (solo con il token `owner`). Linux provato con bwrap vero, macOS solo il profilo, **Windows non disponibile** (il server resta «non isolato»; un nuovo server con isolamento richiesto non parte). I server già presenti restano `off` («non isolato», pulsante «Isola») e sono stati approvati una volta sola in blocco alla migrazione; un server `off` eredita ancora tutto l'ambiente dell'app.
- Dipendenze (controllo di oggi): `cargo audit` 0 vulnerabilità dopo l'aggiornamento di `rusqlite` 0.37 → 0.42 (`libsqlite3-sys` 0.35 → 0.40, RUSTSEC-2026-0459) e di `async-executor` 1.14.0 → 1.14.1 (ritirata dal registro). Restano avvisi senza correzione, tutti nel ramo Tauri/GTK o in dipendenze transitive e **non ignorati**: `proc-macro-error`, `smartstring`, `ttf-parser`, `unsafe-libyaml`, `zbus_names`, `zvariant`, `zvariant_derive` (non mantenuti) e `glib` 0.18.5 (RUSTSEC-2024-0429, serve glib ≥ 0.20: richiede un aggiornamento di Tauri/GTK). `npm audit --omit=dev`: 0 vulnerabilità in `apps/ui` e `apps/desktop`.
- Token e TLS: il token `owner` di questo computer sta in chiaro nel file `token` (permessi 0600, serve alla
  CLI e alla shell desktop); il database ne tiene solo l'hash e non si cambia dall'API (cancella il file e riavvia).
  I dispositivi che si erano accoppiati prima dei token per dispositivo hanno in mano il token `owner`: il server non ha mai
  registrato chi sono, quindi non si possono migrare uno per uno; senza TLS funzionano con i soli permessi di un dispositivo e
  vanno accoppiati di nuovo per avere un token loro (revocabile). Con TLS un token `owner` remoto è `full`: finché non si
  ruota il file `token`, quei dispositivi restano onnipotenti.
- TLS: un browser su un altro dispositivo mostra un avviso finché non si installa l'autorità locale (non c'è fissaggio nel
  browser). Sul telefono la shell Tauri parla con il computer accoppiato dal motore Rust (reqwest/rustls: il fissaggio vale),
  ma aprire l'interfaccia del computer nel browser del telefono (QR) è come ogni altro browser; il codice Kotlin/Swift non fa
  richieste HTTP verso il computer. Niente di mobile è stato compilato qui. I pin dei server MCP, agenti remoti A2A inseriti a mano
  e provider esterni non esistono: usano le radici di sistema (un certificato autofirmato non funziona).
- I flussi IPC della shell Tauri non hanno token (sono il proprietario): la chiusura alla revoca vale per SSE/HTTP.
- Gli endpoint dei provider (HF, ModelScope, Ollama) non sono mai stati raggiunti dall'ambiente di
  sviluppo: i test usano server locali finti.
- Le build GPU (CUDA, Vulkan) e i percorsi nativi Apple e Android non sono stati eseguiti su
  hardware reale; lo stesso vale per il router automatico dei runtime.
- Piper e Kokoro non sono mai stati eseguiti dall'ambiente di sviluppo (solo un eseguibile e un server finti); le dimensioni delle voci Piper nel catalogo sono indicative.

## 3. Come procedere

Ordine consigliato: prima ciò che sblocca l'uso reale su hardware, poi i layer agentici mancanti.

**P0 · Verifica su hardware reale**
1. Eseguire una volta i lavori facoltativi di `release.yml` (targets = `arm64`, `cuda`) e correggere ciò che l'immagine `windows-11-arm` o l'installazione di CUDA non permettono; poi provare una build CUDA su una GPU NVIDIA vera. (Il job `daemon-gpu`/Vulkan non è più nel flusso: vedi §2.4.) Verificare llama.cpp Metal su un Mac Apple Silicon e su iPhone.
2. Provare i provider del model hub contro i servizi reali (HF, ModelScope, Ollama) e
   rigenerare l'indice Ollama da CI. Le build GPU e i percorsi nativi Apple e Android vanno
   provati su hardware reale (vedi §2.9).
3. Selezionare Xcode 26 nella CI iOS (stesso accorgimento del linker llama.cpp del job macOS), così il ramo Foundation Models viene compilato davvero.
   Il build macOS desktop usa già Xcode 26 e fallisce se il helper Swift non si compila
   (`AIW_REQUIRE_APPLE_FM=1`): provare `aiw-apple-fm` su un Mac con macOS 26 e Apple Intelligence attiva.

**P1 · Inferenza**
4. Router automatico modello → runtime → acceleratore con fallback tra runtime e stima della KV cache
   anche per le varianti remote: **fatto in gran parte** (`router.rs`, vedi §2.3). Resta da provarlo su
   hardware vero (GPU, Apple Silicon, dispositivi accoppiati reali), applicare `gpu_layers` ai runtime
   (oggi è solo riportato) e stimare la memoria dei server remoti semplici (che non la dichiarano).
5. MLX in-process con MLX Swift (macOS/iOS) al posto del solo sidecar Python.
6. Embeddings end to end: `ChatBackend::embed`, `/v1/embeddings` nel gateway, client per i runtime
   che li offrono.
7. Provisioning dei binari dei runtime (llama-server con la build giusta per l'acceleratore).

**P2 · Layer agentici**
8. Knowledge base locale (documenti → embeddings → ricerca) come tool degli agenti, poi memoria a
   lungo termine.
9. Browser: scroll, attese, schede, download, albero di accessibilità, eval e intercettazione CDP per la policy di rete ci sono (riga «Browser via CDP»); resta da provarlo su siti reali, aggiungere la stampa in PDF e gli iframe nell'albero. La ricerca web (`web_search`: SearXNG, Brave, DuckDuckGo)
   c'è, ma va provata contro i provider veri.
10. Client MCP: resources, prompts, sampling, elicitation, OAuth e riconnessione ci sono (riga «Client MCP»);
    resta da provarli con server MCP e provider OAuth veri.
11. Rewind dei checkpoint, workflow durevoli (ripresa, foreach, retry, timeout), approvazioni con
    timeout e notifiche desktop.
12. Canali (Telegram, Slack, email: fatti, vedi §2.1) e voce neurale locale: **fatta con Piper e con
    un server OpenAI-compatibile (Kokoro-FastAPI)**, ma mai provata con binari o server veri; restano da
    verificare le dimensioni delle voci nel catalogo, tenere Piper «caldo» tra una frase e l'altra e la
    voce neurale sui telefoni.

**P3 · Sicurezza e distribuzione**
13. Rete della shell chiusa di default, `~/.aiw` fuori dalla sandbox, server MCP stdio in sandbox e
    con conferma, regole di policy sulla shell che non si aggirano con comandi concatenati: **fatti**
    (vedi riga «Shell in sandbox» e §2.9; Linux provato con bwrap vero, macOS solo il testo del profilo,
    Windows senza chiusura della rete né nascondiglio in lettura né isolamento MCP). Restano: provare macOS su un Mac,
    rete chiusa e nascondiglio su Windows (AppContainer o firewall), isolare hook, valutatori e browser controllato,
    socket dell'agente SSH, protezione SSRF anche dietro proxy.
14. Token per dispositivo con revoca e TLS per la LAN: **fatti** (vedi riga «API locale» e §2.9). Restano: prova su due computer e telefoni reali, installazione guidata dell'autorità nei browser, fissaggio per i server MCP e gli agenti A2A inseriti a mano, token `owner` ruotabile senza riavvio, migrazione dei dispositivi già accoppiati (oggi si accoppiano di nuovo).
15. Firma e notarizzazione delle release (il flusso c'è, mancano i segreti e una prima prova: `docs/release/FIRMA.md`), pubblicazione su GitHub Releases (procedura in `docs/release/RELEASE.md`); target arm64 e CUDA scritti ma mai eseguiti (`release.yml`, `release-check.yml`).
16. Font inclusi nell'app invece di Google Fonts: **fatto** (woff2 latin e latin-ext nella build, CSP senza origini esterne, controllo `scripts/check-no-external.mjs` nella build). i18n it/en: **fatto per l'interfaccia, inglese non rivisto**. Il livello c'è (`shared/i18n`, impostazione `ui.language`, selettore in Impostazioni, «Altro» e primo avvio, test di parità dei cataloghi, smoke in entrambe le lingue a 390 px senza scroll orizzontale) e tutte le schermate, con le etichette e le validazioni dei moduli `model/*.ts`, sono nei cataloghi: `npm run i18n:scan` conta 58 letterali JSX, tutti tasti, comandi, segnaposto e frammenti di codice (erano 1878). Restano in italiano gli errori e i testi che arrivano dal backend (prompt degli agenti, nomi degli agenti predefiniti, riepilogo, avvisi), il menu e le notifiche del guscio desktop e i dati demo. **Nessun madrelingua ha rivisto l'inglese** e non è stato provato su dispositivo.

## iOS e iPadOS (v0.5)

- Una sola base per macOS (binario universale), iOS e iPadOS: Tauri 2 + la stessa UI React.
- iPad: tutti gli orientamenti, niente schermo intero obbligatorio (Split View e Stage Manager), iOS 17+.
  Il layout a icone laterali parte da 701 px, quindi l'iPad mini 7 (744 pt in verticale) usa la barra
  laterale, non quella da telefono. Sui dispositivi touch il tema scuro usa il nero vero (OLED).
- Modelli: Apple Foundation Models (iOS 26+, Apple Intelligence attiva) e i GGUF locali con llama.cpp
  Metal (Gemma 3 1B, Gemma 3n E2B, Llama 3.2 1B/3B, Qwen3 0.6B/1.7B nel catalogo, scaricabili dal model hub).
- Fallback: se il modello di sistema Apple fallisce prima di rispondere, la richiesta prosegue sul più
  piccolo GGUF scaricato, con una nota in chat. Senza GGUF scaricati resta l'errore originale.
- Galleria (Modelli → Galleria): schede dei modelli di sistema e dei GGUF "mobile" con Scarica, Prova (messaggio di prova, anche sui modelli scaricati) e Usa nel profilo.
- Da provare su iPad mini 7 reale: Apple FM e prestazioni Metal, nessun test su dispositivo finora.
- Browser dell'agente su iOS, iPadOS e Android: WebView in-app visibile, solo https, rete filtrata da un proxy locale in Rust (iOS 17+). Scritto ma mai eseguito su dispositivo né compilato in CI mobile. Computer use mobile: non previsto (Google Play, e iOS lo vieta).
