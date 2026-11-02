# Architettura della UI (`apps/ui`)

> Versione 0.4 · 2026-11-02 · Sostituisce la struttura piatta `views/` + `components/` della v0.3.
> Restano: l'interfaccia `Transport` con i tre carrier (ADR 0002), il renderer A2UI, l'identità
> visiva "rack di strumenti" (DESIGN.md §2) e la modalità `--mode mockup`.

## 1. Struttura

```text
src/
  main.tsx                   boot: crea il Transport (con errore e "riprova"), monta <Providers><App/>
  app/
    App.tsx                  layout: Rail | contenuto | TabBar, banner, centro conferme, palette
    providers.tsx            QueryClientProvider + AppProvider (api, toast, tema, kill)
    routes.tsx               UNICA tabella delle rotte (genera rail, tabbar, "Altro", palette, rendering)
    router.ts                router su hash: parse/match/navigate, useRoute(), useNavigate(), <Link>
    shell/                   Rail, TabBar, MoreSheet, DeviceChip, KillSwitch, ConnectionBanner
    palette/                 CommandPalette (Ctrl/Cmd+K): rotte + azioni registrate
  shared/
    api/                     transport.ts, http.ts, tauri.ts, types.ts, mock/{index.ts,fixtures.ts}
    query/                   keys.ts (fabbrica delle query key), client.ts (QueryClient e default)
    ui/                      componenti del design system (§5), un file per componente
    lib/                     format.ts (Intl), errors.ts, useAction.ts, useHotkey.ts, cx.ts
    styles/                  tokens.css, base.css, components.css, utilities.css
  features/<dominio>/
    api.ts                   hook di query e mutation del dominio (useModels, usePullModel, …)
    <Nome>View.tsx           vista di rotta (≤ 250 righe)
    components/              componenti del dominio
    model/                   logica pura del dominio (testata)
```

Domini: `home`, `chat`, `agents`, `workflows`, `capabilities` (tool, server MCP, browser e
computer-use), `integrations`, `models`, `runtimes`, `devices`, `security`, `activity`,
`settings`, `pairing`, `approvals`.

Regole:
- Una vista importa da `shared/*` e dal proprio dominio. Tra domini si passa solo da `shared/` o
  dalla navigazione (le query key sono condivise in `shared/query/keys.ts`).
- Niente `style={{…}}` per layout e spaziature: classi o componenti. Gli stili inline sono ammessi
  solo per valori calcolati (larghezza di una barra, colore di un grafico).
- Niente `catch → toast` copiato: si usa `useAction()` o `useMutation` con `onError` comune.

## 2. Data layer

- **TanStack Query v5** sopra `Transport`. Il `QueryClient` ha `staleTime` 5 s, `retry` 1 (0 per
  401), `refetchOnWindowFocus: true`, `refetchIntervalInBackground: false`.
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

## 3. Routing

Router su hash scritto in casa (nessuna dipendenza). Percorsi:

| Rotta | Percorso | Note |
|---|---|---|
| home | `#/` | Panoramica |
| chat | `#/chat`, `#/chat/:sessionId` | la sessione attiva sta nell'URL |
| agents | `#/agents`, `#/agents/:agentId` | |
| workflows | `#/workflows`, `#/workflows/:workflowId` | |
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
- **Palette comandi** (Ctrl/Cmd+K): navigazione, nuova sessione, scarica modello, avvia/ferma
  runtime, kill switch, cambio tema.

## 5. Design system (`shared/ui`)

`Icon`, `Button` (varianti `primary|ghost|danger`, `size sm|md`), `IconButton` (aria-label
obbligatoria), `Pill`, `StatusPill`, `Led`, `Meter`, `Chip`, `Toggle` (`role="switch"`),
`Segmented`, `Tabs` (con `aria-selected` e frecce), `Field`/`Input`/`Select`/`Textarea`,
`Dialog` e `Drawer` (focus trap, Esc, ripristino del focus, `aria-modal`), `Sheet` (telefono),
`PageHeader` (eyebrow, titolo, sottotitolo, azioni), `Panel`, `Section`, `MasterDetail` (lista e
dettaglio affiancati; sul telefono il dettaglio diventa una pagina), `DataTable`, `KeyValue`,
`EmptyState`, `ErrorState`, `Skeleton`, `QueryState`, `Toast`, `CodeBlock` (con copia),
`Markdown`, `SchemaForm`, `CopyField`.

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
  | { provider: string; model: string; variant: string; revision?: string }
  | { id: string }
  | { repo: string; file: string }
  | { url: string; file_name?: string; sha256?: string };
export interface ModelSearchParams { provider: string; q: string; format?: string; sort?: "downloads" | "likes" | "updated"; limit?: number; cursor?: string }
```

`DownloadProgress`, `LocalModel` e `CatalogModel` ricevono i campi aggiunti in model-hub.md §3.

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
`HookConfig = { event: "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop"; matcher?: string | null; command: string; timeout_s: number }`.

Eventi: il runner emette `CUSTOM` con `name: "aiw.image"` e
`value: { toolCallId, mime, data }` (base64) per gli screenshot di browser e computer-use; la chat
li mostra nella card del tool. `HttpTransport.subscribeRun` scarta gli eventi con `seq` già visto
(il server rimanda lo storico alla riconnessione) come fa il carrier Tauri.

## 8. Test

- **Vitest** (`npm test`) per la logica pura: router, formattatori, query key, riduttore della
  chat e ricostruzione dello storico, funzioni A2UI, raggruppamento delle varianti.
- **Smoke Playwright** (`npm run test:smoke`) sul build mockup: ogni rotta a 1440 e 390 px, tema
  chiaro e scuro, nessun errore in console e nessuno scroll orizzontale.
- La CI esegue `typecheck`, `test`, `build`, `build:mockup` e lo smoke.
