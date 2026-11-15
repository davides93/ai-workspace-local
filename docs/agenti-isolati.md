# Agenti isolati: workspace, sandbox e credenziali per agente

Issue #71 (roadmap #5, P2). Un agente può lavorare **da solo**: in una cartella sua che gli altri agenti non leggono né scrivono, con
una rete della shell più chiusa del resto e con credenziali che sono sue e di nessun altro. Non è un nuovo meccanismo di
isolamento: riusa la sandbox di #23 (cartelle nascoste ai file tool e ai comandi) e il vault di #121/#126 (segreti cifrati con la
chiave nel portachiavi). Di serie nulla cambia: un agente senza `isolation` lavora nel workspace condiviso come sempre.

## Che cosa significa «identità»

In questo lavoro l'identità di un agente è **il suo id con tre cose legate a quell'id**: il suo profilo (ruolo, modalità, sandbox,
strumenti: `AgentProfile`, già persistente), la sua cartella di lavoro (`<dati>/agents/<id>/workspace`) e le sue credenziali
(documenti `agent_credential` con proprietario esatto). Non c'è altro: niente chiavi di firma per agente, niente account, niente
attestazione verso terzi. Un agente cancellato perde il profilo e le credenziali, e la sua cartella va da parte (vedi sotto).

## Come si attiva

| Cosa | Come |
|---|---|
| Workspace dedicato e rete della shell chiusa | `aiw agents isolation <id> --workspace dedicated --shell-network closed`, oppure `PUT /api/agents/{id}` con `"isolation": {"dedicated_workspace": true, "close_shell_network": true}` (accesso completo) |
| Spegnere l'isolamento | `aiw agents isolation <id> --workspace shared --shell-network inherit`, oppure `PUT` con i due flag a `false` |
| Una credenziale | `printf %s "$TOKEN" \| aiw agents credential set <agente> <nome> --host api.example.org`, oppure `PUT /api/agents/{id}/credentials/{nome}` (`{"hosts": [...], "value": "…"}`, accesso completo) |
| Vedere lo stato | `aiw agents`, `aiw agents credential list <agente>`, `GET /api/agents/{id}/credentials` (nomi, host, stato; mai il valore) |
| Togliere | `aiw agents credential remove <agente> <nome>`, `DELETE /api/agents/{id}/credentials/{nome}`, oppure eliminare l'agente |
| Far usare una credenziale all'agente | metti `web_fetch_auth` tra i suoi `tools` (uno strumento a concessione esplicita: un `*` non lo copre) |

Solo il proprietario fa tutto questo: la rotta richiede l'ambito `full` (un telefono accoppiato o un'app di terzi ricevono 403),
nessuno strumento dell'Assistente tocca credenziali o isolamento, e `aiw_agent_save` **non modifica** un agente che ha un isolamento
o delle credenziali (si cambia dalla vista Sviluppatore, dalla CLI o dall'API). Il campo `isolation` assente in un salvataggio
**lascia quello salvato**: un editor che non lo conosce non può spegnerlo salvando; per spegnerlo si manda l'oggetto con i due flag a `false`.
Non c'è ancora un'interfaccia grafica: la UI non è stata toccata.

## Workspace dedicato

La cartella è `<dati>/agents/<id>/workspace`, dentro la cartella dei dati dell'app. Quella cartella è già nascosta a **ogni** agente
(strumenti per i file e comandi in sandbox), e un workspace che sta dentro una cartella nascosta è raggiungibile solo dal run che ci
lavora: lo stesso meccanismo dei worktree delle sessioni isolate (`<dati>/worktrees`). Quindi:

- l'agente lavora lì e scrive lì; non vede il workspace condiviso (né con i file tool né con la shell), né le cartelle che l'utente ha
  consentito (App Store), né l'`AGENTS.md` e le skill del workspace condiviso (restano le skill globali e l'`AGENTS.md` globale);
- gli altri agenti non leggono né scrivono quella cartella: non dai file tool (percorso assoluto, relativo, con `..`, tramite un
  collegamento simbolico), non dalla shell. Un agente con la sandbox `off` invece legge tutto quello che legge l'account: per
  questo un agente con workspace dedicato **non può avere la sandbox `off`** (rifiutato al salvataggio);
- un sotto-agente o il membro di un team lavora **nella cartella del genitore** quando il genitore o il sotto-agente è dedicato: il
  confine è l'intero run, quindi un genitore non legge ciò che un sotto-agente tiene separato e un sotto-agente non raggiunge ciò che
  il genitore non raggiunge. Un sotto-agente usa le credenziali del **genitore**, mai le sue (e solo se il genitore gli ha dato lo
  strumento: la restrizione degli strumenti è quella di sempre);
- il passaggio di conversazione (`handoff`) da o verso un agente dedicato o con credenziali è rifiutato (la cronologia porterebbe ciò
  che un lato ha visto nella cartella e nelle credenziali dell'altro);
- i run in background (proposte) e dei canali di un agente dedicato usano la cartella di quell'agente, ma **non** le sue credenziali
  (un testo che arriva da fuori non deve poterle usare);
- una sessione in un git worktree non si può dare a un agente dedicato;
- rimuovere l'agente sposta la sua cartella in `<dati>/agents/.removed/<id>-<data>` (nessun agente la raggiunge: sta nei dati) e non
  cancella nulla: un agente creato dopo con lo stesso id parte da una cartella vuota e non eredita file né credenziali;
- l'id deve poter essere il nome di una cartella (lettere, cifre, `- _ .`, al massimo 64 caratteri) e non differire da un altro solo
  per maiuscole (su macOS e Windows sarebbe la stessa cartella).

Rete: `close_shell_network` chiude la rete della shell di quell'agente anche se `sandbox.shell_network` è acceso; non la apre mai
se le impostazioni la chiudono, e un sotto-agente non la riapre. (Non c'è invece un elenco di domini per agente.)

### Che cosa **non** isola

- Con la sandbox `off` non c'è nessun confine (è il significato di `off`).
- **La shell legge il resto del disco**, come per ogni agente: la sandbox nasconde cartelle precise (la cartella dei dati, `sandbox.deny_read`,
  e per un agente dedicato anche il workspace condiviso), non tutto ciò che sta fuori dal workspace. Un agente dedicato non è quindi protetto da
  ciò che un agente qualsiasi può già leggere (la cartella personale fuori da `sandbox.deny_read`).
- **Windows**: la shell non può nascondere cartelle in lettura (un token a integrità bassa limita le scritture, non le letture) e non
  chiude la rete. Lì a tenere separati gli agenti sono i file tool; un comando di shell di un agente può leggere la cartella di un altro.
  Il test con comandi veri gira solo dove c'è bubblewrap o Seatbelt e altrove si salta dicendolo.
- Le memorie globali, la base di conoscenza (`knowledge` vuoto = tutte le raccolte), le lezioni approvate in `~/.aiw/AGENTS.md`, i
  server MCP e i connettori sono **comuni** a tutti gli agenti: la cifratura per agente e la cartella per agente non li toccano. Le
  memorie con ambito di un agente restano sue come già prima.
- Hook, valutatori degli eval e browser controllato non passano dalla sandbox (limite già noto di #23).

## Credenziali per agente

Un documento `agent_credential` per ogni credenziale: proprietario, nome, host consentiti, intestazione, prefisso e il **valore**,
che è cifrato dal vault come ogni altro segreto (campo `secret`, tabella `vault/rules.rs`): AES-256-GCM, chiave nel portachiavi, e
dove il portachiavi non c'è resta in chiaro con lo stato dichiarato (vedi `docs/segreti.md`).

- **Il modello non vede mai il valore.** Non lo scrive (non ha modo di impostarlo), non lo legge (le liste dell'API e dell'ispezione
  hanno solo nome, host e stato), e non lo riceve in risposta: lo strumento `web_fetch_auth` prende l'**URL** e il **nome**, il motore
  aggiunge l'intestazione e toglie il valore (in chiaro e codificato) da ciò che il server rimanda, per i server che rimandano la richiesta.
  Il prompt dice all'agente il nome e gli host delle sue credenziali, mai il valore.
- **Ambito.** La credenziale parte solo verso gli host indicati (e i loro sottodomini; un indirizzo IP solo uguale a sé), solo in
  **https** (http solo verso loopback, per le prove, e nessuna build lo abilita), e l'ambito si ricontrolla a **ogni redirect**: una
  pagina nell'ambito che rimanda altrove non porta la credenziale. Un'estensione senza punto (`com`) non è un host valido.
- **Revoca immediata.** Il motore legge lo store a ogni chiamata: togliere la credenziale o l'agente la rende inutilizzabile alla
  chiamata successiva, anche per un run già in corso. Togliere l'agente toglie anche le sue credenziali.
- **Chi la riceve.** Il run dell'agente stesso (da una persona o da un workflow). Non i run in background, non quelli dei canali, non
  le valutazioni; un sotto-agente usa quelle del genitore.
- **Nei backup.** Senza `--include-secrets` il valore non c'è, né in chiaro né cifrato; restano proprietario, nome e host con il valore
  vuoto (dopo un ripristino la credenziale risulta «senza valore» e va reinserita). Con `--include-secrets` il valore entra in chiaro
  nell'archivio, come ogni altro segreto (l'archivio diventa sensibile). L'elenco dei tipi deriva dalla stessa tabella del vault.
- **Nei log e negli export.** Eventi, messaggi, audit, tracce (anche senza mascheratura), diagnostica e lista degli agenti non
  contengono il valore (test); l'audit registra chi la imposta, la toglie e dove viene usata (`alfa/github → api.example.org`).

Che cosa non copre: **la shell**. Una credenziale non finisce nell'ambiente di un comando: `env` la mostrerebbe al modello. Un agente
che deve lanciare `git` o `curl` con un token non ha oggi una via; resta fuori da questo lavoro.

## Valutazione: contenitori Docker/Podman (solo opzionali, solo Linux)

*Scritta, non implementata né provata: nessun Docker o Podman vero è stato eseguito per questo lavoro. I comportamenti dei due
prodotti citati qui sotto sono quelli che conosco e vanno **verificati sulle fonti primarie** (documentazione di Docker, Podman,
bubblewrap, SELinux) prima di scrivere codice, come dice la stessa issue.*

**Che cosa darebbe in più rispetto a bubblewrap/Seatbelt di #23.**

- Limiti di risorse per agente (CPU, memoria, numero di processi) tramite cgroup: oggi un comando può consumare tutta la macchina.
- Un filesystem di sistema proprio (un'immagine), quindi niente lettura del resto del disco e un ambiente ripetibile per gli strumenti
  che l'agente installa; è il buco più grosso della sandbox attuale («la shell legge il resto del disco»).
- Rete per contenitore (nessuna, o una rete con un proxy di uscita che filtra i domini): oggi la rete è tutto-o-niente per la shell.
- Un utente e uno spazio dei processi propri, e profili seccomp/capacità ridotte già pronti.

**Che cosa costa.**

- Un demone (Docker) o Podman senza demone con utente normale (*rootless*): su molte macchine non c'è, e l'app non deve richiederlo.
- Immagini da scaricare (rete, spazio) e da aggiornare; un avvio di contenitore per comando è molto più lento di bubblewrap (decine di
  millisecondi contro centinaia); un contenitore longevo per agente richiede di gestirne vita, stato e pulizia.
- Permessi dei file condivisi: mappatura degli utenti (con Podman `--userns=keep-id`, con Docker rootless dipende dalla configurazione),
  etichette SELinux sui volumi (`:Z`), proprietà dei file creati dall'agente nella cartella montata.
- **Solo la shell e i server MCP stdio** girerebbero nel contenitore: i file tool, le credenziali e il motore restano nel processo
  dell'app. Il confine dei file tool resta quello di `resolve()`.
- Su Windows e macOS Docker lavora dentro una macchina virtuale: altra superficie, altri permessi per il volume, e non è «locale senza
  niente di installato». Per questo la proposta è solo Linux.

**Come si innesterebbe** (schema, non codice): un `sandbox.backend = auto | bwrap | container` per macchina (mai per agente né dal
modello); `SandboxSpec` (`aiw-tools/src/sandbox.rs`) è già una descrizione pura che `wrap()` traduce in una riga di comando, quindi un
terzo traduttore produrrebbe `podman run --rm -i --network=none --read-only --cap-drop=ALL --security-opt=no-new-privileges
--userns=keep-id -v <workspace>:/work:rw -w /work <immagine> …`; con `hidden` e `readable` che diventano semplicemente «non montati». Le
credenziali non entrerebbero mai come variabili d'ambiente del contenitore (`inspect` e `/proc` le mostrano): resta l'iniezione del
motore di `web_fetch_auth`. Se il contenitore richiesto non c'è o non parte, il comando **non parte** (mai un ripiego silenzioso senza
isolamento), e il funzionamento locale senza contenitori resta quello di serie.

**Raccomandazione.** Non implementarlo ora. L'isolamento dei file tra agenti e le credenziali per agente non ne hanno bisogno e sono
completi senza; il vantaggio vero (limiti di risorse, filesystem proprio, rete filtrata) vale quando un agente esegue codice che
non si controlla o si vogliono limiti di risorse, e allora conviene farlo come servizio opzionale su Linux dopo aver provato su una macchina
con Podman rootless e una con Docker. Prima di costruirlo servono: la verifica delle opzioni sulle fonti primarie, una prova di
latenza e di permessi dei volumi, un test che dimostri che un agente non legge la cartella di un altro **dentro** il contenitore, e la decisione
sul ripiego quando il contenitore manca. Nulla di tutto questo è stato fatto.

## Che cosa è provato, e che cosa no

Provato con test automatici (`crates/aiw-engine/src/tests_isolation.rs`, `tests_credentials.rs`, `crates/aiw-server/src/tests_agents.rs`,
più i test del vault e del backup che derivano dalla stessa tabella):

- un agente dedicato scrive e legge solo nella sua cartella; un altro agente non la raggiunge con file tool (percorso assoluto,
  relativo, `..`, link simbolico, `list_dir`, `grep`, `glob`), né con comandi veri (Seatbelt su macOS: provato su questo Mac); il suo
  `AGENTS.md`, le sue skill e le cartelle consentite non arrivano all'altro e viceversa;
- il confine regge per sotto-agenti e membri di team, per l'handoff e per i run in background e dei canali;
- le credenziali: un agente non usa quelle di un altro (nome uguale, id che inizia come un altro, sotto-agente), partono solo verso i
  loro host in https e non dopo un redirect altrove, si revocano subito (credenziale o agente, anche in un run in corso), sopravvivono
  al riavvio cifrate e non sono in chiaro su disco; nessun valore in eventi, messaggi, audit, tracce, diagnostica e backup predefinito;
- persistenza: profilo, cartella e credenziali dopo un riavvio con lo stesso portachiavi (finto).

**Non provato:** Docker o Podman veri; Windows (la shell non nasconde cartelle, il test con comandi veri non gira); Linux con
bubblewrap (il test con comandi veri è scritto per girarci ma non l'ho eseguito); macOS con il portachiavi vero e l'app dell'App Store;
un modello vero che usi `web_fetch_auth` (il modello è sempre a copione); un server vero con credenziale vera (il server è un finto su
loopback, la restrizione a https non è stata esercitata con TLS); l'interfaccia grafica (non c'è).
