# Collegare un client AG-UI (`POST /agui/{agent}`)

Gli eventi dei run di AI Workspace Local sono già nel formato AG-UI (ADR
[0004](architecture/adr/0004-ag-ui-events.md)) e la UI li legge da `GET /api/runs/{id}/events`. Questo endpoint è
l'**ingresso standard** del protocollo: un client AG-UI (`HttpAgent` di `@ag-ui/client`, CopilotKit) fa **una sola
`POST`** con un corpo `RunAgentInput` e riceve lo stream SSE del run nella risposta, senza conoscere l'API REST di AIW.

> Stato onesto: è un **primo taglio**, provato solo con test in-process (`cargo test -p aiw-server agui`, e per
> l'agente `assistant` anche `cargo test -p aiw-server tests_harness`). Non è mai stato provato con un client AG-UI
> reale (`@ag-ui/client`, CopilotKit). Se un client vero si aspetta qualcosa che qui manca, è un difetto da segnalare.
> La issue #69 resta aperta per il resto (vedi «Limiti» sotto). Le domande, le richieste di segreto e le conferme degli
> strumenti di gestione dell'agente `assistant` (sezione dedicata più sotto) viaggiano come eventi `CUSTOM aiw.*` con
> risposta su endpoint REST: **non** sono il human-in-the-loop interoperabile di AG-UI.

## Cosa fa

`POST /agui/{agent}` (`{agent}` è l'id di un agente, per esempio `planner` o `operator`):

1. legge il `RunAgentInput` e prende il testo dell'**ultimo messaggio con `role: "user"`** (le parti di testo sono
   unite, una per riga) e i suoi file;
2. sceglie la sessione AIW del `threadId` (o ne crea una), come descritto nella sezione «Thread e sessioni»;
3. avvia il run e risponde `200` con `Content-Type: text/event-stream`: ogni evento è una riga `data: <json>` (con
   `id:` uguale al numero d'ordine dell'evento nel run). Lo stream si apre sempre con `RUN_STARTED`: gli eventi `CUSTOM`
   che il motore emette prima (scelta del profilo, avvisi sugli allegati) arrivano subito dopo, quindi in quel caso gli
   `id:` non sono in ordine crescente. Nei soli eventi `RUN_STARTED` e `RUN_FINISHED` i campi
   `threadId` e `runId` sono quelli **inviati dal client**; il run interno ha un altro id, che trovi nell'intestazione
   `x-aiw-run-id`. L'intestazione `x-aiw-session-id` dice quale sessione AIW ha ricevuto il messaggio;
4. chiude lo stream subito dopo `RUN_FINISHED` o `RUN_ERROR`.

Corpo accettato (camelCase; i campi sconosciuti sono tollerati):

| Campo | Note |
|---|---|
| `threadId`, `runId` | stringhe, obbligatorie e non vuote |
| `messages` | lista obbligatoria; ogni messaggio ha `id`, `role` e `content` |
| `forwardedProps` | letto **solo** `forwardedProps.aiw.interactive` (vedi «L'agente `assistant`»); ogni altra proprietà è ignorata |
| `tools`, `context`, `state`, `parentRunId` | accettati e **ignorati** in questo taglio |

`content` di un messaggio utente è una stringa oppure una lista di parti:

- `{"type": "text", "text": "…"}`;
- file con dati inline, in una delle due forme del protocollo: `{"type": "binary", "mimeType": "…", "data": "<base64>",
  "filename": "…"}` (forma storica) oppure `{"type": "image" | "audio" | "video" | "document", "source": {"type":
  "data", "value": "<base64>", "mimeType": "…"}}` (forma attuale; il nome del file si legge da `metadata.filename`).
  I file diventano **allegati del messaggio**, con gli stessi limiti dell'API REST (al massimo 8 file, 10 MB l'uno,
  25 MB in tutto);
- ogni altro tipo di parte, e ogni parte il cui file è solo un `url` o un `id` (senza dati), è rifiutato con `400` e
  un messaggio che ne nomina il tipo.

Esempio con `curl` (`-N` disattiva il buffer, così gli eventi si vedono man mano):

```sh
curl -N http://127.0.0.1:7777/agui/planner \
  -H 'Content-Type: application/json' \
  -d '{
        "threadId": "conversazione-1",
        "runId": "run-1",
        "messages": [{"id": "m1", "role": "user", "content": "Che cosa c'\''è nella cartella di lavoro?"}],
        "tools": [], "context": [], "state": {}, "forwardedProps": {}
      }'
```

Risposta (accorciata):

```text
id: 0
data: {"type":"RUN_STARTED","threadId":"conversazione-1","runId":"run-1","timestamp":1788000000000}

id: 4
data: {"type":"TEXT_MESSAGE_START","messageId":"…","role":"assistant","timestamp":…}

id: 5
data: {"type":"TEXT_MESSAGE_CONTENT","messageId":"…","delta":"La cartella è vuota.","timestamp":…}

id: 9
data: {"type":"RUN_FINISHED","threadId":"conversazione-1","runId":"run-1","timestamp":…}
```

## Thread e sessioni

Un thread AG-UI corrisponde a una sessione AIW:

- se `threadId` è l'**id di una sessione esistente**, si usa quella sessione;
- altrimenti si cerca `threadId` in una mappa **in memoria** `threadId → sessione`; se manca, si crea una sessione con
  l'agente `{agent}` (titolo: la prima riga del messaggio) e la si registra;
- due richieste con lo stesso `threadId` proseguono quindi nella stessa sessione e la cronologia cresce;
- se la sessione trovata è di un **altro agente**, la risposta è `409`.

La mappa vive nella memoria del server: **dopo un riavvio si perde**. Un `threadId` che non è un id di sessione
ricomincia allora da una sessione nuova (la vecchia resta nell'elenco delle sessioni e si può riaprire dalla UI). Per
conversazioni che devono sopravvivere ai riavvii, usa come `threadId` l'id della sessione (header
`x-aiw-session-id` della prima risposta).

La cronologia è quella **salvata dal server** per la sessione. I messaggi precedenti che il client manda dentro
`messages` non vengono riconciliati con essa: conta solo l'ultimo messaggio utente.

## Errori prima dello stream

Sono JSON `{"error": "<messaggio in italiano>"}` con lo stato HTTP del resto dell'API:

| Stato | Quando |
|---|---|
| `400` | corpo non valido o con campi mancanti, nessun messaggio utente, messaggio vuoto, parte di contenuto non supportata, allegato non leggibile o troppo grande |
| `404` | l'agente `{agent}` non esiste |
| `409` | la sessione ha già un run in corso, oppure appartiene a un altro agente |
| `403` / `401` | token mancante o con ambito insufficiente (vedi sotto) |

## Token e ambito

La rotta è protetta come le altre dell'API. Dal computer stesso funziona senza token; da un altro dispositivo serve un
token con ambito **`device`** (o l'owner). Un token **`gateway`** (pensato per le app di terze parti su `/v1/*`) riceve
`403`: l'endpoint avvia agenti con i loro strumenti, come `/api/sessions/**`. Il limite del corpo è quello di
`POST /api/sessions/{id}/runs` (allegati compresi).

Il browser può chiamare l'endpoint solo dalle origini già ammesse dall'API locale: le web view dell'app,
`localhost` e `127.0.0.1` su qualsiasi porta, e gli indirizzi di questo dispositivo sulla LAN. Per le altre origini la
richiesta (e il preflight) riceve `403 origine non consentita`; un client che non è un browser non è toccato da questa
regola. L'intestazione `Authorization: Bearer …` è tra quelle consentite.

## Approvazioni

Quando un'azione chiede conferma (agente in modalità `ask`, per esempio `operator`) il run si ferma e lo stream porta,
invariato, un evento `CUSTOM` con `name: "aiw.approval_request"`: `value.id` è l'id della richiesta. Gli eventi
`CUSTOM` `aiw.*` passano così come sono. Il client risponde con l'API esistente, **in un'altra richiesta**:

```sh
curl -X POST http://127.0.0.1:7777/api/approvals/<id> \
  -H 'Content-Type: application/json' -d '{"decision": "allow"}'   # oppure "deny"
```

Lo stream riprende da solo e finisce con `RUN_FINISHED`; quando la richiesta si chiude il motore emette anche
`CUSTOM aiw.approval_resolved` con `value` `{"id": "…", "allowed": true|false}`. Questo non è il meccanismo
interoperabile di human-in-the-loop di AG-UI (vedi «Limiti»): un client generico che non conosce
`aiw.approval_request` non può rispondere e il run resta fermo finché non lo fa l'utente dalla UI o la richiesta viene
chiusa. Non c'è un ripiego come per le domande: l'attesa non ha limite di tempo, a meno che l'impostazione
`approvals.timeout_s` (0 = nessun limite, predefinito) ne fissi uno.

**La conferma di uno strumento di gestione vale solo se arriva dal proprietario.** Gli strumenti `aiw_*` che
modificano qualcosa (e `aiw_secret_clear`) chiedono sempre conferma, in qualunque modalità dell'agente e anche dopo
«Consenti per la sessione» (che per loro non esiste: `remember` è ignorato). `POST /api/approvals/{id}` ha ambito
`device`, ma per una richiesta di uno strumento di gestione risponde `403` («Questa richiesta può essere decisa solo
dal proprietario…») se chi risponde non ha accesso completo: un token `device` o il token `owner` presentato dalla LAN
senza TLS. La richiesta resta aperta. Anche la risposta di un canale (Telegram, Slack, email) è rifiutata per queste
richieste: i canali rispondono come non proprietari.
Hanno accesso completo le richieste dal computer stesso senza token e quelle col token `owner` su TLS. Le conferme
degli strumenti ordinari (`write_file`, `shell`, …) si possono ancora dare con un token `device`.

## L'agente `assistant`: domande, segreti e strumenti di gestione

`POST /agui/assistant` avvia l'agente `assistant` («Assistente»), l'agente della chat in modalità semplice
dell'app: modalità `ask`, profilo `auto:agent`, al massimo 40 turni. È un agente predefinito come gli altri (7 in
tutto: `operator`, `coder`, `browser`, `desktop`, `planner`, `teacher`, `assistant`); le installazioni esistenti lo
ricevono all'avvio. Ha 25 strumenti:

- 7 interni: `agent` (delega ai sottoagenti), `todo`, `memory_save`, `memory_search`, `memory_update`,
  `memory_forget`, `kb_search`;
- `ask_user`: una domanda all'utente (vedi sotto);
- 17 strumenti `aiw_*`, che sono **strumenti del registro** e passano da policy, hook, audit e approvazioni come ogni
  altro. Senza conferma: `aiw_inspect` (stato compatto, valori segreti mai inclusi), `aiw_model_search`,
  `aiw_secret_request`. Con conferma sempre chiesta (regola `ask` incorporata nell'app, valida anche in `auto` e
  `yolo`): `aiw_model_download`, `aiw_model_use`, `aiw_runtime_control`, `aiw_setting_set`, `aiw_agent_save`,
  `aiw_memory_review`, `aiw_secret_clear` e, mostrati come rischio alto, `aiw_model_delete`, `aiw_runtime_install`,
  `aiw_setting_set_protected`, `aiw_connector_save`, `aiw_connector_remove`, `aiw_agent_delete`,
  `aiw_memory_delete`.

Gli strumenti di gestione funzionano **solo** in un run di primo livello avviato dal server per una persona
(`POST /api/sessions/{id}/runs` o questo endpoint): chiamati da un subagente, da un workflow, dal server MCP di AIW,
da un canale, dall'assistente proattivo, da un'API A2A o da `aiw run` sul terminale rispondono «disponibile solo
all'Assistente in una chat» e non chiedono nemmeno conferma. Le impostazioni di sicurezza (modalità di approvazione,
sandbox, rete, TLS, `approvals.timeout_s`, porta, telemetria) l'agente non le può cambiare con nessuno strumento.

Una richiesta fatta così mostra il flusso, con un client che sa mostrare domande e segreti:

```sh
curl -N http://127.0.0.1:7777/agui/assistant \
  -H 'Content-Type: application/json' \
  -d '{
        "threadId": "chat-1", "runId": "run-1",
        "messages": [{"id": "m1", "role": "user", "content": "Scarica un modello adatto a questo computer"}],
        "forwardedProps": {"aiw": {"interactive": ["questions", "secrets"]}}
      }'
```

### `forwardedProps.aiw.interactive`: che cosa sa mostrare il client

È l'unica proprietà di `forwardedProps` che l'endpoint legge. È una lista che può contenere `"questions"` (il client
mostra `aiw.question` e risponde su `POST /api/questions/{id}`) e `"secrets"` (lo stesso per `aiw.secret_request` e
`POST /api/secret-requests/{id}`). Qualunque altra forma (assente, `null`, una stringa, nomi diversi) vale come nessuna
dichiarazione; i nomi sconosciuti sono ignorati.

| Dichiarazione | `ask_user` | `aiw_secret_request` |
|---|---|---|
| assente | risponde subito `unsupported`: l'agente fa la domanda **in testo** e l'utente risponde col messaggio dopo | risponde subito `unsupported`: l'agente spiega che il segreto va inserito dall'app su questo computer |
| `["questions"]` | il run aspetta la risposta | `unsupported` |
| `["questions", "secrets"]` | il run aspetta la risposta | il run aspetta il segreto **solo se il chiamante è il proprietario**; con un token `device` risponde `unsupported`, perché l'endpoint del segreto richiede accesso completo |

Con `unsupported` non c'è nessun evento `aiw.*` e nessuna attesa: l'esito sta solo nel risultato dello strumento. I run
avviati dall'app con `POST /api/sessions/{id}/runs` contano come se il client avesse dichiarato le domande, e i segreti
solo se chi chiama è il proprietario. Se un client dichiara una capacità che poi non rispetta, il run aspetta
`approvals.timeout_s` (300 secondi se non è impostato) e va avanti con l'esito `timeout`.

### Eventi

Quattro eventi `CUSTOM`, con campi in `snake_case`; un valore assente è un `null` esplicito (i tipi sono in
`crates/aiw-core/src/api.rs`). Arrivano in questo ordine rispetto al `TOOL_CALL_START` dello strumento (la chiamata
si annuncia prima, poi si apre la richiesta) e, per ogni richiesta, quello di chiusura precede `TOOL_CALL_RESULT`.

`aiw.question`, all'apertura di una domanda di `ask_user`:

```json
{"type": "CUSTOM", "name": "aiw.question", "value": {
  "id": "ask_…", "run_id": "run_…", "kind": "choice", "title": "Quale modello?",
  "message": "Ho trovato tre varianti adatte a questo computer.",
  "options": [{"id": "q4", "label": "Q4_K_M · 4,7 GB", "description": "Consigliata"}, {"id": "q8", "label": "Q8_0 · 8,5 GB", "description": null}],
  "multiple": false, "allow_other": false, "fields": [], "confirm_label": null, "deny_label": null,
  "expires_at": "2026-11-14T10:05:00Z"}}
```

`kind` è `choice` (scelta tra 2 e 8 `options`, con `multiple` e `allow_other`), `confirm` (sì o no, con
`confirm_label` e `deny_label`) o `form` (da 1 a 8 `fields` con `name`, `label`, `type` tra `string`, `number`,
`integer`, `boolean`, `enum`, e per `enum` le `options`; `required`, `placeholder`). Un campo il cui nome, etichetta o
segnaposto chiede un segreto (password, token, chiave) è rifiutato: i segreti non passano da `ask_user`.

`aiw.question_resolved`, alla chiusura:

```json
{"type": "CUSTOM", "name": "aiw.question_resolved", "value": {
  "id": "ask_…", "outcome": "answered",
  "answer": {"selected": ["q4"], "other": null, "confirmed": null, "values": null}}}
```

`outcome` è `answered`, `declined`, `timeout` o `cancelled`; `answer` è `null` quando non c'è una risposta. (`unsupported`
esiste solo nel risultato dello strumento.)

`aiw.secret_request`, all'apertura della richiesta di un segreto. **Non contiene mai un valore** e non c'è un campo che
possa contenerlo:

```json
{"type": "CUSTOM", "name": "aiw.secret_request", "value": {
  "id": "sec_…", "run_id": "run_…", "target": "provider_token:hf", "label": "Token di Hugging Face",
  "reason": "Per i modelli riservati", "destination": "Hugging Face (huggingface.co)",
  "expires_at": "2026-11-14T10:05:00Z"}}
```

`target` dice dove andrà il segreto ed è anche il riferimento con cui il modello lo nomina senza vederlo:
`provider_token:<provider>`, `mcp_env:<connettore>:<NOME>`, `mcp_header:<connettore>:<Nome>`, `setting:<chiave>` (solo
impostazioni segrete), `runtime_setting:<motore>:<chiave>` (solo campi segreti del motore). `destination` è il testo
che il **motore** calcola da sé (il modello non lo può scrivere) e che il client deve mostrare vicino al campo.

`aiw.secret_resolved`:

```json
{"type": "CUSTOM", "name": "aiw.secret_resolved", "value": {"id": "sec_…", "outcome": "stored"}}
```

`outcome` è `stored`, `declined`, `timeout` o `cancelled`.

Risultati che il modello riceve (anche questi compaiono in `TOOL_CALL_RESULT`): `ask_user` →
`{"outcome": "answered", "selected": […], "other": null, "confirmed": null, "values": null}`, oppure
`{"outcome": "declined"}`, `{"outcome": "timeout"}`, `{"outcome": "cancelled"}`,
`{"outcome": "unsupported", "hint": "Fai la domanda in testo e aspetta il prossimo messaggio."}`;
`aiw_secret_request` → `{"outcome": "stored", "target": "provider_token:hf"}` e gli altri esiti come sopra. La risposta
a una domanda non è un segreto: resta nei risultati e nella cronologia come ogni altro risultato di uno strumento.

### Endpoint

| Metodo e percorso | Corpo | Risposta | Ambito |
|---|---|---|---|
| `GET /api/questions` | — | lista delle domande aperte (stessa forma di `value` di `aiw.question`), le più vecchie prima | `device` |
| `POST /api/questions/{id}` | `{"action": "answer", "selected": ["q4"], "other": null, "confirmed": null, "values": null}` (i campi che non servono possono mancare) oppure `{"action": "decline"}` | `200 {}`; `400` risposta che non va con la domanda (resta aperta); `404` già chiusa o inesistente | `device` |
| `GET /api/secret-requests` | — | lista delle richieste aperte, mai con un valore | `device` |
| `POST /api/secret-requests/{id}` | `{"action": "provide", "value": "…"}` oppure `{"action": "decline"}` | `200 {"stored": true}` oppure `200 {}`; vedi gli errori sotto | **`full`** |

Regole della risposta a una domanda: `choice` → `selected` con id esistenti (uno solo se `multiple` è falso), oppure
`other` se `allow_other` (non entrambi con una scelta singola; al massimo 2000 caratteri); `confirm` → `confirmed`
booleano; `form` → `values` controllato sui campi (tipi, valori di `enum`, campi `required`). Un corpo di tipo sbagliato
(per esempio `selected` come stringa) è un `400`, non un `422`.

Errori di `POST /api/secret-requests/{id}`, tutti con una frase fissa o con un testo ripulito del valore (il valore non
viene mai ripetuto):

| Stato | Quando |
|---|---|
| `403` | chi chiama non ha accesso completo: un token `device`, un token `gateway`, il token `owner` presentato dalla LAN senza TLS. La richiesta resta aperta e non si scrive nulla |
| `400` | corpo che non va (manca `action`, `value` non è un testo, azione sconosciuta), valore vuoto, più lungo di 4096 caratteri, uguale alla maschera `••••••` o con caratteri di controllo. La richiesta resta aperta |
| `401` | la destinazione rifiuta il valore (per esempio un token che il provider non accetta). La richiesta resta aperta e si può riprovare |
| `404` | richiesta già chiusa, scaduta o inesistente |
| `409` | in due casi: un altro valore è in fase di salvataggio («il valore è in fase di salvataggio»); oppure la destinazione **non è più quella che la richiesta mostrava** (per esempio il connettore è stato modificato o rimosso): il valore non viene scritto, la richiesta si chiude e il modello riceve `declined` con un avviso che chiede di richiederlo |
| altro | un errore della destinazione (per esempio un connettore che non si salva) ha lo stato del proprio errore, sempre senza il valore |

Dove passa il valore: il corpo di quella sola richiesta HTTP, la memoria del motore e la destinazione, scritto con la
funzione che esiste già per quella destinazione. Non passa da argomenti o risultato dello strumento, eventi (compreso
questo stream), messaggi salvati, riga di audit (`secret.set` con il solo `target`), richiesta di approvazione e prompt
del modello. Dove viene salvato resta **in chiaro** nello stesso store di prima (vedi `SECURITY.md`): questo flusso non
lo cambia.

Attesa: `approvals.timeout_s` se impostato, altrimenti 300 secondi; allo scadere l'esito è `timeout`. Un run annullato
(compresa la richiesta HTTP chiusa dal client) o il kill switch chiudono domande e richieste in attesa con `cancelled`.

### Che cosa vede un client che non conosce `aiw.*`

Se non dichiara nulla: `TOOL_CALL_START` con `toolCallName: "ask_user"`, i `TOOL_CALL_ARGS` con la domanda in JSON
leggibile, il `TOOL_CALL_RESULT` con `unsupported` e poi il testo con cui l'agente rifà la domanda. Se dichiara
`questions` senza saperle mostrare vede la chiamata dello strumento e l'evento `aiw.question` (che ignora), e poi
nulla: il run si ferma fino alla scadenza o finché la domanda non si chiude da un'altra parte (per esempio dall'app).

## Annullare un run

In AG-UI il modo di fermare un run è **interrompere la richiesta** (`AbortController` del client). Se la connessione
si chiude prima di `RUN_FINISHED` o `RUN_ERROR`, il run viene annullato (come `POST /api/runs/{id}/cancel`).
Un run già terminato non è toccato.

## Limiti (cosa non c'è ancora)

- **Strumenti definiti dal client** (`tools`): ignorati. L'agente usa solo i propri strumenti: un frontend non può
  fornire un proprio `ask_user`.
- **`context`**: ignorato, non entra nel prompt.
- **`state` e `STATE_DELTA`**: lo stato inviato dal client è ignorato e non si emettono `STATE_DELTA`; gli eventi
  `STATE_SNAPSHOT` sono quelli già prodotti dal motore (`RunState`), non lo stato condiviso del protocollo.
- **`MESSAGES_SNAPSHOT`**: non emesso; la cronologia non viene sincronizzata con il client.
- **Riconciliazione della cronologia**: i messaggi precedenti dell'ultimo messaggio utente, e i messaggi non utente,
  sono ignorati.
- **Human-in-the-loop interoperabile: non c'è.** Approvazioni, domande (`aiw.question`) e richieste di segreto
  (`aiw.secret_request`) sono eventi `CUSTOM` `aiw.*` che si risolvono con endpoint REST di AIW in una richiesta
  separata (`POST /api/approvals/{id}`, `/api/questions/{id}`, `/api/secret-requests/{id}`), non con il meccanismo
  standard di AG-UI (`resume` del `RunAgentInput`, ignorato). Un client generico che non li conosce non può rispondere.
  La dichiarazione `forwardedProps.aiw.interactive` è un'estensione di AIW, non del protocollo: serve solo a evitare
  che il run aspetti una risposta che nessuno può dare per domande e segreti. Per le **approvazioni non c'è un ripiego**
  e il run resta fermo finché qualcuno risponde.
- **Segreti da un client remoto**: l'endpoint del valore richiede accesso completo. Un client con un token `device`
  non può fornire segreti (l'agente ne riceve `unsupported`), e lo stesso vale per il token `owner` dalla LAN senza TLS.
  Le conferme degli strumenti di gestione hanno la stessa regola: solo il proprietario.
- **Ripresa dopo una riconnessione**: se la connessione cade il run viene annullato e una domanda o una richiesta di
  segreto aperta si chiude con `cancelled`; non c'è ripresa con `Last-Event-ID` su questa rotta (la ripresa esiste su
  `GET /api/runs/{id}/events`).
- **Agenti**: niente team (`team:…`) né agenti interni; `parentRunId` e `resume` sono ignorati, e di `forwardedProps` si
  legge solo `aiw.interactive`.
- **Mappa dei thread in memoria**: persa al riavvio (vedi sopra).
- **Prova con un client AG-UI reale**: non fatta. Gli eventi sono nel formato AG-UI 1.0 usato dal resto dell'app, ma
  l'intero giro con `@ag-ui/client` o CopilotKit non è stato verificato, e non si sa che cosa faccia un client vero
  con eventi `CUSTOM` che non conosce; l'unica verifica sono i test in-process (per l'agente `assistant`,
  `cargo test -p aiw-server tests_harness`: un client simulato legge lo stream, risponde sugli endpoint REST e verifica
  l'ordine degli eventi).
