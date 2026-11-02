# Model Hub multi-provider

> Versione 0.4 · 2026-11-02 · Sostituisce il download a file singolo da Hugging Face descritto in
> ARCHITECTURE §4. Contratto condiviso da `aiw-core`, `aiw-models`, `aiw-engine`, `aiw-server`,
> `aiw-cli` e `apps/ui`: i nomi dei campi JSON qui sotto sono vincolanti (snake_case, come il resto
> dell'API).

## 1. Obiettivi

1. **Scoperta live** dei modelli sui provider pubblici, non solo le 18 voci del catalogo curato.
2. **Varianti**: per un repository GGUF si sceglie la quantizzazione (con tutti gli shard e
   l'`mmproj` per la vision). Per MLX, ONNX e safetensors si scarica il repository intero (o una
   sua sottocartella).
3. **Un solo downloader** per tutti i provider: ripresa con Range, verifica sha256, controllo dello
   spazio disco, annullamento, coda con concorrenza limitata e timeout di lettura.
4. **Compatibilità esplicita**: ogni modello e ogni variante elencano i runtime che li possono
   eseguire su questo device, derivati dai `formats`/`platforms` dei manifest e non da una tabella
   scritta a mano.
5. **Nessuna fuga di credenziali**: il token di un provider viaggia solo verso l'host di quel
   provider.

## 2. Provider

| id | Nome | Ricerca | File / varianti | Download | Auth |
|---|---|---|---|---|---|
| `catalog` | Consigliati (catalogo curato) | filtro locale su `models.json` | dalla voce del catalogo | come il provider della voce | — |
| `hf` | Hugging Face | `GET {endpoint}/api/models?search={q}&filter={format}&sort={sort}&direction=-1&limit={n}`; pagina successiva dall'header `Link: <…>; rel="next"` (cursore = URL completo) | `GET {endpoint}/api/models/{repo}` (`gated`, `tags`, `cardData.license`, `downloads`, `likes`) e `GET {endpoint}/api/models/{repo}/tree/{rev}?recursive=true` (`[{type,path,size,lfs:{oid,size}}]`, `lfs.oid` = sha256) | `GET {endpoint}/{repo}/resolve/{rev}/{path}` | token opzionale (`Authorization: Bearer`), obbligatorio per i repo gated; validazione con `GET {endpoint}/api/whoami-v2` |
| `modelscope` | ModelScope | best effort: `PUT https://www.modelscope.cn/api/v1/dolphin/models` con body `{"PageSize":n,"PageNumber":p,"SortBy":"Default","Target":"","SingleCriterion":[],"Name":q}` (API del sito, non documentata: se fallisce si accetta comunque `owner/name` esatto) | `GET https://www.modelscope.cn/api/v1/models/{owner}/{name}/repo/files?Revision=master&Recursive=true` → `{"Data":{"Files":[{"Path","Size","Sha256","Type"}]}}` | `GET https://www.modelscope.cn/api/v1/models/{owner}/{name}/repo?Revision=master&FilePath={path}` | token opzionale |
| `ollama` | Registry Ollama | nessuna API pubblica di ricerca: filtro su un indice locale (`crates/aiw-models/catalog/ollama-library.json`, nomi e tag popolari) più nome esatto `name[:tag]` o `user/name[:tag]` | `GET https://registry.ollama.ai/v2/{ns}/{name}/manifests/{tag}` con `Accept: application/vnd.docker.distribution.manifest.v2+json`; layer `application/vnd.ollama.image.model` = GGUF, `application/vnd.ollama.image.projector` = mmproj; `ns` = `library` per i modelli ufficiali | `GET https://registry.ollama.ai/v2/{ns}/{name}/blobs/{digest}` (redirect al CDN, Range supportato; il digest è lo sha256) | — |
| `url` | URL diretto | — | `HEAD` per dimensione e `Accept-Ranges` | `GET {url}` | — (mai inviare token di altri provider) |

Note:
- `endpoint` di `hf` è `https://huggingface.co`, sovrascrivibile da `HF_ENDPOINT` o dalle
  impostazioni del provider (mirror).
- GitHub Releases e Kaggle sono in roadmap (§8). LM Studio non ha un hub proprio: scarica da
  Hugging Face; i suoi modelli locali si riusano con la scansione delle sorgenti esterne (§6).
- Nessuno di questi endpoint è raggiungibile dall'ambiente di sviluppo in cui è stato scritto il
  codice: i test usano server HTTP locali che riproducono le risposte reali.

## 3. Tipi (`aiw-core::api`)

```rust
pub struct ModelProviderInfo {
    pub id: String,                // catalog | hf | modelscope | ollama | url
    pub name: String,
    pub description: String,
    pub homepage: String,
    pub capabilities: Vec<String>, // search | files | auth | multi_file
    pub formats: Vec<String>,      // formati che può fornire: gguf, mlx, onnx, safetensors, litertlm, …
    pub auth: String,              // none | optional | required
    pub auth_configured: bool,
    pub endpoint: String,          // base URL effettiva ("" se non applicabile)
}

pub struct RemoteModel {
    pub provider: String,
    pub id: String,                // riferimento nel provider: "Qwen/Qwen3-4B-GGUF", "qwen3:4b", "https://…"
    pub name: String,
    pub publisher: String,
    pub description: Option<String>,
    pub formats: Vec<String>,
    pub tags: Vec<String>,
    pub capabilities: Vec<String>, // chat | tools | vision | embeddings | reasoning (dedotte da tag e pipeline)
    pub downloads: Option<u64>,
    pub likes: Option<u64>,
    pub updated_at: Option<String>,
    pub license: Option<String>,
    pub gated: bool,
    pub params_b: Option<f64>,
    pub url: String,               // pagina web del modello ("" se non c'è)
}

pub struct SearchPage {
    pub items: Vec<RemoteModel>,
    pub next_cursor: Option<String>,
}

pub struct RemoteFile {
    pub path: String,              // percorso nel repository
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub role: String,              // weights | mmproj | config | tokenizer | other
}

pub struct ModelVariant {
    pub id: String,                // chiave stabile nel modello: "Q4_K_M", "repo", "cpu-int4", "latest"
    pub label: String,
    pub format: String,
    pub quant: Option<String>,
    pub files: Vec<RemoteFile>,    // tutti gli shard + mmproj o tutti i file del repo/sottocartella
    pub size_bytes: u64,
    pub fit: String,               // great | ok | tight | no
    pub fit_detail: String,
    pub recommended: bool,         // al massimo una variante per modello
    pub runtimes: Vec<RuntimeCompat>,
    pub installed: bool,
}

pub struct RemoteModelDetail {
    pub model: RemoteModel,
    pub revision: String,          // sha o "main"/"master"/tag
    pub variants: Vec<ModelVariant>,
}

pub struct RuntimeCompat {
    pub id: String,
    pub name: String,
    pub available: bool,           // installato o registrato su questo device
    pub reason: String,            // es. "solo macOS", "da installare", ""
}

/// Corpo di POST /api/models/pull (enum untagged, la prima forma che combacia vince).
pub enum PullRequest {
    Hub { provider: String, model: String, variant: String, revision: Option<String> },
    Catalog { id: String },
    HfFile { repo: String, file: String },               // compatibilità con la v0.3
    Url { url: String, file_name: Option<String>, sha256: Option<String> },
}
```

Estensioni compatibili (campi nuovi con `#[serde(default)]`):

- `DownloadProgress` += `provider: String`, `model_ref: String`, `variant: String`,
  `files_total: u32`, `files_done: u32`, `speed_bps: u64`, `dest: String`. `status` vale
  `queued | running | verifying | done | error | cancelled`. `file` resta il file corrente,
  `received`/`total` sono aggregati su tutti i file del job.
- `LocalModel` += `origin: String` (`aiw | ollama | lmstudio | hf-cache | manual`),
  `removable: bool`, `runtime_compat: Vec<RuntimeCompat>`, `fit: String`, `fit_detail: String`.
  `runtimes: Vec<String>` resta (ids di `runtime_compat`, prima i disponibili).
- `CatalogModel` += `runtime_compat: Vec<RuntimeCompat>`, `downloadable: bool` (falso per i modelli
  di sistema). La voce MLX diventa scaricabile come repository intero.

## 4. Endpoint HTTP (stessi percorsi per Tauri, che riusa il router)

| Metodo e percorso | Corpo / query | Risposta |
|---|---|---|
| `GET /api/models/providers` | — | `ModelProviderInfo[]` |
| `PUT /api/models/providers/{id}/auth` | `{ "token": string \| null, "endpoint"?: string }` | `ModelProviderInfo` (il token non torna mai indietro) |
| `GET /api/models/search` | `provider`, `q`, `format?`, `sort?` (`downloads\|likes\|updated`), `limit?` (≤ 50), `cursor?` | `SearchPage` |
| `GET /api/models/remote` | `provider`, `model`, `revision?` | `RemoteModelDetail` |
| `POST /api/models/pull` | `PullRequest` | `DownloadProgress` |
| `GET /api/models/downloads` | — | `DownloadProgress[]` |
| `POST /api/models/downloads/{id}/cancel` | — | `DownloadProgress` |
| `DELETE /api/models/downloads/{id}` | — | `204` (rimuove dalla lista un job concluso) |
| `DELETE /api/models/{id}` | — | `204`; solo modelli `removable` dentro `models.dir`, registrato nell'audit |

CLI: `aiw models search <q> [--provider hf] [--format gguf]`, `aiw models show <provider> <model>`,
`aiw models pull <catalog-id> | --provider <p> --model <m> --variant <v> | --url <url>`,
`aiw models rm <id>`, `aiw models providers`.

## 5. Downloader

- Job = elenco di `FileJob { url, dest, size, sha256, headers }` prodotto dal provider. Coda con
  al massimo 2 job attivi; stato `queued` finché non parte.
- Layout: `models.dir/<provider>/<owner>/<repo>/<percorso nel repo>` (Ollama:
  `models.dir/ollama/<ns>/<name>/<tag>/model.gguf` e `mmproj.gguf`; URL:
  `models.dir/url/<host>/<file>`). Alla fine il downloader scrive `.aiw-model.json` nella cartella
  del repo: `{provider, model, variant, revision, format, quant, files:[{path,size_bytes,sha256}], installed_at}`.
  I file della v0.3 in `models.dir/<file>` restano validi.
- Ogni file passa da `<dest>.part`. Ripresa con `Range` (206 = append, 200 = da capo, 416 con
  `.part` della dimensione attesa = completato). Hash sha256 calcolato in streaming e confrontato
  con quello del provider; in caso di differenza il file viene scartato con errore esplicito.
- Prima di partire: controllo dello spazio libero sul volume di `models.dir` (dimensione mancante +
  5 % + 512 MiB); errore `spazio insufficiente` altrimenti.
- `connect_timeout` 15 s e `read_timeout` 60 s; tre tentativi con backoff (1 s, 4 s, 10 s) sugli
  errori di rete e sui 5xx; nessun tentativo su 401/403/404, che diventano messaggi chiari
  ("repository gated: serve un token Hugging Face", "file non trovato").
- Annullamento immediato (token di cancellazione controllato a ogni chunk e durante le attese). Il
  `.part` resta su disco per una ripresa successiva.
- Gli header di autenticazione sono attaccati dal provider solo alle URL del proprio host; il
  downloader li toglie se un redirect porta su un altro host (i CDN di HF e Ollama non li
  richiedono).
- Al passaggio a `done` il motore riesegue la scansione **una volta** (non a ogni poll).

## 6. Scansione e sorgenti esterne

`GET /api/models` restituisce i modelli di `models.dir` (origine `aiw`, rimovibili) più quelli già
presenti in altre app, in sola lettura e senza copiarli:

- Ollama: `$OLLAMA_MODELS` o `~/.ollama/models` → `manifests/registry.ollama.ai/<ns>/<name>/<tag>`
  (JSON) → blob `blobs/sha256-<hex>` del layer `application/vnd.ollama.image.model` (è un GGUF).
- LM Studio: `~/.lmstudio/models/<publisher>/<repo>/*.gguf` (e `~/.cache/lm-studio/models`).
- Cache Hugging Face: `$HF_HOME/hub` o `~/.cache/huggingface/hub/models--<org>--<repo>/snapshots/<rev>/`.

L'id dei modelli esterni ha un prefisso (`ollama:qwen3:4b`, `lmstudio:<publisher>/<file>`,
`hf-cache:<org>/<repo>/<file>`) ed è accettato come `model` nei profili.

## 7. Compatibilità e fit

- `RuntimeCompat` viene da `RuntimeRegistry`: manifest il cui `formats` contiene il formato e i cui
  `platforms` contengono l'OS corrente; prima i runtime `available`/`running`, poi gli altri con il
  motivo. I runtime solo-manifest senza codice (`windows-ml`, `mlx-swift`, `core-ai`, `executorch`,
  `mlc-llm`, `phi-silica`) risultano `available: false` con motivo "non ancora implementato".
- Il fit di una variante usa la dimensione reale dei file: `min_memory_mb = size_MiB × 1,1 + 700`,
  come il catalogo, più la KV cache stimata quando i metadati GGUF la permettono. MLX richiede macOS
  su Apple Silicon (`arch = aarch64`).
- `recommended` = la variante più grande con fit `great`, altrimenti la più grande con fit `ok`; per
  i GGUF si preferisce `Q4_K_M` a parità di fit.

## 8. Roadmap

GitHub Releases (llamafile, bundle `.pte`/`.task`), Kaggle Models, aggiornamento firmato del
catalogo curato, eventi SSE per i download al posto del polling, pausa/ripresa esplicite.

## 9. Note di implementazione del backend (v0.4)

Dettagli che il contratto sopra lascia aperti e su cui UI e integrazione possono contare.

- **Errori HTTP**: `404` modello/provider/variante/download inesistente, `400` richiesta non valida
  (anche il corpo di `pull` che non combacia con nessuna forma), `401` token mancante o rifiutato dal
  provider (repository gated, token non valido in `PUT …/auth`), `403` accesso vietato, `409` file già
  in scrittura da un altro download, `502` provider irraggiungibile o in errore, `507` spazio
  insufficiente (rifiutato **prima** di accodare il download). Il corpo è sempre `{"error": "…"}` in
  italiano. `aiw-core::Error` ha le varianti nuove `Unauthorized`, `InsufficientStorage`, `Upstream`.
- **`POST /api/models/pull`**: in `Hub` `variant` può mancare o essere vuoto (si usa l'unica variante o
  quella `recommended`); `variant` accetta `id`, etichetta o quant (senza distinzione tra maiuscole).
  Una voce del catalogo si installa come il repository Hugging Face a cui punta (cartella `hf/…`,
  sidecar con `provider: "hf"`), ma il download mantiene `model_id` = id del catalogo.
  `DownloadProgress.model_id` è l'id del catalogo oppure `provider:model`; per associare un download a
  una card usare `provider` + `model_ref` + `variant`. Lo stesso `(provider, model_ref, variant)` già in
  coda o in corso restituisce il job esistente.
- **`PUT /api/models/providers/{id}/auth`**: `token` assente = invariato, `null` = rimosso; `endpoint`
  assente = invariato, `""` = predefinito. Un token nuovo viene verificato col provider (Hugging Face:
  `/api/whoami-v2`): un rifiuto definitivo dà `401` e non salva nulla; se il servizio non risponde il
  token viene salvato lo stesso. All'avvio, se non c'è un token salvato per `hf`, vale `HF_TOKEN`.
  Il provider `ollama` accetta solo `endpoint`, `url` e `catalog` nessuna impostazione.
- **Sidecar `.aiw-model.json`**: la radice del documento è sempre l'ultima installazione (forma del
  contratto); in più `installs[]` elenca tutte le installazioni della cartella, perché lo stesso
  repository può ospitare più varianti (Q4_K_M e Q8_0). I file condivisi (l'`mmproj`) non vengono
  cancellati finché un'altra variante li usa.
- **Id e provenienza dei modelli locali**: i file della v0.3 e quelli copiati a mano hanno `origin:
  "manual"` (rimovibili, senza `source`); le installazioni con sidecar hanno `origin: "aiw"` e
  `source = "provider:model@revisione#variante"`. L'id è il nome file senza estensione né shard (per
  Ollama `nome:tag`, per i repository `nome-del-repo`); se due modelli hanno lo stesso id, quelli nel
  layout nuovo prendono il percorso relativo in minuscolo (`hf/owner/repo/file`). I modelli esterni
  usano `ollama:…`, `lmstudio:<publisher>/<file>.gguf`, `hf-cache:<org>/<repo>/<file>` (o `…/<repo>`
  per i repository non GGUF) e hanno `removable: false`.
- **Eliminazione**: `DELETE /api/models/{id}` accetta id con `/` (percorso catch-all). Toglie i soli file
  dell'installazione e la cartella quando non resta nessun'altra variante; rifiuta i modelli esterni
  (`400`) e quelli raggiunti tramite un link a una cartella fuori da `models.dir` (`403`).
- **Ollama**: le varianti sono i tag del registro (verificati con un manifest ciascuno, in parallelo);
  `latest` non compare se è lo stesso blob di un altro tag. `model` è `nome` o `utente/nome`: un
  `nome:tag` restituisce la sola variante `tag`. Quant e parametri vengono dal blob di configurazione.
- **Scansione esterna**: `Engine::set_external_roots` (e `aiw_models::ExternalRoots`) sostituisce i
  percorsi di Ollama (`$OLLAMA_MODELS`), LM Studio e cache Hugging Face (`$HF_HUB_CACHE`,
  `$HF_HOME/hub`); con `EngineOptions.offline` la scansione esterna è disattivata.
- **Variabili d'ambiente**: `HF_ENDPOINT`, `AIW_MODELSCOPE_ENDPOINT`, `AIW_OLLAMA_REGISTRY` (letti da
  `HubConfig::from_env`) e `HF_TOKEN`.
