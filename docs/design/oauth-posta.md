# Accesso OAuth per Gmail e Microsoft: valutazione (fuori dal perimetro di #72 e #76)

15 novembre 2026. **Nessuna delle affermazioni sui prodotti esterni è stata verificata sulle fonti primarie in questa sessione**: la roadmap chiede di farlo prima dello sviluppo, ed è il primo passo di questo lavoro, non una conclusione.

## Stato oggi

Posta in arrivo (IMAP, sola lettura) e in uscita (SMTP, canale e bozze) usano nome utente e password; per Gmail e Microsoft serve una «password per le app»
(`docs/channels.md`). Le password stanno nel database in chiaro, come gli altri segreti (#121).

## Che cosa servirebbe

1. **Un client OAuth registrato** presso ogni provider (Google, Microsoft Entra) e la decisione su *di chi è*: dell'app (distribuita a tutti, con le verifiche e gli ambiti «sensibili/ristretti» che il provider può richiedere per leggere o inviare posta) oppure di chi la usa (ognuno registra il suo).
2. **Il flusso nel client**: autorizzazione con PKCE e reindirizzamento su loopback (o codice di dispositivo), con il browser di sistema; AIW ha già un flusso OAuth per i server MCP (`mcp_oauth.rs`) da cui partire.
3. **Un meccanismo SASL OAuth** nei client IMAP e SMTP (XOAUTH2/OAUTHBEARER): oggi `AUTH PLAIN` per SMTP e `LOGIN` per IMAP.
4. **Rinnovo e conservazione dei token**: un *refresh token* vale più di una password per le app (non si limita a una casella e non si può «spegnere» con una password); metterlo in chiaro nel database senza cifratura è peggio di oggi. Dipende da #121.
5. **Interfaccia e documentazione** per collegare e scollegare l'account, e per l'errore quando il consenso scade.

## Raccomandazione

Non prima di #121 (cifratura a riposo dei segreti) e di una decisione sul client OAuth (punto 1), che è di prodotto e di conformità, non di codice. Nel frattempo la password per le app, la casella dedicata e il controllo del mittente (`require_auth`) restano la
strada documentata. Stima grossolana, da rifare dopo la verifica: il flusso e i due meccanismi SASL sono lavoro di qualche giorno ciascuno; la registrazione e le verifiche dei provider sono il vero tempo di attesa.

## Da verificare sulle fonti primarie prima di cominciare

- Se, e a quali condizioni, Google e Microsoft consentono ancora password per le app/autenticazione di base per IMAP e SMTP, e fino a quando.
- Gli ambiti minimi per leggere e per inviare, e se per un'app distribuita richiedono verifica o audit.
- Quale meccanismo SASL accettano i server IMAP e SMTP di ciascuno (XOAUTH2, OAUTHBEARER) e se serve l'API del provider al posto di IMAP/SMTP.
