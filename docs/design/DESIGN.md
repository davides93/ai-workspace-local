# Design · AI Workspace Local

> Mockup interattivo: [`mockup/index.html`](mockup/index.html) (file unico, si apre offline) ·
> pubblicato anche come artifact privato. Il mockup **è la UI reale** (`apps/ui`) compilata con
> `npm run build:mockup`, che usa il `MockTransport` al posto del core.

## 1. Principi

1. **Stato prima del dettaglio**: runtime, modelli e run mostrano lo stato con forma e colore
   (LED, pill, meter) prima dei numeri.
2. **Il controllo è visibile**: modalità di approvazione, sandbox e kill switch sono sempre a
   portata (rail sinistro, intestazione della chat, pannello del run).
3. **Ogni layer è configurabile, senza schermate scritte a mano**: i form di runtime e
   impostazioni sono generati dai `settings_schema` dei manifest.
4. **Una UI per tutti i dispositivi**: rail laterale su desktop, tab bar in basso su telefono,
   pannelli che diventano sheet sotto i 1180 px e i 760 px.

## 2. Identità visiva: "rack di strumenti"

| Token | Chiaro | Scuro | Uso |
|---|---|---|---|
| `--bg` | `#f3f4f1` | `#131617` | fondo, ardesia con lieve dominante verde |
| `--surface` | `#ffffff` | `#1a1e1f` | pannelli, rail |
| `--accent` | `#c77700` | `#f2a93b` | **ambra da LED di stato**: azione primaria, elemento attivo, run in corso |
| `--ok` / `--warn` / `--crit` / `--info` | verde / ocra / rosso / blu | versioni luminose | solo semantica (stato, rischio) |
| Display | Bricolage Grotesque 650–750 | | titoli, nomi di runtime |
| Body | IBM Plex Sans 400–600 | | testo |
| Mono | IBM Plex Mono | | modelli, quantizzazioni, endpoint, argomenti dei tool |

Dettagli del dominio usati come contenuto: chip degli acceleratori (`cuda`, `metal`, `npu`…)
evidenziati quando sono presenti sul device, quantizzazioni (`Q4_K_M`), finestre di contesto,
tok/s, classi di device XS–XL, codice di pairing.

## 3. Architettura dell'informazione

| Gruppo | Sezione | Contenuto |
|---|---|---|
| Lavoro | **Workspace** | Sessioni · conversazione con reasoning, tool card, conferme, subagenti · pannello del run (runtime/modello, tok/s, contesto, piano) |
| | **Agenti** | Profili agente (prompt, tool, modalità, sandbox, turni) · profili di inferenza |
| | **Workflow** | Elenco con trigger · passi a timeline · sorgente JSON/YAML · esecuzioni |
| Inferenza | **Runtime** | Catalogo per tipo (sidecar, embedded, di sistema, remoti) · avvio/stop · impostazioni generate |
| | **Modelli** | Installati · catalogo con compatibilità per il device · download con progresso |
| | **Dispositivi** | Profilo hardware, classe del device, acceleratori · dispositivi accoppiati, pairing |
| Controllo | **Tool e MCP** | Server MCP (stdio/HTTP) · tool con rischio e on/off |
| | **Attività** | Audit trail filtrabile |
| | **Impostazioni** | Configurazione a strati con lo strato vincente evidenziato · tema |

## 4. Componenti

`StatusPill` (LED + etichetta), `Meter`, `chip`, `SchemaForm` (enum → select, numeri con
min/max → slider + campo, booleani → toggle, liste, segreti), `Drawer`, `Markdown` sicuro (senza
HTML), tool card espandibile, card di conferma con rischio, timeline dei passi del workflow,
segmented control, tabelle dati con numeri tabulari.

## 5. Flussi chiave

1. **Primo avvio**: rilevamento hardware → classe del device → catalogo filtrato per
   compatibilità → un clic per scaricare il modello consigliato → profilo "Bilanciato" pronto.
2. **Task con conferma**: l'utente scrive → l'agente pianifica (piano nel pannello) → la tool call
   a rischio produce una card "Serve la tua conferma" (anche nel pannello) → Consenti, Consenti
   sempre per la sessione oppure Nega → la decisione finisce nell'audit.
3. **Da chat a workflow**: un task riuscito si salva come workflow con trigger cron, intervallo o file.
4. **Telefono + PC**: pairing in LAN → il telefono vede i runtime del PC come `remote`.

## 6. Accessibilità

Focus visibile (`:focus-visible` in ambra), `aria-current` su navigazione e sessioni, ruoli
`dialog`/`alert`/`meter`, `prefers-reduced-motion` rispettato, contrasto verificato in entrambi i
temi, target di almeno 32 px.
