# Collegare un client AG-UI (`POST /agui/{agent}`)

Gli eventi dei run di AI Workspace Local sono nel formato AG-UI (ADR [0004](architecture/adr/0004-ag-ui-events.md)) e la UI
li legge da `GET /api/runs/{id}/events`. Questo endpoint è l'**ingresso standard** del protocollo: un client AG-UI
(`HttpAgent` di `@ag-ui/client`, il runtime di CopilotKit) fa **una sola `POST`** con un corpo `RunAgentInput` e riceve lo
stream SSE del run nella risposta, senza conoscere l'API REST di AIW. Come puntare modelli e agenti di altri programmi
su AIW (token, CORS, rete, OpenBot, OpenDots) è nella pagina **Integrazioni** dell'app, che usa questi stessi esempi.

> Stato onesto. **Provato con client veri**: `tests/agui-client` avvia il vero `aiw serve`, un provider OpenAI finto e
> `HttpAgent` di `@ag-ui/client` **1.0.4** e **1.0.2**, 24 scenari; `tests/agui-copilotkit` mette davanti allo stesso server il
> **runtime di CopilotKit** 1.88.0 (lato server, senza account) e ne fa passare un messaggio con ragionamento e una conferma con
> `resume` (vedi «Provarlo con un client vero»). **Non provato**: i componenti React di CopilotKit (la parte nel browser), OpenBot
> e OpenDots (richiedono un account CopilotKit e CopilotKit Intelligence, licenza a parte), un modello vero, un telefono,
> Windows. Se un client reale si aspetta qualcosa che qui manca, è un difetto da segnalare. Che cosa **non** c'è (strumenti
> del client, `STEP_*`, attività) è in «Limiti». Il client vero ha trovato e fatto correggere tre difetti che i test in-process
> non vedevano: `REASONING_MESSAGE_*` mancanti, `SUBAGENT_*` con i campi sbagliati, `STATE_SNAPSHOT` che cancellava lo stato
> del client.

## Cosa fa

`POST /agui/{agent}` (`{agent}` è l'id di un agente, per esempio `planner` o `operator`):

1. legge il `RunAgentInput` e prende il testo dell'**ultimo messaggio con `role: "user"`** (le parti di testo sono
   unite, una per riga) e i suoi file;
2. sceglie la sessione AIW del `threadId` (o ne crea una), come descritto in «Thread, cronologia e ripresa»;
3. avvia il run e risponde `200` con `Content-Type: text/event-stream`: ogni evento è una riga `data: <json>`, con
   `id:` uguale al numero d'ordine nel log del run quando l'evento viene da lì (gli eventi che il server aggiunge, come gli
   snapshot dei messaggi, non hanno `id:`; un evento del run può diventare più eventi con lo stesso `id:`). Lo stream si apre
   con `RUN_STARTED`: gli eventi `CUSTOM` che il motore emette prima (scelta del profilo, avvisi sugli allegati)
   arrivano subito dopo, quindi in quel caso gli `id:` non sono in ordine crescente (eccezione: un run che finisce o fallisce prima
   di `RUN_STARTED` manda prima quei `CUSTOM`, e il client vero lo rifiuta). In `RUN_STARTED` e `RUN_FINISHED` i
   campi `threadId` e `runId` sono quelli **inviati dal client**; il run interno ha un altro id, nell'intestazione
   `x-aiw-run-id`; `x-aiw-session-id` dice quale sessione AIW ha ricevuto il messaggio;
4. chiude lo stream dopo `RUN_FINISHED` o `RUN_ERROR`, oppure dopo un `RUN_FINISHED` con esito `interrupt` (vedi
   «Approvazioni e human-in-the-loop interoperabile»).

Corpo accettato (camelCase; i campi sconosciuti sono tollerati):

| Campo | Significato qui |
|---|---|
| `threadId`, `runId` | stringhe, obbligatorie e non vuote |
| `messages` | lista obbligatoria; conta l'ultimo messaggio utente (`id`, `role`, `content`); la cronologia è quella del server |
| `state` | lo stato condiviso del client: se è un oggetto, resta in ogni `STATE_SNAPSHOT` e arriva al modello (vedi «Stato e contesto») |
| `context` | `[{description, value}]`, dati per il modello per questo solo run |
| `forwardedProps` | letto **solo** `forwardedProps.aiw.interactive` (vedi «L'agente `assistant`») |
| `resume` | le risposte alle interruzioni di un run in pausa (vedi «Approvazioni…») |
| `tools`, `parentRunId` | accettati e **ignorati**: gli strumenti definiti dal client non sono supportati e non c'è ramificazione dei run |

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
        "messages": [{"id": "m1", "role": "user", "content": "Che cosa c'\''è nella cartella di lavoro?"}]
      }'
```

Negli esempi gli id sono fissi (`m1`, `run-1`): rilanciando lo stesso comando con lo stesso testo il server riconosce il messaggio già
eseguito e rimanda la risposta di prima senza chiamare il modello. Cambia l'`id` del messaggio (e il testo) a ogni prova.

Con `@ag-ui/client` (provato, è lo stesso schema dei test):

```js
const { HttpAgent } = await import("@ag-ui/client");
const agent = new HttpAgent({ url: "http://127.0.0.1:7777/agui/planner", threadId: "conversazione-1" });
agent.messages = [{ id: crypto.randomUUID(), role: "user", content: "Saluta" }];
await agent.runAgent({ runId: crypto.randomUUID() });
console.log(agent.messages.at(-1).content);
```

### Che cosa vede il client: gli eventi

Gli eventi di un run sono salvati e serviti così come sono da `/api/runs/{id}/events`; `/agui` li passa da un traduttore
(`crates/aiw-server/src/agui_wire.rs`), perché `@ag-ui/client` controlla ogni stream e si ferma al primo evento fuori forma
o fuori ordine. Le differenze dal log:

| Evento del run | Su `/agui` |
|---|---|
| `REASONING_START` … `REASONING_END` | con `REASONING_MESSAGE_START` (`role: "reasoning"`) prima del primo pezzo e `REASONING_MESSAGE_END` prima della fine; pezzi vuoti scartati |
| `SUBAGENT_STARTED` / `SUBAGENT_FINISHED` | con `subagentRunId` (l'id del run del sottoagente) al posto di `subagentId`/`runId`, e `parentToolCallId` se la chiamata dello strumento `agent` è una sola con quel nome; un sottoagente fallito è `SUBAGENT_ERROR` |
| `STATE_SNAPSHOT` | lo stato del motore (`RunState`: piano, consumo, modello) sta sotto la chiave **`aiw`**, accanto allo stato del client; il primo è uno snapshot, i seguenti sono **`STATE_DELTA`** (RFC 6902, `replace`/`add`/`remove` di `/aiw/<chiave>`) e uno stato che non è cambiato non si manda |
| `TOOL_CALL_RESULT` | con `role: "tool"` |
| `CUSTOM` | invariati, `aiw.*` compresi |
| (aggiunti) `MESSAGES_SNAPSHOT` | la cronologia del thread, nei casi di «Thread, cronologia e ripresa» e prima di un'interruzione |

La spec, per ognuno: `STATE_SNAPSHOT` «sostituisce» lo stato del client e `STATE_DELTA` sono operazioni RFC 6902 applicate in
ordine ([concetti: stato](https://docs.ag-ui.com/concepts/state), [eventi](https://docs.ag-ui.com/concepts/events)); per
questo lo stato del motore non va alla radice (cancellerebbe quello dell'applicazione) e il primo evento dello stato dopo una
ripresa è di nuovo uno snapshot (un client che ha perso la risposta non ha la base di un delta).

## Stato e contesto

- **`state`** (spec: lo stato condiviso, `any`; l'assenza o `null` valgono nessuno stato). Se è un oggetto: (1) resta in ogni
  `STATE_SNAPSHOT` della risposta, con le chiavi che il client ha mandato (quella `aiw` è del motore e vince); (2) arriva al
  modello, con `context`, in una sezione del prompt di sistema («Stato condiviso dall'applicazione»). L'agente **non** scrive
  nello stato del client: non ha uno strumento per farlo e la risposta non lo cambia.
- **`context`** (spec: `{description, value}`, descrizione e valore testuali). Ogni voce con un valore non vuoto va al modello
  nella stessa sezione, che dice di trattarla come informazione e non come istruzione: non cambia le regole dell'agente e non
  dà permessi. Per questo run soltanto: **non entra nella cronologia della sessione** (né nelle tracce esportate, che sono i messaggi della sessione). Tolti i caratteri di controllo, tagliato a
  16 000 caratteri in tutto. **Non metterci segreti**: arrivano al modello come ogni altro testo del prompt.
- Chi controlla i dati è il client già autenticato (token `device` o proprietario): se la tua applicazione vi inoltra
  contenuti di altri (pagine web, posta), restano contenuti di altri, ed è l'agente con le sue regole e approvazioni a
  decidere cosa farne.

## Thread, cronologia e ripresa

Un thread AG-UI corrisponde a una sessione AIW, e il legame è **durevole**: un documento `agui_thread` nello store dei dati
(sopravvive ai riavvii e sparisce con la sessione; solo id, mai testi).

- se `threadId` è l'**id di una sessione esistente**, si usa quella sessione;
- altrimenti si cerca `threadId` nel record; se manca, si crea una sessione con l'agente `{agent}` (titolo: la prima riga del
  messaggio) e la si registra. Un record la cui sessione è stata cancellata riparte da una sessione nuova;
- due richieste con lo stesso `threadId` proseguono nella stessa sessione, anche dopo un riavvio;
- se la sessione trovata è di un **altro agente**, la risposta è `409`.

**La cronologia è quella del server.** I messaggi precedenti che il client manda in `messages` non si ricostruiscono: conta
l'ultimo messaggio utente. Per questo:

- **un messaggio già eseguito non si rifà.** Se l'ultimo messaggio utente ha un `id` che il thread ha già eseguito, con lo stesso
  testo e lo stesso numero di file, e quel run è finito bene (o è ancora in corso o in pausa), la risposta rimanda quel run:
  `RUN_STARTED` con i `threadId`/`runId` di questa richiesta, la cronologia fino al messaggio (`MESSAGES_SNAPSHOT`) e gli
  eventi del run dall'inizio. Nessuna nuova chiamata al modello, nessun messaggio doppio nella sessione. Vale dopo un
  riavvio (gli eventi dei run sono nello store). Un messaggio con un id noto ma un testo diverso (un client che manda sempre
  `m1`) è un messaggio nuovo; un run fallito o annullato si può rifare, e allora il messaggio resta due volte nella cronologia;
- **un client senza la cronologia del thread** (una pagina ricaricata che manda solo il messaggio nuovo, o una cronologia che il
  server non riconosce) riceve subito dopo `RUN_STARTED` la cronologia del server, con gli id che il client ha dato ai suoi
  messaggi utente e gli id del server per gli altri. È un `MESSAGES_SNAPSHOT` **in due tempi** (prima senza il messaggio nuovo,
  poi con): `@ag-ui/client` tiene al loro posto i messaggi che lo snapshot ripete, toglie gli altri (salvo ragionamento e attività: un
  messaggio `system` o `developer` che il client ha aggiunto da sé sparisce a ogni snapshot) e aggiunge dietro quelli nuovi, quindi
  un solo snapshot metterebbe la cronologia dopo il messaggio appena scritto. Un client già allineato non riceve nulla;
- gli snapshot non contengono mai i byte di un allegato: solo `metadata.aiw.attachments` (id, nome, tipo, dimensione), i byte
  sono a `/api/sessions/{id}/attachments/{id}`. Il ragionamento e le note di errore dei run non sono messaggi dello snapshot.

La spec dei messaggi e della ripresa è in [messaggi](https://docs.ag-ui.com/concepts/messages), [eventi](https://docs.ag-ui.com/concepts/events)
e [interruzioni](https://docs.ag-ui.com/concepts/interrupts). **Non c'è `Last-Event-ID` su questa rotta**: `HttpAgent` non lo
usa e non implementa `connect()` (`connectAgent()` non fa nulla: lo si vede nel codice di `@ag-ui/client` 1.0.4); la ripresa con
`Last-Event-ID` esiste su `GET /api/runs/{id}/events`. Ripresa vuol dire quindi: rimandare lo stesso messaggio (sopra) o rispondere a
un'interruzione (`resume`, sotto).

## Errori prima dello stream

Sono JSON `{"error": "<messaggio in italiano>"}` con lo stato HTTP del resto dell'API:

| Stato | Quando |
|---|---|
| `400` | corpo non valido o con campi mancanti, nessun messaggio utente, messaggio vuoto, parte di contenuto non supportata, allegato non leggibile o troppo grande |
| `404` | l'agente `{agent}` non esiste |
| `409` | la sessione ha già un run in corso, oppure appartiene a un altro agente |
| `403` / `401` | token mancante o con ambito insufficiente (vedi sotto), oppure un `resume` a una conferma di uno strumento di gestione da chi non è il proprietario |

## Token e ambito

La rotta è protetta come le altre dell'API. Dal computer stesso funziona senza token; da un altro dispositivo serve un
token con ambito **`device`** (o l'owner). Un token **`gateway`** (pensato per le app di terze parti su `/v1/*`) riceve
`403`: l'endpoint avvia agenti con i loro strumenti, come `/api/sessions/**`. Il limite del corpo è quello di
`POST /api/sessions/{id}/runs` (allegati compresi).

Il browser può chiamare l'endpoint solo dalle origini già ammesse dall'API locale: le web view dell'app,
`localhost` e `127.0.0.1` su qualsiasi porta, e gli indirizzi di questo dispositivo sulla LAN. Per le altre origini la
richiesta (e il preflight) riceve `403 origine non consentita`; un client che non è un browser non è toccato da questa
regola. L'intestazione `Authorization: Bearer …` è tra quelle consentite.

## Approvazioni e human-in-the-loop interoperabile

Quando un'azione chiede conferma (agente in modalità `ask`, per esempio `operator`) o l'agente fa una domanda con `ask_user`, il run
si ferma ad aspettare una persona. Ci sono **due modi** di accorgersene dal client, e si sceglie con
`forwardedProps.aiw.interactive`:

**1. Eventi `CUSTOM` (predefinito).** Lo stream porta, invariato, `CUSTOM aiw.approval_request` (`value.id` è l'id della
richiesta) e, con `"questions"` dichiarato, `aiw.question`. Il client risponde con l'API REST, **in un'altra richiesta**:

```sh
curl -X POST http://127.0.0.1:7777/api/approvals/<id> \
  -H 'Content-Type: application/json' -d '{"decision": "allow"}'   # oppure "deny"
```

Lo stream riprende da solo e finisce con `RUN_FINISHED`; quando la richiesta si chiude il motore emette anche
`CUSTOM aiw.approval_resolved` con `value` `{"id": "…", "allowed": true|false}`. Un client generico che non conosce
`aiw.approval_request` non può rispondere e il run resta fermo finché non lo fa l'utente dalla UI o la richiesta viene chiusa
(nessun limite di tempo, a meno di `approvals.timeout_s`). Chiudere la richiesta annulla il run.

**2. Interruzioni AG-UI (si chiede con `"interrupts"`).** Il client che dichiara `{"aiw": {"interactive": ["interrupts"]}}` sa
fare ciò che la spec definisce in [interruzioni](https://docs.ag-ui.com/concepts/interrupts): una pausa è un `RUN_FINISHED` con
`outcome: {type: "interrupt", interrupts: […]}` e si continua con un **nuovo** `RunAgentInput` sullo stesso thread che porta
`resume`. È opt-in perché un client che non lo conosce non saprebbe riprendere (lo stesso fa l'integrazione LangGraph di
AG-UI, dove l'esito `interrupt` è disattivato di default). L'esito arriva così:

- prima, gli snapshot che la spec chiede: `MESSAGES_SNAPSHOT` (la cronologia e i messaggi del run fino a lì, con la chiamata
  dello strumento) e `STATE_SNAPSHOT`;
- se un **sottoagente** è aperto (una conferma o una domanda che arriva da dentro un sottoagente), prima un `SUBAGENT_FINISHED`
  con `outcome: {type: "suspended", interruptIds: […]}`: `@ag-ui/client` rifiuta un `RUN_FINISHED` mentre un sottoagente è
  aperto. Alla ripresa il sottoagente è annunciato di nuovo (`SUBAGENT_STARTED`, stesso `subagentRunId`) subito dopo
  `RUN_STARTED`, e l'interruzione porta `subagentRunId`;
- poi `RUN_FINISHED` con una `Interrupt` per ogni richiesta aperta entro 150 ms dalla prima. Gli strumenti che modificano
  qualcosa girano uno alla volta (di norma una conferma per pausa, la successiva è una nuova pausa dopo la risposta), ma le
  domande di `ask_user` e i sottoagenti dello stesso turno girano insieme: più interruzioni nella stessa pausa sono possibili
  (due domande, due sottoagenti), e quelle che arrivano più tardi sono pause successive:

| Richiesta del motore | `reason` | Contenuto |
|---|---|---|
| conferma di uno strumento (`aiw.approval_request`) | `tool_call` (o `confirmation` se la chiamata non si riconosce) | `id` = id della conferma; `toolCallId` della chiamata; `message` = motivo; `responseSchema` `{approved: boolean}`; `metadata`: `ag-ui:tool-execution: "server"` (lo strumento lo esegue il server: il client non deve inventare un risultato) e `aiw: {tool, args, risk}` |
| domanda di `ask_user` (`aiw.question`) | `input_required` | `id` = id della domanda; `message` = titolo e testo; `responseSchema` della risposta (scelta: `selected` con gli id; conferma: `confirmed`; modulo: `values`); `expiresAt`; `metadata.aiw.question` = la domanda intera, per un client che sa farne un'interfaccia |

Il run **non si annulla** quando il client chiude la connessione dopo l'interruzione: è in pausa, e aspetta nel motore come ogni
conferma. Si risponde con:

```json
{"threadId": "…", "runId": "run-2", "messages": ["…come prima…"],
 "resume": [{"interruptId": "<id>", "status": "resolved", "payload": {"approved": true}}]}
```

Un `status: "cancelled"` vale come rifiuto (conferma) o come domanda declinata. Il `payload` di una domanda ha la forma di
`responseSchema` (la stessa del corpo di `POST /api/questions/{id}`: `{"selected": ["q4"]}`, `{"confirmed": true}`,
`{"values": {…}}`). La risposta riprende **lo stesso run**: `RUN_STARTED` con il `runId` nuovo e `parentRunId` = il run
interrotto, poi il `TOOL_CALL_RESULT` con l'esito vero (la chiamata non si ripete: la spec vuole solo il risultato), il resto del
run, e `RUN_FINISHED` (o una nuova pausa). Passa dalle stesse funzioni di `POST /api/approvals/{id}` e `/api/questions/{id}`:
audit, ambito e controllo del proprietario sono quelli di sempre.

Le regole della spec e come le tratta il server (un errore è un `RUN_STARTED` e un `RUN_ERROR` con `code`, non un errore HTTP;
la pausa resta aperta):

| Regola della spec | Se non vale |
|---|---|
| risposta a tutte le interruzioni aperte, nessuna parziale | `resume_incomplete` |
| `interruptId` che esiste tra quelle aperte, stesso thread | `interrupt_not_found` (anche dopo un riavvio: le pause sono in memoria) |
| `payload` della forma attesa, `status` `resolved` o `cancelled`, nessuna interruzione due volte | `resume_invalid`: prima si controllano **tutte** le risposte e solo dopo se ne applica una, quindi una risposta che non va non ne consuma nessuna e la pausa resta |
| l'interruzione è ancora aperta | `interrupt_closed` se è già stata chiusa altrove (nell'app, o la domanda è scaduta dopo 300 s): la risposta non si applica in silenzio, la pausa finisce e rimandare il messaggio riprende lo stream del run |
| altro input su un thread con interruzioni aperte | `interrupt_pending` |
| `resume` insieme a un messaggio utente nuovo | `resume_with_new_input` |
| stesso `resume` rimandato (anche da un client che ha perso la risposta, e anche dopo una seconda pausa: si tengono le ultime 8 già risposte) | **sicuro**: si rimanda il run da dov'era, senza applicare due volte la decisione. Se due risposte uguali arrivano insieme vince la prima registrata; con due flussi aperti sullo stesso run, quello che resta non è annullato dalla chiusura dell'altro |
| un altro `resume` per interruzioni già risposte | `resume_conflict` |

Il client vero fa già da sé una parte di questi controlli (`@ag-ui/client` 1.0.4 rifiuta di mandare un `resume` che non risponde
a tutte le interruzioni aperte o un input nuovo mentre ce ne sono): i casi che arrivano al server sono provati con richieste scritte
a mano. Le conferme non portano `expiresAt` (la scadenza, se c'è, è `approvals.timeout_s`: nessuna per impostazione predefinita);
le domande sì.

**La conferma di uno strumento di gestione vale solo se arriva dal proprietario**, anche da `resume`: gli strumenti `aiw_*` che
modificano qualcosa (e `aiw_secret_clear`) chiedono sempre conferma, in qualunque modalità dell'agente e anche dopo «Consenti per la
sessione» (che per loro non esiste: `remember` è ignorato). Un `resume` di una di queste richieste da un token `device` o dal
token `owner` presentato dalla LAN senza TLS riceve `403` («Questa richiesta può essere decisa solo dal proprietario…»), come
`POST /api/approvals/{id}`, e la pausa resta. Hanno accesso completo le richieste dal computer stesso senza token e quelle col
token `owner` su TLS. Le conferme degli strumenti ordinari (`write_file`, `shell`, …) si danno anche con un token `device`. Un run
avviato con un token `device` che chiama uno strumento di gestione che modifica qualcosa non apre nemmeno la conferma: lo
strumento è rifiutato subito («accesso completo…»), perché nessuno potrebbe darle esito.

**I segreti non passano mai da `resume`**: la richiesta di un segreto non diventa un'interruzione (il valore non deve stare in un
evento né nel corpo di un `RunAgentInput`, che il client tiene nei suoi messaggi). Con `"interrupts"` e senza `"secrets"`
`aiw_secret_request` risponde `unsupported`; con `"secrets"` dichiarato resta come descritto sotto (evento `aiw.secret_request` e
valore su `POST /api/secret-requests/{id}`). Limiti: le pause stanno in memoria (dopo un riavvio il run non c'è più: `resume`
dà `interrupt_not_found`). Una pausa senza risposta per 24 ore, o la più vecchia oltre le 256, viene tolta **e il suo run
annullato** (altrimenti terrebbe la sessione per sempre); prima di allora il run aspetta come ogni conferma, anche se nessuno
risponde, finché non lo decidi (nell'app o con `resume`) o non lo annulli. Anche `threadId` e `runId` hanno un massimo di 256
byte e lo `state` del client di 1 MiB (altrimenti `400`).

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
mostra `aiw.question` e risponde su `POST /api/questions/{id}`), `"secrets"` (lo stesso per `aiw.secret_request` e
`POST /api/secret-requests/{id}`) e `"interrupts"` (il client sa fare `resume`: conferme e domande diventano interruzioni
AG-UI, vedi sopra; senza `"questions"` anche le domande). Qualunque altra forma (assente, `null`, una stringa, nomi diversi)
vale come nessuna dichiarazione; i nomi sconosciuti sono ignorati.

| Dichiarazione | `ask_user` | `aiw_secret_request` |
|---|---|---|
| assente | risponde subito `unsupported`: l'agente fa la domanda **in testo** e l'utente risponde col messaggio dopo | risponde subito `unsupported`: l'agente spiega che il segreto va inserito dall'app su questo computer |
| `["questions"]` | il run aspetta la risposta | `unsupported` |
| `["questions", "secrets"]` | il run aspetta la risposta | il run aspetta il segreto **solo se il chiamante è il proprietario**; con un token `device` risponde `unsupported`, perché l'endpoint del segreto richiede accesso completo |
| `["interrupts"]` | il run si ferma con un'interruzione `input_required` e continua con `resume` | `unsupported` (un segreto non passa da `resume`) |

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
Un run già terminato non è toccato, e uno **in pausa su un'interruzione** neppure (la connessione si è chiusa apposta).

## Provarlo con un client vero

```sh
cargo build -p aiw-cli                    # il binario che i test avviano (target/debug/aiw, o AIW_BIN)
cd tests/agui-client && npm ci && npm test
```

La suite (`tests/agui-client/test/interop.test.mjs`, 16-40 secondi, nessun account né rete) avvia il vero `aiw serve` in una
cartella temporanea, lo punta a un provider OpenAI finto (`lib/fake-provider.mjs`) con le risposte scritte a mano e usa
`HttpAgent` di `@ag-ui/client`. Fanno fallire il test gli avvisi del client sui campi che toglie perché non li conosce e sulle patch dello
stato che non riesce ad applicare.
Scenari, per `@ag-ui/client` 1.0.4 e 1.0.2: testo con ragionamento, stato e contesto; strumento e piano; sottoagente; allegati
(documento e immagine); conferma come `CUSTOM` con risposta REST; conferma come interruzione con `resume`. Solo per la 1.0.4:
rifiuto e annullamento, regole del `resume` (richieste scritte a mano), catena di conferme, domanda come interruzione,
messaggio già eseguito rimandato, client senza cronologia, annullamento con `abortRun()` e pausa che non annulla, segreti fuori
da stream, messaggi salvati, richieste al modello e log, strumenti del client ignorati, riavvio. I test Rust restano:
`cargo test -p aiw-server agui`, `cargo test -p aiw-server tests_harness`, `cargo test -p aiw-engine agui_threads`.

Il **runtime di CopilotKit** (`tests/agui-copilotkit`, a parte perché il suo `npm ci` pesa circa 300 MB) si prova così:

```sh
cd tests/agui-copilotkit && SCARF_ANALYTICS=false npm ci && npm test
```

Registra gli agenti AIW come nella guida di CopilotKit (`new CopilotRuntime({ agents: { … new HttpAgent({ url }) } })`), li fa girare
dal suo gestore HTTP (`POST /agent/{agente}/run`, come il frontend) con la telemetria spenta e verifica due cose: un messaggio con
ragionamento arriva intero, e una conferma esce come interruzione, passa dal runtime e il `resume` arriva ad AIW (il file viene
scritto solo dopo). Non usa React né l'interfaccia di CopilotKit.

Il modello è finto e il server è il vero: dice che il protocollo regge un client vero, non che un modello locale vero usi bene
gli strumenti.

## Limiti (cosa non c'è)

- **Strumenti definiti dal client** (`tools`): ignorati. L'agente usa solo i propri strumenti: un frontend non può fornire un proprio
  `ask_user` né uno strumento che gira nel browser. La spec li prevede ([strumenti](https://docs.ag-ui.com/concepts/tools): il
  client manda `tools`, l'agente li chiama con `TOOL_CALL_*`, il client risponde con un messaggio `role: "tool"`); richiedono che
  il ciclo dell'agente si fermi su una chiamata senza risultato e riparta da un messaggio dello strumento, e non è stato fatto.
- **`STEP_STARTED` / `STEP_FINISHED`**: non si emettono. Il motore non ha passi nel flusso di un run (li hanno i workflow, che
  non passano da qui) e non se ne inventano.
- **Attività** (`ACTIVITY_*`), `RAW`, capacità dichiarate dall'agente (`getCapabilities`): non c'è niente di tutto questo.
- **Stato scritto dall'agente**: lo stato del client arriva al modello e torna indietro, ma l'agente non lo modifica; i
  `STATE_DELTA` sono del solo stato del motore (`aiw`).
- **Ramificazione e viaggio nel tempo** (`parentRunId` in ingresso): ignorati.
- **Segreti da un client remoto**: l'endpoint del valore richiede accesso completo. Un client con un token `device` non può fornire
  segreti (l'agente ne riceve `unsupported`), e lo stesso vale per il token `owner` dalla LAN senza TLS. Le conferme degli
  strumenti di gestione hanno la stessa regola: solo il proprietario. Un segreto non passa mai da `resume`.
- **Pause in memoria**: dopo un riavvio una pausa non c'è più (il thread, la cronologia e i messaggi già eseguiti sì).
- **Agenti**: niente team (`team:…`) né agenti interni. Un agente con lo strumento `handoff` può passare la sessione a un altro: la
  richiesta successiva a `/agui/{agente originale}` riceve `409` (la sessione è di un altro agente) e il run del passaggio non è
  seguito da questo stream (preesistente).
- **Non provato**: i componenti React di CopilotKit, OpenBot, OpenDots; un modello vero; Windows e telefono.

## Riferimenti alla specifica

Letti il 15 novembre 2026 (`@ag-ui/client` e `@ag-ui/core` 1.0.4, protocollo 1.0):

- eventi e `RunFinished.outcome`: <https://docs.ag-ui.com/concepts/events>;
- stato (`STATE_SNAPSHOT` sostituisce, `STATE_DELTA` RFC 6902): <https://docs.ag-ui.com/concepts/state>;
- interruzioni, `resume`, regole, motivi (`tool_call`, `input_required`, `confirmation`), `ag-ui:tool-execution`:
  <https://docs.ag-ui.com/concepts/interrupts>;
- messaggi e strumenti: <https://docs.ag-ui.com/concepts/messages>, <https://docs.ag-ui.com/concepts/tools>;
- tipi (`RunAgentInput`, `Context`, `Tool`): <https://docs.ag-ui.com/sdk/js/core/types>;
- CopilotKit registra un agente remoto con `HttpAgent` in `CopilotRuntime({ agents })`, e l'intestazione dell'agente vince su
  quella inoltrata: <https://docs.copilotkit.ai/guides/self-hosting>.
