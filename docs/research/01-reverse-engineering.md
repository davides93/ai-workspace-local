# 01 · Reverse engineering degli agenti di frontiera (REA)

> Stato: completato il 2026-11-01 · Strumento: [REA – Reverse Engineer Anything](https://github.com/morluto/rea) `rea-agents@6.8.0`
> Evidenze grezze compatte in [`evidence/`](evidence/).

## 1. Metodo e perimetro (clean-room)

L'obiettivo è capire **come** sono costruiti gli agenti di frontiera (idee, architetture,
protocolli) per progettare un workspace AI locale originale. Non copiamo codice.

| Fase | Cosa | Strumenti |
|---|---|---|
| Acquisizione | Solo artefatti pubblici e scaricabili (npm, download ufficiali), mai account o servizi | `npm pack`, download ufficiale Antigravity |
| Osservazione statica | Inventario dei formati, grafi di moduli JS, confini Electron/IPC, metadati Go/Rust, superficie di identificatori | `rea inspect-artifact`, `rea analyze-javascript-application`, `rea inspect-desktop-application`, `rea inspect-go-binary`, `rea inspect-rust-binary`, `strings`/`readelf` |
| Sintesi | Catalogo di pattern (sezione 4), indipendente dal codice osservato | — |
| Implementazione | Codice nuovo scritto a partire dal catalogo e da specifiche pubbliche (MCP, AG-UI, OpenAI API) | — |

Note legali e d'uso:

- Analizziamo soltanto il funzionamento per determinarne idee e principi (osservazione e studio,
  art. 5(3) della Direttiva UE 2009/24/CE) e i confini di interoperabilità (protocolli). Nessun
  bypass di licenze o protezioni e nessuna estrazione o redistribuzione di codice proprietario.
- Per i progetti open-source (Codex – Apache-2.0, Gemini CLI – Apache-2.0, OpenCode – MIT,
  CopilotKit/AG-UI – MIT, OpenDots – MIT, OpenClaw – MIT) la fonte primaria è il codice o la
  documentazione. REA serve solo per verificare cosa viene **effettivamente distribuito**.
- I conteggi di identificatori sono **segnali**: dicono che una funzionalità c'è, non come è implementata.

## 2. Artefatti analizzati

| Target | Versione | Formato rilevato | SHA-256 (prefisso) | Analisi REA | Esito / limiti |
|---|---|---|---|---|---|
| Claude Code (`@anthropic-ai/claude-code-linux-x64`) | 2.1.316 | ELF x86-64, eseguibile **Bun `--compile`** (sezione `.bun` di 181 MB) | `fd13c70000fec3f2…` | `inspect-artifact`, `extract-artifact` | REA 6.8 non riconosce questo layout del module graph Bun (supporta i layout da 36 e 52 byte) → fallback: dump della sezione `.bun` e inventario di 2.917 percorsi `/$bunfs/…` (≈2.640 chunk con code-splitting e 273 asset con nome) |
| Codex CLI (`@openai/codex@…-linux-x64`) | 0.166.0 | ELF musl statico, Rust (319 MB) + `codex-code-mode-host` (78 MB) + `codex-voice-host` + `rg` | `b8a11fedf24b2dfb…` | `inspect-rust-binary` | Binario principale oltre il limite di 256 MiB di REA; nessuna sezione `.dep-v0` (non `cargo-auditable`) → fallback: percorsi `index.crates.io/<crate>-<ver>` nelle stringhe dei panic (398 crate) |
| OpenCode (`opencode-linux-x64`) | 1.19.5 | ELF, eseguibile Bun | `7df4e4136301f165…` | `inspect-artifact` | Stesso limite di layout Bun; superficie di identificatori estratta |
| Google Antigravity (hub) | 2.28.2 | Electron (`app.asar` 4,6 MB) + `language_server` Go (191 MB) + `webm_encoder` | asar `046fa2ba9abe96bc…`, LS `8015e953e72e7070…` | `inspect-desktop-application`, `analyze-javascript-application`, `inspect-go-binary` | Analisi completa dell'asar (676 file JS, 560 moduli, 4.919 findings). Build info Go: `go1.28-20261015-RC02 … X:boringcrypto,simd` senza metadati di modulo (build interna Bazel/Blaze) → fallback: prefissi dei pacchetti dal pclntab |
| CopilotKit runtime | 1.83.0 | npm, JS | — | `analyze-javascript-application` | 533 file JS analizzati, 4.924 findings, nessun errore di parsing |
| AG-UI (`@ag-ui/core`, `@ag-ui/client`) | 1.0.3 | npm, JS/TS | — | lettura delle dichiarazioni dei tipi | 30 tipi di evento ([elenco](evidence/ag-ui-core-1.0.3.event-types.json)) |
| Gemini CLI | 0.66.0 | npm, bundle JS (96 MB) | — | — (open source → lettura diretta) | — |
| Perplexity Personal Computer | — | solo macOS/Windows, servizio in abbonamento | — | — | Nessun artefatto Linux; solo fonti pubbliche (sezione 3.7) |

## 3. Risultati per target

### 3.1 Claude Code 2.1.316 (Bun single-executable)

Superficie osservata ([conteggi](evidence/claude-code-2.1.316.identifiers.json)):

- **Tool**: `Bash`, `Read`, `Write`, `Edit`, `Glob`, `Grep`, `WebFetch`, `WebSearch`, `TodoWrite`,
  `NotebookEdit`, `AskUserQuestion`, `Skill`, `ToolSearch` (caricamento lazy degli schemi dei tool),
  `Monitor`, `EnterWorktree`, `EnterPlanMode`/`ExitPlanMode`, `LSP`, `Agent`/`Task` (subagenti).
- **Hook di ciclo di vita** (molto presenti): `PreToolUse` 287, `PostToolUse` 158, `UserPromptSubmit`,
  `SessionStart`/`SessionEnd`, `Stop`, `SubagentStart`/`SubagentStop`, `PreCompact`,
  `PermissionRequest`, `Notification`. Esiste un modulo `src/plugins/functionHooks/hooks-worker` →
  gli hook girano in un **worker isolato**.
- **Permessi**: modalità `default`/`acceptEdits`/`plan`/`bypassPermissions`/`dontAsk`, con regole
  per tool e pattern.
- **Sandbox**: `bwrap`/`bubblewrap` (60), `seccomp` (39), `sandbox-exec`/Seatbelt (macOS), `unshare`.
- **Contesto**: `compact`/`autocompact`/`microcompact`, `context_management`, `CLAUDE.md` (389),
  `AGENTS.md`, `SKILL.md` (218), `checkpoint`/`rewind`, `worktree` (1.376 → isolamento del lavoro in git worktree).
- **MCP** completo: `stdio`, `sse`, `streamable-http`, `tools/list`, `tools/call`, `resources/*`,
  `prompts/*`, `elicitation` (142), `sampling/createMessage`, `roots/list`, OAuth.
- **Interfacce headless**: `stream-json` (183), `jsonrpc`; provider `bedrock`/`vertex` (gateway cloud).
- **Asset bundlati**: 273 file con nome (documentazione delle skill in Markdown compressa zstd,
  template come `simple_plan`, `visual_plan`, `three_subagents_with_critique`) → **skill e prompt
  come dati versionati dentro il binario**, caricati con progressive disclosure.

### 3.2 Codex CLI 0.166.0 (Rust)

Dipendenze ([398 crate](evidence/codex-0.166.0.crates.txt)) e superficie ([conteggi](evidence/codex-0.166.0.identifiers.json)):

| Segnale | Interpretazione architetturale |
|---|---|
| `starlark` 0.14 (748 occorrenze) nel binario **e** nel processo separato `codex-code-mode-host` | **Code mode**: il modello scrive un piccolo programma in un linguaggio ermetico e deterministico (Starlark, simile a Python) che orchestra più tool in un solo turno, eseguito in un processo host separato |
| `axum`, `websocket`, `app-server` (170), `thread/start`, `turn/start` | Core esposto come **server JSON-RPC** (stdio/WebSocket) con gerarchia Thread → Turn → Item; TUI, IDE e app desktop sono client |
| `bubblewrap` (57), `seccompiler`, `seccomp`, `sandbox_mode` (`read-only`/`workspace-write`/`danger-full-access`), `approval_policy` (`untrusted`/`on-request`/`never`) | Sandbox a livello di sistema operativo **separata** dalla politica di approvazione |
| `portable-pty`, `exec_command`, `write_stdin`, `unified_exec` | Esecuzione di comandi in **PTY persistenti** (sessioni interattive in cui il modello scrive su stdin) |
| `tree-sitter` | Parsing dei comandi shell per classificare quelli sicuri o in sola lettura |
| `apply_patch`, `diffy`, `similar`, `undo`, `snapshot`, `ghost_commit` | Modifiche come patch, annullabili con snapshot git |
| `sqlx-sqlite` | Indice locale di sessioni e thread in SQLite |
| `notify`, `ignore`, `globset` (+ `rg` incluso) | Ricerca file stile ripgrep e file watching |
| `keyring`, `oauth2` | Credenziali nel portachiavi del sistema operativo; OAuth per MCP e login |
| `ollama` (7), `lmstudio`, `oss` | Provider **locali** di prima classe (`--oss`) |
| `spawn_agent`, `multi_agent`, `subagent`, `hooks`, `plugins`, `memories`, `SKILL.md`, `AGENTS.md` | Subagenti, hook, plugin, memoria persistente e skill |
| `opentelemetry*`, `tonic` | Telemetria OTLP |

### 3.3 OpenCode 1.19.5 (Bun)

Superficie ([conteggi](evidence/opencode-1.19.5.identifiers.json)): `session` (1.098), `snapshot`,
`LSP` (295), `permission`, `plugins`, `worktree`, `compact`, `openai`/`anthropic` (multi-provider),
`acp` (Agent Client Protocol), `jsonrpc`, `lmstudio`, `oss`. È noto e verificabile dal sorgente MIT
che usa un'architettura **client/server**: TUI, desktop e SDK parlano con un server HTTP locale.
Il segnale `LSP` alto conferma la **diagnostica LSP come feedback** al modello dopo le modifiche.

### 3.4 Google Antigravity 2.28.2 (Electron + Go)

**Confine Electron** (REA `analyze-javascript-application`): 4 BrowserWindow, 13 API
`contextBridge` (45 membri esposti), 88 operazioni IPC (45 canali letterali, 39 handler nel main),
42 osservazioni di validazione del mittente, nessun addon nativo. [Canali](evidence/antigravity-2.28.2.ipc-channels.txt)
rilevanti: `script-jail:run/abort` e `agy-script-jail:port` (**esecuzione di script in un jail**),
`webview:execute-js-isolated-world` (browser incorporato con JS in un *isolated world*),
`wsl:connect`/`wsl:get-state` (backend nel WSL su Windows), `agent:update-active-count`, `storage:*`, `updater:*`.

**Cervello in Go** (`language_server`, codename *jetski*, [115 pacchetti](evidence/antigravity-2.28.2.cortex-packages.txt)), per frequenza:
`connectrpc` (RPC protobuf tra UI e server), `jetski/cortex` (motore agentico), `cel/go` (**policy in CEL**),
`cdp` (Chrome DevTools Protocol → subagente browser), `pion` (WebRTC per lo streaming), `gorm` (SQLite),
`gitv5`, `opentelemetry`, `devtools/ai/sandbox`, `gdm/security/agi_control`.

Sotto-pacchetti di `cortex` → mappa del motore:

| Area | Pacchetti | Pattern |
|---|---|---|
| Esecuzione | `trajectory`, `dbtrajectory`, `trajectory_store`, `cascade_run_state`, `autorun`, `accumulator` | Il run è una **traiettoria di step persistita su DB** |
| Ripresa/ramificazione | `rehydration`, `fork`, `revert`, `snapshot_recorder/{gitclient,gitbundle}` | Checkpoint come **git bundle**, fork e revert della traiettoria |
| Sicurezza | `policyguardian/{vetting,profiles,deterministic_vetting_configs}`, `permissions`, `command/sandboxproxy` | **Guardiano di policy** (regole deterministiche + vetting) prima dell'esecuzione; comandi tramite un proxy sandbox |
| Hook | `core/{prehooks,pretoolhooks,posttoolhooks,posthooks}`, `executors/stophooks`, `customization/hooks` | Pipeline di hook a più stadi |
| Tool | `tools/{tools,code,notebook,subagent,tooloverrides}`, `core/tooldecorators`, `subagents`, `slashcommands` | Registry di tool con decoratori e override |
| Browser/media | `agentbrowser`, `utils/webm`, `utils/media` | Subagente browser con **registrazione video** delle azioni |
| Prodotto | `artifacts`, `workspaces`, `battleworkspace`, `cost`, `tokens`, `truncator`, `systemprompt`, `providers/mcp` | Artefatti (piani, walkthrough), confronto tra modelli, contabilità di costi e token, MCP |

### 3.5 CopilotKit / AG-UI / OpenDots

- **AG-UI 1.0.3**: protocollo a eventi agente→UI (`RUN_*`, `TEXT_MESSAGE_*`, `TOOL_CALL_*`,
  `STATE_SNAPSHOT`/`STATE_DELTA` con JSON Patch, `MESSAGES_SNAPSHOT`, `ACTIVITY_*`, `REASONING_*`,
  `SUBAGENT_*`, `CUSTOM`). È il layer "runtime" sotto A2UI (UI dichiarativa) e MCP Apps (UI aperta).
- **CopilotKit runtime 1.83**: REA ricostruisce 533 file con un alto numero di relazioni
  `exposes`/`calls`/`awaits`: un runtime server che adatta agenti eterogenei (LangGraph, ADK,
  Mastra…) allo stream AG-UI.
- **OpenDots** (template MIT di CopilotKit): "colleghi AI sempre attivi", **ognuno con il proprio
  computer** (container con browser e file persistenti, permessi browser/file/shell per agente),
  canali testo, voce (WebRTC) e Slack, MCP per agente con approvazione dei tool non read-only,
  lavoro in background schedulato, apprendimento che pubblica skill.

### 3.6 OpenClaw e agenti personali

Pubblico, MIT. Gateway sempre attivo (control plane) → adapter di canale → runtime agente + tool;
memoria su file (`AGENTS.md`, `SOUL.md`, `TOOLS.md`), registry di skill (ClawHub) con precedenza
workspace > utente > bundled. Lezione di sicurezza: un registry pubblico di skill ha richiesto la
rimozione di migliaia di skill malevole → **skill firmate e in quarantena** di default.

### 3.7 Perplexity Personal Computer

Agente desktop 24/7 (Mac da maggio 2026, Windows da luglio 2026) che orchestra molti modelli
(routing per task), con approvazione delle azioni sensibili, **audit trail completo per sessione** e
**kill switch**. Ibrido locale/cloud. Non analizzabile staticamente da Linux: usiamo solo fonti pubbliche.

## 4. Catalogo dei pattern (input per l'architettura)

| ID | Pattern | Evidenze | Adozione in AI Workspace Local |
|---|---|---|---|
| P01 | **Core headless + client multipli** su protocollo tipizzato | Codex app-server JSON-RPC, OpenCode client/server, Antigravity ConnectRPC | `aiw-server` (HTTP+SSE) e IPC Tauri condividono lo stesso core in Rust |
| P02 | Gerarchia **Thread → Turn → Item** con eventi incrementali | Codex `thread/start`, `turn/start`, `item/*` | Modello `Session → Run → Event` |
| P03 | **Stream di eventi standard verso la UI** | AG-UI (CopilotKit, OpenDots) | Eventi del run serializzati come AG-UI 1.0 |
| P04 | Loop agentico con tool calling parallelo per i tool read-only | tutti | `aiw-agent::AgentLoop` |
| P05 | **Permessi a strati**: modalità + regole allow/ask/deny per tool o pattern + approvazione | Claude Code, Codex `approval_policy`, Antigravity `permissions` | `PolicyEngine` con regole a pattern e modalità |
| P06 | **Policy guardian** deterministico prima dell'esecuzione | Antigravity `policyguardian` + CEL | Regole dichiarative valutate prima di ogni tool, con log di audit |
| P07 | **Sandbox OS separata dalla policy** (bwrap/seccomp, Seatbelt, AppContainer) | Codex, Claude Code | `SandboxProfile` (read-only / workspace-write / full) che avvolge il processo |
| P08 | **Hook** di ciclo di vita (Pre/PostToolUse, UserPromptSubmit, Stop, SessionStart, PreCompact) | Claude Code, Codex, Antigravity | `HookBus` con hook comando o builtin |
| P09 | **Subagenti** con contesto isolato | Task/Agent, `spawn_agent`, `subagents` | Tool `agent` che avvia un run figlio con un profilo diverso |
| P10 | **Compattazione del contesto** automatica a soglia | `autocompact`, `compact` | `ContextManager` con budget di token e riassunto |
| P11 | **Memoria di progetto su file** (AGENTS.md/CLAUDE.md) e **skill** (SKILL.md, progressive disclosure) | tutti | Caricamento di `AGENTS.md` e skill con frontmatter, indicizzate nel system prompt |
| P12 | **MCP client completo** (stdio/HTTP, elicitation, OAuth) | tutti | `aiw-tools::mcp` (stdio + streamable HTTP) |
| P13 | **Code mode** (programma in linguaggio ermetico che orchestra tool) | Codex `starlark` + host separato, script-jail di Antigravity | Tool `code` con interprete embedded senza I/O diretto: può solo chiamare i tool consentiti |
| P14 | **PTY persistenti** per comandi interattivi | Codex `portable-pty`, `write_stdin` | Tool `shell` con sessioni |
| P15 | **Checkpoint/rewind** del filesystem (snapshot git, ghost commit, git bundle) | Codex, Claude Code, Antigravity | `Checkpointer` che salva i file prima di ogni modifica |
| P16 | **Isolamento con git worktree** per lavori paralleli | Claude Code `worktree`, OpenCode | Workspace task in worktree separati (roadmap) |
| P17 | **Traiettoria persistita** e riprendibile (rehydration, fork) | Antigravity `trajectory_store` | Eventi su SQLite: replay, resume, fork |
| P18 | **Subagente browser** via CDP con registrazione | Antigravity `agentbrowser`, `cdp`, `webm` | Tool `browser` CDP-first (snapshot DOM/AX + screenshot) |
| P19 | **Computer-use** via screenshot + input + albero di accessibilità | Perplexity PC, OpenDots computers | Tool `computer` (desktop) e AccessibilityService (Android) |
| P20 | **Provider locali di prima classe** | Codex `--oss` (Ollama/LM Studio) | Runtime Registry multi-backend (sezione architettura) |
| P21 | **Skill/plugin come pacchetti versionati** con precedenza e quarantena | Claude Code asset bundlati, OpenClaw ClawHub | Plugin con manifest e permessi dichiarati, non attivi finché l'utente non li approva |
| P22 | **Audit trail + kill switch** | Perplexity PC | Log immutabile per run, stop globale |
| P23 | **Diagnostica LSP come feedback** dopo le modifiche | OpenCode, Claude Code `LSP` | Hook post-edit che esegue linter e type-check (roadmap) |
| P24 | **Telemetria opzionale** OTLP | Codex, Antigravity | Disattivata di default; esportatore OTLP opzionale |
