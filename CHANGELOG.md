# Changelog

## Non rilasciato

- Workflow: passo `team` (`{kind: team, team, prompt}`) che affida il compito a un team e ne usa la risposta finale; accetta `retry`, `timeout_s` e `on_error`, si può usare dentro `foreach` e `parallel`, ha il suo selettore nell'editor dei workflow. Un team usato da un workflow non si può eliminare (409).
- Workflow: i passi `tool`, `agent` e `llm` accettano `retry` (0–5 nuovi tentativi), `retry_delay_s` (attesa, default 2 s), `timeout_s` (limite per tentativo) e `on_error: continue` (il passo resta fallito ma il run prosegue; l'errore è in `{{steps.<id>.error}}`). Opzioni modificabili dall'editor visuale. Se scade il limite di un passo `agent`, anche il run dell'agente viene annullato.
- Workflow durevoli: lo stato del run è salvato a ogni passo (tabella `workflow_state`) e all'avvio i run interrotti da un riavvio riprendono dal passo in corso, che ricomincia da capo (almeno una volta); un'approvazione in attesa viene richiesta di nuovo. Se il workflow è stato eliminato il run risulta `failed`. Audit `workflow.resume`.
- Approvazioni con scadenza: il passo `approval` accetta `timeout_s` e `default` (`deny`/`allow`) e l'impostazione `approvals.timeout_s` (0 = nessun limite) rifiuta le richieste dei tool degli agenti rimaste senza risposta, spiegando il timeout all'agente. L'audit registra `decision: "timeout"`. Modificabili dall'editor visuale.
- Workflow: passi `foreach` (un passo inline per ogni voce di un elenco, fino a 8 alla volta e 200 voci, con `{{item}}`, `{{item.campo}}`, `{{index}}`) e `parallel` (più passi insieme, output come oggetto JSON). Creabili e modificabili dall'editor visuale, con il passo inline in JSON.
- Workflow: passo `workflow` (sotto-workflow) con input, profondità massima 3. Il figlio è un run normale con un riferimento al padre (`parent_run_id`); l'output è quello dell'ultimo suo passo completato. Scelta del workflow dall'editor visuale.
- Workflow: segnaposto per l'output JSON dei passi (`{{steps.x.json}}`, `{{steps.x.json.a.b}}`, `{{steps.x.json.items.0.nome}}`; vale anche per un blocco ```json nel testo) e `{{steps.x.error}}`; li usano anche le `condition`. Il menu dei segnaposto dell'editor li propone.
- Checkpoint: rewind dei file di un run. Il checkpoint ricorda anche i file creati dal run (`.aiw-run.json`); `aiw rewind <run_id>` (con `--dry-run`), `GET /api/runs/{id}/checkpoint`, `POST /api/runs/{id}/rewind` e `GET /api/sessions/{id}/checkpoints` rimettono gli originali ed eliminano i file creati, solo dentro il workspace e mai con il run in corso (409); audit `checkpoint.rewind`. Nella chat compare «Ripristina i file» con la conferma che elenca cosa cambia.
- Team multi-agente: nuova pagina Team (editor con strategie supervisore, parallelo e catena, membri, budget e schema di risposta); un team si sceglie in chat come un agente. I membri lavorano con i limiti del padre, con budget condiviso, e la loro traccia (testo, tool, risultati) si apre dentro la scheda del subagente.
- Handoff: un agente può passare la conversazione a un altro (tool `handoff`), fino a 3 passaggi per messaggio, senza mai passare a una modalità più permissiva; la chat mostra una nota a ogni passaggio.
- Stato condiviso del team (tool `team_state`), visibile nell'Inspector.
- Agenti remoti A2A: registro in Integrazioni (token mascherato) più i dispositivi accoppiati; si usano come membri di un team (`remote:<id>`) o subagenti. Il client interroga il task fino alla fine, rispetta il timeout (`a2a.timeout_seconds`), annulla il task remoto se annulli il run e scrive ogni delega nel registro delle attività.
- A2A server: i task sono salvati nel database (`a2a_tasks`) e scadono dopo 24 ore, invece di stare in memoria. Il flusso `SendStreamingMessage` non c'è ancora.
- `DELETE /api/agents/{id}` risponde 409 se l'agente è usato da un team.

- Hook: nuovi eventi `SessionStart` (contesto di avvio nel prompt di sistema), `SubagentStart`, `SubagentStop` e `PreCompact`, anche nell'editor degli hook.
- Subagenti: sandbox e tool del subagente sono ristretti a quelli dell'agente padre (oltre alla modalità di approvazione, già limitata).
- Subagenti in parallelo: più chiamate `agent` nello stesso turno girano insieme, fino a `agents.max_parallel_subagents` (default 3, impostazione nella sezione Agenti).
- Output strutturato: un agente può avere uno schema JSON della risposta finale (campo «Risposta» nell'editor); la risposta viene validata, con un tentativo di correzione, e il JSON valido è nel risultato del run.
- Budget per run: limiti facoltativi di token, turni del modello e secondi, per agente o nelle Impostazioni (`agents.budget.*`), condivisi con i subagenti; a limite esaurito il run si chiude con un avviso invece di fallire.

## 0.4.0-rc.2 - release candidate

- Aggiornamenti automatici dalle release del repository pubblico su GitHub, con scelta del canale (`auto`, produzione/`stable`, `beta`, `alpha`): controllo all'avvio e ogni 6 ore, download con verifica SHA-256, installazione con l'installer del sistema (dmg, MSI, AppImage). Pannello in Impostazioni e indicatore nella barra laterale.
- macOS desktop: Apple Foundation Models (macOS 26+, Apple Silicon) come runtime `apple-fm`, tramite il helper Swift `aiw-apple-fm` incluso nell'app. La cancellazione di una richiesta ferma ora anche la generazione nativa.
- La release macOS usa Xcode 26 solo per il helper Swift e collega il runtime di clang; la pubblicazione crea la release come bozza con tutti i file e la pubblica alla fine (release immutabili). La `v0.4.0-rc.1` esiste senza file: usa la rc.2.
- Installazione automatica dei runtime: MLX LM e MLX VLM (pip in un ambiente Python isolato di AIW, Python 3.12 via Homebrew se manca), llama.cpp server e Ollama (Homebrew). Un clic dalla pagina Runtime o dalla scheda di un modello senza runtime pronto, con avanzamento e output in diretta.

Note complete: [docs/release-notes/v0.4.0-rc.2.md](docs/release-notes/v0.4.0-rc.2.md).

## 0.4.0-beta.1 - pre-release

- Model hub multi-provider (Hugging Face, ModelScope, Ollama, URL diretto) con downloader robusto.
- Inferenza: llama.cpp embedded con Metal/CUDA/Vulkan, supporto Ollama, probe hardware migliorato.
- Sicurezza dell'API locale e dei tool rafforzata; correzioni all'agent loop.
- Nuova UI (nuova architettura dell'informazione, palette comandi, layout mobile).
- Build non firmate per Linux, Windows, macOS, Android (APK arm64); iOS solo simulatore.

Note complete: [docs/release-notes/v0.4.0-beta.1.md](docs/release-notes/v0.4.0-beta.1.md).
