# Cosa fa l'assistente in background sul telefono

Vale per l'Assistente proattivo (`proactive.level` diverso da `off`). Sul computer l'app resta accesa nella barra delle
applicazioni; sul telefono no, e a decidere quando il codice può girare è il sistema, non l'app.

## Cosa gira, su tutte le piattaforme

Un solo punto di ingresso, `Engine::proactive_background_tick` (`crates/aiw-engine/src/background.rs`), raggiungibile
anche con `POST /api/proactive/background-tick`. Riusa lo stesso battito del computer (orario di silenzio, budget
giornaliero, un controllo alla volta, proposte idempotenti) con un tetto duro di 25 secondi e una ragione:
`interval` (controllo periodico), `briefing` (riepilogo del mattino) o `shortcut` (lanciato da un Comando). Restituisce un
riepilogo corto con le notifiche da mostrare.

Senza un modello locale sul telefono:

1. fa le parti che non servono al modello (promemoria scaduti, riepilogo del mattino dai dati, connettori accesi);
2. il filtro che sceglie cosa vale una proposta lo affida a un dispositivo accoppiato raggiungibile, che riceve il
   contesto compatto (titoli e brevi frasi, non file); la risposta torna sul telefono e la proposta si scrive lì;
3. se nessun dispositivo risponde, salta il filtro e registra il motivo (`TickResult.model_skipped`, visibile in
   `GET /api/proactive/status`). Non si carica mai un modello di nascosto.

## Android

| Cosa | Come |
| --- | --- |
| Controllo periodico | WorkManager, lavoro periodico di almeno 15 minuti (il sistema non permette meno). Con `proactive.only_on_power` il vincolo è «in carica». |
| Riepilogo del mattino | lavoro a parte, riprogrammato all'ora `proactive.briefing_time`. |
| «Controlla adesso» | lavoro accelerato una tantum. Prima di Android 12 WorkManager lo esegue come breve servizio in primo piano. |
| Notifiche | canale «Assistente», notifica locale che apre `#/inbox`. Il permesso (Android 13+) si chiede solo quando accendi l'assistente. |
| Servizio sempre acceso | no. |

Limiti: Doze e il risparmio energetico possono rimandare i lavori anche di ore; le app «limitate» dall'utente o dal
produttore del telefono non vengono eseguite. L'orario non è preciso.

## iPhone e iPad

| Cosa | Come |
| --- | --- |
| Controllo periodico | `BGAppRefreshTask` e `BGProcessingTask` (identificatori in `BGTaskSchedulerPermittedIdentifiers`, modalità `fetch` e `processing`). |
| Riepilogo del mattino | notifica locale giornaliera all'ora `proactive.briefing_time`, riprogrammata ogni volta che l'app gira. |
| Comandi | due azioni App Intents: «Controlla adesso» e «Riepilogo di oggi». |
| Notifiche | `UNUserNotificationCenter`, permesso chiesto solo quando accendi l'assistente. |

**iOS decide quando e se far partire il lavoro in background.** Dipende da quanto usi l'app, dalla batteria, dalla rete
e dal Risparmio energia; se non apri l'app per giorni può non partire mai. Non c'è modo di ottenere un orario preciso con
i soli compiti in background. L'unico modo è un Comando con automazione «Ora del giorno» che esegue «Riepilogo di
oggi»: l'app lo spiega in tre passi nelle Impostazioni, «Assistente proattivo», «In background».

La notifica locale ricorrente del mattino invita sempre ad aprire l'app per il riepilogo aggiornato: non
ripete riepiloghi salvati nei giorni precedenti. Il testo del riepilogo viene notificato solo da un controllo fresco.

## Cosa non è stato provato

Il 12 novembre 2026 sono riuscite le build locali Android arm64 debug (APK) e iOS arm64 debug
(archivio senza firma), includendo Kotlin, Swift e shell Rust. Restano da provare esecuzione su telefono,
sospensione/riavvio, permessi, notifiche e Comandi Rapidi. La CI ordinaria non compila ancora i target mobili.

Comandi di verifica dalla cartella `apps/desktop`:

```sh
npx tauri android init --ci
# Applicare Kotlin 2.4.0 ai progetti generati come nel passo Build AAB di .github/workflows/store.yml.
npx tauri android build --debug --target aarch64 --apk --ci
npx tauri ios init --ci
npx tauri ios build --debug --target aarch64 --no-sign --archive-only --ci
```

Su Xcode 27.2 il wrapper Tauri non ha rilevato l'SDK del simulatore, pur elencato da Xcode:
la verifica completa è stata fatta sul target dispositivo, senza installazione né pubblicazione.
`cargo test -p aiw-engine`: 272 test passati, incluso il timeout che deve consumare budget e rilasciare il lock.
`cargo clippy -p aiw-engine --all-targets`: passato, con avvisi preesistenti nelle dipendenze interne.

## I canali (Telegram, Slack, email) sul telefono

I canali hanno bisogno di una connessione sempre aperta (long polling, WebSocket, controllo della casella) e quindi non
possono lavorare in background su un telefono: sul telefono restano «non disponibili» finché la shell non chiama
`Engine::set_channels_foreground(true)` (app in primo piano). Questo collegamento nella shell mobile non è ancora fatto:
oggi sul telefono i canali non partono. Sul computer partono con l'app (`Engine::channels_start()`) o con `aiw serve`.
