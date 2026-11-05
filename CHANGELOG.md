# Changelog

## Non rilasciato

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
