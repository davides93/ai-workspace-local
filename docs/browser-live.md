# Browser: vista live e presa di controllo umana

Issue #73 (live view, login e 2FA dell'utente) e residui di #18 (collegamento a un Chrome già aperto, dialoghi interattivi, scritture atomiche). Stato: scritto e provato con test automatici, con un CDP finto e con Chromium headless su pagine locali. **Mai provato su siti reali, con un lettore di schermo vero, con WebAuthn/passkey, su Wayland, su Windows o con l'app compilata.**

## Cosa fa

- L'utente **vede** il browser controllato dall'agente durante il task (un fotogramma JPEG circa due volte al secondo) dalla chat: pulsante «Browser» nell'intestazione, anche in modalità semplice.
- L'utente può **mettere in pausa** l'agente, **prendere il controllo** (clic, scorrimento, tasti, testo incollato nella pagina) e **restituirlo**. Login, 2FA e CAPTCHA li fa lui: le credenziali vanno solo alla pagina.
- L'agente può **chiedere l'intervento** con lo strumento `browser_handover` (anche quando il lavoro sul browser lo fa il sotto-agente `browser` a cui l'Assistente delega: la domanda, l'origine del run e gli eventi sono quelli del run della persona, e un run da telefono è rifiutato lì come altrove) («Accedi a example.com e completa la verifica in due passaggi»): in chat compare una scheda con la vista live e «Ho finito: restituisci il controllo» / «Non posso ora», e il run aspetta.
- Dopo la restituzione l'agente riprende **lo stesso task**: la chiamata che aspettava prosegue.
- I `confirm` e i `prompt` del sito non vengono più annullati in silenzio: restano in attesa di una risposta dell'agente (`browser_dialog`) o dell'utente (vista live).
- Con `browser.attach_url` l'agente usa un **Chrome già aperto dall'utente** invece del suo browser separato.

## Esclusione reciproca (`aiw-tools/src/browser/control.rs`)

Tre modi: `agent`, `paused`, `human`. Ogni azione dell'agente passa da `Control::enter` **prima** di toccare il browser e il suo lock: passa solo in modo `agent` e tiene una guardia finché finisce. Se l'utente ha messo in pausa o guida, la chiamata attende (massimo 300 s, poi risponde che l'utente ha ancora il browser e di non insistere; si ferma subito con lo stop del run; il kill switch riporta tutto a `agent` e sveglia chi aspetta). L'input dell'utente è accettato solo in modo `human` **e** senza azioni dell'agente in corso: `take` cambia subito modo (nessuna nuova azione parte) e l'azione già in corso finisce da sé.

Fotogramma, stato e controlli usano la connessione `live` del manager, mai il lock che l'azione dell'agente tiene per tutta la sua durata (un `browser_wait` può durare 30 s): non si bloccano a vicenda e non avviano mai un browser.

**Dopo una restituzione** l'agente:
- non può agire su un elemento (`ref`) finché non rifà uno snapshot: la pagina può essere cambiata sotto i numeri (errore chiaro, «Rifai browser_snapshot»);
- vede gli indirizzi **senza query né frammento** (un login può atterrare su un URL con `code=` o `access_token=`) finché non naviga da sé; vale per `URL:` degli snapshot, elenco schede e avvisi.

## Credenziali: non arrivano mai al modello

- Il valore dei campi `type=password`, `autocomplete` `one-time-code`, `current-password`, `new-password`, `cc-*` **non compare mai** negli snapshot: «(nascosto)» sia nel formato a elenco (che prima usava anche il valore come etichetta di un campo senza nome) sia in quello ad albero (confronto con i valori letti dalla pagina). Provato con Chromium vero. `browser_eval` resta l'eccezione, a concessione esplicita (è codice eseguito nella pagina con i suoi cookie).
- I tasti e il testo dell'utente vanno alla pagina e basta: nessun log, evento, messaggio salvato né riga di audit. L'audit contiene solo le **transizioni** (`browser.control`, `browser.handover`, `browser.handover_end`, `browser.attach`) con al più il dominio, mai titoli, indirizzi, motivi, tasti o fotogrammi. I corpi delle rotte sono letti a mano con frasi fisse: un errore non cita mai ciò che era stato scritto.
- I log: la libreria WebSocket (`tungstenite`) scrive i fotogrammi che invia a livello `trace`, e quelli verso il browser portano ciò che l'utente digita. `QUIET_LOG_TARGETS` (aiw-core) li porta a `warn` in ogni caso, anche con `RUST_LOG=trace` (`aiw serve` e app desktop). Le richieste bloccate dalla policy si registrano senza query.
- La **rete** non si allenta mentre guida l'utente: ogni richiesta della pagina passa dalla stessa policy dell'ultima azione dell'agente (domini consentiti, reti private, DNS). Un login che rimanda a un dominio non consentito (un provider di identità) viene bloccato come per l'agente; si aggiunge il dominio a `network.allow_domains`.
- Un download avviato **mentre guida l'utente** non viene tenuto (finirebbe nella cartella del run dell'agente e conterebbe nel suo limite).

## Protezione dello stream: ambiti dei token

Le rotte `/api/browser`, `/api/browser/frame`, `/api/browser/control`, `/api/browser/input`, `/api/browser/dialog` sono dell'ambito **`full`** (il computer stesso senza token o il token `owner` su TLS). **Decisione: un token `device` non vede né comanda il browser**: la pagina può mostrare una sessione autenticata (posta, banca) e l'input remoto è lo stesso potere dell'utente; un telefono accoppiato riceve 403 (la UI nasconde in silenzio il pulsante). Un token `gateway`, A2A o MCP idem. Un ambito dedicato per dare la vista a un telefono, con la sua conferma, non c'è: sarebbe una decisione nuova.

Cosa può comunque fare un dispositivo accoppiato: vedere e chiudere (`POST /api/questions/{id}`) la **domanda** `handover` di un run avviato dal computer, che dice solo cosa è richiesto, senza pagina né frame; «Ho finito» da lì restituisce il browser senza che nulla sia stato fatto, e l'agente se ne accorge al primo passo. Un run avviato da un dispositivo, un canale o dalla riga di comando non può chiedere un handover: `browser_handover` risponde subito `unsupported` («l'accesso va fatto dall'app su questo computer») invece di restare in attesa di qualcuno che non c'è.

## API (tutte `full`)

| Rotta | Corpo | Risposta |
|---|---|---|
| `GET /api/browser` | | `{running, headless, mode, agent_busy, stale, handover, page:{title,host,secure}, dialog, focus}`; `Cache-Control: no-store` |
| `GET /api/browser/frame` | | `{running:false}` o `{running:true, image:"data:image/jpeg;base64,…", width, height}` (JSON perché l'IPC di Tauri porta solo JSON); non avvia il browser; 409 con una finestra del sito aperta |
| `POST /api/browser/control` | `{action: pause\|resume\|take\|give_back}` | stato; ogni cambio di chi guida è una riga di audit |
| `POST /api/browser/input` | `click` (x, y in frazioni del fotogramma), `scroll`, `key` (tasti nominati o un carattere, solo `shift`), `text` (max 512) | `{}`; 409 se non è il turno dell'utente |
| `POST /api/browser/dialog` | `{action: accept\|dismiss, text?}` | `{}` |

Il fotogramma è la viewport in pixel CSS, scalata a 1000 px al massimo; le posizioni sono frazioni, quindi la **scala dello schermo (HiDPI) non entra**: provato con Chromium vero e `deviceScaleFactor: 2`. Nessuna scorciatoia del browser (ctrl, alt, cmd) può essere inviata. Un browser **con finestra** (non headless) viene portato in primo piano alla presa di controllo: la finestra vera resta un'opzione (gestore di password, passkey, chiavi di sicurezza: **con input sintetico via CDP WebAuthn non funziona**). Senza schermo (Linux senza `DISPLAY`) il browser è headless e la vista live è l'unico modo.

`focus` (solo mentre guida l'utente) dice a parole quale controllo ha il focus nella pagina («campo password «Chiave»»), mai il valore: serve a chi non vede il fotogramma.

## Dialoghi (`alert`, `confirm`, `prompt`, `beforeunload`)

`alert` e `beforeunload` hanno una sola uscita e si chiudono subito (con nota all'agente). `confirm` e `prompt` aspettano: l'agente risponde con `browser_dialog` (`accept`/`dismiss`, `text` per il prompt), l'utente dalla vista live. Una chiamata bloccata dalla finestra (un clic che apre un `confirm` non torna finché è risolto) **rientra subito** con la domanda e il modo di rispondere, invece di aspettare i 30 s del browser. Ogni scheda ha la sua finestra (due schede non si sostituiscono a vicenda; si mostra e si risolve prima quella della scheda in primo piano). Se nessuno risponde entro 120 s la finestra viene annullata (la risposta sicura) e l'agente ne riceve la nota. **Cambio di comportamento voluto**: prima ogni «sei sicuro?» riceveva «no» in silenzio.

## Collegamento a un Chrome già aperto (`browser.attach_url`)

Impostazione (livello globale o dispositivo; **vietata al workspace e agli strumenti di gestione dell'assistente**: l'agente non allarga da solo il proprio raggio d'azione). Vuota di serie: l'agente usa il suo browser con profilo separato.

- Va indicato l'indirizzo di debug di un Chrome **avviato dall'utente** con `--remote-debugging-port=9222 --user-data-dir=<cartella>` (per esempio `http://127.0.0.1:9222`). **Da Chrome 136 la porta di debug viene ignorata con il profilo predefinito**: serve una cartella di profilo dedicata (fonte: <https://developer.chrome.com/blog/remote-debugging-port>). L'interruttore `chrome://inspect/#remote-debugging` di Chrome 144 e successivi (chiede il permesso a ogni connessione) **non è stato provato qui e non è supportato**: un indirizzo `ws://127.0.0.1:<porta>/devtools/browser/<id>` si può incollare, ma non c'è la ricerca automatica.
- Solo su questo computer: l'indirizzo dato **e** quello che Chrome risponde devono essere loopback (`127.x.y.z`, `[::1]`, `localhost`, con la porta, senza credenziali): controllo al salvataggio e a ogni connessione.
- L'agente apre **schede sue** dentro quel Chrome e **non vede né intercetta** le schede dell'utente: quelle che si agganciano da sole vengono lasciate subito, senza un comando nostro. Agisce però con i **cookie del profilo**: ciò che è già connesso lì lo è anche per l'agente. Vale la regola dei ref e della policy di rete come nel browser separato (`Fetch` solo sulle sue schede).
- Il comportamento dei **download di Chrome non viene cambiato**: restano a Chrome (cartella Download dell'utente) e non passano dal workspace; `browser_downloads` è vuoto. Limite dichiarato: un link che scarica un file lo scarica Chrome.
- Alla chiusura (kill switch compreso) l'agente chiude le **proprie** schede e si scollega; non invia mai `Browser.close` e non uccide Chrome.
- Vale dal prossimo avvio del browser: uno già in esecuzione mantiene il suo fino alla chiusura.

## Scritture di file atomiche

`write_file` (anche in append), `edit_file` e `copy_file` scrivono in un file temporaneo nascosto nella stessa cartella (`.<nome>.aiw-tmp-<casuale>`), `fsync`, poi `rename` sulla destinazione: un'interruzione a metà lascia il file com'era, mai troncato. Conservati i permessi; i collegamenti simbolici interni al workspace restano collegamenti (si sostituisce il file puntato; `ctx.resolve` blocca quelli che escono); un file di sola lettura o una cartella al posto del file continuano a dare errore. Il ripiego di copia dei download è atomico. Differenze note: gli hard link aggiuntivi restano al contenuto vecchio, proprietario/ACL/xattr non si conservano, nessun `fsync` della cartella, in una cartella di sola lettura la scrittura ora fallisce anche se il file è scrivibile. Su Windows un `rename` rifiutato perché un altro programma tiene aperta la destinazione (editor, antivirus) è ritentato fino a 5 volte in circa mezzo secondo prima di dare errore. Provato su macOS; **Windows non verificato** (i test sono scritti per girarci; il ripetitore non è stato mai eseguito lì).

## Cosa NON è provato

- Siti reali (login veri, 2FA, CAPTCHA, WebAuthn): solo pagine locali con Chromium headless e un CDP finto.
- Lettori di schermo veri (VoiceOver, NVDA): la UI ha etichette, regioni `aria-live`, ordine del focus e test automatici con axe; il contenuto della pagina remota resta un'immagine, **non c'è un elenco testuale degli elementi** (c'è solo `focus` e i pulsanti Tab/Invio/Cancella per muoversi).
- Windows, Linux con Wayland, mobile, app compilata; più client contemporanei sulla stessa vista.
- Il collegamento a un Chrome con profilo normale su Chrome 144+ (interruttore con permesso per connessione).
- I test `#[ignore]` con Chromium reale lanciati **in parallelo** falliscono a caso (due test per volta, diversi a ogni lancio): preesistente, verificato su `origin/main` senza queste modifiche; passano tutti con `--test-threads=1` (30 s). Vanno lanciati così: `cargo test -p aiw-tools --lib browser:: -- --ignored --test-threads=1`.

## Residui di #18 non fatti (e perché)

- **Computer use Linux/Wayland e Android**: servono un compositor Wayland e un dispositivo/emulatore Android che qui non ci sono; per Android resta ferma la decisione sulla policy di Google Play (`docs/design/mobile-browser-computer-use.md`).
- **HiDPI del computer use**: `computer.rs` converte le coordinate dello screenshot in pixel dello schermo con il rapporto fra la cattura (pixel fisici) e l'immagine ridotta, ma su macOS Retina `enigo` lavora in punti: sospetto un clic sbagliato di un fattore pari alla scala dello schermo. Non cambiato perché non verificabile senza muovere il mouse reale dell'utente con una cattura vera; da provare su un Mac Retina e su Windows con scala al 150 %.
- **Flusso dei permessi macOS** (Registrazione schermo, Accessibilità): richiede un'app firmata e il pannello di sistema.
- **Verifica del browser su siti reali**: non affidabile qui (rete, anti-bot); vedi sopra.
