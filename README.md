# AI Workspace Local

Workspace AI **locale** e multipiattaforma (Windows, macOS, Linux, Android, iOS) che riunisce sotto
un'unica API **tutti i metodi di inferenza on-device**: llama.cpp, Ollama, LM Studio, vLLM, SGLang,
TensorRT-LLM, MLX, Foundry Local, Windows ML, LiteRT-LM, Gemini Nano, Apple Foundation Models,
ExecuTorch, MLC e altri. Sopra ci sono layer agentici per i **task operativi**, non solo per il
coding: file, shell in sandbox, browser via CDP, computer-use, MCP e workflow schedulati con
conferme umane.

![Conferma di un'azione dell'agente nella demo interattiva: la shell attende il consenso dell'utente](docs/design/screenshots/chat-conferma.png)

## Documenti

| | |
|---|---|
| Reverse engineering (REA) degli agenti di frontiera | [docs/research/01-reverse-engineering.md](docs/research/01-reverse-engineering.md) |
| Panorama runtime, modelli e protocolli (nov 2026) | [docs/research/02-frontier-landscape.md](docs/research/02-frontier-landscape.md) |
| Stato dell'app per layer, limiti noti e come procedere (v0.4) | [docs/STATUS.md](docs/STATUS.md) |
| Architettura e ADR | [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) |
| Model hub multi-provider: contratto API e provider | [docs/architecture/model-hub.md](docs/architecture/model-hub.md) |
| Design system e flussi | [docs/design/DESIGN.md](docs/design/DESIGN.md) |
| Architettura della UI: struttura, router, data layer, design system | [docs/design/UI-ARCHITECTURE.md](docs/design/UI-ARCHITECTURE.md) |
| Parlare con l'assistente da Telegram, Slack e email: guida all'attivazione e alla sicurezza | [docs/channels.md](docs/channels.md) |
| Mockup interattivo (file unico, si apre offline) | [docs/design/mockup/index.html](docs/design/mockup/index.html) |

## Struttura

```
crates/
  aiw-core       tipi API, eventi AG-UI 1.0, schemi delle impostazioni, configurazione a strati
  aiw-hw         probe hardware: CPU, RAM, CUDA/ROCm/Metal/Vulkan/DirectML, NPU → classe del device
  aiw-runtimes   33 manifest di runtime, rilevamento, adapter OpenAI/Ollama/nativo, supervisor dei sidecar, client di trascrizione (whisper.cpp e API OpenAI)
  aiw-models     model hub multi-provider (Hugging Face, ModelScope, registry Ollama, URL diretto), parser GGUF, compatibilità per device (anche i modelli Whisper), download con ripresa
  aiw-tools      file, ricerca, shell (bwrap/sandbox-exec), web_fetch anti-SSRF, web_search, browser CDP, computer-use, client MCP
  aiw-agent      loop agentico: streaming, tool paralleli, policy, hook, skill, subagenti, compattazione
  aiw-workflow   workflow YAML/JSON: tool/agent/llm/approval/condition, trigger cron/intervallo/file
  aiw-engine     facciata unica: SQLite, sessioni, run, approvazioni, audit, kill switch, gateway, voice mode
  aiw-server     REST + SSE (AG-UI), gateway OpenAI /v1, pairing LAN, UI statica
  aiw-winsandbox sandbox Windows: integrità bassa e job object (usata da aiw-tools)
  aiw-cli        `aiw`: serve, doctor, runtimes, models, profile, run, workflow
apps/
  ui             UI React unica per desktop, mobile e browser (transport Tauri/HTTP/mock)
  desktop        shell Tauri 2 (desktop + mobile) e plugin nativo Swift/Kotlin per i modelli di sistema
```

## Avvio rapido

Prerequisiti: Rust 1.85+, Node 22+. Per usare un modello locale serve almeno un runtime:
`llama-server` di llama.cpp nel `PATH` (oppure Ollama, LM Studio…).

```bash
cargo build --release -p aiw-cli
```

```bash
./target/release/aiw doctor
```

```bash
./target/release/aiw models pull qwen3-1.7b
```

Altri provider del model hub (`aiw models search|show|pull|rm|providers`):

```bash
./target/release/aiw models providers
./target/release/aiw models search qwen --provider hf --format gguf --sort downloads --limit 10
./target/release/aiw models show hf Qwen/Qwen3-4B-GGUF
./target/release/aiw models pull --provider hf --model Qwen/Qwen3-4B-GGUF --variant Q4_K_M
./target/release/aiw models pull --url https://example.com/modello.gguf
```

```bash
./target/release/aiw profile --runtime llama-server --model qwen3-1.7b-q8_0 --context 8192
```

```bash
./target/release/aiw run "Crea il file note.md con un riepilogo della cartella" --agent operator
```

Interfaccia web servita dal daemon (anche dal telefono, dopo il pairing):

```bash
npm ci --prefix apps/ui && npm run build --prefix apps/ui
```

```bash
./target/release/aiw serve --ui apps/ui/dist
```

App desktop (Tauri 2; su Linux servono `libwebkit2gtk-4.1-dev` e simili):

```bash
npm ci --prefix apps/desktop && npm run dev --prefix apps/desktop
```

Mobile: `npm run android:init` / `npm run ios:init` in `apps/desktop`, poi `android:dev` / `ios:dev`.

### Gateway e protocolli

`http://127.0.0.1:7777` espone i modelli locali e gli agenti a qualsiasi client. Dalla LAN serve il
token ottenuto con il codice di pairing a 6 cifre.

| Endpoint | Per cosa |
|---|---|
| `/v1/chat/completions`, `/v1/models` | Client OpenAI-compatibili (editor, SDK, CopilotKit, agenti CLI `--oss`) |
| `/v1/messages` | Client Anthropic Messages, streaming e tool compresi |
| `/v1/audio/transcriptions` | Trascrizione (multipart OpenAI) con il whisper di questo dispositivo |
| `/mcp` | Server MCP: strumenti e agenti locali per altre app o dispositivi |
| `/a2a`, `/.well-known/agent-card.json` | A2A 1.0 (JSON-RPC, anche in streaming SSE): altri agenti affidano task a quelli locali |
| `/hooks/{id}` | Trigger webhook dei workflow (segreto o firma HMAC-SHA256 stile GitHub) |

### Voce

Serve un motore di trascrizione: `whisper-server` di whisper.cpp (su macOS `brew install whisper-cpp`)
più un modello Whisper, oppure LocalAI, oppure un dispositivo accoppiato che ce l'ha.

```bash
./target/release/aiw models pull whisper-small-q8
```

Nella chat il microfono detta il messaggio; l'icona accanto avvia la conversazione a voce: parli,
l'agente lavora e risponde con una voce locale del sistema, poi torna in ascolto. Nel browser il
microfono richiede `localhost` o HTTPS.

### Rete di dispositivi

```bash
./target/release/aiw serve --lan --ui apps/ui/dist
```

In Dispositivi compaiono gli altri AI Workspace trovati via mDNS e il QR: il telefono lo inquadra e
apre l'interfaccia già accoppiata. Un dispositivo accoppiato presta i suoi modelli (runtime
`aiw-peer`), i suoi strumenti e agenti (server MCP) e la sua trascrizione.

## Stato (v0.4)

Dettagli, limiti noti e prossimi passi in [docs/STATUS.md](docs/STATUS.md).

| Area | Stato |
|---|---|
| Core Rust (11 crate) | Implementato; 357 test |
| Inferenza | 33 manifest; sidecar OpenAI-compatibili, Ollama, llama.cpp e ONNX Runtime GenAI in-process; 6 runtime solo manifest |
| Modelli | Model hub: Hugging Face, ModelScope, registry Ollama, URL diretto; download con ripresa e sha256 |
| Allegati | Immagini e file nella chat (graffetta, trascinamento, incolla): testo e PDF con testo letti dal modello, immagini ai modelli con visione |
| Documenti | Raccolte di file locali (testo, Markdown, codice, PDF con testo) che l'assistente cerca per parole chiave e, con un modello di embedding avviato, per significato; pagina «Documenti», `aiw kb` e gateway `/v1/embeddings` |
| Canali e posta | Parla con l'assistente da Telegram, Slack o email (spenti di serie, solo mittenti in elenco, di serie in sola lettura); posta IMAP in sola lettura per il riepilogo e gli strumenti; pagina «Canali», `aiw channels`, guida in [docs/channels.md](docs/channels.md). Mai provato contro i servizi veri |
| Voce | Trascrizione con whisper.cpp o con server OpenAI-compatibili (LocalAI, Nexa, dispositivo accoppiato); dettatura e conversazione a voce nella chat con le voci locali del sistema |
| Policy e sandbox | Cinque modalità, regole, audit, kill switch; bwrap (Linux), sandbox-exec (macOS), integrità bassa + job object (Windows) |
| Tool | File, shell, `web_fetch` anti-SSRF, `web_search` (SearXNG, Brave, DuckDuckGo), browser CDP, computer-use, client MCP |
| Agenti e workflow | Loop con subagenti, hook, skill; workflow con trigger manuale, cron, intervallo, file, webhook |
| API | REST + SSE, gateway OpenAI e Anthropic, server MCP, A2A; token, CORS ristretto, controllo di Origin/Host |
| UI | React unica per desktop, mobile e web; 629 test Vitest e smoke test Playwright (29 rotte, 2 larghezze, 2 temi) |
| Piattaforme | Tauri su Windows, macOS, Linux, Android; iOS: app per simulatore (senza firma), costruita in CI |
| Non verificato | Build GPU (CUDA, Vulkan), percorsi nativi Apple e Android su hardware reale, provider contro i servizi reali |

Il reverse engineering usa solo artefatti pubblici e scaricabili e ne estrae idee e architettura
(metodo clean-room descritto nel documento di ricerca): nessun codice proprietario è copiato.
