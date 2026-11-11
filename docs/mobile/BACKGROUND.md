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

La notifica del mattino compare comunque all'ora giusta, ma il suo testo è quello preparato all'ultimo avvio in
background o in primo piano: se iOS non ha fatto girare nulla, dice di aprire l'app.

## Cosa non è stato provato

Il codice Kotlin (`Background.kt`), Swift (`Background.swift`) e della shell Tauri (`apps/desktop/src-tauri/src/background.rs`)
non è stato compilato né eseguito su un telefono: lo verificano la CI e una prova a mano su un dispositivo vero. Il
motore (`aiw-engine`, `aiw-server`) e l'interfaccia sono invece coperti da test.
