# AI Workspace Local

Workspace AI **locale** e multipiattaforma (Windows, macOS, Linux, Android, iOS) che riunisce sotto
un'unica API **tutti i metodi di inferenza on-device**: llama.cpp, Ollama, LM Studio, vLLM, SGLang,
TensorRT-LLM, MLX, Foundry Local, Windows ML, LiteRT-LM, Gemini Nano, Apple Foundation Models,
ExecuTorch, MLC e altri. Sopra ci sono layer agentici per i **task operativi**, non solo per il
coding: file, shell in sandbox, browser via CDP, computer-use, MCP e workflow schedulati con
conferme umane.

![Conferma di un'azione dell'agente, con runtime e modello locali reali](docs/design/screenshots/chat-conferma.png)

## Documenti

| | |
|---|---|
| Reverse engineering (REA) degli agenti di frontiera | [docs/research/01-reverse-engineering.md](docs/research/01-reverse-engineering.md) |
| Panorama runtime, modelli e protocolli (nov 2026) | [docs/research/02-frontier-landscape.md](docs/research/02-frontier-landscape.md) |
| Architettura e ADR | [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) |
| Design system e flussi | [docs/design/DESIGN.md](docs/design/DESIGN.md) |
| Mockup interattivo (file unico, si apre offline) | [docs/design/mockup/index.html](docs/design/mockup/index.html) |

## Struttura

```
crates/
  aiw-core       tipi API, eventi AG-UI 1.0, schemi delle impostazioni, configurazione a strati
  aiw-hw         probe hardware: CPU, RAM, CUDA/ROCm/Metal/Vulkan/DirectML, NPU → classe del device
  aiw-runtimes   33 manifest di runtime, rilevamento, adapter OpenAI/Ollama/nativo, supervisor dei sidecar,
                 llama.cpp e ONNX Runtime GenAI in-process, client di trascrizione (whisper.cpp/OpenAI)
  aiw-models     parser GGUF, catalogo con compatibilità per device (anche modelli Whisper), download con ripresa
  aiw-tools      file, ricerca, shell (bwrap/sandbox-exec/integrità Low su Windows), web_fetch anti-SSRF,
                 browser CDP, computer-use, client MCP
  aiw-winsandbox sandbox Windows: token a integrità Low, job object, window station e desktop dedicati
  aiw-agent      loop agentico: streaming, tool paralleli, policy, hook, skill, subagenti, compattazione,
                 renderer A2UI (render_ui), code mode Rhai, controllo automatico degli errori
  aiw-workflow   workflow YAML/JSON: tool/agent/llm/approval/condition, trigger cron/intervallo/file/webhook
  aiw-engine     facciata unica: SQLite, sessioni, run, worktree git, approvazioni, audit, kill switch,
                 pairing tra dispositivi, voice mode
  aiw-server     REST + SSE (AG-UI), gateway OpenAI e Anthropic, server MCP, A2A 1.0, mDNS e QR, UI statica
  aiw-cli        `aiw`: serve, doctor, runtimes, models, profile, run (anche in worktree), workflow
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
| `/a2a`, `/.well-known/agent-card.json` | A2A 1.0 (JSON-RPC): altri agenti affidano task a quelli locali |
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

## Stato (v0.3)

| Area | Stato | Verifica |
|---|---|---|
| Core Rust (11 crate) | Implementato | 87 test unitari e di integrazione su Linux, più i test reali della sandbox su Windows |
| Build di rilascio | Windows, macOS universale, Linux, Android (APK arm64), iOS (app per simulatore, senza firma), daemon ×3 | `release.yml`: tutti i job verdi |
| Inferenza reale | llama.cpp sidecar avviato, supervisionato e fermato dal kill switch | E2E con Qwen3-1.7B su CPU: `aiw run` e chat dalla UI con tool reali |
| Catalogo runtime | 33 manifest (sidecar, embedded, sistema, LAN) con form generati | Test di validità del catalogo |
| llama.cpp in-process | Su tutte le piattaforme (Metal su Apple), runtime `llama-embedded` | E2E con Qwen3-1.7B: worktree, A2UI e tool reali |
| ONNX Runtime GenAI | Caricato a runtime (CPU, CUDA, DirectML, QNN, OpenVINO, CoreML, WebGPU) | Una run reale con Qwen2.5-0.5B int4 (~43 tok/s su CPU) |
| Policy e sandbox | Modalità plan/ask/auto-edit/auto/yolo, regole, bwrap su Linux, sandbox-exec su macOS, integrità Low + job object su Windows | Test reali su Linux e sul runner Windows (scrittura solo nel workspace, timeout dell'albero) |
| Worktree git | Sessioni isolate su un branch `aiw/…`, diff, applica, scarta | Test e verifica Playwright con modello reale |
| A2UI v0.9 | Tool `render_ui` (18 componenti), azioni dell'utente all'agente | Playwright desktop/telefono |
| Code mode | Script Rhai ermetici: ogni `tool()` passa da policy, conferme e audit | Test con limiti e tentativi negati |
| Rete di dispositivi | mDNS, QR di pairing, peer come runtime e server MCP, A2A 1.0 | Due daemon reali: scoperta, pairing dalla UI, telefono accoppiato dal link del QR |
| Voice mode | Trascrizione locale (whisper.cpp, server OpenAI-compatibili, peer), dettatura, conversazione a voce con voci locali | Test con whisper-server finto; Playwright con microfono finto: WAV 16 kHz, VAD, lettura della risposta |
| Workflow webhook | `POST /hooks/{id}` con segreto o firma HMAC | Test RFC 4231 e flusso HTTP completo |
| Browser CDP | navigate / snapshot / click / type / screenshot | Test con Chromium reale (`cargo test -p aiw-tools -- --ignored`) |
| MCP | Client stdio + streamable HTTP | Test con un server MCP finto via stdio |
| Workflow | Passi, condizioni, conferme, scheduler | Test dell'engine e dello scheduler |
| UI | Tutte le sezioni, collegata al backend reale | Playwright: desktop/mobile, chiaro/scuro, nessun overflow |
| Shell Tauri desktop | Compila su Linux | `cargo build -p aiw-desktop` |
| Plugin nativi mobile | Swift (Foundation Models), Kotlin (ML Kit GenAI, LiteRT-LM) | Scritti, **non compilati qui**: verifica nel workflow `release.yml` |
| Computer-use desktop | Feature `computer-use` (xcap + enigo) | Compila nelle build desktop; non testato in questo ambiente senza display |
| MLX Swift su iOS, Kokoro TTS, trascrizione nativa su mobile, catalogo EP di Windows ML | Roadmap v0.4 | — |

Il reverse engineering usa solo artefatti pubblici e scaricabili e ne estrae idee e architettura
(metodo clean-room descritto nel documento di ricerca): nessun codice proprietario è copiato.
