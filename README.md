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
  aiw-runtimes   32 manifest di runtime, rilevamento, adapter OpenAI/Ollama/nativo, supervisor dei sidecar
  aiw-models     parser GGUF, catalogo con compatibilità per device, download con ripresa da Hugging Face
  aiw-tools      file, ricerca, shell (bwrap/sandbox-exec), web_fetch anti-SSRF, browser CDP, computer-use, client MCP
  aiw-agent      loop agentico: streaming, tool paralleli, policy, hook, skill, subagenti, compattazione
  aiw-workflow   workflow YAML/JSON: tool/agent/llm/approval/condition, trigger cron/intervallo/file
  aiw-engine     facciata unica: SQLite, sessioni, run, approvazioni, audit, kill switch, gateway
  aiw-server     REST + SSE (AG-UI), gateway OpenAI /v1, pairing LAN, UI statica
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

### Gateway compatibile OpenAI

`http://127.0.0.1:7777/v1` espone i profili di inferenza e i modelli locali a qualsiasi client
OpenAI-compatibile (editor, SDK, CopilotKit, agenti CLI in modalità `--oss`). Dalla LAN serve il
token ottenuto con il codice di pairing a 6 cifre.

## Stato (v0.1)

| Area | Stato | Verifica |
|---|---|---|
| Core Rust (10 crate) | Implementato | 55 test unitari e di integrazione |
| Inferenza reale | llama.cpp sidecar avviato, supervisionato e fermato dal kill switch | E2E con Qwen3-1.7B su CPU: `aiw run` e chat dalla UI con tool reali |
| Catalogo runtime | 32 manifest (sidecar, embedded, sistema, LAN) con form generati | Test di validità del catalogo |
| Policy e sandbox | Modalità plan/ask/auto-edit/auto/yolo, regole, bwrap su Linux, sandbox-exec su macOS | Test: scrittura fuori dal workspace bloccata da bwrap |
| Browser CDP | navigate / snapshot / click / type / screenshot | Test con Chromium reale (`cargo test -p aiw-tools -- --ignored`) |
| MCP | Client stdio + streamable HTTP | Test con un server MCP finto via stdio |
| Workflow | Passi, condizioni, conferme, scheduler | Test dell'engine e dello scheduler |
| UI | Tutte le sezioni, collegata al backend reale | Playwright: desktop/mobile, chiaro/scuro, nessun overflow |
| Shell Tauri desktop | Compila su Linux | `cargo build -p aiw-desktop` |
| Plugin nativi mobile | Swift (Foundation Models), Kotlin (ML Kit GenAI, LiteRT-LM) | Scritti, **non compilati qui**: verifica nel workflow `release.yml` |
| Computer-use desktop | Feature `computer-use` (xcap + enigo) | Compila nelle build desktop; non testato in questo ambiente senza display |
| Windows sandbox, llama.cpp embedded su mobile, MLX Swift, Windows ML | Roadmap v0.2 | — |

Il reverse engineering usa solo artefatti pubblici e scaricabili e ne estrae idee e architettura
(metodo clean-room descritto nel documento di ricerca): nessun codice proprietario è copiato.
