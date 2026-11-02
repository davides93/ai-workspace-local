# Stato dell'app e come procedere

> 2026-11-02 · v0.4 · Basato su un audit del codice (non della documentazione) eseguito per layer, con
> verifica avversariale di ogni lacuna e riferimenti `file:riga`. Le voci **v0.4** indicano cosa è
> stato chiuso in questa versione.

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

La v0.4 chiude questi tre punti e rifà la UI. Test Rust: da 82 a oltre 250. Test UI: da 0 a oltre 100
(Vitest), più uno smoke test Playwright su tutte le rotte.

## 2. Copertura per layer

Legenda: ✅ implementato end to end · 🟡 parziale · 📄 solo manifest o dichiarato · ❌ assente.

### 2.1 Layer applicativo e agentico (L5)

| Funzione | Stato | Note |
|---|---|---|
| Agenti (5 predefiniti: operator, coder, browser, desktop, planner) e loop con streaming AG-UI | ✅ | tool read-only in parallelo, max turni, self-check sugli errori dei tool |
| Subagenti (tool `agent`, contesto isolato, profondità 2) | ✅ | nessun subagente in parallelo |
| Skill `SKILL.md`, memoria `AGENTS.md` | ✅ | nessuna UI per le skill |
| Hook (UserPromptSubmit, PreToolUse, PostToolUse, Stop) | 🟡 | SessionStart mai invocato; **v0.4** editor degli hook nella UI |
| Policy (plan/ask/auto-edit/auto/yolo), regole, approvazioni, audit, kill switch | ✅ | regole solo globali; **v0.4** editor delle regole, kill switch raggiungibile da telefono |
| Code mode (Rhai), A2UI `render_ui`, worktree git per sessione | ✅ | |
| Messaggi dei run annullati o falliti | ✅ **v0.4** | prima andavano persi |
| Checkpoint prima delle scritture | 🟡 | salvati, ma il rewind non esiste |
| Workflow (tool, agent, llm, approval, condition) e trigger (manuale, cron, intervallo, file, webhook) | 🟡 | mancano foreach, sotto-workflow, retry/timeout e ripresa dopo riavvio; **v0.4** editor visuale dei passi |
| A2A 1.0 server, server MCP, delega ai peer via MCP | 🟡 | A2A senza streaming, task in memoria |
| Team multi-agente (supervisor, handoff) | ❌ | |
| Bot e canali (Telegram, Slack, email) | ❌ | |
| Documenti e RAG (knowledge base, embeddings) | ❌ | la capacità `embeddings` è solo un'etichetta nei manifest |
| Memoria a lungo termine | ❌ | |
| Voce (STT/TTS) | 📄 | `whisper-server` è solo un manifest |
| Allegati dell'utente (immagini, file) in chat | ❌ | |

### 2.2 Tool (L4) e interfacce (L6)

| Funzione | Stato | Note |
|---|---|---|
| File: read, write, edit, list, glob, grep, move | ✅ | **v0.4** confinamento anche rispetto ai symlink |
| Shell in sandbox: bwrap (Linux), sandbox-exec (macOS), integrità bassa + job object (Windows) | 🟡 | rete della shell aperta di default; `~/.aiw` leggibile |
| `web_fetch` anti-SSRF | ✅ **v0.4** | redirect rivalidati a ogni salto, DNS fissato, IPv6 e range speciali bloccati |
| Ricerca web | ❌ | nessun provider (SearXNG, Brave…) |
| Browser via CDP: navigate, snapshot, click, type, screenshot | 🟡 | mancano scroll, attese, tab, eval, download, albero di accessibilità; **v0.4** screenshot visibili nella chat |
| Computer-use: screenshot, mouse, tastiera | 🟡 | 5 azioni, solo build desktop Windows e macOS; **v0.4** screenshot visibili nella chat |
| Client MCP: stdio e streamable HTTP, `tools/list` e `tools/call` | 🟡 | mancano resources, prompts, sampling, elicitation, OAuth, list_changed e riconnessione; **v0.4** env e header dalla UI |
| Gateway OpenAI `/v1/chat/completions` e `/v1/models`, gateway Anthropic `/v1/messages` | ✅ | mancano `/v1/embeddings` e `/v1/responses` |
| API locale: token, CORS, Origin/Host | ✅ **v0.4** | allowlist delle origini, protezione da DNS rebinding e CSRF; pairing con limite globale ai tentativi |

### 2.3 Inferenza: runtime (L2–L3)

| Gruppo | Runtime | Stato |
|---|---|---|
| Sidecar con adapter OpenAI | llama-server, LM Studio, vLLM, SGLang, TensorRT-LLM, mlx-lm, mlx-vlm, Foundry Local, Lemonade, OpenVINO, LocalAI, koboldcpp, llamafile, Nexa, mistral.rs, TabbyAPI, exo | ✅ avvio, health check e chat; il binario lo installa l'utente. **v0.4**: contesto del profilo inoltrato (`-c`, `--max-model-len`…), percorso reale dei modelli locali per MLX, vLLM, SGLang, TRT-LLM e mistral.rs |
| Ollama | `ollama` | ✅ **v0.4** elenco dei modelli installati da `/api/tags`, `OLLAMA_CONTEXT_LENGTH` e `OLLAMA_KEEP_ALIVE` |
| Distribuito | llama.cpp RPC worker, exo | ✅ |
| Embedded in-process | llama.cpp (FFI), ONNX Runtime GenAI (dlopen) | ✅ **v0.4** llama.cpp ora registrato anche con Metal/CUDA/Vulkan; ORT GenAI richiede la libreria installata |
| Di sistema | Apple Foundation Models (iOS), Gemini Nano e LiteRT-LM (Android) | 🟡 plugin Swift e Kotlin reali ma minimi; Foundation Models non collegato su macOS desktop |
| Solo manifest | Windows ML, MLX Swift, Core AI, ExecuTorch, MLC LLM, Phi Silica | 📄 la UI li mostra come "non ancora implementato" |
| Voce | whisper-server | 📄 |
| Remoti | OpenAI-compatibile, peer AIW | ✅ |

### 2.4 Acceleratori

| Acceleratore | Come viene usato | Stato |
|---|---|---|
| CPU (AVX2/AVX-512/NEON/dotprod/i8mm) | tutti i backend | ✅ |
| Metal | llama.cpp embedded (automatico su macOS/iOS), llama-server, mlx-lm | ✅ **v0.4** (prima il motore embedded non veniva registrato) |
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

### 2.6 UI (v0.4)

Struttura per dominio (`app/`, `shared/`, `features/`), TanStack Query con polling mirato, router
su hash con parametri (sessione, agente e workflow nell'URL), nuova architettura dell'informazione
(Panoramica, Capacità, Integrazioni, Sicurezza), palette comandi Ctrl/Cmd+K, design system
estratto, contrasto AA in entrambi i temi, layout corretti su telefono e tablet. Dettagli in
[UI-ARCHITECTURE.md](design/UI-ARCHITECTURE.md).

### 2.7 Piattaforme e build

| | Windows x64 | macOS arm64 | Linux x64 | Android arm64 | iOS |
|---|---|---|---|---|---|
| App Tauri + motore in-process | ✅ | ✅ | ✅ | ✅ APK non firmato | 🟡 solo simulatore in CI |
| llama.cpp embedded | CPU | Metal **v0.4** | CPU | CPU | Metal **v0.4** |
| Computer-use | ✅ | ✅ | ❌ escluso in CI | ❌ | ❌ |
| Firma e notarizzazione | ❌ | ❌ | — | ❌ | ❌ |

Non costruiti: Windows arm64, Linux arm64, Android x86_64/armv7.

## 3. Come procedere

Ordine consigliato: prima ciò che sblocca l'uso reale su hardware, poi i layer agentici mancanti.

**P0 · Verifica su hardware reale**
1. Eseguire il job `daemon-gpu` (Vulkan) e aggiungere CUDA nella matrice di release desktop
   (Windows, Linux). Verificare llama.cpp Metal su un Mac Apple Silicon e su iPhone.
2. Provare i provider del model hub contro i servizi reali (HF, ModelScope, Ollama) e
   rigenerare l'indice Ollama da CI.
3. Selezionare Xcode 26 nella CI iOS, così il ramo Foundation Models viene compilato davvero.
   Collegare Foundation Models anche su macOS desktop.

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
12. Team multi-agente (supervisor e handoff), canali (Telegram, Slack, email) e voce (whisper.cpp,
    Kokoro/Piper).

**P3 · Sicurezza e distribuzione**
13. Rete della shell chiusa di default, `~/.aiw` fuori dalla sandbox, server MCP stdio in sandbox e
    con conferma, regole di policy sulla shell che non si aggirano con comandi concatenati.
14. Token per dispositivo con revoca e TLS per la LAN.
15. Firma e notarizzazione delle release, pubblicazione su GitHub Releases, target arm64.
16. Font inclusi nell'app invece di Google Fonts, i18n it/en.
