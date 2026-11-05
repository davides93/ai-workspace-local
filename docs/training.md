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
