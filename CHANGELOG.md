# Changelog

## Non rilasciato

- Hook: nuovi eventi `SessionStart` (contesto di avvio nel prompt di sistema), `SubagentStart`, `SubagentStop` e `PreCompact`, anche nell'editor degli hook.
- Subagenti: sandbox e tool del subagente sono ristretti a quelli dell'agente padre (oltre alla modalità di approvazione, già limitata).

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
