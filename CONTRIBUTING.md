# Contribuire ad AI Workspace Local

**English summary.** AI Workspace Local (AIW) is a Rust workspace (daemon/CLI `aiw`, Tauri 2 desktop and mobile shell) with a
React UI. Build the daemon with `cargo build --release -p aiw-cli`; run the UI in dev mode with `npm ci && npm run dev` in
`apps/ui` (it proxies `/api` to a daemon on `127.0.0.1:7777`). CI runs `cargo fmt --all --check`, `cargo clippy --all-targets`,
`cargo test`, and in `apps/ui` `npm run typecheck`, `npm test`, `npm run build`, `npm run build:mockup`, `npm run test:smoke`, `npm run test:e2e`, `npm run test:a11y`:
please run the same before opening a pull request. Code comments are in English; user-visible strings are in Italian **and**
English through the i18n catalog under `apps/ui/src/shared/i18n/`. Update `CHANGELOG.md` ("Non rilasciato") and keep
`docs/STATUS.md` honest: "written" is not "tested". Security problems go to [SECURITY.md](SECURITY.md), privately. **The
project has no licence yet, so contributions cannot be formally accepted until the owner chooses one.** Releases and signing:
[docs/release/RELEASE.md](docs/release/RELEASE.md).

---

## Licenza

La licenza del progetto non è ancora definita: finché il proprietario non ne sceglie una, non è possibile accettare formalmente
contributi esterni. Puoi comunque aprire una issue per segnalare errori o proporre idee.

## Cosa serve per lavorare

Per tutti i sistemi: **Rust 1.85 o più nuovo** (`rust-version` del workspace; la CI usa lo stable), **Node 22**, **git**.
Per un modello locale serve almeno un runtime (per esempio `llama-server` di llama.cpp nel `PATH`, oppure Ollama o LM
Studio); il motore llama.cpp incorporato (`--features llama-embedded`) richiede in più **CMake**, un compilatore C++ e
**libclang** (lo usa `bindgen`).

| Sistema | Per il daemon e i test Rust | In più per l'app desktop (Tauri) |
| --- | --- | --- |
| Linux | `build-essential`, `pkg-config`; `bubblewrap` per i test della sandbox della shell (`sudo apt-get install -y bubblewrap`, come in CI) | `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev` (lo stesso elenco di `ci.yml`) |
| macOS | Xcode Command Line Tools | Xcode; per il helper di Apple Foundation Models serve Xcode 26 (`release.yml` lo seleziona da solo) |
| Windows | Visual Studio Build Tools con «Sviluppo di applicazioni desktop con C++»; **PowerShell** (non la bash di Git, il suo `link.exe` oscura quello di MSVC) | WebView2 (già presente su Windows 11) |

Per Android e iOS: `npm run android:init` / `npm run ios:init` in `apps/desktop` (servono Android SDK/NDK o Xcode); i passi
esatti usati in CI sono in `.github/workflows/store.yml`.

## Compilare ed eseguire

Dalla radice del repository:

```bash
cargo build --release -p aiw-cli        # il daemon e la CLI `aiw`
./target/release/aiw doctor             # controlla runtime e hardware
./target/release/aiw serve --ui apps/ui/dist   # API su 127.0.0.1:7777 + interfaccia web
```

La UI (`apps/ui`, React + Vite) si compila prima di essere servita dal daemon o incorporata nella shell Tauri:

```bash
npm ci --prefix apps/ui
npm run build --prefix apps/ui
```

Per sviluppare la UI con ricarica a caldo, in due terminali:

```bash
./target/release/aiw serve                 # terminale 1: il daemon su 127.0.0.1:7777
cd apps/ui && npm run dev                  # terminale 2: Vite su http://localhost:5173 (inoltra /api e /v1 al daemon)
```

Senza daemon la UI funziona lo stesso in modalità demo: `npm run build:mockup` produce un file unico (`dist-mockup/`) con
dati finti, lo stesso che usa il test di fumo.

L'app desktop (Tauri 2):

```bash
npm ci --prefix apps/desktop
npm run dev --prefix apps/desktop          # tauri dev
npm run build --prefix apps/desktop        # tauri build: i pacchetti (la CI usa --bundles e --features, vedi release.yml)
```

Funzioni opzionali dei motori: `--features llama-embedded` (llama.cpp incorporato, CPU), `llama-metal` (macOS),
`llama-vulkan`, `llama-cuda` (le ultime due richiedono SDK Vulkan o CUDA Toolkit). Sull'app: `computer-use`, `cuda`, `vulkan`;
`mas` è la build sandbox del Mac App Store ([docs/store/README.md](docs/store/README.md)) e non si combina con `computer-use`.

## Provare le modifiche

La CI (`.github/workflows/ci.yml`) esegue queste prove: fai lo stesso in locale prima di aprire una pull request.

```bash
cargo fmt --all --check                     # rustfmt.toml: larghezza 140
cargo clippy --all-targets
cargo test                                  # tutto il workspace
cargo test -p aiw-engine                    # o un crate solo (aiw-core, aiw-tools, aiw-server, …)
cargo test -p aiw-engine nome_del_test      # o un solo test per nome
```

Nella cartella `apps/ui`:

```bash
npm run typecheck                           # tsc -b --noEmit
npm test                                    # vitest run
npm run build                               # tsc -b + vite build + controllo «niente host esterni» nel risultato
npm run build:mockup                        # la demo a file unico
npx playwright install --with-deps chromium   # una volta sola
npm run test:smoke                          # ogni pagina della demo a 1440 e 390 px, tema chiaro e scuro (serve build:mockup)
npm run test:e2e                            # i flussi principali contro la demo (Playwright Test; serve build:mockup)
npm run test:a11y                           # axe-core su ogni rotta, chiaro/scuro, 390/1440 px, più la prova da tastiera (serve build:mockup)
npm run check:bundle                        # la dimensione di dist e dist-mockup resta sotto il budget (serve build e build:mockup)
npm run bench:startup                       # tempo fino alla prima schermata utilizzabile della demo (solo misura, nessuna soglia)
```

Il test di fumo accetta `-- --route=<percorso>` per una sola pagina e `-- --shots=<cartella>` per salvare le schermate.

I test E2E e di accessibilità sono in `apps/ui/e2e/` (`flows/` e `a11y/`), si lanciano con Playwright Test contro
`dist-mockup/index.html` aperto da file: niente server, niente rete, dati di esempio sempre uguali. Per un solo test:
`npm run test:e2e -- -g "chat"` (oppure `npx playwright test --project=e2e e2e/flows/chat.spec.ts`); `--ui` apre l'interfaccia
di Playwright. Per partire da un'installazione vuota la demo accetta `?scenario=first-run` (vedi
`src/shared/api/mock/scenario.ts`). Regole: selettori per ruolo e nome accessibile (`getByRole`), mai per classe CSS, mai
`waitForTimeout`. Se il Chromium installato da Playwright non è quello da usare, `AIW_CHROMIUM=/percorso/chromium`. Il rapporto
HTML finisce in `apps/ui/playwright-report/`. Il test di accessibilità fallisce sulle violazioni `critical` e `serious`; le
regole eventualmente disattivate stanno, con il motivo, in `e2e/support/axe.ts`.

I benchmark Rust (criterion) non girano con `cargo test`; si lanciano uno per volta, e la CI li compila soltanto:

```bash
cargo bench -p aiw-agent  --bench shellparse    # il lexer della policy della shell
cargo bench -p aiw-engine --bench hot_paths     # ricerca nei documenti, chunking, store di sessioni e messaggi
cargo bench -p aiw-server --bench server        # avvio a freddo e latenza di GET /api/system
cargo bench --no-run -p aiw-agent -p aiw-engine -p aiw-server   # solo compilare
```

Come leggere i numeri, su che macchina sono stati presi e cosa NON misurano: `docs/benchmarks.md`.

Test **ignorati** (`#[ignore]`) che servono risorse vere e non girano in CI:

```bash
cargo test -p aiw-tools browser -- --ignored        # browser dell'agente contro un Chrome/Chromium vero (headless)
AIW_TEST_GGUF=/percorso/modello.gguf cargo test -p aiw-runtimes --features llama-embedded -- --ignored   # un GGUF vero
```

(Altri test ignorati e come lanciarli sono scritti nei commenti in testa ai rispettivi file.) Se un test dipende da un servizio
o da un dispositivo che non hai, dillo nella pull request: vedi «Onestà» sotto.

## Com'è fatto il repository

Dettagli e decisioni in [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) e nella cartella `docs/architecture/adr/`.

| Cartella | Cosa contiene |
| --- | --- |
| `crates/aiw-core` | tipi dell'API, eventi AG-UI, schemi delle impostazioni, configurazione a strati |
| `crates/aiw-hw` | rilevamento dell'hardware (CPU, RAM, CUDA/ROCm/Metal/Vulkan, NPU) e classe del dispositivo |
| `crates/aiw-runtimes` | manifest dei runtime, rilevamento, adapter (OpenAI/Ollama/nativi), supervisor dei sidecar, motore llama.cpp incorporato |
| `crates/aiw-models` | model hub (Hugging Face, ModelScope, Ollama, URL), parser GGUF, compatibilità per dispositivo, download con ripresa |
| `crates/aiw-tools` | strumenti degli agenti: file, ricerca, shell in sandbox, web, browser CDP, computer use, client MCP |
| `crates/aiw-agent` | il ciclo dell'agente: streaming, policy, hook, skill, subagenti, compattazione |
| `crates/aiw-workflow` | workflow YAML/JSON con trigger, approvazioni e condizioni |
| `crates/aiw-engine` | la facciata: database SQLite, sessioni, run, approvazioni, registro delle attività, gateway, voce, router, aggiornamenti |
| `crates/aiw-server` | API REST + SSE, gateway OpenAI `/v1`, pairing e token, UI statica |
| `crates/aiw-winsandbox` | sandbox di Windows (integrità bassa, job object) |
| `crates/aiw-eval` | valutazione degli agenti (suite di prova) |
| `crates/aiw-cli` | il programma `aiw` |
| `apps/ui` | la UI React unica per desktop, mobile e browser |
| `apps/desktop` | shell Tauri 2 (desktop e mobile) e plugin nativo Swift/Kotlin (`plugins/tauri-plugin-aiw-native`) |
| `docs/` | stato (`STATUS.md`), architettura, design, guide, note delle release |
| `scripts/` | script di supporto (addestramento in `scripts/train`, CI in `scripts/ci`) |

## Convenzioni

- **Lingua.** I commenti nel codice sono in **inglese**. Tutto ciò che vede l'utente è in **italiano e inglese**: nella UI
  passa dal catalogo in `apps/ui/src/shared/i18n/` (messaggi `it` ed `en`; non scrivere testi fissi nei componenti: se `npm run
  i18n:scan` è disponibile elencali). Documentazione, `CHANGELOG.md`, `docs/STATUS.md`, messaggi di commit e note di release
  sono in italiano; `SECURITY.md` e questo file hanno un riassunto in inglese.
- **Stile Rust.** `cargo fmt` (`rustfmt.toml`) e `cargo clippy --all-targets` senza avvisi: la CI li esegue.
- **Test.** Ogni correzione porta il test che l'avrebbe vista; i test non toccano la rete né la cartella dei dati
  dell'utente (usano server finti e cartelle temporanee, come quelli esistenti).
- **Commit.** Una riga in italiano che dice cosa cambia, al presente o come titolo, senza prefissi («Router automatico dei
  runtime con fallback e voce neurale locale», «Windows: percorsi dei download mostrati con / …»). Il numero della pull request
  lo aggiunge GitHub. Una pull request = un argomento. Per le release il titolo è «Prepare vX.Y.Z-beta.N release (store build
  NNNN)». Se il tuo strumento aggiunge righe `Co-Authored-By`, lasciale.
- **Pull request.** Compila il modello (`.github/pull_request_template.md`): cosa e perché, come verificato, provato su
  dispositivo sì/no, e la lista dei controlli.
- **CHANGELOG.** Ogni modifica visibile a chi usa AIW entra in `CHANGELOG.md`, sezione «Non rilasciato», con i limiti noti.
  Chi aggiorna un'installazione esistente deve poter leggere «Cambia per chi già usa AIW» quando un comportamento cambia.
- **Onestà (`docs/STATUS.md`).** Lo stato deve dire la verità: **«scritto» non vuol dire «provato»**. Scrivi 🟡 e cosa non è stato
  eseguito (un dispositivo, un servizio, un modello vero) finché non l'hai eseguito davvero; non promuovere una voce a ✅ per
  il solo fatto che compila o che i test con un finto passano. Se correggi una voce che era più ottimista della realtà, è un bene.
- **Sicurezza.** Non committare segreti, token, chiavi, file `.env`. Una vulnerabilità non si segnala in una issue pubblica:
  usa la segnalazione privata di GitHub descritta in [SECURITY.md](SECURITY.md). Le modifiche alla sandbox, alla policy o ai
  token hanno bisogno di test che dimostrino il confine.
- **Dipendenze.** `deny.toml` e `audit.yml` controllano le dipendenze (Rust e npm); i crate del workspace sono `publish = false`.
  Una dipendenza nuova va giustificata nella pull request.

## Rilasci e firma

Il processo completo (file da aggiornare, tag, lavori di CI, target facoltativi arm64 e CUDA, canali di aggiornamento) è in
[docs/release/RELEASE.md](docs/release/RELEASE.md). La firma del codice (Developer ID, Windows) è in
[docs/release/FIRMA.md](docs/release/FIRMA.md); la pubblicazione su App Store Connect e Google Play in
[docs/store/README.md](docs/store/README.md).

## Segnalare un errore o proporre un'idea

Usa i moduli delle issue (errore, funzione). Indica versione, sistema operativo e passi per riprodurre, senza incollare token
o conversazioni private.
