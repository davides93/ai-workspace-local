# Segreti cifrati a riposo

Issue #121. Token dei provider, header e variabili dei server MCP, token OAuth, password IMAP, credenziali di canali e agenti
remoti, chiavi API dei motori e delle impostazioni (`search.brave_api_key`) restano nello store di sempre, ma **cifrati con
una chiave casuale**. Solo la chiave sta nel portachiavi del sistema, in un unico elemento. Dove il portachiavi non c'è, i
segreti restano com'erano e lo stato è **dichiarato**: mai un errore che blocca l'avvio, mai dati persi.

## Che cosa protegge, e che cosa no

Protegge da: una copia del disco o della cartella dei dati, un backup, una cartella sincronizzata, un file letto per errore o
da un altro utente del computer.

**Non protegge da un programma che gira con il tuo stesso utente** (per esempio un comando della shell approvato, su Windows
dove le cartelle non si possono nascondere in lettura): quel programma può leggere l'elemento nel portachiavi quanto l'app.
macOS chiede il permesso una volta per programma; Windows e la maggior parte dei servizi di Linux non chiedono. La cifratura alza
il costo, non lo elimina. Non è una precondizione dell'invariante «il modello non vede mai un segreto», difeso da altro
(`SECURITY.md`).

Restano **in chiaro** e fuori da questo lavoro (restano in #24): il file `token` del proprietario, le chiavi TLS (cartella `tls`),
gli annunci mDNS, il pairing.

## Come funziona

- **Un solo punto di innesto.** `Store::put_doc`, `get_doc` e `list_docs` cifrano e decifrano le stringhe segrete dei documenti
  elencati in `crates/aiw-engine/src/vault/rules.rs`; `LayeredConfig` fa lo stesso per le impostazioni con `secret: true`. Chi
  chiama vede sempre valori in chiaro. Nessun altro codice cifra, e nessuno legge i documenti con SQL.
- **La tabella** (`vault/rules.rs`) decide quali sono segreti: `secret`, `peer_token`, `mcp_oauth` (documenti fatti di segreti:
  ogni stringa è cifrata, il backup li lascia fuori) e `mcp`, `channel`, `model_provider`, `remote_agent`, `runtime_settings`
  (le stringhe sotto una chiave che nomina un segreto, i valori della mappa `secrets` dei canali e gli indirizzi con credenziali
  dentro: `https://utente:password@host`, `?api_key=…`). Il backup deriva le sue liste dalla stessa tabella; un test legge i sorgenti
  e fallisce se il motore scrive un tipo di documento non classificato.
- **La busta.** AES-256-GCM (`ring`, già nell'albero: nessuna dipendenza crittografica nuova), nonce casuale a ogni cifratura. Il
  valore cifrato è una stringa al posto di quello in chiaro: `aiwenc1:<id della chiave>:<base64>`. L'id (4 byte di un hash della
  chiave) dice con quale chiave è stato cifrato; non rivela nulla della chiave. Un valore con un numero di versione più nuovo è
  trattato come cifrato e lasciato com'è.
- **La chiave.** 256 bit casuali, un elemento nel portachiavi (servizio «AI Workspace Local - chiave dei segreti», account =
  l'id del vault, salvato nel documento `vault/main` del database insieme all'id della chiave: mai la chiave). Un elemento per
  cartella dei dati: più installazioni (`AIW_HOME`) non si pestano i piedi.
- **Portachiavi usato**, per piattaforma: macOS Keychain, Gestione credenziali di Windows, Secret Service di Linux (GNOME
  Keyring, KWallet) su D-Bus con zbus (Rust puro: nessuna libreria di sistema da installare per compilare). Dipendenze:
  `keyring-core` e l'archivio di ogni piattaforma, non il crate `keyring` completo (senza archivio compilato ripiega su uno finto
  in memoria che perderebbe la chiave a ogni riavvio). Licenze MIT/Apache-2.0; `cargo deny check licenses bans sources` e
  `cargo audit` non danno nulla di nuovo.

## Stati

Visibili in `aiw doctor`, `aiw secrets status`, Sicurezza → Accessi → «Segreti su questo computer», nella diagnostica
(`health.secrets_at_rest`) e nella scheda della richiesta di un segreto. Solo conteggi e parole, mai un valore.
`GET /api/security/secrets` (accesso completo).

| Stato | Quando | Che cosa succede ai segreti |
|---|---|---|
| `active` | c'è un portachiavi e la chiave è leggibile | cifrati; quelli in chiaro di prima si cifrano all'avvio |
| `not_used` | questa build o questo processo non usa un portachiavi (iOS, Android, altri Unix, il lavoro in background del telefono, l'app dell'App Store) | in chiaro, com'erano; lo stato dice perché |
| `unavailable` | c'è un portachiavi ma ora non si usa (bloccato, accesso negato, nessuna sessione grafica, nessun servizio dei segreti su Linux, una chiamata che non risponde in 20 s) | in chiaro e nulla cancellato; **mai una chiave nuova**; i valori già cifrati si leggono come «non impostati» finché il portachiavi non torna; al prossimo avvio con il portachiavi si cifra tutto |
| `off` | `security.secrets_at_rest = off` | in chiaro; quelli cifrati tornano in chiaro all'avvio e l'elemento si toglie dal portachiavi |
| `key_lost` | ci sono valori cifrati e la chiave non è più nel portachiavi (elemento cancellato, cartella dei dati copiata da un altro computer) | **nulla viene toccato e nessuna chiave nuova viene fatta sopra**; i valori si leggono come «non impostati»; salvare un documento li conserva |

## Migrazione, e ritorno

- **All'avvio**, a ogni avvio: ciò che è in chiaro si cifra (database in una sola transazione; file di impostazioni riscritti
  solo se c'è un segreto in chiaro). Ripetibile: una seconda volta non cambia nulla. Dopo la migrazione le pagine che
  contenevano il valore in chiaro sono sovrascritte (`secure_delete`) e il log (`-wal`) è troncato: un test cerca i valori nei
  byte di `aiw.db`, `aiw.db-wal` e dei file di impostazioni.
- **La chiave si verifica prima di usarla**: si salva nel portachiavi, si rilegge e si confronta, e solo allora si cifra
  qualcosa. Se non torna indietro, niente viene cifrato e lo stato è `unavailable`.
- **Reversibile**: `security.secrets_at_rest = off` (o `aiw secrets decrypt`) riscrive tutto in chiaro e toglie l'elemento dal
  portachiavi; `aiw secrets encrypt` (o `auto`) lo rifà con una chiave nuova. `aiw secrets encrypt|decrypt|forget-unreadable`
  rifiutano di girare se un altro processo ha i dati aperti.
- **Chiave persa**: `aiw secrets forget-unreadable` toglie (dopo conferma) solo i segreti che non si possono più aprire, e
  fa una chiave nuova. È l'unico caso in cui un valore cifrato viene scartato.
- **Prima di tornare a una versione precedente** esegui `aiw secrets decrypt`: una versione senza questa funzione leggerebbe
  `aiwenc1:…` come se fosse il token e lo manderebbe così (non è un segreto, ma non funziona finché non lo reinserisci).

## Backup, ripristino, cambio di computer

- **Senza `--include-secrets`**: nessun segreto, né in chiaro né cifrato, e nessuna traccia del vault. Prima il database della
  copia si apre con la chiave, così un indirizzo con una password perde la password ma tiene host e percorso. Come prima.
- **Con `--include-secrets`**: i segreti entrano **in chiaro** nell'archivio (aperti con la chiave di questo computer), così
  si ripristina su qualunque computer; al primo avvio là si cifrano con la chiave di quel computer. L'archivio è «sensibile» ed
  è sensibile quanto la cartella dei dati di prima (non è cifrato). Il manifesto lo dice (`notes`); se la chiave non era
  raggiungibile, i segreti non apribili non entrano e il manifesto dice quanti (`skipped`).
- **La copia di sicurezza prima di un ripristino** (`<dati>/backups/`) tiene i segreti **cifrati** con la chiave di questo
  computer: non è mai in chiaro su disco, e si rilegge solo qui.
- **Cambiare computer**: copiare la cartella dei dati a mano non porta la chiave (sta nel portachiavi del vecchio computer): i
  segreti diventano `key_lost`. Si usa un backup con `--include-secrets`, oppure si copia e si reinseriscono i segreti.
- Non si può ripulire: i backup fatti **prima** di questo aggiornamento (anche le copie di sicurezza in `backups/`) hanno i
  segreti in chiaro e restano come sono; i residui che il filesystem o il disco possono tenere di blocchi riscritti.

## Per chi sviluppa (la sessione #71, le credenziali per agente)

1. Per salvare un segreto in un tipo di documento nuovo: aggiungi il tipo a `WHOLE_KINDS` o `BY_NAME_KINDS` in
   `vault/rules.rs` e usa `store.put_doc(kind, id, &valore)` / `get_doc` / `list_docs` come sempre. Cifratura, migrazione, stato,
   backup e il test «nessun valore in chiaro sul disco» seguono. Finché il tipo non è classificato un test fallisce.
2. Un segreto in un posto che non è un documento (un file, una tabella): non scriverlo in chiaro. Usa `Vault` direttamente
   (`SecretCodec::seal` / `open`, nel crate `aiw-core::sealed`; `engine.secrets_codec()`), e tratta `SecretRead::Unreadable`
   lasciando la stringa com'è.
3. Mai leggere o scrivere i documenti con SQL: salterebbe il vault (il backup e la migrazione lo fanno apposta, sulla forma
   salvata).
4. Un valore che non si apre torna da `get_doc` come stringa cifrata e si risalva identico: se il tuo codice lo usa come
   credenziale la chiamata fallirà (è una stringa che non vale come token) e lo stato (`unreadable`) lo dichiara.
5. Nei test: `EngineOptions::keychain` vale `Keychain::None` di default (nessun portachiavi, mai quello vero). Per un portachiavi
   finto: `Keychain::Custom(MemoryKeyStore::new())`. Il guscio desktop e il demone passano `Keychain::System`.

## Che cosa non è stato provato

- **macOS, Windows, Linux con e senza servizio dei segreti, con il portachiavi vero**: vedi il rapporto della PR per ciò che è
  stato provato davvero (il portachiavi vero su questo Mac, il servizio dei segreti in un container Linux, la CI su Windows) e
  ciò che no. Non si dichiarano supporti non provati.
- **App dell'App Store (App Sandbox)**: il portachiavi con la sandbox, gli entitlement e i gruppi di accesso non sono mai stati
  provati: lì i segreti restano in chiaro e lo stato lo dice.
- **iOS e Android**: nessun portachiavi usato; i segreti restano in chiaro e lo stato lo dice. Un archivio sicuro per telefono è un
  lavoro separato.
- **Programmi diversi che condividono l'elemento su macOS** (l'app firmata e `aiw` da terminale, o due build): il portachiavi di
  macOS può chiedere il permesso al secondo programma; se non si può rispondere (nessuna sessione grafica) la chiamata fallisce e lo
  stato è `unavailable`.
- Credenziali che stanno in un posto che non è un segreto per l'app: gli argomenti dei server MCP stdio (`args`), i file di un
  repository. Come prima.
