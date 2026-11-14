# Spazi e pagine condivise agente-utente con revisioni

Data: 2026-11-14 · Stato: **Decisione approvata dal proprietario il 14 novembre 2026** · Riferimento: issue #74 (priorità P3, discovery)

Questo documento conclude la discovery approvata dal proprietario (sezione 10). Non implementa la funzione: prima si misura l'utilità, poi si valuta l'MVP in base alle soglie definite.

## 1. Contesto e stato attuale

La issue #74 (vedi `docs/integrazione-tecnologie-frontiera.md`) chiede di valutare «documenti vivi» locali: modifica concorrente, cronologia, diff, approvazione prima del salvataggio e ripristino, senza adottare lo stack cloud di OpenDots/OpenBot. Oggi non esiste un oggetto «pagina» condiviso tra agente e utente. Esistono però molti pezzi riusabili.

- **Checkpoint e rewind dei file.** Prima di una scrittura, `ToolContext::checkpoint` in `crates/aiw-tools/src/lib.rs` copia l'originale in una cartella per run; `crates/aiw-tools/src/checkpoint.rs` gestisce i metadati (`.aiw-run.json`) e il ripristino. Le rotte sono `/api/runs/{id}/checkpoint` e `/api/runs/{id}/rewind` in `crates/aiw-server/src/lib.rs`. Il modello è per run: restituisce lo stato precedente a un run, non una cronologia di versioni, e non registra le modifiche fatte dall'utente fuori da un run. In `crates/aiw-server/src/scopes.rs` il rewind richiede l'ambito `full`.
- **Worktree git per sessione.** `crates/aiw-engine/src/worktree.rs` isola una sessione su un ramo; l'utente rivede il diff e lo applica o lo scarta. È pensato per codice in un repository, non per documenti di uso quotidiano.
- **Proposte e approvazione.** La tabella `proposals` (`crates/aiw-engine/src/store/proposals.rs`) e le lezioni di `crates/aiw-engine/src/learning.rs` seguono già il modello «l'agente propone, l'utente approva o scarta, solo allora si scrive». Vedi anche `docs/STATUS.md`, paragrafo 2.1.
- **Documenti e ricerca.** Le raccolte in `crates/aiw-engine/src/kb.rs` (tabelle `kb_*` in `crates/aiw-engine/src/store/kb.rs`) indicizzano file caricati con sha256 e FTS5. Sono però copie in sola lettura per il RAG, senza modifica né revisioni.
- **Superfici A2UI.** `render_ui` (`crates/aiw-agent/src/a2ui.rs`) mostra superfici dichiarative e rimanda le azioni dell'utente all'agente: base possibile per rendere e commentare una pagina.
- **Schema e accessi.** Le migrazioni passano da `SCHEMA_VERSION` in `crates/aiw-engine/src/store.rs`, con passi idempotenti. I token per dispositivo hanno ambiti (`crates/aiw-engine/src/store/tokens.rs`, mappa delle rotte in `crates/aiw-server/src/scopes.rs`).
- **Memoria.** `crates/aiw-engine/src/memory.rs` inietta nel prompt memorie fisse e richiamate: è il precedente da tenere presente per il rischio del paragrafo 8.

Su OpenDots e CopilotKit si dispone solo di quanto scritto in `docs/research/01-reverse-engineering.md` (paragrafo 3.5): colleghi AI sempre attivi, ciascuno con il proprio computer, permessi per agente, MCP per agente, lavoro in background. Niente di più è affermato qui: se serve un confronto sulle pagine condivise, va verificato sulle fonti primarie.

## 2. Casi d'uso

1. **Piano o report vivo.** L'agente `planner` mantiene un piano di lavoro o un report settimanale; l'utente corregge una frase e vuole che l'agente parta dalla sua versione, non la sovrascriva.
2. **Appunti tra telefono e computer.** Una pagina di appunti modificata dal telefono accoppiato e dal computer: il telefono, con ambito `device`, legge, scrive e propone; la cronologia dice da dove è venuta ogni modifica.
3. **Pagina tenuta aggiornata dall'assistente proattivo.** Il riepilogo del mattino o una pagina «cose in sospeso» è aggiornata dal battito dell'assistente proattivo (`docs/STATUS.md`, paragrafo 2.1, dove oggi le proposte vivono nella tabella `proposals`): ogni aggiornamento è una proposta da rivedere, non una scrittura silenziosa.
4. **Documento di decisione costruito in più sessioni.** Un ADR o una nota di ricerca cresce in sessioni diverse, con possibilità di tornare alla versione di tre giorni prima.
5. **Checklist condivisa con un team di agenti.** Un team (strategia catena o supervisor) legge e aggiorna una checklist; oggi usa `team_state`, limitato a 64 chiavi e 32 KB e non revisionabile.

## 3. Opzioni valutate

### A. File Markdown nel workspace, revisioni via checkpoint o git

- **Pro:** nessuno schema nuovo; i file si aprono con qualunque editor; il worktree di `crates/aiw-engine/src/worktree.rs` dà già diff e applicazione.
- **Contro:** il checkpoint è per run e non registra le modifiche dell'utente; git richiede un repository e commit per ogni modifica; un dispositivo accoppiato raggiunge il computer solo tramite l'API, non i suoi file; nessun punto unico dove applicare il controllo di concorrenza; l'approvazione prima della scrittura andrebbe aggirata con file temporanei.

### B. Pagine in SQLite con tabella delle revisioni e diff testuale

- **Pro:** una sola fonte di verità, già coperta da backup, migrazioni e FTS5; revisioni numerate per pagina rendono semplice il blocco ottimistico; funziona identico su desktop e telefono tramite l'API; l'approvazione si aggancia al modello delle proposte.
- **Contro:** tabelle e rotte nuove da mantenere; i file non sono modificabili da fuori senza un export; diff e ripristino vanno scritti (il testo è piccolo, il costo è contenuto).

### C. CRDT (per esempio un documento condiviso con unione automatica)

- **Pro:** unione senza conflitti anche con modifiche simultanee e offline.
- **Contro:** dipendenza e formato binario nuovi; la cronologia leggibile e il diff umano vanno ricostruiti; l'unione automatica di testo scritto da un modello è proprio ciò che l'utente non deve subire senza vederlo; risolve un problema (collaborazione in tempo reale tra più persone) che il progetto oggi non ha.

### D. Ibrido: B come fonte di verità, con esportazione opzionale in file

Aggiunge a B un export Markdown nel workspace (a senso unico, dalla pagina al file). Si può rimandare dopo l'MVP.

## 4. Modello dati proposto

Opzione B, con una migrazione nel primo passo libero dopo quello attuale (oggi `SCHEMA_VERSION` vale 6), idempotente come richiesto dal commento in `crates/aiw-engine/src/store.rs`. Nomi indicativi; file proposto: *crates/aiw-engine/src/store/spaces.rs*.

- **spaces**: `id`, `name`, `description`, `created_at`, `archived_at`.
- **pages**: `id`, `space_id`, `title`, `kind` (`markdown` all'inizio), `head_rev` (numero della revisione corrente), `agent_mode` (`none`, `propose`, `direct`; default `propose`), `created_at`, `updated_at`, `archived_at`.
- **page_revisions**: `page_id`, `rev` (intero crescente per pagina), `parent_rev`, `body`, `sha256`, `author_kind` (`user`, `agent`, `device`, `restore`), `author_id` (agente o dispositivo), `run_id`, `session_id`, `proposal_id`, `note`, `created_at`. Chiave `(page_id, rev)`: le revisioni non si modificano mai.
- **page_proposals**: `id`, `page_id`, `base_rev`, `proposed_body`, `diff`, `agent_id`, `run_id`, `status` (`pending`, `approved`, `rejected`, `stale`, `expired`), `created_at`, `expires_at`, `decided_at`, `decided_by`, `result_rev`.
- **space_agents**: `space_id`, `agent_id`, `mode` (`read`, `propose`, `direct`): permessi per agente.
- **page_fts**: indice FTS5 sul corpo della revisione corrente, come per `memories` e `kb_chunks`.

Strumenti proposti per l'agente: *page_read*, *page_search*, *page_propose* e, solo con permesso esplicito, *page_write*. Rotte `/api/spaces/**` classificate in `crates/aiw-server/src/scopes.rs` (il test sulle rotte senza voce lo impone).

## 5. Flusso di modifica, revisioni e approvazione

1. L'agente legge la pagina e ottiene `head_rev` (la revisione di partenza, `base_rev`).
2. Chiama *page_propose* con il nuovo testo e il `base_rev`. Il motore calcola il diff testuale e crea una proposta `pending`; non viene scritto nulla nella pagina.
3. L'utente vede il diff nell'app (o sul telefono) e sceglie: approva (anche con modifiche a mano), scarta, oppure chiede all'agente di rifare.
4. All'approvazione il motore confronta `base_rev` con `head_rev` in una transazione. Se coincidono, crea la revisione `head_rev + 1` con `author_kind = agent`, collegata a proposta e run; la proposta diventa `approved`.
5. Se nel frattempo l'utente ha salvato una nuova revisione, `base_rev` è vecchio: la proposta diventa `stale`, non si applica e l'agente (o l'utente) la rigenera sulla versione nuova.
6. Ripristino: scegliere la revisione N crea la revisione `head_rev + 1` con lo stesso testo di N e `author_kind = restore`. La cronologia non si riscrive mai.

**Modifica concorrente.** Si propone il blocco ottimistico per revisione, non la fusione. Anche il salvataggio dell'utente porta il suo `base_rev` e fallisce con un conflitto se la pagina è avanzata, mostrando le due versioni. Motivo: l'unione automatica di testo scritto da un modello produce risultati che nessuno ha riletto; un rifiuto esplicito costa un secondo giro, una fusione sbagliata costa fiducia. Una fusione a tre vie può essere un'estensione futura, sempre con approvazione.

## 6. Criteri per un MVP

**Quando vale la pena (da misurare prima di costruire).** Proposte di soglia, da confermare con il proprietario:

- almeno 3 documenti ricorrenti che oggi l'utente tiene a mano o chiede all'agente di riscrivere;
- in una finestra di due settimane, almeno 5 casi in cui un file modificato da un agente è stato corretto a mano o ripristinato con il rewind;
- almeno un caso reale di perdita o sovrascrittura di una correzione dell'utente.

Se non si raggiungono, la raccomandazione è non costruire e rimandare.

**Ambito dell'MVP.** Opzione B, una sola pagina markdown per volta, spazi come semplice raggruppamento, solo proposte (`agent_mode` = `propose`), diff testuale per righe, cronologia con ripristino, interfaccia nel desktop e nel telefono accoppiato.

**Criteri di accettazione.**

- Una proposta con `base_rev` superato non modifica mai la pagina (test).
- Ripristino e approvazione producono sempre una nuova revisione; nessuna revisione esistente cambia (test).
- La nuova migrazione è idempotente e un database con schema più recente viene ancora rifiutato intatto (stile dei test in `crates/aiw-engine/src/store/migration_tests.rs`).
- Ogni nuova rotta ha una voce in `crates/aiw-server/src/scopes.rs`.
- Un agente senza permesso sullo spazio non legge né propone (test).
- Backup e diagnostica includono le tabelle, senza segreti nei diff.
- Corpo massimo per pagina (per esempio 256 KB) e numero massimo di revisioni conservate, con pulizia delle più vecchie dichiarata.

## 7. Non obiettivi

- Collaborazione in tempo reale tra più persone, cursori, presenza, commenti a thread.
- Qualunque componente cloud o servizio ospitato; nessuna dipendenza da OpenDots o OpenBot.
- CRDT e fusione automatica del testo.
- Formati ricchi (docx, fogli di calcolo), allegati nelle pagine, pagine che contengono codice eseguibile.
- Sostituire il worktree git per il codice o il rewind per i file del workspace.

## 8. Rischi e domande aperte

**Sicurezza.**

- **Contenuti dell'agente in altri prompt.** Una pagina scritta da un agente e poi letta da un altro (o iniettata come la memoria) è un canale di prompt injection persistente. Mitigazioni proposte: nessuna iniezione automatica nel prompt, solo lettura esplicita con lo strumento; contenuto marcato come dato non fidato; revisione approvata dall'utente prima che diventi leggibile da altri agenti.
- **Permessi per agente.** Il default è sola proposta; la scrittura diretta è una scelta per spazio e per agente, mai ereditata dai subagenti oltre i permessi del padre (come già per sandbox e policy).
- **Dispositivi accoppiati.** Un token `device` può leggere e proporre; se può anche approvare è una domanda aperta, perché il rewind oggi richiede `full`. Un telefono rubato non dovrebbe poter riscrivere la cronologia: le revisioni sono immutabili e il ripristino ne crea una nuova.
- **Segreti nelle pagine.** Il corpo è testo libero: va escluso dalle tracce esportate e dai dataset, oppure deve passare dallo stesso mascheramento dei segreti usato per le tracce.

**Rischi di prodotto.** Un secondo posto dove vivono i documenti, accanto a file e knowledge base; rischio di scarso uso se la proposta richiede troppi passaggi.

**Domande aperte.**

1. L'approvazione dal telefono è accettabile o serve `full`?
2. Le pagine devono comparire nella knowledge base per la ricerca, o restare separate?
3. Serve l'export in file fin dall'MVP?
4. Scadenza delle proposte `pending`: quanto?
5. Come si comporta l'assistente proattivo se una pagina ha già una proposta in attesa (sostituirla o accodarsi)?

## 9. Decisione proposta

Si propone di **non implementare subito** e di fare prima la misura del paragrafo 6 per un paio di settimane. Se le soglie sono raggiunte, si propone di procedere con l'opzione B (pagine in SQLite con revisioni, proposta e approvazione, blocco ottimistico), senza CRDT e senza cloud.

Motivi: riusa il modello «proposta, approva, scrivi» già presente in `crates/aiw-engine/src/store/proposals.rs` e `crates/aiw-engine/src/learning.rs`; una fonte unica funziona anche sul telefono accoppiato; il blocco ottimistico è semplice da provare con test; il costo è una migrazione e un gruppo di rotte, non una nuova infrastruttura. L'opzione A non copre le modifiche dell'utente né il telefono; la C risolve un problema che non c'è.

La decisione del proprietario è registrata nella sezione 10 e conclude la discovery #74: il documento contiene casi d'uso, modello dati e criteri per l'eventuale MVP.


## 10. Decisione del proprietario — 14 novembre 2026

Il proprietario ha approvato la proposta: misurare prima l’utilità per due settimane con i criteri del paragrafo 6; poi, se le soglie sono soddisfatte, procedere con l’opzione B (pagine locali in SQLite, revisioni, proposte e approvazione). Questa decisione conclude la discovery di #74. La misurazione e l’eventuale implementazione sono attività successive: il merge di questo documento non implementa le pagine.
