# Sicurezza

**English summary.** AI Workspace Local (AIW) is a local-first app: it listens on `127.0.0.1` unless you enable LAN access. Report vulnerabilities **privately** through GitHub: *Security* tab → *Report a vulnerability* (Security Advisories) of this repository; please do not open a public issue. Only the latest release receives fixes. The shell sandbox closes the network and hides the app data folder and credential folders on Linux (verified with real bubblewrap) and, with the same rules, on macOS (profile text tested only, never run on a Mac); **on Windows the network cannot be closed and folders cannot be hidden from reads**. MCP stdio servers can be isolated (not on Windows) and need owner approval on first run. Known limits are listed below.

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

## Limiti noti

Elenco completo e aggiornato in [docs/STATUS.md](docs/STATUS.md) §2.9. In sintesi:

- **Windows**: rete dei comandi non chiudibile, cartelle non nascondibili in lettura, server MCP non isolabili (servirebbe un AppContainer o regole del firewall).
- **macOS**: il profilo Seatbelt non è mai stato eseguito su un Mac.
- Non coperti dalla sandbox: socket dell'agente SSH e `/run/user`, hook, valutatori degli eval, browser controllato. Server MCP «non isolati» (quelli già presenti prima di questa versione) girano con l'ambiente e i permessi dell'app finché non premi «Isola».
- Wrapper che nascondono il comando vero (`ssh host «…»`, `docker run …`, interpreti con script propri) non vengono aperti dall'analisi delle regole di policy.
- Token e TLS: il token `owner` è in chiaro nel file `token`; un browser su un altro dispositivo mostra un avviso finché non installa l'autorità locale; niente fissaggio dell'impronta per server MCP e agenti A2A inseriti a mano.
- Protezione SSRF dietro un proxy: non implementata.
- Dipendenze: dopo l'aggiornamento di `rusqlite` e `async-executor`, `cargo audit` non segnala vulnerabilità. Restano avvisi di crate non più mantenuti (`proc-macro-error`, `smartstring`, `ttf-parser`, `unsafe-libyaml`, `zbus_names`, `zvariant`, `zvariant_derive`) e `glib` 0.18.5 (RUSTSEC-2024-0429, serve un aggiornamento di Tauri/GTK), tutti senza correzione compatibile. Non sono ignorati. Un workflow (`.github/workflows/audit.yml`) li ricontrolla a ogni modifica dei lockfile e ogni settimana.
