# Architettura della UI (`apps/ui`)

> Versione 0.4 · 2026-11-02 · Sostituisce la struttura piatta `views/` + `components/` della v0.3.
> Restano: l'interfaccia `Transport` con i tre carrier (ADR 0002), il renderer A2UI, l'identità
> visiva "rack di strumenti" (DESIGN.md §2) e la modalità `--mode mockup`.
> Descrive quello che è stato costruito, dopo la fusione delle quattro tracce di lavoro e gli
> adeguamenti alle correzioni del backend (§6.1).

## 1. Struttura

```text
src/
  main.tsx                   boot: crea il Transport (con errore e "riprova"), monta <Providers><App/>
  app/
    App.tsx                  layout: Rail | contenuto | TabBar, banner, centro conferme, palette
    providers.tsx            QueryClientProvider + AppProvider (api, toast, tema, kill)
    routes.tsx               UNICA tabella delle rotte (genera rail, tabbar, "Altro", palette, rendering)
    router.ts                router su hash: parse/match/navigate, useRoute(), useNavigate(), <Link>
    shell/                   Rail, TabBar, MoreSheet, DeviceChip, KillSwitch, Banners, ErrorBoundary
    palette/                 CommandPalette (Ctrl/Cmd+K), useCommands, commands.ts (filtro),
                             openPalette.ts (apre la palette da altrove, senza simulare il tasto)
  shared/
    api/                     transport.ts, http.ts, tauri.ts, types.ts, mock/{index.ts,fixtures.ts,hub.ts}
    query/                   keys.ts (fabbrica delle query key), client.ts (QueryClient e default), system.ts
    ui/                      componenti del design system (§5), un file per componente, index.ts
    lib/                     logica pura e hook generici: format (Intl), errors, text, secrets, kvRows,
                             keyedList, useAction, useDraft, useUnsavedGuard, navGate, useHotkey…
    styles/                  tokens.css, base.css, components.css, utilities.css
  features/<dominio>/
    api.ts                   hook di query e mutation del dominio (useModels, usePullModel, …)
    use<Nome>.ts             hook con stato della vista, alla radice del dominio
                             (chat/useConversation, chat/useLayout, models/useQueryText)
    <Nome>View.tsx           vista di rotta
    <dominio>.css            stili del dominio, con un prefisso di classe proprio (ws-, hm-, cap-, mh-…)
    components/              componenti del dominio
    model/                   logica pura del dominio (testata)
```

Domini: `home`, `chat`, `agents`, `workflows`, `evals`, `capabilities` (tool, server MCP, browser e
computer-use), `integrations`, `models`, `runtimes`, `devices`, `security`, `activity`,
`settings`, `pairing`, `approvals`.

Regole:
- Una vista importa da `shared/*` e dal proprio dominio. Tra domini si importano **solo gli hook di
  `features/<altro>/api.ts`** (la Panoramica legge `agents/api`, `models/api`, `runtimes/api`…; le
  query key sono condivise in `shared/query/keys.ts`). Unica eccezione: `ApprovalCard`
  (`approvals/components`), la stessa card in chat, pannello del run, Panoramica e centro conferme.
  Rotte e `navigate` arrivano da `app/router`. `shared/` non importa mai da `features/` né da `app/`.
- Gli hook con stato della vista stanno **alla radice del dominio**, non in `model/`: `model/` è
  logica pura, testabile senza DOM (Vitest gira in ambiente `node`).
- Gli import dei componenti condivisi passano da `shared/ui/index.ts`, non dal file del componente.
- Gli stili del dominio stanno nel suo CSS con prefisso di classe; `shared/styles/components.css`
  tiene solo shell, componenti base e superfici condivise (le regole pre-0.4 di chat, panoramica e
  worktree sono state tolte).
- Niente `style={{…}}` per layout e spaziature: classi o componenti. Gli stili inline sono ammessi
  solo per valori calcolati (larghezza di una barra, colore di un grafico).
- Niente `catch → toast` copiato: si usa `useAction()` o `useMutation` con `onError` comune.

**Voce nella chat** (`features/chat`). Logica pura e testata in `voice/`: `vad.ts` (rilevatore di voce
e ritaglio dei silenzi), `wav.ts` (ricampionamento a 16 kHz, WAV 16-bit mono), `speech.ts`
(Markdown → testo da leggere, frasi, scelta della voce); le parti che toccano il browser stanno in
`voice/capture.ts` (microfono) e `voice/tts.ts` (solo voci locali del sistema, mai quelle cloud).
`useVoice.ts` sequenzia dettatura e conversazione (ascolto → invio → lettura della risposta →
ascolto; le conferme dell'agente vengono annunciate; Esc o cambio sessione terminano) e legge lo
stato del motore con `useVoiceStatus` e la trascrizione con `useTranscribe` (`chat/api.ts`). Il
`Composer` mostra il microfono (`aria-pressed` mentre registra) e il pulsante della conversazione;
`VoiceBar` dice cosa sta facendo l'agente. Il `Transport` ha `voice()` e `transcribe(wav, lingua?)`:
HTTP manda l'audio grezzo, Tauri lo manda in base64 dentro JSON (l'IPC porta solo JSON), il mock
risponde con frasi di esempio. Le impostazioni `voice.*` arrivano dallo schema della configurazione e
stanno nella sezione "Voce" di Impostazioni.

## 2. Data layer

- **TanStack Query v5** sopra `Transport`. Il `QueryClient` ha `staleTime` 5 s, `retry` 1 (0 per
  ogni 4xx: 401 va al pairing, 404 vuol dire che la risorsa non c'è più), `refetchOnWindowFocus:
  true`, `refetchIntervalInBackground: false`.
- Query key in `shared/query/keys.ts` (`qk.models.installed()`, `qk.models.search(params)`, …).
- Polling solo dove serve e solo mentre la vista è montata:
  `system` 15 s, `approvals` 3 s, `downloads` 1 s **solo se c'è un job `queued|running|verifying`**,
  `runtimes` 5 s nella vista Runtime, run dei workflow 2 s solo con run attivi, audit 5 s.
- Le mutation invalidano le query collegate (es. un download che passa a `done` invalida
  `models.installed` e `models.catalog`).
- Stati uniformi con `<QueryState query>`: `Skeleton` durante il primo caricamento, `ErrorState`
  con "Riprova", `EmptyState` con l'azione suggerita.
- Connessione: se le query di `system` falliscono (non 401) compare `ConnectionBanner`
  ("Connessione al workspace persa · Riprova").
- Errori del backend: i messaggi sono già in italiano e la UI li mostra così come sono (toast
  centrale, `InlineError`, o dentro il dialogo che ha chiesto l'azione: `DecisionDialog`). I form
  con regole di validazione (policy, hook, agenti, server MCP) le ripetono in locale per dirlo prima
  della richiesta; se il backend rifiuta comunque, la sua frase resta visibile.

## 3. Routing

Router su hash scritto in casa (nessuna dipendenza). Percorsi:

| Rotta | Percorso | Note |
|---|---|---|
| home | `#/` | Panoramica |
| chat | `#/chat`, `#/chat/:sessionId` | la sessione attiva sta nell'URL |
| agents | `#/agents`, `#/agents/:agentId` | |
| workflows | `#/workflows`, `#/workflows/:workflowId` | |
| evals | `#/evals/:tab?` | `suites` (default; `?suite=` la suite aperta, `new` per crearne una), `reports` (`?report=` il report aperto, `?b=` quello con cui confrontarlo) |
| capabilities | `#/capabilities/:tab?` | `tools` (default), `mcp`, `browser` |
| integrations | `#/integrations` | |
| models | `#/models/:tab?` | `installed` (default), `discover`, `downloads`, `catalog`; query `?provider=&q=&format=` |
| runtimes | `#/runtimes`, `#/runtimes/:runtimeId` | |
| devices | `#/devices` | |
| security | `#/security/:tab?` | `policy` (default), `hooks`, `sandbox` |
| activity | `#/activity` | |
| settings | `#/settings` | |

I vecchi hash (`#chat`, `#models`, `#preferences`, `#tools`…) vengono reindirizzati ai nuovi.
Ogni vista è caricata con `React.lazy`.

**Modifiche non salvate (`navGate`).** Ogni modo di uscire da un form (link, rail, indietro, scheda)
finisce in un `hashchange`; il router chiede a `navGate.check()` *prima* di renderizzare la nuova
rotta (una rotta che si rende per prima azzererebbe il form che sta lasciando). Un form si registra
con `useUnsavedGuard(dirty, { onDiscard, keep })`: finché `dirty` è vero il vecchio hash viene rimesso
con `history.replaceState`, la destinazione resta in `pending` e la vista mostra
`UnsavedChangesDialog` (Continua a modificare · Scarta · Salva e continua). `guard.go(to)` serve alle
navigazioni che il form stesso provoca (dopo il salvataggio); `keep(from, to)` lascia passare i
cambi di scheda o di drawer della stessa pagina; chiudere la scheda del browser passa dal suo avviso.
Lo usano agenti, workflow, regole di policy e hook, insieme a `useDraft` (copia modificabile del valore
del server, che segue il server finché l'utente non ha toccato nulla) e a `SaveBar`.

## 4. Architettura dell'informazione

| Gruppo | Voci |
|---|---|
| — | **Panoramica** |
| Lavoro | Workspace · Agenti · Workflow |
| Capacità | Tool, MCP e browser · Integrazioni |
| Inferenza | Modelli · Runtime · Dispositivi |
| Controllo | Sicurezza · Attività · Impostazioni |

- **Panoramica**: device e acceleratori, runtime attivi, modello del profilo predefinito, conferme in
  attesa, sessioni recenti, download in corso, prossimi workflow, checklist di primo avvio
  (runtime → modello consigliato → profilo → prima chat; DESIGN §5.1).
- **Telefono** (≤ 760 px): tabbar con Panoramica, Workspace, Modelli, Runtime, Altro. "Altro" apre
  un foglio con tutte le sezioni, il device e il **kill switch** (sul telefono oggi non è
  raggiungibile).
- **Tablet** (761–1100 px): rail compressa a sole icone.
- **Palette comandi** (Ctrl/Cmd+K, la ricerca della rail e del foglio "Altro", o `openPalette()` dal
  tassello della Panoramica): navigazione, nuova sessione, scarica modello, rileva runtime, kill
  switch, cambio tema.

**Punti di rottura.** Gli stessi cinque ovunque; altri (520, 640, 900 px) restano locali al CSS di
un singolo componente.

| Larghezza | Cosa cambia |
|---|---|
| 560 px | `KeyValue`: etichetta e valore vanno in colonna |
| 760 px | telefono: tabbar al posto della rail, master/detail a pagina singola, pannello del run come sheet |
| 1000 px | master/detail e griglie a due colonne passano a una; in chat l'elenco sessioni diventa drawer |
| 1100 px | rail a icone (da 761 px); azioni rapide della Panoramica su due colonne |
| 1320 px | sopra: pannello del run come colonna fissa; sotto: drawer (tablet) o sheet (telefono) |

## 5. Design system (`shared/ui`)

Tutto si importa da `shared/ui/index.ts`.

Base: `Icon`, `Button` (varianti `primary|ghost|danger`, `size sm|md`), `IconButton` (aria-label
obbligatoria), `Pill`, `StatusPill`, `Led`, `Meter`, `Chip`/`Chips`, `Toggle` (`role="switch"`),
`Segmented`, `Tabs` (con `aria-selected` e frecce), `Field`/`Input`/`Select`/`Textarea`, `Dialog` e
`Drawer` (focus trap, Esc, ripristino del focus, `aria-modal`), `Sheet` (telefono), `ModalShell`,
`PageHeader` (eyebrow, titolo, sottotitolo, azioni), `Page`, `Panel`, `Section`, `MasterDetail`
(lista e dettaglio affiancati; sul telefono il dettaglio diventa una pagina), `DataTable`,
`KeyValue`, `EmptyState`, `ErrorState`, `Skeleton`, `QueryState`, `ToastHost`, `CodeBlock` (con
copia), `CopyField`, `CopyButton`, `InlineText`, `SchemaForm`.

Markdown: `Markdown` (memoizzato e incrementale durante lo streaming) e gli helper puri del
parser, `parseMarkdown` e `safeHref`. Nessun HTML passante, link `javascript:` svuotati, immagini
remote mostrate come link (un `<img>` remoto farebbe uscire dati da una risposta con prompt injection).

Form di modifica (nati con agenti, workflow e sicurezza, usati anche da Impostazioni e MCP):
- `SaveBar`: barra fissa in fondo alla pagina con lo stato ("Tutto salvato", "Modifiche non salvate",
  il motivo per cui non si può salvare), Salva e Annulla modifiche.
- `RowControls`: Sposta su, Sposta giù e Rimuovi di una riga di un elenco ordinabile (il fuoco resta
  sullo stesso pulsante dopo lo spostamento); le righe hanno un id stabile (`shared/lib/keyedList`).
- `FormSection` / `FormSections`: sezioni con titolo e nota di un form lungo.
- `DecisionDialog`: conferma di un'azione irreversibile; l'errore del backend (per esempio "agente
  integrato" nel 409) resta visibile dentro il dialogo.
- `UnsavedChangesDialog`: il dialogo di `useUnsavedGuard` (§3).
- `TagInput`: elenco di valori come tag rimovibili; con `splitShellWords` gli spazi tra virgolette
  restano nell'argomento.
- `KeyValueEditor`: variabili d'ambiente e header HTTP. Le chiavi che sembrano segreti mostrano il
  valore mascherato con "Mostra"; un segreto già salvato (il server lo manda come `••••••`) è un
  campo mascherato di sola lettura con "Cambia", e "Mantieni" rimette la maschera (§6.1).

Token (`shared/styles/tokens.css`): un solo blocco per il tema scuro (selettore combinato), scale
`--space-1…8`, `--z-*`, `--dur-*`, contrasto ≥ 4,5:1 per testo tenue in entrambi i temi.

## 6. Tipi nuovi (`shared/api/types.ts`)

Rispecchiano `aiw-core::api` (vedi [model-hub.md](../architecture/model-hub.md) §3):

```ts
export interface ModelProviderInfo { id: string; name: string; description: string; homepage: string;
  capabilities: string[]; formats: string[]; auth: "none" | "optional" | "required"; auth_configured: boolean; endpoint: string }
export interface RemoteModel { provider: string; id: string; name: string; publisher: string; description?: string;
  formats: string[]; tags: string[]; capabilities: string[]; downloads?: number; likes?: number; updated_at?: string;
  license?: string; gated: boolean; params_b?: number; url: string }
export interface SearchPage { items: RemoteModel[]; next_cursor?: string }
export interface RemoteFile { path: string; size_bytes: number; sha256?: string; role: "weights" | "mmproj" | "config" | "tokenizer" | "other" }
export interface RuntimeCompat { id: string; name: string; available: boolean; reason: string }
export interface ModelVariant { id: string; label: string; format: string; quant?: string; files: RemoteFile[]; size_bytes: number;
  fit: Fit; fit_detail: string; recommended: boolean; runtimes: RuntimeCompat[]; installed: boolean }
export interface RemoteModelDetail { model: RemoteModel; revision: string; variants: ModelVariant[] }
export type PullRequest =
  | { provider: string; model: string; variant?: string; revision?: string }
  | { id: string }
  | { repo: string; file: string }
  | { url: string; file_name?: string; sha256?: string };
export interface ModelSearchParams { provider: string; q: string; format?: string; sort?: "downloads" | "likes" | "updated"; limit?: number; cursor?: string }
```

`DownloadProgress`, `LocalModel` e `CatalogModel` ricevono i campi aggiunti in model-hub.md §3.

### 6.1 Contratto con il backend dopo le correzioni dell'E2E

`shared/api/types.ts` segue `crates/aiw-core/src/api.rs`. Cosa la UI fa di ciò che il backend ha
corretto:

- **Run finito in errore.** Il backend lascia nella cronologia un messaggio `system` con `name:
  "aiw.run_error"` (`RUN_ERROR_MESSAGE`; mai inviato al modello). `historyToItems` lo rende come la
  stessa riga di errore del `RUN_ERROR` dal vivo (a capo, senza `role="alert"` perché è storico). Il
  messaggio salvato non ha il `code`: si ricava dal testo del runner ("Errore del runtime…", "Runtime
  non disponibile…", "Prompt bloccato da un hook…", "Run annullato"). La nota sotto lo storico, ricavata
  da `last_status`, non compare se l'ultima riga è già l'errore: lo stesso errore non si legge due volte.
- **Tool interni.** `ToolInfo.source` ha tre valori: `builtin`, `mcp`, `internal`. I tool interni
  (`todo`, `agent`, `skill`, `render_ui`, `code`) sono sempre attivi, di rischio `read`, e
  `PUT /api/tools/{name}` risponde 400: nella tabella dei tool formano il gruppo **Interni** con
  l'etichetta "Interno" al posto dell'interruttore, nel selettore dei tool di un agente lo stesso
  gruppo. L'elenco fisso del selettore resta solo come ripiego per un backend che non li elenca: la
  voce del catalogo vince, mai doppioni.
- **Segreti MCP.** Header e variabili d'ambiente il cui nome contiene `auth`, `cookie`, `token`,
  `key`, `secret`, `password`, `passwd`, `credential` o `signature` arrivano come `SECRET_MASK`
  (`••••••`); rimandare la maschera in salvataggio conserva il valore (`shared/lib/secrets`). Il drawer
  li mostra come campi mascherati di sola lettura ("Cambia" per scrivere un nuovo valore, "Mantieni"
  per tornare al salvato) e rimanda la maschera invariata se non li si tocca. Il backend cerca il
  segreto per nome: rinominare la riga ne svuota il valore, e una maschera scritta a mano non ha niente
  dietro e blocca il salvataggio.
  `McpServer.env` e `headers` sono `Record<string, unknown>` (stringhe, in pratica).
- **404 e 405 in JSON.** Un 404 sulla sessione aperta (eliminata da un altro client) porta a `#/chat`
  con un avviso, sia caricando lo storico sia inviando un messaggio; eliminare una sessione o un server
  MCP che non c'è più conta come fatto; una query non ritenta mai su 4xx.
- **Validazione.** I form ripetono le regole del backend e ne usano le parole: regole di policy (tool
  non vuoto, azione `allow|ask|deny`, glob valido), hook (evento noto, comando, `timeout_s` 1–600,
  filtro glob valido), agenti (`max_turns` ≥ 1, profilo esistente, solo tool noti: `*`, `mcp__…`,
  interni o del catalogo), server MCP (nome, trasporto `stdio|http`, comando per stdio, URL http(s)
  con host per http). Un glob che il crate `globset` rifiuta ora blocca il salvataggio (prima era un
  avviso).
- **Download da indirizzo diretto.** Ogni URL ha una cartella propria (`url/<host>[_<porta>]/<nome>-<hash>/<file>`)
  e un'installazione si identifica con (provider, model, variant): due URL dello stesso host sono due
  modelli. La UI non ricostruisce percorsi: mostra `dest` e `variant.installed` come arrivano (il demo
  usa lo stesso schema).
- **Altri tipi.** `RuntimeInfo.api` può essere `none` (motore in-process senza API HTTP: "senza API");
  `LocalModel.origin` può essere `""`, letto come `manual`.

## 7. Transport: metodi aggiunti

```ts
readonly capabilities: { demo: boolean; native: boolean; pairing: boolean };
readonly notice?: string;
pair?(code: string): Promise<void>;

refreshRuntimes(): Promise<RuntimeInfo[]>;            // POST /api/runtimes/refresh
modelProviders(): Promise<ModelProviderInfo[]>;       // GET  /api/models/providers
setProviderAuth(id: string, token: string | null, endpoint?: string): Promise<ModelProviderInfo>;
searchModels(p: ModelSearchParams): Promise<SearchPage>;               // GET /api/models/search
remoteModel(provider: string, model: string, revision?: string): Promise<RemoteModelDetail>;
pullModel(req: string | PullRequest): Promise<DownloadProgress>;      // stringa = id del catalogo
cancelDownload(id: string): Promise<DownloadProgress>;
clearDownload(id: string): Promise<void>;
deleteModel(id: string): Promise<void>;
deleteAgent(id: string): Promise<void>;               // DELETE /api/agents/{id}
deleteProfile(id: string): Promise<void>;             // DELETE /api/profiles/{id}
renameSession(id: string, title: string): Promise<Session>;  // PATCH /api/sessions/{id}
getSession(id: string): Promise<Session>;
deleteWorkflow(id: string): Promise<void>;
policy(): Promise<{ rules: PolicyRule[] }>;           // GET/PUT /api/policy
setPolicy(rules: PolicyRule[]): Promise<{ rules: PolicyRule[] }>;
hooks(): Promise<{ hooks: HookConfig[] }>;            // GET/PUT /api/hooks
setHooks(hooks: HookConfig[]): Promise<{ hooks: HookConfig[] }>;
```

`PolicyRule = { tool: string; pattern?: string | null; action: "allow" | "ask" | "deny" }`,
`HookConfig = { event: "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop" | "SubagentStart" | "SubagentStop" | "PreCompact"; matcher?: string | null; command: string; timeout_s: number }`.

Eventi: il runner emette `CUSTOM` con `name: "aiw.image"` e
`value: { toolCallId, mime, data }` (base64, senza prefisso `data:`) per gli screenshot di browser e
computer-use; la chat li mostra come miniatura nella card del tool, con vista a schermo intero. Le
immagini non sono nei messaggi salvati: `chat/model/imageCache` ricorda quelle viste, entro un
budget di memoria. `HttpTransport.subscribeRun` scarta gli eventi con `seq` già visto
(il server rimanda lo storico alla riconnessione) come fa il carrier Tauri.

## 8. Test

- **Vitest** (`npm test`, ambiente `node`) per la logica pura: router e `navGate`, formattatori,
  query key, riduttore della chat e ricostruzione dello storico (errori salvati compresi), funzioni
  A2UI, parser Markdown, modelli di ogni dominio (varianti, filtri, policy e glob, hook, passi dei
  workflow, form MCP), `kvRows`, `secrets` e il `MockTransport`.
- **Smoke Playwright** sul build mockup: ogni rotta della tabella di `routes.tsx` (con valori di prova
  per i parametri, più una pagina inesistente) a 1440 e 390 px, tema chiaro e scuro; fallisce su
  errori in console, schermata di errore di una vista, scroll orizzontale e tema sbagliato.

  ```bash
  cd apps/ui
  npm run build:mockup           # lo smoke non compila nulla
  npm run test:smoke             # tutte le rotte, dimensioni e temi
  npm run test:smoke -- --route=runtimes     # solo le rotte che iniziano con /runtimes
  npm run test:smoke -- --shots=/percorso    # salva anche uno screenshot per visita
  ```

  `AIW_SMOKE_HTML` indica un altro build. Serve Chromium (`npx playwright install chromium`).
- La CI esegue `npm ci`, `typecheck`, `test`, `build`, `build:mockup` e lo smoke.
