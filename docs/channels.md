# Parlare con l'assistente da Telegram, Slack e email

Questa guida spiega come collegare un canale. Tutto è **spento di serie**: un canale non si collega a nulla finché non lo
accendi tu, e risponde solo a chi hai scritto nell'elenco. Tutti gli altri vengono ignorati (e contati nella scheda del
canale come «messaggi ignorati da sconosciuti»): non ricevono risposta e il loro testo non arriva mai a un modello.

> Stato onesto: i canali sono stati provati solo contro server finti nei test. Non sono mai stati provati contro Telegram,
> Slack o caselle di posta vere. Se qualcosa non torna con un servizio reale, è un difetto da segnalare.
> Vale anche per le conversazioni collegate e per le bozze email di questa guida: provate con Telegram, Slack, IMAP e SMTP finti
> (compresi guasti simulati), mai con un account o una casella veri.

## Prima di cominciare

- **L'app (o il servizio sul computer) deve essere accesa.** I canali funzionano solo mentre l'app o `aiw serve` girano.
  Sul telefono non lavorano in background.
- **Due modalità.** «Solo lettura» (di serie): l'assistente può rispondere e consultare, ma non scrive file, non esegue
  comandi e non chiede nulla. «Chiede conferma»: può agire, e ogni azione delicata chiede conferma nell'app **e** nel
  canale (rispondi «sì» o «no», oppure premi il pulsante). Se non rispondi, dopo 10 minuti la richiesta scade ed è un «no».
- **Mai in automatico.** Se l'assistente scelto ha la modalità automatica o «yolo», in un canale non viene rispettata:
  funziona come «Chiede conferma». L'app lo segnala sulla scheda.
- **Interruttore d'emergenza.** Ferma anche i canali e li spegne: dopo un riavvio non si ricollegano da soli.
- **Registro.** Ogni messaggio in entrata e in uscita compare nelle Attività.
- **Limiti.** Al massimo 10 messaggi al minuto per mittente; un solo lavoro alla volta per conversazione, con una coda
  di 5 messaggi.
- **Segreti.** I token e le password sono salvati nell'archivio dell'app (come le intestazioni dei server MCP), non sono
  mai mostrati dopo il salvataggio (la API e l'app mostrano `••••••`) e vanno solo al servizio a cui appartengono.
  Non sono cifrati sul disco: chi può leggere la cartella dei dati dell'app può leggerli.

## Telegram

1. Su Telegram scrivi a **@BotFather** il comando `/newbot`, scegli un nome e un indirizzo: ti risponde con un **token**
   (per esempio `123456:ABC-DEF…`).
2. In «Canali» premi «Aggiungi un canale», scegli Telegram, incolla il token e salva.
3. Premi **«Genera il codice»** e, dal tuo Telegram, scrivi al bot il messaggio che compare (`/start 123456`).
   Il codice vale 10 minuti, una sola volta, con al massimo 5 tentativi: se è giusto, ti aggiunge all'elenco.
4. Accendi il canale. Scrivi al bot: la conversazione compare nella chat come «Telegram · il tuo nome».

Nell'elenco puoi anche scrivere a mano il numero di Telegram di una persona.

**Gruppi.** Nell'elenco vale anche l'identificativo di una chat: se ci metti un gruppo, chiunque sia nel gruppo parla
con l'assistente. Meglio evitarlo, soprattutto in modalità «Chiede conferma».

## Slack

Si usa Socket Mode: nessun indirizzo pubblico da aprire.

1. Su `api.slack.com/apps` crea un'app **From scratch** nel tuo spazio di lavoro.
2. **Socket Mode**: attivalo e crea un token a livello di app con il permesso `connections:write` (inizia con `xapp-`).
3. **OAuth & Permissions**: nei permessi del bot (Bot Token Scopes) aggiungi `chat:write`, `im:history`,
   `app_mentions:read` e `users:read`; poi installa l'app nello spazio di lavoro. Il token del bot inizia con `xoxb-`.
4. **Event Subscriptions**: attivale e aggiungi gli eventi del bot `message.im` (messaggi privati) e `app_mention`
   (quando qualcuno cita il bot in un canale).
5. **App Home**: consenti i messaggi al bot («Allow users to send Slack commands and messages from the messages tab»).
6. In «Canali» scegli Slack, incolla i due token, scrivi nell'elenco gli identificativi delle persone che possono parlare
   (su Slack: profilo → «Copia ID membro», inizia con `U` o `W`) e accendi il canale.

Slack non è ancora supportato per gli allegati.

## Email

1. Crea una **casella dedicata** all'assistente: non usare la tua posta personale.
2. Per Gmail e Microsoft serve una «password per le app». L'accesso con il loro login (OAuth) non c'è ancora.
3. In «Canali» scegli Email e scrivi i server IMAP (posta in arrivo) e SMTP (posta in uscita) con utente e password.
   L'invio usa la porta 587 con STARTTLS oppure la 465 cifrata; la password non viaggia mai in chiaro.
4. Scrivi gli **indirizzi** che possono scrivere all'assistente.
5. Accendi il canale e premi «Prova».

Come si comporta:

- Quando si accende per la prima volta, non risponde alla posta già presente: conta solo quella che arriva dopo.
- Risponde nello stesso filo (`In-Reply-To`/`References`) e **senza citare** l'originale.
- Un mittente in elenco non basta: il server di posta deve garantirne l'autenticità (`Authentication-Results` con DKIM o
  SPF «pass» allineato al dominio dell'indirizzo). Si può spegnere questo controllo, ma è una scelta rischiosa: chiunque
  potrebbe scrivere a nome di un indirizzo in elenco.
- Non risponde mai a posta automatica (`Auto-Submitted`, `Precedence: bulk/list/junk`, `List-Id`).
- Al massimo 10 risposte all'ora per mittente.
- Non cambia la tua casella: i messaggi non vengono segnati come letti, a meno che tu non scelga una cartella dedicata
  (diversa da INBOX) e accenda «segna come letti»; in quel caso viene messo `\Seen` solo lì.

## Il riepilogo del mattino sui canali

Su ogni canale c'è l'interruttore «Il riepilogo del mattino va anche su questo canale». Il testo è quello che vedi
nell'app, inviato solo a chi è nell'elenco.

## Lo strumento `channel_send`

Permette all'assistente di scrivere sul canale di sua iniziativa. Richiede un permesso esplicito (il canale è il
«soggetto» del permesso), manda solo a chi è nell'elenco e non è mai disponibile nei lavori di sola lettura (compresi
quelli dei canali in «Solo lettura» e dell'assistente proattivo).

## Una conversazione sola su più canali

Di serie ogni conversazione esterna (una chat Telegram, un messaggio diretto Slack, un filo di posta) ha la sua sessione e niente
passa dall'una all'altra. Se sei tu a scrivere da Telegram e da email, puoi dire all'app che sono la stessa persona:
**Canali → Conversazioni collegate → «Collega degli account»**. Scegli un nome e, per ogni canale, il tuo account
(uno solo per canale, preso dall'elenco di chi può scrivere). Da quel momento i messaggi diretti di quegli account vanno in
**una sola conversazione**, con una sola cronologia, che compare anche nella chat dell'app con l'etichetta «Condivisa».
Chi ti risponde risponde sul canale da cui hai scritto.

Regole, tutte verificate dai test:

- **Lo decidi solo tu.** Non si collega mai nulla da solo e non si indovina che due account siano la stessa persona. Serve
  l'accesso completo (un telefono accoppiato non può). Le conversazioni che avevi già restano separate: collegare non le unisce.
- **Solo le conversazioni dirette.** Un messaggio in un gruppo, o la menzione del bot in un canale Slack, resta nella conversazione
  del suo canale: la risposta andrebbe a tutti quelli che la leggono e porterebbe con sé la cronologia privata. Su Telegram si collega
  la persona, mai un gruppo.
- **Posta: solo se il canale controlla il mittente** (DKIM/SPF). Il controllo prova il dominio, non la casella: non collegare indirizzi di domini che usi con altre persone. Con il controllo spento un `From` falsificato potrebbe leggere la
  cronologia degli altri canali: l'account non si collega, e se lo spegni dopo smette di partecipare (la scheda dice perché).
- **Mai più libertà del canale più restrittivo.** Se un canale è in «Solo lettura» lo è tutta la conversazione, ricalcolato a ogni messaggio.
  Tutti i canali di un contesto usano lo stesso assistente: il campo «Assistente che risponde» di un canale collegato ad altri non si cambia finché resta collegato.
- **Le richieste di permesso si decidono dove sono state fatte.** Una richiesta aperta da un messaggio su Telegram si approva su
  Telegram (o nell'app); un «sì» che arriva dalla posta, dallo stesso contesto, non vale e riceve un rimando. Lo stesso vale per un
  lavoro che hai avviato tu nell'app: un canale non ne approva le conferme. Le modifiche di gestione restano del proprietario.
- **Messaggi in coda per contesto, senza doppioni.** Due messaggi di due canali aspettano il proprio turno nella stessa coda (nessun errore
  di «lavoro già in corso»); lo stesso messaggio consegnato due volte dalla piattaforma (un nuovo tentativo di Telegram, una riconnessione di Slack,
  lo stesso `Message-ID`) fa partire un solo lavoro.
- **Un account esce da solo** se lo togli dall'elenco del canale o elimini il canale. Eliminare il contesto lascia la cronologia nella chat.

La voce non è cambiata: la dettatura e la lettura ad alta voce lavorano sulla chat aperta nell'app, quindi con la conversazione condivisa aperta
usano la sua cronologia (non provato con il microfono). Non c'è un canale vocale esterno.

Dettagli del modello (mappa identità → canale → sessione, concorrenza, conferme): `docs/design/canali-contesto-e-bozze.md`.

## Bozze email: scrivi, rivedi, invia

Sono un'altra cosa dalle risposte automatiche del canale Email (quelle vanno solo a chi è nell'elenco, nello stesso filo, con
l'intestazione «risposta automatica»). Una **bozza** è un'email che l'assistente prepara (o che scrivi tu) e che **non parte mai da sola**:
la trovi in **Canali → Bozze email**, con mittente, destinatari (anche le copie nascoste), oggetto, testo intero, allegati e origine
(una mail, un evento del calendario, la chat).

1. **Come nasce.** Chiedi all'assistente «rispondi a questa mail» (o approvi una proposta dell'assistente proattivo che lo dice):
   usa lo strumento `email_draft` e lascia la bozza in revisione. Lo strumento **non può inviare** e non è offerto ai lavori di sola
   lettura (l'assistente proattivo, i canali in «Solo lettura»): la proposta proattiva resta un suggerimento, e diventa una bozza solo quando
   la approvi. Puoi anche scrivere una bozza a mano. Serve un canale di tipo Email (con i server di posta scritti: è lui a conoscere quello di invio) per preparare una bozza, e acceso per inviarla.
2. **La revisione.** Guarda con attenzione chi la riceve e il testo: **una bozza può avere qualunque destinatario**, e se nasce da una mail,
   una pagina o un file di altri potrebbe contenere ciò che quel testo ha suggerito. La tua revisione è l'unica barriera.
3. **L'approvazione.** «Approva e invia» chiede di nuovo, con i destinatari, e approva *esattamente quel contenuto*: se la bozza cambia dopo
   la tua revisione (testo, destinatari, allegati o mittente) l'invio è rifiutato e la riguardi. Un telefono accoppiato può leggere e rifiutare,
   non inviare.
4. **Una volta sola.** Premere due volte, o ripetere la richiesta, non manda due email. Se il server dice di no *prima* di prendere il messaggio la bozza
   risulta «Non partita» (non è partito nulla) e si può ripetere; se la connessione cade *dopo* la consegna, o la risposta non arriva, è «Esito incerto»:
   controlla Posta inviata, e solo se vuoi puoi inviarla comunque (potrebbe arrivare due volte, con lo stesso `Message-ID`).
5. **Rifiuto e annullamento.** «Rifiuta» (con un motivo se vuoi) e «Annulla la bozza» non inviano niente. Una bozza chiusa si toglie dall'elenco.
6. **Allegati.** Sono file del workspace o delle cartelle consentite (stesse regole degli strumenti di file: niente fuori da lì, niente cartella dei dati o credenziali; con il
   contenimento spento per scelta tua il confine del workspace non c'è, come per gli strumenti di file, ma i dati dell'app e le credenziali restano esclusi),
   copiati nel momento in cui li aggiungi: se il file cambia dopo, la bozza resta com'era. Al massimo 5, 5 MB l'uno, 10 MB in tutto.
7. **Interruttore d'emergenza e canale spento** fermano i nuovi invii (non uno già partito). Il registro riporta chi l'ha scritta, chi l'ha approvata e a chi è andata
   (destinatari, oggetto, nomi degli allegati), mai il testo del messaggio.

Limiti: al massimo 20 destinatari e 20 invii all'ora; 50 bozze in attesa. Le bozze (e i loro allegati) sono salvate nel database come le chat,
senza cifratura. I messaggi inviati non sono firmati da AIW: valgono le regole del tuo server di posta.

## Posta IMAP per il riepilogo e le domande

È un'altra cosa: in Impostazioni → «Calendario e posta» → «Account di posta (IMAP)» l'assistente legge la posta dal server,
**solo in lettura** (`EXAMINE` e `BODY.PEEK`: non cambia, non segna, non cancella, non scrive). Connessione sempre cifrata
(implicita sulla 993, STARTTLS altrove e solo se riesce). Con il pulsante «Prova» si controlla l'accesso. Gli strumenti
`mail_recent` e `mail_search` e il riepilogo del mattino la usano senza altro da fare. Per Gmail e Microsoft serve una
«password per le app».

## Che cosa manca

WhatsApp, Discord, Matrix, i webhook, l'accesso con OAuth per Gmail e Microsoft (valutato in `docs/design/oauth-posta.md`), gli allegati su
Slack, un canale vocale esterno. I canali non sono mai stati provati contro i servizi veri.
