# Collegare un client AG-UI (`POST /agui/{agent}`)

Gli eventi dei run di AI Workspace Local sono già nel formato AG-UI (ADR
[0004](architecture/adr/0004-ag-ui-events.md)) e la UI li legge da `GET /api/runs/{id}/events`. Questo endpoint è
l'**ingresso standard** del protocollo: un client AG-UI (`HttpAgent` di `@ag-ui/client`, CopilotKit) fa **una sola
`POST`** con un corpo `RunAgentInput` e riceve lo stream SSE del run nella risposta, senza conoscere l'API REST di AIW.

> Stato onesto: è un **primo taglio**, provato solo con test in-process (`cargo test -p aiw-server agui`). Non è mai
> stato provato con un client AG-UI reale (`@ag-ui/client`, CopilotKit). Se un client vero si aspetta qualcosa che qui
> manca, è un difetto da segnalare. La issue #69 resta aperta per il resto (vedi «Limiti» sotto).

## Cosa fa

`POST /agui/{agent}` (`{agent}` è l'id di un agente, per esempio `planner` o `operator`):

1. legge il `RunAgentInput` e prende il testo dell'**ultimo messaggio con `role: "user"`** (le parti di testo sono
   unite, una per riga) e i suoi file;
2. sceglie la sessione AIW del `threadId` (o ne crea una), come descritto nella sezione «Thread e sessioni»;
3. avvia il run e risponde `200` con `Content-Type: text/event-stream`: ogni evento è una riga `data: <json>` (con
   `id:` uguale al numero d'ordine dell'evento nel run). Nei soli eventi `RUN_STARTED` e `RUN_FINISHED` i campi
   `threadId` e `runId` sono quelli **inviati dal client**; il run interno ha un altro id, che trovi nell'intestazione
   `x-aiw-run-id`. L'intestazione `x-aiw-session-id` dice quale sessione AIW ha ricevuto il messaggio;
4. chiude lo stream subito dopo `RUN_FINISHED` o `RUN_ERROR`.

Corpo accettato (camelCase; i campi sconosciuti sono tollerati):

| Campo | Note |
|---|---|
| `threadId`, `runId` | stringhe, obbligatorie e non vuote |
| `messages` | lista obbligatoria; ogni messaggio ha `id`, `role` e `content` |
| `tools`, `context`, `state`, `forwardedProps`, `parentRunId` | accettati e **ignorati** in questo taglio |

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

Lo stream riprende da solo e finisce con `RUN_FINISHED`. Questo non è il meccanismo interoperabile di
human-in-the-loop di AG-UI (vedi «Limiti»): un client generico che non conosce `aiw.approval_request` non può
rispondere e il run resta fermo finché non lo fa l'utente dalla UI o la richiesta viene chiusa.

## Annullare un run

In AG-UI il modo di fermare un run è **interrompere la richiesta** (`AbortController` del client). Se la connessione
si chiude prima di `RUN_FINISHED` o `RUN_ERROR`, il run viene annullato (come `POST /api/runs/{id}/cancel`).
Un run già terminato non è toccato.

## Limiti (cosa non c'è ancora)

- **Strumenti definiti dal client** (`tools`): ignorati. L'agente usa solo i propri strumenti.
- **`context`**: ignorato, non entra nel prompt.
- **`state` e `STATE_DELTA`**: lo stato inviato dal client è ignorato e non si emettono `STATE_DELTA`; gli eventi
  `STATE_SNAPSHOT` sono quelli già prodotti dal motore (`RunState`), non lo stato condiviso del protocollo.
- **`MESSAGES_SNAPSHOT`**: non emesso; la cronologia non viene sincronizzata con il client.
- **Riconciliazione della cronologia**: i messaggi precedenti dell'ultimo messaggio utente, e i messaggi non utente,
  sono ignorati.
- **Human-in-the-loop interoperabile**: le approvazioni passano come eventi `CUSTOM` `aiw.*` e si risolvono con
  `POST /api/approvals/{id}`, non con il meccanismo standard di AG-UI (`resume` del `RunAgentInput`, ignorato).
- **Ripresa dopo una riconnessione**: se la connessione cade il run viene annullato; non c'è ripresa con
  `Last-Event-ID` su questa rotta (la ripresa esiste su `GET /api/runs/{id}/events`).
- **Agenti**: niente team (`team:…`) né agenti interni; `parentRunId`, `forwardedProps` e `resume` sono ignorati.
- **Mappa dei thread in memoria**: persa al riavvio (vedi sopra).
- **Prova con un client AG-UI reale**: non fatta. Gli eventi sono nel formato AG-UI 1.0 usato dal resto dell'app, ma
  l'intero giro con `@ag-ui/client` o CopilotKit non è stato verificato; l'unica verifica sono i test in-process.
