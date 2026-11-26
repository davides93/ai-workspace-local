# Catalogo aggiornabile

I modelli leggeri e potenti di peso aperto escono più in fretta delle versioni dell'app. Il catalogo incorporato (`crates/aiw-models/catalog/models.json`) resta la base; un file nella cartella dei dati ci aggiunge, o ci corregge, voci senza ricostruire l'app.

## Il file

`<cartella dati>/catalog-overlay.json`: un array di voci con gli stessi campi del catalogo (vedi `CatalogModel` in `crates/aiw-core/src/api.rs`). Una voce con un `id` già noto **sostituisce** quella incorporata, una nuova si **aggiunge**. Se il file manca non cambia nulla.

```json
[
  {
    "id": "nuovo-leggero-q4",
    "name": "Nuovo leggero Q4",
    "family": "nuovo",
    "publisher": "acme",
    "description": "Un modello piccolo e veloce.",
    "params_b": 4.0,
    "format": "gguf",
    "quant": "Q4_K_M",
    "size_bytes": 2600000000,
    "context_length": 32768,
    "capabilities": ["chat", "tools"],
    "license": "Apache-2.0",
    "source": { "kind": "hf", "repo": "acme/nuovo-GGUF", "file": "nuovo-Q4_K_M.gguf" },
    "min_memory_mb": 3900,
    "tags": ["agent"]
  }
]
```

## Cosa succede

- Le voci aggiunte compaiono in Modelli, si scaricano come le altre e le vedono il **consigliere**, la scelta automatica (`auto:*`), i ruoli per modello e l'assistente proattivo: il router dinamico sceglie fra tutto il catalogo, non solo fra le voci incorporate.
- Ogni voce è controllata da sola, così una voce sbagliata non fa perdere le altre (l'avviso va nel registro): `id` con lettere, cifre e `. _ -`; nome, famiglia, editore e licenza presenti; formato noto; repository `autore/nome` senza `..`; percorso del file relativo; dimensione, parametri, contesto e memoria minima maggiori di zero.
- Sono accettate solo voci scaricabili da Hugging Face. I modelli di sistema (Apple, Gemini Nano, Phi Silica) vengono dal sistema operativo e un file non li aggiunge né li sostituisce.
- Il file si rilegge a ogni richiesta del catalogo: niente riavvio.

## Cosa non c'è ancora

- Nessun aggiornamento automatico da un indirizzo in rete: il file si scrive a mano o lo aggiorna un'altra PR. Un catalogo scaricato da un server andrebbe firmato, ed è una decisione da prendere.
- Nessuna schermata per modificarlo.
- Le voci non vengono misurate: i punteggi (`suitability`) sono quelli scritti nella voce, altrimenti quelli ricavati da capacità e dimensione.
