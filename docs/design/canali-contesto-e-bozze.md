# Canali: contesto condiviso e bozze email (#72, #76)

Note di progetto del 15 novembre 2026. Codice: `crates/aiw-engine/src/channels/` (`context.rs`, `drafts.rs`, `draft_tool.rs`, `smtp.rs`, `mod.rs`),
rotte in `crates/aiw-server/src/lib.rs` e `scopes.rs`, interfaccia in `apps/ui/src/features/channels/`. Guida per chi usa l'app: `docs/channels.md`.
Quello che segue dice **che cosa è stato deciso e perché**; che cosa è stato provato e che cosa no sta nella descrizione della PR e in `docs/channels.md`.

## 1. Contesto condiviso tra canali (#72)

### La mappa

```
identità  =  (canale, mittente)            un account di una persona, come scritto nell'elenco «chi può scrivere» del canale
conversazione esterna = (canale, conv)     chat Telegram, canale[:thread] Slack, filo di posta  →  `ConvDoc`  →  una sessione
contesto  =  { nome, membri: [identità] }  scelto dal proprietario; ha UNA sessione sua  →  `ChannelContext` (documento `channel_context`)
```

Senza contesto vale quello che c'era già: `(canale, conv) → sessione`. Con un contesto, un messaggio **diretto** di un'identità membro va
nella sessione del contesto, qualunque sia il canale. Le sessioni per conversazione già esistenti non vengono toccate né unite: la sessione del contesto nasce vuota.

### Perché solo così

| Decisione | Motivo |
|---|---|
| Il collegamento lo fa il proprietario (`/api/contexts`, ambito `full`), mai l'app da sola | «Mai unioni automatiche di contesti privati». Un telefono accoppiato, un canale o un run non collegano. |
| Un solo account per canale, in un solo contesto, preso dall'elenco del canale | Un'identità è un account; l'elenco è la dichiarazione del proprietario su chi può parlare. Due persone non finiscono insieme per un errore di battitura. |
| Solo messaggi **diretti** (campo `direct` di `Inbound`, deciso dal connettore; il predefinito è «no») | In un gruppo o nella menzione in un canale la risposta la leggono tutti: con la cronologia privata sarebbe una fuga. Telegram: `chat.type == "private"`; Slack: `channel_type == "im"`; posta: sempre. |
| Telegram: l'identità è l'id della **persona**, mai un id di chat (un gruppo è negativo) | `senders` contiene anche l'id della chat, che è come funziona oggi un gruppo in elenco. |
| Posta: solo con `require_auth` acceso, controllato al collegamento **e a ogni messaggio** | Con il controllo spento un `From` falsificato leggerebbe la cronologia degli altri canali. Anche acceso prova il dominio e non la casella: limite dichiarato in `SECURITY.md`. |
| Modalità = la più restrittiva dei canali **di adesso**, ricalcolata a ogni turno (la sessione porta `channel:ro|ask:<assistente>`) | Una sessione condivisa non deve poter ereditare i poteri della sessione di un altro canale; e un canale reso «solo lettura» un minuto fa tiene subito indietro tutto il contesto. |
| Stesso assistente per tutti i canali; `PATCH` dell'assistente di un canale collegato ad altri è un 409 | Altrimenti il prompt di sistema cambierebbe da un turno all'altro. |
| Un account esce se lo togli dall'elenco o elimini il canale (`contexts_prune`) | La sessione resta finché resta un membro. |

### Concorrenza, duplicati, conferme

- **Coda per contesto.** Il lavoratore è per chiave: `ctx|<id>` per un contesto, `canale|conv` altrimenti. Ogni elemento in coda porta canale e trasporto, perché la
  coda di un contesto contiene messaggi di canali diversi. Due messaggi di due canali non partono in parallelo sulla stessa sessione (niente «run già in corso»).
- **Duplicati.** `Inbound.id` (Telegram `chat:message_id`, Slack `canale:ts`, posta `mittente|Message-ID`) con un elenco per canale nel motore (500 id, 6 ore). Slack ricostruiva già il suo
  elenco a ogni riconnessione: quello del motore regge. Gli offset di Telegram e gli UID della posta restano com'erano.
- **Conferme (`TurnOrigin`).** Quando parte un turno di un canale il motore registra *chi l'ha avviato*: `(canale, conversazione, account)`, per la sessione. Una richiesta di
  permesso aperta in quella sessione si chiede e si decide solo lì: il «sì»/«no» o il pulsante devono venire dallo stesso canale, conversazione e account. Un «sì» da un altro canale dello stesso
  contesto, o per un run avviato dall'app (nessuna origine registrata), non decide e riceve un rimando («rispondi dove è stata chiesta, oppure nell'app»). Prima la
  risposta di un canale valeva per qualunque richiesta della sessione, quindi anche per la conferma di un run avviato dal proprietario nell'app: ora no. Le modifiche di gestione restano
  rifiutate prima ancora di chiedere (nessuna origine in un run di canale).
- **Posta.** Un «sì» in risposta a una mail non veniva riconosciuto perché `text` comincia con la riga «Oggetto: …»: ora `Inbound.answer` porta il solo corpo.

### Che cosa non c'è

Nessun canale vocale esterno (la voce è dettatura e lettura nell'app); nessun «adotta una sessione esistente» (sarebbe un'unione, anche se esplicita); nessuna unione fra più persone;
nessuna prova con Telegram, Slack o caselle vere.

## 2. Bozze email (#76)

### Stati

```
            ┌─ modifica ─┐
            ▼            │
 [draft] ───────────────►│ (nuova revisione, nuova impronta)
   │  │ \
   │  │  └─ rifiuta / annulla ─► [rejected] / [cancelled]            nessun invio
   │  └──── approva e invia ───► [sending] ─┬─ 250 ──────────────────► [sent]
   │                                        ├─ rifiutata prima ──────► [failed]     (si può ripetere con la stessa impronta)
   │                                        └─ caduta dopo DATA ─────► [uncertain]  (solo «Invia comunque», con conferma)
   └─ riavvio durante l'invio (>90 s) ──────────────────────────────► [uncertain]
```

La transizione `draft|failed|uncertain → sending` è un compare-and-swap sotto blocco, con controllo della revisione: due richieste insieme non mandano due messaggi, e un cambio tra la revisione e
l'invio vince (409). La stessa richiesta su una bozza già `sent` restituisce la bozza com'è. `Message-ID` = `<draft-<id>@aiw.local>`.

### L'approvazione è del contenuto

`content_hash` = SHA-256 di `["aiw-email-draft-v1", canale, mittente, a, cc, ccn, oggetto, testo, in_reply_to, references, [nome, tipo, dimensione, sha256 degli allegati]]`, calcolato dal motore. L'interfaccia
rimanda l'impronta che ha mostrato (`approved_hash`); una diversa è un 409. Gli allegati si copiano al momento dell'inserimento, con le regole di `ToolContext::resolve` (workspace, cartella dei dati e credenziali nascoste, collegamenti simbolici risolti): sono il
percorso più facile per far uscire un segreto, quindi l'impronta li copre e non si rileggono dal disco.

### Perché lo strumento non invia

`email_draft` è un solo strumento, di rischio *scrittura*, che crea o corregge una bozza *dell'assistente*. Non esiste uno strumento che invii: l'invio è `POST /api/email-drafts/{id}/send` (ambito `full`).
Un'istruzione indotta da testo di terzi può dunque ottenere una bozza, non una email: la revisione del proprietario è l'unica barriera, e la UI lo dice (avviso sulle bozze dell'assistente, destinatari e copie nascoste in evidenza,
conferma con i destinatari). Lo strumento non è offerto ai lavori di sola lettura: l'assistente proattivo resta di sola lettura, e un suo suggerimento («rispondi a questa mail») diventa una bozza
solo se il proprietario approva la *proposta*; poi c'è la seconda approvazione, quella della bozza.

### SMTP

`smtp.rs` ora accetta più destinatari (tutti accettati *prima* di `DATA`: niente invii parziali), `multipart/mixed` con allegati, `Message-ID` dato, nessun `Auto-Submitted` per un messaggio approvato; `Bcc` solo nella busta. Un guasto
prima di `DATA` è «non partita»; dopo l'invio dei dati, o con una risposta che non arriva, è «incerto» (mai ritentato da solo).

## 3. Verifica

Rust: `tests_contexts.rs` (13), `tests_drafts.rs` (13), `aiw-server/src/tests_channels.rs` e la matrice degli ambiti in `tests_tokens.rs`. Le prove di mutazione fatte a mano (e annullate) sono nella descrizione della PR.
UI: test unitari del mock e dei modelli, E2E `e2e/flows/mail.spec.ts`, accessibilità `e2e/a11y/mail.spec.ts` (390 e 1440 px, chiaro e scuro).
