# Changelog

## Non rilasciato

- Workflow: i passi `tool`, `agent` e `llm` accettano `retry` (0–5 nuovi tentativi), `retry_delay_s` (attesa, default 2 s), `timeout_s` (limite per tentativo) e `on_error: continue` (il passo resta fallito ma il run prosegue; l'errore è in `{{steps.<id>.error}}`). Opzioni modificabili dall'editor visuale. Se scade il limite di un passo `agent`, anche il run dell'agente viene annullato.
- Workflow durevoli: lo stato del run è salvato a ogni passo (tabella `workflow_state`) e all'avvio i run interrotti da un riavvio riprendono dal passo in corso, che ricomincia da capo (almeno una volta); un'approvazione in attesa viene richiesta di nuovo. Se il workflow è stato eliminato il run risulta `failed`. Audit `workflow.resume`.
- Approvazioni con scadenza: il passo `approval` accetta `timeout_s` e `default` (`deny`/`allow`) e l'impostazione `approvals.timeout_s` (0 = nessun limite) rifiuta le richieste dei tool degli agenti rimaste senza risposta, spiegando il timeout all'agente. L'audit registra `decision: "timeout"`. Modificabili dall'editor visuale.
- Workflow: passi `foreach` (un passo inline per ogni voce di un elenco, fino a 8 alla volta e 200 voci, con `{{item}}`, `{{item.campo}}`, `{{index}}`) e `parallel` (più passi insieme, output come oggetto JSON). Creabili e modificabili dall'editor visuale, con il passo inline in JSON.

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
