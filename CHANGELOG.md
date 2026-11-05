# Changelog

## Non rilasciato

- Hook: nuovi eventi `SessionStart` (contesto di avvio nel prompt di sistema), `SubagentStart`, `SubagentStop` e `PreCompact`, anche nell'editor degli hook.
- Subagenti: sandbox e tool del subagente sono ristretti a quelli dell'agente padre (oltre alla modalità di approvazione, già limitata).
- Subagenti in parallelo: più chiamate `agent` nello stesso turno girano insieme, fino a `agents.max_parallel_subagents` (default 3, impostazione nella sezione Agenti).
- Output strutturato: un agente può avere uno schema JSON della risposta finale (campo «Risposta» nell'editor); la risposta viene validata, con un tentativo di correzione, e il JSON valido è nel risultato del run.
- Budget per run: limiti facoltativi di token, turni del modello e secondi, per agente o nelle Impostazioni (`agents.budget.*`), condivisi con i subagenti; a limite esaurito il run si chiude con un avviso invece di fallire.
- Export delle traiettorie: `aiw traces export` e `GET /api/traces/export` scrivono una riga JSON per sessione (messaggi in formato OpenAI, run, decisioni sulle approvazioni), filtrabili per agente, date, stato e numero; i segreti sono mascherati di default e si possono escludere percorsi.
- Feedback sulle risposte: pollice su/giù (con nota facoltativa dopo un pollice giù) sotto la risposta finale dell'assistente, salvato per messaggio (`PUT/DELETE /api/sessions/{id}/messages/{mid}/feedback`, `GET /api/sessions/{id}/feedback`) ed esportato con le traiettorie (`feedback`, filtro `--rated up|down|any`). Il messaggio salvato dell'assistente ha ora lo stesso id del messaggio trasmesso in diretta.
- Valutazione degli agenti: nuovo crate `aiw-eval` con suite YAML/JSON (task, file iniziali, grader `file_exists`, `file_contains`, `output_matches`, `json_schema`, `command`, `judge` con un modello come giudice), `aiw eval run <file|id>` con `--repeat`, `--keep` e `--out`, report salvati (`aiw eval list`) e una suite di esempio in `docs/evals/esempio.yaml`. Ogni task gira in una cartella temporanea, senza conferme; le suite si leggono da `~/.aiw/evals`, mai dalla cartella del workspace. `aiw eval compare <A> <B>` confronta due report (differenze per task e totali) e `aiw eval run --profile a --profile b` esegue la suite con più profili e li confronta.

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
