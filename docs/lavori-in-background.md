# Lavori in background: osservare, mettere in pausa, parlare e riprendere (issue #75)

Un **lavoro in background** è ciò che l'assistente fa mentre la persona è altrove. Lo crea l'approvazione di una proposta
(pagina «Proposte», o la chat in modalità semplice), oppure `POST /api/tasks`. Il riepilogo del mattino e le proposte
esistevano già (`docs/mobile/BACKGROUND.md`, `crates/aiw-engine/src/proactive.rs`): questo documento descrive quello che
c'è in più (`crates/aiw-engine/src/bgtasks.rs`) e che cosa è stato provato.

## Come è fatto

Un lavoro è un **record durevole sopra i run di una sola sessione**. Quello che l'agente ha fatto sta già nei messaggi della
sessione, quindi non c'è altro stato da salvare: *mettere in pausa* ferma l'agente tra due passi, *riprendere* avvia un nuovo
run nella stessa sessione. Un lavoro è una catena di **passi** (run): ne parte uno dopo l'altro finché l'agente ha finito,
una regola lo ferma o lo ferma la persona.

Il ciclo dell'agente (`aiw-agent`) ha un solo punto in più: all'inizio di ogni turno di un run di primo livello chiede
all'host (`RunHost::checkpoint`) se deve fermarsi e se la persona ha detto qualcosa nel frattempo. Per un run che non è un
passo di un lavoro la risposta è sempre «niente», quindi nulla cambia per le chat e i workflow.

| Stato | Significato |
| --- | --- |
| `queued` | creato, sta per partire |
| `running` | un passo è in corso (`stopping`: pausa chiesta, finisce il passo; `waiting_approval`: aspetta una conferma) |
| `paused` | fermo, con il motivo in `pause_reason` e la frase per la persona in `pause_text` |
| `succeeded` · `failed` · `cancelled` | finito (il risultato o l'errore sono nel record) |

Motivi di pausa: `user` (l'hai chiesta tu), `quiet_hours`, `battery`, `daily_budget` (regole del proattivo, riparte da solo),
`task_budget` (ha finito il suo budget), `kill_switch`, `restart` (interrotto dal riavvio), `error` (non è riuscito a partire).

## Osservazione

`GET /api/tasks` e `GET /api/tasks/{id}` danno: stato, motivo, avanzamento (passi del modello, token, secondi, ultimo
strumento, ultima frase dell'agente), la **conferma che sta aspettando** (`waiting_approval`, la stessa di `GET /api/approvals`),
le istruzioni con il loro stato, la cronologia (`notes`, sopravvive al riavvio) e i run (`run_ids`, `current_run_id`: gli
eventi di ogni run restano su `GET /api/runs/{id}/events`). La sessione del lavoro è una chat normale: si può aprire.

## Pausa, istruzioni, ripresa

* **Pausa** (`POST /api/tasks/{id}/pause`): il passo in corso finisce, poi il lavoro si ferma e dice di averlo fatto. Se sta
  aspettando una conferma si ferma subito (il run viene fermato, la richiesta ritirata, il passo tagliato resta nella sessione
  come «annullato» così che il passo successivo sa che non è successo).
* **Istruzioni** (`POST /api/tasks/{id}/instructions`, `source` `user` o `voice`): finiscono in una coda durevole. Un lavoro in
  corso le riceve **al turno successivo** (messaggio utente `aiw.instruction`, passa dagli hook `UserPromptSubmit` come ogni
  prompt: se un hook la blocca resta `blocked`). Un lavoro in pausa le riceve nel primo messaggio del passo seguente. Se
  arriva mentre l'agente scrive la risposta finale, il lavoro non finisce: parte un passo in più per leggerla.
* **Ripresa** (`POST /api/tasks/{id}/resume`): il lavoro viene *reclamato* sotto lock (due richieste insieme: una vince, l'altra
  riceve 409) e parte un nuovo run con «Riprendi il lavoro da dove eri arrivato» più le istruzioni in attesa. L'obiettivo non
  si ripete: sta già nella sessione.
* **Annulla** (`POST /api/tasks/{id}/cancel`): per sempre; un lavoro finito non riceve più comandi (409).

## Che cosa lo tiene dentro le regole

* **Mai più libero di `ask`**: i passi di un lavoro girano con la politica dell'agente ma con la modalità limitata a `ask`
  (un agente `auto-edit` o `yolo` non scrive né esegue nulla senza conferma). Le conferme aspettano come tutte le altre
  (`approvals.timeout_s` le fa scadere come rifiuto). **«Consenti per la sessione» non vale per un lavoro**: i suoi passi
  partono con un insieme vuoto di strumenti consentiti e quello che si consente nella chat della stessa sessione non si porta
  dietro (e consentire per la sessione una conferma di un lavoro non ricorda nulla). Le regole `allow` che la persona ha scritto
  in Sicurezza valgono come per ogni run. Il run non ha un'origine, quindi gli strumenti `aiw_*` rifiutano. Un agente che chiede
  un `handoff` fa **fallire** il lavoro con un messaggio chiaro (il passaggio non è seguito: il nuovo agente lavorerebbe fuori
  dalle regole e dai limiti del lavoro); prima, approvare una proposta seguiva il passaggio.
* **Regole del proattivo**, valutate a ogni turno e prima di ogni ripresa automatica: orario di silenzio
  (`proactive.quiet_hours`), batteria (`proactive.only_on_power`, dove il sistema lo dice), tempo del giorno
  (`proactive.daily_budget_minutes`, **condiviso con il battito**: il tempo dei lavori conta e `GET /api/proactive/status`
  lo somma; si somma a ogni turno e **non conta l'attesa di una conferma**, che non è lavoro; con il tempo predefinito di 15
  minuti un lavoro lungo si ferma e riprende il giorno dopo; con 0 minuti un lavoro resta in pausa e dice di aumentare il
  tempo, non aspetta un «domani» che non arriva). Il lavoro si ferma al turno successivo, dice perché e **riparte da solo** quando la regola finisce (il
  controllo è nel giro di 20 secondi del motore). «Riprendi comunque» (`force`) supera silenzio, batteria e tempo del giorno
  per quel lavoro fino alla prossima pausa. Non supera il pulsante di emergenza né i limiti del lavoro.
* **Pulsante di emergenza**: ferma subito ogni lavoro (in pausa `kill_switch`, niente riparte da solo), anche con «comunque».
  Ripartono solo dopo che la persona sceglie di nuovo il livello del proattivo.
* **Budget del lavoro** (`limits`: secondi, passi del modello, token, sommati su **tutti** i passi; di default 30 minuti e 150
  passi se né il lavoro né l'agente né `agents.budget.*` ne dicono altri): ogni passo parte con quello che resta, quindi
  pausa e ripresa non lo aggirano. I secondi sono controllati a ogni turno **senza l'attesa delle conferme** (l'orologio del
  run è spento per i passi di un lavoro). Esaurito, il lavoro resta in pausa `task_budget` e `resume` risponde 409 finché la
  persona non aggiunge spazio (`extend`, che alza solo i limiti che il lavoro ha: non ne crea di nuovi).
* Un lavoro approvato in orario di silenzio viene preso in carico ma aspetta (la proposta resta «approvata»).

## Riavvio e durata

Sopravvivono: il record (stato, istruzioni, cronologia, tempo e passi usati) e i messaggi della sessione **fino all'ultimo
passo finito** (il run li scrive a ogni turno, non solo alla fine). Non sopravvive il passo che stava girando.

All'avvio un lavoro che risultava `running` torna `paused` con motivo `restart` e **non parte nulla da solo**: il passo
tagliato può aver lasciato un effetto (un file scritto, un comando lanciato) e decide la persona. La ripresa avvia un passo
nuovo, quindi quel passo può essere rifatto (almeno una volta, come i workflow). Le istruzioni consegnate a un passo che il
riavvio ha tagliato, e che la sessione non ha, tornano in attesa; quelle già nella sessione non si consegnano due volte (si
cerca solo nei messaggi dell'utente: un'istruzione corta come «ok» non risulta consegnata perché uno strumento ha detto «ok»).

Il record di un lavoro (istruzione intera, note) vive nel database: **si cancella con la sua sessione** (il lavoro in corso
viene prima fermato); non esiste una rotta per cancellarlo da solo, e i lavori finiti oltre i 100 più recenti vengono
dimenticati. Mentre un passo gira il battito aspetta (come quando usi la chat).

Su telefono il sistema sospende o chiude l'app: un lavoro non gira mentre l'app è sospesa e, se il processo viene chiuso, al
rientro è in pausa `restart`. Il controllo che il sistema fa partire in background (`docs/mobile/BACKGROUND.md`) **non** avvia né
riprende lavori: lavora solo il proattivo, con il suo tetto di 25 secondi.

## Dalla chat (modalità semplice)

In semplice non c'è la pagina Proposte: tutto passa dalla chat. `aiw_inspect` ha il topic `tasks` (lavori, proposte aperte con
l'istruzione, riepilogo del mattino, stato del proattivo). `aiw_task` (un solo strumento, `action`: `start`,
`approve_proposal`, `dismiss_proposal`, `pause`, `resume`, `instruct`, `cancel`) è uno strumento di gestione come gli altri:
conferma **sempre** chiesta, vale solo dal proprietario, rifiutato fuori da una chat. La scheda di conferma mostra l'istruzione
come argomento (il testo che la persona ha scritto non entra nella riga che finisce nell'audit: lì c'è solo la lunghezza);
per approvare una proposta la riga contiene titolo e istruzione della proposta, perché è quella che la persona decide.

**Il confine dell'Assistente resta com'è**: l'Assistente non ha strumenti che leggono posta, file o web, e `aiw_inspect tasks`
non gliene dà uno. Il testo del riepilogo del mattino (mittenti e oggetti della posta, agenda) **non** è nella risposta (solo
se ce n'è uno non visto); ci sono titolo e istruzione delle proposte e quello che un lavoro ha detto o prodotto (lo stesso
tipo di testo che un sottoagente restituisce), ripulito, accorciato e marcato come materiale da leggere. Per sapere cosa c'è
in agenda o in posta l'Assistente delega all'operatore, come prima. **Costo**: circa 1 000 caratteri in più nel contesto fisso
dell'Assistente (da circa 7 300 a circa 7 600 token stimati su una finestra da 8 192): su modelli con finestra piccola il margine
si riduce.

## Voce (#22)

Il collegamento è: l'app registra → `POST /api/voice/transcribe` → la persona legge il testo → lo invia come istruzione con
`source: "voice"`. La rotta delle istruzioni è **solo testo** (una trascrizione sbagliata non deve diventare un ordine
senza che qualcuno la veda). Provato con un riconoscitore finto (`FakeStt`): trascrizione, errore su audio vuoto, consegna al
lavoro. **Non provato**: voce vera, microfono, qualità del riconoscimento (whisper.cpp o sistema), lingue, rumore, telefono.

## Riepilogo del mattino: che cosa è stato validato

Con fonti che somigliano a quelle vere ma sono **doppioni** (`crates/aiw-engine/src/tests_brief.rs`): un calendario con evento
già finito, in UTC con riga ripiegata e virgola, di tutto il giorno, ricorrente con eccezione, annullato e di domani; posta in
Maildir (intestazioni codificate, quoted-printable, solo HTML), in mbox e su un server IMAP finto. Verificato: gli eventi
giusti nell'ordine giusto, la posta di tre fonti insieme dalla più recente, nessun file modificato né spostato, il server IMAP
vede solo `EXAMINE` e `BODY.PEEK` (mai `STORE`, `COPY`, `EXPUNGE`…), nessun messaggio segnato come letto, un solo
riepilogo al giorno anche dopo un riavvio. Trovato e corretto: una fonte illeggibile non lasciava traccia e un calendario vuoto
sembrava una giornata libera; ora il riepilogo dice «Non ho potuto leggere N fonti del calendario/della posta» (senza percorsi:
può andare a un canale).

**Non verificabile qui, da provare con fonti reali**: un calendario in abbonamento (https), le particolarità di un'app di
calendario vera (fusi orari con nome non nella tabella interna, regole di ricorrenza complesse: mostra solo la prima
occorrenza), un provider di posta vero (Gmail e Microsoft richiedono password per app, OAuth non c'è), giornate con il cambio
dell'ora legale, il riepilogo letto da una persona vera su desktop e telefono, le notifiche del sistema.

## Cosa non è stato provato

Desktop e telefono veri (app compilata, notifiche, sospensione del sistema, WorkManager e BGTaskScheduler), un modello vero (tutti i
test usano un modello a copione), voce vera, fonti vere del riepilogo, Windows. I tempi (silenzio, giorno) si provano con
fasce costruite attorno all'ora corrente e con l'orologio spostato nelle funzioni che lo accettano.
