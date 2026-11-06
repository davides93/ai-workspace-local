# Addestramento locale

AIW **orchestra** il fine-tuning, non lo esegue in Rust: i trainer sono programmi Python esterni
(mlx-lm su Apple Silicon, TRL/PEFT o Unsloth su GPU CUDA) che AIW installa nel proprio ambiente
(`~/.aiw/pyenv`, solo `pip`, mai `curl | sh`) e lancia come processi supervisionati, annullabili,
con i log salvati su file. Questa guida dice cosa serve, quali comandi usare e quali sono i limiti.

## Prerequisiti

| Trainer | Dispositivo | Metodi | Rilevamento |
|---|---|---|---|
| `mlx-lm-lora` (mlx-lm) | Mac con Apple Silicon | `sft` | modulo Python `mlx_lm` |
| `trl` (TRL + PEFT) | Linux o Windows con GPU CUDA | `sft`, `dpo`, `grpo` | `trl`, `peft`, `transformers`, `datasets` |
| `unsloth` (facoltativo) | Linux o Windows con GPU CUDA | `sft`, `dpo` | `unsloth`, `trl`, `peft` |

- Python 3.10 o successivo (se manca, su macOS AIW installa Python 3.12 con Homebrew).
- Memoria: per un fine-tuning LoRA servono, indicativamente, 8 GB (modello fino a ~1,5B), 16 GB
  (~4B), 32 GB (~8B), 48 GB (~14B) di memoria unificata su Mac, o la stessa quantità di VRAM con
  TRL (6, 12, 24, 40 GB; Unsloth ne usa meno). DPO richiede circa 1,5 volte quella memoria, GRPO 2
  volte e un dispositivo di classe **L** o superiore.
- I modelli base sono quelli che il trainer sa leggere: cartelle MLX per mlx-lm, safetensors o un
  repository Hugging Face per TRL. Un file GGUF non si addestra.

## `aiw train doctor`

```
aiw train doctor
```

Mostra il dispositivo (classe, memoria, acceleratori), lo stato di ogni trainer (`ready`,
`missing` se si potrebbe usare ma non è installato, `unsupported` con il motivo: piattaforma,
acceleratore o memoria) e cosa si può addestrare qui, ad esempio «LoRA SFT fino a ~4B su questo
Mac». Se il dispositivo non può addestrare, indica i dispositivi accoppiati che potrebbero (la
classe, la memoria e gli acceleratori dei dispositivi vengono letti al pairing: quelli accoppiati
prima di questa versione vanno accoppiati di nuovo).

```
aiw train install mlx-lm-lora     # pip nell'ambiente di AIW, con l'output in diretta
```

API: `GET /api/trainers`, `GET /api/train/doctor`, `GET /api/trainers/{id}/install-plan`,
`POST /api/trainers/{id}/install` (avvia) e `GET /api/trainers/{id}/install` (avanzamento).

## Adapter LoRA in un profilo di inferenza

Un profilo può nominare uno o più adapter con `params.adapters`, una lista di
`{ "path": "…", "scale": 0.8 }` (`scale` facoltativa; il campo non viene scritto se la lista è vuota).
La forma scelta è il **percorso**: l'adapter registrato da AIW (registro degli adapter) espone il suo
`path` e si incolla qui. Il campo «Adapter LoRA» dell'editor dei profili e `aiw profile --adapter <percorso>`
(ripetibile) fanno lo stesso.

| Runtime | Come viene caricato | Formato |
|---|---|---|
| llama.cpp server | `--lora <file>` (`--lora-scaled <file> <scala>` con la scala), fino a 8 adapter | un file `.gguf` |
| MLX LM | `--adapter-path <cartella>`, un solo adapter, senza scala | cartella con `adapters.safetensors` e `adapter_config.json` |
| Ollama | non supportato dal profilo: crea un modello con `ADAPTER <percorso>` nel Modelfile (`ollama create`) e usa quello | — |
| Gli altri runtime e i motori in-process | non supportato | — |

- Un runtime che non carica adapter rifiuta il profilo **quando lo salvi** (errore con cosa fare),
  non al primo messaggio. Lo stesso per un percorso che non esiste o non ha il formato atteso (un adapter
  PEFT dato a llama.cpp: il messaggio indica `convert_lora_to_gguf.py` di llama.cpp).
- Gli adapter si caricano all'avvio del sidecar: se il profilo cambia adapter (o li toglie), AIW riavvia il
  sidecar, come già fa per il contesto. Se il server è già attivo fuori da AIW non si può cambiare adapter:
  fermalo e lascia che lo avvii AIW.
- vLLM (`--enable-lora --lora-modules`) non è collegato: richiede di scegliere l'adapter per nome a ogni
  richiesta, cosa che i profili non esprimono.

## SFT dalle tue conversazioni

Il percorso completo, dalla chat all'adapter:

1. **Dai un pollice in su** alle risposte buone (sotto la risposta finale dell'assistente). Una sessione con
   un pollice in giù non entra mai nel dataset.
2. **Crea il dataset**. Le sorgenti (`--source`, ripetibile) sono `feedback` (sessioni con un pollice in su,
   il predefinito), `succeeded` (sessioni il cui ultimo run è riuscito) ed `eval` (i task superati dei report
   indicati con `--eval-report`; un solo turno: prompt e risposta finale). Filtri: `--agent`, `--since`,
   `--until`, `--limit`. I segreti (chiavi, token, valori dei server MCP) sono mascherati come
   nell'export delle traiettorie. Servono **almeno 10 esempi**; il 10% va nel file di validazione.
3. **Addestra**.

```
aiw train sft --base Qwen/Qwen3-4B --name "Operator v1"
aiw train sft --base Qwen3-4B-MLX --source feedback --source succeeded --agent operator --since 2026-01-01
aiw train sft --base Qwen/Qwen3-4B --dataset ds_… --iters 200 --rank 16 --lr 2e-4 --batch 2
aiw train sft --base Qwen/Qwen3-4B --eval esempio --eval-profile default
```

`--base` è un modello locale (id o nome, non GGUF) oppure un repository Hugging Face che il trainer
scarica. Senza `--trainer` si usa il primo pronto che conosce il metodo. L'avanzamento e il log
scorrono nel terminale; Ctrl+C interrompe l'addestramento. Un solo addestramento alla volta per
dispositivo. I parametri non indicati usano i predefiniti del trainer (3 epoche, rango 8 con mlx-lm
e 16 con TRL, learning rate 2e-4 con TRL).

4. **Usa l'adapter**. A fine corsa `aiw adapters` lo elenca con il percorso. mlx-lm produce una cartella
   che si usa subito (`aiw profile --adapter <cartella>` su un profilo `mlx-lm`, oppure «Usa in un
   profilo» nella pagina Addestramento). TRL produce un adapter PEFT: per llama.cpp va convertito in
   GGUF con `convert_lora_to_gguf.py`, che AIW non esegue.

Con `--eval <suite> --eval-profile <profilo>` a fine corsa AIW esegue la suite con il modello del
profilo com'è e poi con l'adapter (un profilo temporaneo), e scrive la differenza nelle metriche
del job. Funziona se il runtime del profilo carica adapter di quel formato; altrimenti il confronto
riporta l'errore e l'adapter resta registrato.

Comandi: `aiw train jobs`, `aiw train logs <id>`, `aiw train cancel <id>`, `aiw train datasets`,
`aiw adapters`, `aiw adapters rm <id>`. API: `POST/GET /api/datasets`, `POST/GET /api/train/jobs`,
`GET /api/train/jobs/{id}/log`, `POST /api/train/jobs/{id}/cancel`, `GET /api/adapters`.
Nella UI la pagina **Addestramento** (`#/training`) mostra il dispositivo, i trainer, i dataset,
gli addestramenti con il log e gli adapter.

### Limiti

- Nessun trainer è stato provato in CI: i test usano un `python` finto. La prima esecuzione con
  mlx-lm, TRL o Unsloth veri può scoprire differenze nei formati o negli argomenti.
- I dati sono in formato chat (`{"messages": [...]}`), con gli argomenti dei tool come oggetti JSON.
- Un adapter si registra solo se la cartella ha i file attesi (`adapters.safetensors` e
  `adapter_config.json` per MLX; `adapter_config.json` e i pesi per PEFT).
- I log e i file di configurazione dei job stanno in `~/.aiw/train/<id>/`; il file di configurazione
  per gli script TRL è leggibile solo dall'utente.
