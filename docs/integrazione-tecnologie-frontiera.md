# Integrazione delle tecnologie di frontiera



## Integrazione tecnologie di frontiera — 13 novembre 2026

Pianificazione locale-first dall’analisi del 12/11; nessuna nuova funzionalità implementata. Verificare le specifiche primarie prima dell’integrazione. OpenBot è qui il candidato CopilotKit, non la suite RPA OpenBots: identificazione da confermare prima di lavoro specifico sul prodotto.

- [ ] #69 — AG-UI standard: endpoint RunAgentInput, eventi e approvazioni interoperabili
- [ ] #70 — Interoperabilità: guida e snippet OpenDots/CopilotKit con AIW
- [ ] #71 — Agenti persistenti: workspace, identità e credenziali isolati per agente
- [ ] #72 — Canali: contesto condiviso tra app, voce, Telegram, Slack ed email
- [ ] #73 — Browser: live view e presa di controllo umana per login e 2FA
- [ ] #74 — Spazi e pagine condivise: valutare editing agente-utente con revisioni
- [ ] #75 — Proattivi: validare morning brief e guidare i task in background
- [ ] #76 — Email: bozze revisionabili e invio soltanto dopo approvazione

Riutilizzo: #13 MLX Swift; #24 cifratura dei segreti/portachiavi; #23 policy condizionali per azione e audit (valutare estensione delle regole prima di introdurre CEL); #22 voce reale e mobile. Il morning brief esiste già: nuova attività di validazione/completamento, non nuova implementazione da zero.

## Dettaglio delle attività

### #69 — AG-UI standard: endpoint RunAgentInput, eventi e approvazioni interoperabili

Priorità: **P1**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Implementare POST /agui (o /agui/{agent}) → SSE, mappatura thread/sessione, messaggi inclusi allegati, stato, tool e contesto. Verificare prima la specifica primaria Events e il contratto RunAgentInput; aggiungere MESSAGES_SNAPSHOT, STATE_DELTA e STEP_* dove appropriato e human-in-the-loop interoperabile, mantenendo compatibilità CUSTOM con la UI attuale. Verificare autenticazione, scope, annullamento, riconnessione e ripresa senza duplicati.

## Accettazione e dipendenze
Dopo gli allegati della PR #68. Test con un client AG-UI reale e copertura delle approvazioni/ripresa.

Non risolta dalla PR #68.


### #70 — Interoperabilità: guida e snippet OpenDots/CopilotKit con AIW

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Verificare README e API primarie dei client e documentare configurazione modelli su /v1 e agenti su /agui, token, CORS, rete e limiti. Integrare esempi nella pagina Integrazioni. Compatibilità senza incorporare OpenDots/OpenBot come dipendenze.

## Accettazione e dipendenze
Dipende dall’endpoint AG-UI; esempio riproducibile con client reale.

Non risolta dalla PR #68.


### #71 — Agenti persistenti: workspace, identità e credenziali isolati per agente

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Ruolo e permessi persistenti, workspace dedicato, sandbox per agente e credenziali con ambito per agente. Integrare cifratura/portachiavi con #24 e isolamento con #23. Valutare container Docker/Podman solo opzionali su Linux, mantenendo il funzionamento locale senza container.

## Accettazione e dipendenze
Test di isolamento tra agenti, persistenza dopo riavvio, revoca e assenza di segreti in log/export.

Non risolta dalla PR #68.


### #72 — Canali: contesto condiviso tra app, voce, Telegram, Slack ed email

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Definire mappa identità/canale/sessione, collegamento esplicito degli account, cronologia condivisa e routing delle approvazioni. Gestire concorrenza, duplicati e separazione fra utenti, evitando unioni automatiche di contesti privati.

## Accettazione e dipendenze
Dipende dai canali di #21 / PR #68. Proseguire la stessa conversazione su due canali senza perdere contesto o duplicare azioni.

Non risolta dalla PR #68.


### #73 — Browser: live view e presa di controllo umana per login e 2FA

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Mostrare il browser CDP durante i task e consentire pausa, presa di controllo umana e restituzione all’agente. Login e 2FA effettuati dall’utente; credenziali e schermate sensibili protette. Coordinare con #18 e policy/audit #23.

## Accettazione e dipendenze
Test di esclusione reciproca agente/utente, ripresa del task e protezione dello stream.

Non risolta dalla PR #68.


### #74 — Spazi e pagine condivise: valutare editing agente-utente con revisioni

Priorità: **P3**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Discovery separata: valutare documenti vivi locali, modifica concorrente, cronologia, diff, approvazione prima del salvataggio e ripristino. Definire necessità e MVP prima di implementare. Non adottare lo stack cloud di OpenDots/OpenBot.

## Accettazione e dipendenze
Decisione documentata con casi d’uso, modello dati e criteri per eventuale MVP.

Non risolta dalla PR #68.


### #75 — Proattivi: validare morning brief e guidare i task in background

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Il morning brief programmato e Proposte esistono già: verificarli con fonti reali e su dispositivi. Completare osservazione, pausa, istruzioni durante il task e ripresa; preservare budget, orari di silenzio e approvazioni. Verificare messaggi vocali con #22.

## Accettazione e dipendenze
Test su desktop e telefono con sospensione/riavvio e nessuna azione sensibile senza approvazione.

Non risolta dalla PR #68.


### #76 — Email: bozze revisionabili e invio soltanto dopo approvazione

Priorità: **P2**. Roadmap #5. Origine: analisi «Integrare le tecnologie di frontiera», 12 novembre 2026, fornita dall’utente. Proposta, non implementazione; identificazioni e compatibilità dei prodotti esterni da verificare sulle fonti primarie prima dello sviluppo.

## Attività
Distinguere le risposte del canale email già introdotte dalla PR #68 da bozze proattive revisionabili. Preparare bozze da posta/calendario, mostrare destinatari, oggetto, corpo e allegati; inviare solo dopo approvazione esplicita del contenuto finale, con audit e protezione dai duplicati. Valutare OAuth dei provider separatamente.

## Accettazione e dipendenze
Dipende dai connettori/canali #21 e dal modello approvazioni #20; prova con casella dedicata e rifiuto/annullamento senza invio.

Non risolta dalla PR #68.


## PR #68



## Issue e roadmap

Closes #17
Closes #21

La knowledge base, la memoria e gli allegati di #17 sono presenti; #21 è completata nel codice dai team/client A2A già presenti e dai canali aggiunti qui. Restano le validazioni reali dichiarate nella PR.

Copertura parziale, senza chiusura automatica: #9, #12, #14, #18, #19, #20, #22, #23, #24, #25, #28. Restano rispettivamente verifiche hardware, applicazione gpu_layers/stime remote, embedding in-process e altri endpoint, strumenti/desktop residui, compatibilità MCP residua, requisiti agent-loop residui, voce reale/mobile, isolamento completo, cifratura/mDNS, firma/distribuzione e componenti/UI residui.

Nuove integrazioni future censite nella roadmap #5: #69, #70, #71, #72, #73, #74, #75, #76. Queste issue non sono risolte dalla presente PR.
