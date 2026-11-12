# Parlare con l'assistente da Telegram, Slack e email

Questa guida spiega come collegare un canale. Tutto è **spento di serie**: un canale non si collega a nulla finché non lo
accendi tu, e risponde solo a chi hai scritto nell'elenco. Tutti gli altri vengono ignorati (e contati nella scheda del
canale come «messaggi ignorati da sconosciuti»): non ricevono risposta e il loro testo non arriva mai a un modello.

> Stato onesto: i canali sono stati provati solo contro server finti nei test. Non sono mai stati provati contro Telegram,
> Slack o caselle di posta vere. Se qualcosa non torna con un servizio reale, è un difetto da segnalare.

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

## Posta IMAP per il riepilogo e le domande

È un'altra cosa: in Impostazioni → «Calendario e posta» → «Account di posta (IMAP)» l'assistente legge la posta dal server,
**solo in lettura** (`EXAMINE` e `BODY.PEEK`: non cambia, non segna, non cancella, non scrive). Connessione sempre cifrata
(implicita sulla 993, STARTTLS altrove e solo se riesce). Con il pulsante «Prova» si controlla l'accesso. Gli strumenti
`mail_recent` e `mail_search` e il riepilogo del mattino la usano senza altro da fare. Per Gmail e Microsoft serve una
«password per le app».

## Che cosa manca

WhatsApp, Discord, Matrix, i webhook, l'accesso con OAuth per Gmail e Microsoft, gli allegati su Slack. I canali non sono
mai stati provati contro i servizi veri.
