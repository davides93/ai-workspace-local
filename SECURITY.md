# Sicurezza

**English summary.** AI Workspace Local (AIW) is a local-first app: it listens on `127.0.0.1` unless you enable LAN access. Report vulnerabilities **privately** through GitHub: *Security* tab → *Report a vulnerability* (Security Advisories) of this repository; please do not open a public issue. Only the latest release receives fixes. The shell sandbox closes the network and hides the app data folder and credential folders on Linux (verified with real bubblewrap) and, with the same rules, on macOS (profile text tested only, never run on a Mac); **on Windows the network cannot be closed and folders cannot be hidden from reads**. MCP stdio servers can be isolated (not on Windows) and need owner approval on first run. **Secrets (provider tokens, MCP headers and variables, IMAP passwords, the secret settings) are stored unencrypted** in the app's SQLite database and configuration files; there is no OS keychain or encryption at rest. The Assistant never sees a secret value (it is typed into a protected field and sent only to a dedicated endpoint that needs full access), and every management action it takes is confirmed by the owner. Linked conversations across channels are created only by the owner, apply to direct conversations only, and never give a channel more freedom than the most restricted one; email drafts have no send tool and go out only when the owner approves the exact content (hash) they saw. Known limits are listed below.

## Versioni supportate

AIW è in beta. Le correzioni di sicurezza escono solo per l'**ultima versione rilasciata** (oggi la serie 0.5); le versioni precedenti non ricevono patch.

## Come segnalare una vulnerabilità

Usa la segnalazione privata di GitHub: nel repository apri **Security → Report a vulnerability** (Security Advisories). Così la segnalazione resta riservata finché non c'è una correzione. Non aprire una issue pubblica e non scrivere i dettagli in una discussione o in una pull request.

Indica, se puoi: versione, sistema operativo, passi per riprodurre, cosa ottiene chi attacca. Non esiste un indirizzo email per le segnalazioni: GitHub è l'unico canale.

## Cosa è nell'ambito

- Aggiramento della sandbox della shell, dei limiti dei tool di file o del confine del workspace.
- Aggiramento delle regole di policy, della conferma delle azioni o dell'approvazione dei comandi MCP.
- Lettura o scrittura non autorizzata dell'API locale: token, ambiti (`owner`, `device`, `gateway`, …), pairing, TLS.
- Fuga di segreti (token, chiavi, header) in log, registro delle attività, risposte dell'API o variabili d'ambiente dei processi figli.
- SSRF dai tool `web_fetch` e browser, esecuzione di codice da file, allegati o messaggi dei canali.
- Vulnerabilità nelle dipendenze che AIW rende raggiungibili.

Fuori ambito: attacchi che richiedono già di poter eseguire codice con i tuoi permessi su questo computer; la modalità «Senza freni» con «Nessun contenimento» (fa ciò che puoi fare tu, per scelta); server MCP e provider che hai aggiunto tu e che sono malevoli (si limita il danno con l'isolamento, non lo si elimina); problemi dei runtime di terze parti (llama.cpp, Ollama, …) da segnalare a loro.

## Modello di minaccia (una pagina)

**Local-first.** AIW gira sul tuo computer, con i tuoi dati e i tuoi modelli. Il servizio ascolta su `127.0.0.1`; l'accesso dalla LAN è spento di serie. Non c'è un servizio nostro in mezzo: gli unici invii in uscita sono quelli che configuri (provider di modelli, ricerca web, canali, server MCP).

**Cosa si considera non attendibile.** Il testo prodotto da un modello; il contenuto di pagine web, file, allegati e messaggi dei canali (possono contenere istruzioni ostili, la «prompt injection»); i repository aperti come workspace (la loro configurazione può solo **restringere** le impostazioni di sicurezza, mai allentarle, e non può definire server MCP); gli output dei server MCP e degli agenti remoti; i certificati e i dispositivi mai accoppiati.

**Cosa protegge la sandbox, per sistema.**

| | Linux | macOS | Windows |
|---|---|---|---|
| Scritture fuori dal workspace | negate (bubblewrap) | negate (Seatbelt) | negate (integrità bassa + job object) |
| Rete della shell | **chiusa** di default (`--unshare-net`) | chiusa di default (`(deny network*)`) | **non si può chiudere** |
| Cartella dei dati di AIW e credenziali (`~/.ssh`, `~/.aws`, …) | nascoste | nascoste | **non nascondibili ai comandi** (solo ai tool di file) |
| Variabili d'ambiente segrete | tolte | tolte | tolte |
| Isolamento dei server MCP stdio | sì | sì | **no** |
| Verificato qui | con bubblewrap vero | **solo il testo del profilo** | solo compilazione per `x86_64-pc-windows-gnu` |

Senza sandbox (modalità «Nessun contenimento», Linux senza bubblewrap, Android) i comandi hanno i tuoi privilegi: la barriera è la conferma. Le regole di policy leggono ogni comando di una riga composta, ma non sostituiscono la sandbox. Compromesso noto: con le credenziali nascoste, `git` via SSH non trova la chiave; per consentirlo togli `~/.ssh` da Sicurezza → Sandbox → «Cartelle nascoste» (o usa HTTPS con un token).

**Token e TLS.** L'API usa token con ambiti: `owner` (questo computer), `device` (dispositivi accoppiati), `gateway`, `a2a`, `mcp`. Nel database restano solo gli hash con sale; il token `owner` sta nel file `token` (permessi 0600). Con la LAN accesa conviene `server.tls`: il pairing porta l'impronta SHA-256 del certificato e i client Rust verso un dispositivo accoppiato verificano solo quella. Senza TLS in LAN, codice di pairing e token viaggiano in chiaro.

**Backup e diagnostica.** Un backup di default non contiene chiavi e certificati TLS, token, segreti e credenziali né modelli; con `--include-secrets` l'archivio è marcato `sensitive` ed è sensibile quanto la cartella dei dati (non è cifrato). Creare un backup, ispezionarlo e scaricare la diagnostica richiede il token `owner` (un token `device` riceve 403); il ripristino è solo da CLI con il demone fermo e rifiuta percorsi assoluti, `..`, collegamenti e archivi con schema più nuovo. La diagnostica maschera tutti i segreti e non contiene chat, documenti o posta.

## Segreti, strumenti di gestione dell'Assistente e richieste di segreto

**Dove stanno i segreti oggi: in chiaro.** Non esiste un portachiavi del sistema (Keychain, DPAPI, Secret Service) né una cifratura a riposo: nel codice non c'è niente del genere. La cifratura a busta con la chiave nel portachiavi è pianificata nell'issue #121 (priorità P2, da chiudere prima della prima versione non beta con la chat unica); non blocca il merge della chat unica e da sola non protegge da un processo dello stesso utente. I segreti sono salvati così come sono:

- nel database `aiw.db` (SQLite, nella cartella dei dati): i token dei provider di modelli (Hugging Face, ModelScope, …), le variabili d'ambiente e gli header dei server MCP, i token OAuth dei server MCP, le password IMAP, i token degli agenti remoti, dei canali e le impostazioni dei motori;
- nei file di configurazione a strati: le impostazioni segrete come `search.brave_api_key`;
- nel file `token` (permessi 0600): il token `owner`.

Le protezioni che esistono sono queste: l'API non restituisce mai i valori (mostra `••••••`), la cartella dei dati è nascosta ai tool di file e, dove la sandbox lo permette, ai comandi della shell (**su Windows i comandi la possono leggere**), il backup di default non contiene i segreti e la diagnostica li maschera. Chi legge il disco, un backup con `--include-secrets` o un comando non confinato legge i segreti. Una cifratura a riposo o un portachiavi è un lavoro separato, non fatto: il flusso nuovo non lo cambia e, rendendo facile inserire i segreti, ne aumenta l'uso.

**Richiesta di un segreto (`aiw_secret_request`).** Il modello non vede mai un valore: indica solo la destinazione (`target`, per esempio `provider_token:hf`), un'etichetta e il motivo. Il motore controlla che la destinazione esista, scrive da sé in parole dove andrà il valore (`destination`, che il modello non può scrivere) ed emette l'evento `aiw.secret_request`, che non ha un campo per il valore. L'utente lo scrive in un campo protetto e il valore viaggia **solo nel corpo di `POST /api/secret-requests/{id}`** (in loopback, o su TLS), poi nella memoria del motore e nella funzione che già scriveva quella destinazione. Non passa da argomenti o risultato dello strumento, eventi, messaggi salvati, riga di audit (`secret.set`, con il solo `target`), richieste di conferma né prompt: un test (`crates/aiw-server/src/tests_harness.rs`) lo cerca nello stream, nel log del run, nei messaggi, nell'audit, nei prompt inviati al modello e nell'elenco dei provider e non lo trova. Prima di scrivere il motore risolve di nuovo la destinazione: se non è più quella mostrata (per esempio il connettore è cambiato) non scrive nulla, chiude la richiesta (409) e il modello non riceve il valore. Un errore della destinazione (un token che il provider rifiuta) è ripulito del valore, la richiesta resta aperta e si può riprovare; un corpo non valido riceve una frase fissa, mai l'eco di ciò che è stato scritto. Un segreto non si chiede con `ask_user` (i campi che lo chiedono sono rifiutati) e `aiw_setting_set` rifiuta le chiavi segrete.

**Ambiti dei token per queste funzioni.** Il valore di un segreto richiede accesso completo (`full`): il computer stesso senza token o il token `owner` su TLS. Un token `device` (un telefono accoppiato) e il token `owner` presentato dalla LAN senza TLS ricevono 403 e la richiesta resta aperta. Un token `device` può invece vedere l'elenco delle richieste (senza valori), rispondere alle domande dell'agente (`/api/questions`) e conferme di strumenti ordinari; un token `gateway` non arriva a nessuna di queste rotte. Su `POST /agui/{agent}` un client dichiara cosa sa mostrare con `forwardedProps.aiw.interactive`, ma la richiesta di segreti si apre solo per un chiamante con accesso completo.

**Le conferme degli strumenti di gestione valgono solo dal proprietario.** Ogni strumento `aiw_*` che modifica qualcosa (14 su 17) ha una regola `ask` incorporata nell'app: chiede conferma in ogni modalità (anche `auto` e `yolo`) e anche dopo «Consenti per la sessione», che per questi strumenti non esiste. La conferma si dà con `POST /api/approvals/{id}`, ma per questi strumenti solo con accesso completo: un token `device` e il token `owner` dalla LAN senza TLS ricevono 403 e la richiesta resta aperta; anche la risposta di un canale (Telegram, Slack, email) è rifiutata, perché i canali rispondono come non proprietari. Gli strumenti funzionano solo in un run di primo livello avviato dal server per una persona: chiamati da un subagente, da un workflow, dal server MCP di AIW (che non li elenca), da un canale, dall'assistente proattivo, da A2A o da `aiw run` sul terminale sono rifiutati senza chiedere conferma. Lo stesso vale per un run avviato da un dispositivo accoppiato (token `device`) che chiama uno strumento che modifica qualcosa: è rifiutato subito, prima che si apra una conferma che nessuno potrebbe dare. L'agente non può cambiare le proprie protezioni: modalità di approvazione, sandbox, rete dei comandi, domini consentiti, tempo di attesa delle conferme, porta, TLS e telemetria sono chiavi che nessuno strumento tocca; non esistono strumenti per regole di policy, hook, token di accesso o pairing; `aiw_agent_save` non crea agenti più liberi (modalità `plan` o `ask`, niente strumenti di gestione né a concessione esplicita).

**Limiti di questa parte (dichiarati):**

- Una regola `allow` scritta dalla persona in Sicurezza per uno strumento di gestione vince sulla regola incorporata: è una sua scelta esplicita, e fa saltare la conferma.
- Un segreto **incollato nella chat** (o scritto in un campo di `render_ui` con `obscured`, che è solo un campo password: il valore viene salvato e inviato al modello) resta nella sessione: messaggi, eventi e prompt delle chiamate successive. L'avviso nel compositore della modalità semplice segnala testo che somiglia a un segreto ma non blocca l'invio. L'export delle tracce nasconde per valore esatto i segreti salvati nell'app e gli schemi noti (`sk-…`, `ghp_…`, `Bearer …`).
- **Checkpoint e rewind non annullano le modifiche di gestione**: coprono solo i file del workspace. Impostazioni, assistenti, connettori, modelli eliminati e segreti si rimettono a mano; gli strumenti riportano il valore precedente dove ha senso e l'audit registra ogni chiamata.
- **Il kill switch** chiude run, conferme, domande e richieste di segreto in attesa, addestramenti, canali, sidecar e browser; **non è verificato** che interrompa download di modelli e installazioni di motori già avviati (non sono run).
- **Iniezione di istruzioni**: testo di pagine, file, posta, risultati di ricerca o risposte di altri agenti può cercare di convincere l'Assistente a un'azione di gestione. Le mitigazioni sono la conferma sempre richiesta e solo dal proprietario (con un avviso nella scheda quando il turno ha letto contenuto esterno), l'assenza di strumenti di lettura di web, file e posta nell'Assistente, la destinazione del segreto calcolata dal motore, le protezioni che l'agente non può cambiare e una regola nel prompt. **Non c'è un blocco automatico** degli strumenti di gestione dopo contenuto non fidato: se chi approva non legge la scheda (per esempio un connettore HTTP di un terzo seguito da una richiesta di token per un suo header) il rischio resta.
- Il giro con un client AG-UI reale non è stato provato, e AG-UI non è qui human-in-the-loop interoperabile (vedi `docs/agui.md`).

## Canali: contesto condiviso e bozze email

**Conversazioni collegate (contesto condiviso).** Il proprietario può dire che due o più account (per esempio il suo Telegram e la sua email) sono la stessa persona: i loro messaggi
*diretti* vanno in una sola sessione, con una sola cronologia. Per non farne un modo di leggere la cronologia altrui:

- **Lo decide solo il proprietario.** `/api/contexts` richiede accesso completo (un token `device`, un canale o un run non collegano nulla) e non si collega mai niente da solo; collegare non unisce le sessioni che esistevano già. Un account va preso dall'elenco di chi può scrivere sul canale, uno per canale, in un contesto solo.
- **Solo conversazioni dirette** (chat privata Telegram, messaggio diretto Slack, posta). Gruppi e menzioni in un canale restano nella conversazione del loro canale, perché la risposta la leggerebbero tutti; su Telegram l'identità è l'id della persona, mai quello di un gruppo.
- **Posta: solo con il controllo del mittente acceso** (`require_auth`, DKIM/SPF), al collegamento e a ogni messaggio. Con il controllo spento un `From` falsificato leggerebbe la cronologia degli altri canali, quindi quell'account non partecipa.
- **Mai più libertà del canale più restrittivo**, ricalcolata a ogni turno: un canale in «Solo lettura» tiene in sola lettura tutta la conversazione condivisa; i canali di un contesto usano lo stesso assistente.
- **Le richieste di permesso si decidono dove sono state fatte.** Il motore registra quale canale, conversazione e account ha avviato il turno; il «sì»/«no» o il pulsante di un altro canale dello stesso contesto, o per un run avviato dal proprietario nell'app, non decide e riceve un rimando (prima, la risposta di un canale valeva per qualunque richiesta della sessione). Le modifiche di gestione restano rifiutate in un run di canale e confermabili solo dal proprietario.
- **Duplicati e concorrenza**: lo stesso messaggio consegnato due volte (id della piattaforma) fa partire un solo lavoro; i messaggi di un contesto hanno una coda sola.

*Limite dichiarato*: un contesto allarga ciò che un account **compromesso** su uno dei canali può leggere (la cronologia condivisa, non più solo la sua conversazione). Per la posta il controllo del mittente (DKIM/SPF allineati) prova il **dominio**, non la casella: chi può inviare dal server di posta del tuo dominio con il tuo indirizzo nel `From` supera il controllo e, se quell'indirizzo è collegato, apre la cronologia degli altri canali; non collegare indirizzi di domini condivisi con altre persone. Chi non vuole questo rischio non collega gli account. L'identità è quella dell'elenco del proprietario: non c'è una prova dell'altro lato (un codice mandato da un canale all'altro).

**Bozze email (revisione e invio).** Una bozza non è la risposta automatica di un canale: **può avere qualunque destinatario**, quindi il testo di una mail, di una pagina o di un file può indurre l'assistente a scrivere una bozza che rivela dati. La difesa è che:

- **nessuno strumento invia**: `email_draft` crea o corregge una bozza dell'assistente (rischio scrittura, non offerto ai lavori di sola lettura: né all'assistente proattivo né ai canali in «Solo lettura»); l'invio è `POST /api/email-drafts/{id}/send` con accesso completo;
- **l'approvazione è del contenuto**: l'impronta (SHA-256) copre mittente, destinatari (anche i nascosti), oggetto, testo, intestazioni di filo e nome, tipo, dimensione e hash di ogni allegato; una bozza cambiata dopo la revisione è un 409. L'interfaccia mostra destinatari e copie nascoste e chiede una seconda conferma con i destinatari;
- **gli allegati si copiano al momento dell'inserimento**, con le regole degli strumenti di file (workspace e cartelle consentite, cartella dei dati e credenziali nascoste, collegamenti simbolici risolti; con il contenimento spento per scelta del proprietario il confine del workspace non c'è, ma i dati dell'app e le credenziali restano esclusi) e non si rileggono dal disco: all'invio si ricontrollano lunghezza e SHA-256;
- **una bozza parte una volta sola** (transizione atomica, `Message-ID` derivato dall'id); un guasto dopo la consegna al server è «esito incerto» e si ripete solo su conferma esplicita;
- **l'interruttore d'emergenza e un canale spento fermano i nuovi invii** (non uno già partito); massimo 20 invii all'ora e 20 destinatari per bozza; il registro riporta chi l'ha scritta, chi l'ha approvata e a chi è andata (destinatari, oggetto, nomi degli allegati), mai il testo;
- un telefono accoppiato (token `device`) può leggere, rifiutare e annullare, non creare, modificare né inviare.

*Limiti dichiarati*: la revisione umana è l'unica barriera contro testo indotto (non c'è un filtro dei destinatari né un blocco dopo contenuto non fidato); le bozze e i loro allegati restano nel database in chiaro, come le chat; le password dei server di posta sono salvate in chiaro come le altre; l'invio è stato provato solo contro un server SMTP finto (compresi guasti simulati), mai contro un provider vero; l'accesso OAuth per Gmail e Microsoft non c'è (vedi `docs/design/oauth-posta.md`). Dettagli: `docs/design/canali-contesto-e-bozze.md`.

## Limiti noti

Elenco completo e aggiornato in [docs/STATUS.md](docs/STATUS.md) §2.9. In sintesi:

- **Windows**: rete dei comandi non chiudibile, cartelle non nascondibili in lettura, server MCP non isolabili (servirebbe un AppContainer o regole del firewall).
- **macOS**: il profilo Seatbelt non è mai stato eseguito su un Mac.
- **Mac App Store (App Sandbox)**: gli agenti vedono solo la cartella dell'app più le cartelle che l'utente consente col pannello di macOS (Impostazioni → Cartelle consentite). Solo l'utente le aggiunge: nessuna rotta dell'API, impostazione, strumento o modello lo fa. Lì i tool di file leggono e scrivono ma non eliminano; la cartella dei dati e i luoghi di `sandbox.deny_read` (anche alla vera cartella personale) restano nascosti. Il pannello vero e una build firmata con i certificati dello store non sono stati provati; `shell` e MCP non ricevono quelle cartelle. Vedi `docs/store/README.md`.
- Non coperti dalla sandbox: socket dell'agente SSH e `/run/user`, hook, valutatori degli eval, browser controllato. Server MCP «non isolati» (quelli già presenti prima di questa versione) girano con l'ambiente e i permessi dell'app finché non premi «Isola».
- Wrapper che nascondono il comando vero (`ssh host «…»`, `docker run …`, interpreti con script propri) non vengono aperti dall'analisi delle regole di policy.
- Token e TLS: il token `owner` è in chiaro nel file `token`; un browser su un altro dispositivo mostra un avviso finché non installa l'autorità locale; niente fissaggio dell'impronta per server MCP e agenti A2A inseriti a mano.
- **Segreti in chiaro** nel database e nei file di configurazione, senza portachiavi né cifratura a riposo; segreti incollati nella chat o in un campo `obscured` di `render_ui` restano nella sessione; checkpoint e rewind non annullano le modifiche di gestione dell'Assistente; il kill switch non è verificato su download e installazioni già avviati (dettagli nella sezione precedente).
- Protezione SSRF dietro un proxy: non implementata.
- Dipendenze: dopo l'aggiornamento di `rusqlite` e `async-executor`, `cargo audit` non segnala vulnerabilità. Restano avvisi di crate non più mantenuti (`proc-macro-error`, `smartstring`, `ttf-parser`, `unsafe-libyaml`, `zbus_names`, `zvariant`, `zvariant_derive`) e `glib` 0.18.5 (RUSTSEC-2024-0429, serve un aggiornamento di Tauri/GTK), tutti senza correzione compatibile. Non sono ignorati. Un workflow (`.github/workflows/audit.yml`) li ricontrolla a ogni modifica dei lockfile e ogni settimana.
