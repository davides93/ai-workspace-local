# Benchmark di prestazioni

Cosa si misura, come si lancia e che numeri sono usciti **su questa macchina**. I numeri sono **indicativi**: dipendono dal
processore, dal disco, dal carico del momento e dalla versione; servono a vedere se una modifica peggiora un percorso di
ordini di grandezza, non a confrontare macchine diverse. Nessun benchmark ha una soglia che faccia fallire la CI, tranne il
budget di dimensione del bundle della UI (sotto), che misura byte e non tempo.

Macchina dei numeri qui sotto: Intel Xeon @ 2,10 GHz, 4 core, 15 GB di RAM, Linux 6.18 (macchina virtuale), rustc 1.97,
Node 22, Chromium di Playwright 1.63; misure del 13 novembre 2026, con la macchina altrimenti ferma. Compilati in profilo
`bench` (ottimizzato), criterion 0.5 (valore centrale dell'intervallo di confidenza al 95 %).

## Cosa NON è misurato

- **I token al secondo dei modelli veri** (prefill e generazione), il tempo di caricamento di un modello, la memoria che un
  modello occupa, l'effetto di GPU, NPU, quantizzazione e contesto: servono hardware e modelli reali, e un benchmark fatto su
  un modello di prova direbbe solo la velocità del modello di prova. Il numero «tok/s» che la UI mostra in demo è inventato.
- La ricerca per **significato** (embedding): i benchmark qui sotto usano la sola ricerca per parole chiave, perché senza un
  modello di embedding in esecuzione è l'unica che gira. Il costo dei vettori dipende dal modello.
- La latenza di un run di un agente (dipende dal modello), i download, il browser dell'agente, la sandbox dei comandi.
- Windows e macOS: tutti i numeri sono di Linux.
- Telefoni: nessuna misura su Android o iOS.
- Il tempo di avvio della UI è misurato con la **demo** (trasporto finto, dati in memoria): con `aiw serve` ci sono in più le
  richieste vere al server, e nell'app desktop l'avvio della finestra.

## Rust (criterion)

I benchmark non richiedono un modello e usano un corpus sintetico con seme fisso, quindi due lanci misurano gli stessi dati.
Si lanciano un crate alla volta; la CI li compila e li esegue **una volta in modalità test** (`cargo test … --bench …`,
nessuna misura, nessuna soglia) soltanto per accorgersi se non compilano più o se un'ipotesi (ad esempio che `GET /api/system`
risponda 200 senza token da `127.0.0.1`) smette di valere.

```bash
cargo bench -p aiw-agent  --bench shellparse    # il lexer: pochi secondi di misura
cargo bench -p aiw-engine --bench hot_paths     # qualche minuto (ricerca, ingestione, store)
cargo bench -p aiw-server --bench server        # qualche minuto (avvio a freddo, latenza)
cargo bench -p aiw-engine --bench hot_paths -- kb/search     # solo i benchmark il cui nome contiene la stringa
cargo bench -p aiw-engine --bench hot_paths -- --save-baseline prima   # poi: -- --baseline prima, per confrontare
```

Sempre con `--bench <nome>`: senza, `cargo bench` passa i parametri di criterion anche ai test unitari del crate, che non li
conoscono. I rapporti HTML non sono attivi (criterion è compilato senza `plotters`): i numeri escono in console.

### Lexer della policy della shell: `aiw-agent`, `shellparse`

`shellparse::analyze` divide una riga di comando nei comandi semplici prima che le regole di policy la guardino; gira su ogni
comando che un agente vuole eseguire.

| Riga | Tempo | Velocità |
|---|---|---|
| `ls -la src` (semplice) | 0,55 µs | 17 MiB/s |
| pipeline di 5 comandi | 2,5 µs | 23 MiB/s |
| `cd … && cargo test … \| tail \|\| echo` | 2,8 µs | 28 MiB/s |
| virgolette annidate e apostrofi | 2,4 µs | 42 MiB/s |
| `$(…)`, backtick, `${…}`, `$((…))` | 2,6 µs | 23 MiB/s |
| `sudo env … nice … xargs … sh -c '…'` | 4,6 µs | 15 MiB/s |
| `bash -c "sh -c '… (ls; pwd) \| wc -l'"` | 2,3 µs | 19 MiB/s |
| catena di 100 comandi (`a > f && b > g && …`) | 73 µs | 35 MiB/s |
| PowerShell: semplice / pipeline / istruzioni | 0,63 / 3,5 / 2,5 µs | 33 / 27 / 34 MiB/s |

### Documenti, chunker e store: `aiw-engine`, `hot_paths`

Ricerca nei documenti (`Engine::kb_search`, solo parole chiave, i migliori 8 risultati) su un corpus di file Markdown
sintetici: 200 documenti da ~2 KB (0,4 MB) e 1000 da ~4 KB (4 MB). Le quattro ricerche sono una parola comune, due parole,
una parola rara e tre parole.

| Corpus | parola comune | due parole | parola rara | tre parole |
|---|---|---|---|---|
| 200 documenti | 1,7 ms | 2,9 ms | 1,9 ms | 2,6 ms |
| 1000 documenti | 4,5 ms | 13,9 ms | 4,8 ms | 13,7 ms |

| Benchmark | Tempo |
|---|---|
| `kb/ingest/50_docs`: leggere, spezzare, indicizzare 50 file da 4 KB in una raccolta vuota | 167 ms |
| `kb/chunk_text`: 10 KB / 100 KB / 1 MB di testo | 22 µs / 0,30 ms / 3,2 ms (≈ 300–430 MiB/s) |
| `store/upsert_session` (una riga, commit su file in WAL) | 171 µs |
| `store/append_1_message` / `10` / `100` (una transazione) | 265 µs / 538 µs / 2,2 ms |
| `store/list_sessions` con 100 / 1000 sessioni | 80 µs / 438 µs |
| `store/read_messages` di una sessione con 100 / 2000 messaggi (~600 byte l'uno) | 63 µs / 1,4 ms |

### Server: `aiw-server`, `server`

Non esiste un endpoint di sola salute: si misura `GET /api/system`, il più economico tra quelli che la UI chiama a ogni
avvio, su TCP vero verso `127.0.0.1` (client `reqwest`), con il motore offline (niente rilevamento di rete, niente modelli).

| Benchmark | Tempo |
|---|---|
| `Engine::new` su cartella dati vuota (crea lo schema, rileva l'hardware) | 42 ms |
| `Engine::new` su cartella dati esistente (ogni avvio dopo il primo) | 22 ms |
| `aiw_server::router` (costruzione delle rotte) | 0,43 ms |
| avvio a freddo fino alla prima risposta (motore su cartella esistente + listener + un `GET /api/system`) | 27 ms |
| `GET /api/system` con la connessione riusata (keep-alive) | 0,18 ms |
| `GET /api/system` con una connessione nuova per richiesta | 0,39 ms |

L'avvio a freddo è fatto soprattutto di `Engine::new` su una cartella esistente (22 ms); il resto è il listener e la prima richiesta.
Nell'app desktop e in `aiw serve` si aggiungono il rilevamento dei runtime e il caricamento della UI, che qui non sono misurati.

## UI

### Budget di dimensione del bundle

`npm run check:bundle` (dopo `npm run build` e `npm run build:mockup`) fallisce se `dist` o `dist-mockup` superano i limiti di
`apps/ui/scripts/bundle-budget.json`: i byte su disco e i byte gzip di tutti i file `.js`, `.css` e `.html`. I limiti sono la
dimensione di una build di riferimento più circa il 15 %, arrotondata ai 10 kB: segnalano una dipendenza o una risorsa entrata
per sbaglio, non la crescita lenta dell'app. Se l'app ha davvero bisogno di più spazio si alza il numero nello stesso commit.
`npm run check:bundle -- --report` stampa solo le misure.

| Build | Su disco | Gzip | Budget (disco / gzip) |
|---|---|---|---|
| `dist` (UI servita da `aiw serve` e incorporata nell'app) | 1,85 MiB (1 940 196 B) | 519 KiB | 2 240 000 B / 620 000 B |
| `dist-mockup` (demo a file unico) | 1,89 MiB (1 982 603 B) | 703 KiB | 2 280 000 B / 830 000 B |

Il file più grande di `dist` è il blocco principale, `index-*.js`: 773 KiB (Vite avvisa che supera i 500 kB).

### Tempo fino alla prima schermata utilizzabile (demo)

```bash
npm run build:mockup
npm run bench:startup                      # 7 lanci per dimensione
npm run bench:startup -- --runs=15 --json  # più lanci, risultato in JSON
```

Apre `dist-mockup/index.html` da file in un contesto nuovo del browser (niente cache) e misura, dall'inizio della
navigazione, quando sono sullo schermo il titolo della pagina e la navigazione, non c'è più nessuno scheletro di
caricamento e i font sono arrivati. **Solo riportato**, nessuna soglia. Mediana di 15 lanci:

| Finestra | Prima schermata utilizzabile | min – max | First contentful paint | DOMContentLoaded |
|---|---|---|---|---|
| 1440 × 900 | 266 ms | 238 – 401 ms | 224 ms | 163 ms |
| 390 × 844 | 241 ms | 218 – 307 ms | 212 ms | 157 ms |

Con la demo i dati sono in memoria: è il costo di scaricare ed eseguire il bundle, non di parlare con un server.

## Altre prove automatiche della UI (non sono benchmark)

I test E2E (`npm run test:e2e`) e di accessibilità (`npm run test:a11y`) hanno il loro tempo di esecuzione (circa 20 s e 3
minuti su questa macchina) ma non misurano nulla dell'app: vedi `CONTRIBUTING.md`.
