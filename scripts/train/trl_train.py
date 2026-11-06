#!/usr/bin/env python3
"""Fine-tuning LoRA per AI Workspace Local con TRL e PEFT (o Unsloth, se installato).

AIW lancia questo script con un file di configurazione JSON:

    python trl_train.py --config <cartella del job>/config.json

Il file di configurazione contiene:
    method        "sft" o "dpo" (poi "grpo")
    backend       "trl" oppure "unsloth"
    base_model    cartella locale o repository Hugging Face del modello base
    dataset_dir   cartella con train.jsonl e valid.jsonl
    output_dir    dove scrivere l'adapter (cartella PEFT) e metrics.json
    work_dir      cartella di lavoro per i checkpoint intermedi
    epochs, iters, learning_rate, rank, batch_size   facoltativi

Le righe che iniziano con `AIW_PROGRESS ` sono JSON (`step`, `total`, `loss`, `val_loss`) e AIW le usa
per la barra di avanzamento. Tutto il resto finisce nel log del job.
"""

import argparse
import json
import os
import sys
import time

DEFAULT_EPOCHS = 3.0
DEFAULT_LR = 2e-4
DEFAULT_RANK = 16
DEFAULT_BATCH = 2


def progress(step, total, loss=None, val_loss=None):
    """Una riga che AIW legge per l'avanzamento."""
    data = {"step": int(step), "total": int(total)}
    if loss is not None:
        data["loss"] = float(loss)
    if val_loss is not None:
        data["val_loss"] = float(val_loss)
    print("AIW_PROGRESS " + json.dumps(data), flush=True)


def load_jsonl(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def dataset_files(cfg):
    base = cfg["dataset_dir"]
    train = os.path.join(base, "train.jsonl")
    valid = os.path.join(base, "valid.jsonl")
    return train, (valid if os.path.exists(valid) else None)


def write_metrics(cfg, metrics):
    path = cfg.get("metrics_path") or os.path.join(cfg["output_dir"], "metrics.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(metrics, f, indent=2)


def make_progress_callback(TrainerCallback):
    class AiwProgress(TrainerCallback):
        def __init__(self):
            self.last_loss = None
            self.last_val = None
            self.history = []

        def on_log(self, args, state, control, logs=None, **kwargs):
            logs = logs or {}
            loss = logs.get("loss")
            val = logs.get("eval_loss")
            if loss is not None:
                self.last_loss = loss
            if val is not None:
                self.last_val = val
            if loss is not None or val is not None:
                progress(state.global_step, state.max_steps, loss, val)

    return AiwProgress()


def load_model_for_peft(cfg, rank):
    """(modello, tokenizer, peft_config): con Unsloth il modello ha già i suoi adapter LoRA."""
    from peft import LoraConfig
    from transformers import AutoModelForCausalLM, AutoTokenizer

    name = cfg["base_model"]
    peft_config = LoraConfig(
        r=rank,
        lora_alpha=2 * rank,
        lora_dropout=0.05,
        target_modules="all-linear",
        task_type="CAUSAL_LM",
    )
    if cfg.get("backend") == "unsloth":
        try:
            from unsloth import FastLanguageModel

            model, tokenizer = FastLanguageModel.from_pretrained(name, max_seq_length=cfg.get("max_seq_length", 4096), load_in_4bit=True)
            model = FastLanguageModel.get_peft_model(
                model,
                r=rank,
                lora_alpha=2 * rank,
                target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
            )
            return model, tokenizer, None
        except ImportError:
            print("Unsloth non è installato: uso TRL con PEFT.", flush=True)
    import torch

    dtype = torch.bfloat16 if torch.cuda.is_available() and torch.cuda.is_bf16_supported() else torch.float16
    tokenizer = AutoTokenizer.from_pretrained(name)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
    model = AutoModelForCausalLM.from_pretrained(name, torch_dtype=dtype)
    return model, tokenizer, peft_config


def common_args(cfg):
    """Argomenti comuni delle configurazioni di TRL, dal file di configurazione di AIW."""
    args = {
        "output_dir": cfg.get("work_dir") or os.path.join(cfg["output_dir"], "_work"),
        "per_device_train_batch_size": int(cfg.get("batch_size") or DEFAULT_BATCH),
        "learning_rate": float(cfg.get("learning_rate") or DEFAULT_LR),
        "logging_steps": 1,
        "save_strategy": "no",
        "report_to": [],
    }
    if cfg.get("iters"):
        args["max_steps"] = int(cfg["iters"])
    else:
        args["num_train_epochs"] = float(cfg.get("epochs") or DEFAULT_EPOCHS)
    return args


def run_sft(cfg):
    from datasets import load_dataset
    from transformers import TrainerCallback
    from trl import SFTConfig, SFTTrainer

    rank = int(cfg.get("rank") or DEFAULT_RANK)
    train_file, valid_file = dataset_files(cfg)
    files = {"train": train_file}
    if valid_file:
        files["validation"] = valid_file
    data = load_dataset("json", data_files=files)
    model, tokenizer, peft_config = load_model_for_peft(cfg, rank)
    args = common_args(cfg)
    if valid_file:
        args["eval_strategy"] = "epoch" if not cfg.get("iters") else "no"
    callback = make_progress_callback(TrainerCallback)
    trainer = SFTTrainer(
        model=model,
        args=SFTConfig(**args),
        train_dataset=data["train"],
        eval_dataset=data["validation"] if valid_file else None,
        processing_class=tokenizer,
        peft_config=peft_config,
        callbacks=[callback],
    )
    started = time.time()
    result = trainer.train()
    trainer.save_model(cfg["output_dir"])
    tokenizer.save_pretrained(cfg["output_dir"])
    metrics = {
        "train_loss": callback.last_loss if callback.last_loss is not None else result.training_loss,
        "steps": int(trainer.state.global_step),
        "duration_s": round(time.time() - started, 1),
    }
    if callback.last_val is not None:
        metrics["val_loss"] = callback.last_val
    write_metrics(cfg, metrics)


def as_preference_row(row):
    """Una riga di AIW (`prompt` = messaggi, `chosen` e `rejected` = testo) nel formato conversazionale di TRL."""
    return {
        "prompt": row["prompt"],
        "chosen": [{"role": "assistant", "content": row["chosen"]}],
        "rejected": [{"role": "assistant", "content": row["rejected"]}],
    }


def run_dpo(cfg):
    from datasets import Dataset
    from transformers import TrainerCallback
    from trl import DPOConfig, DPOTrainer

    rank = int(cfg.get("rank") or DEFAULT_RANK)
    train_file, valid_file = dataset_files(cfg)
    train = Dataset.from_list([as_preference_row(r) for r in load_jsonl(train_file)])
    valid = Dataset.from_list([as_preference_row(r) for r in load_jsonl(valid_file)]) if valid_file else None
    model, tokenizer, peft_config = load_model_for_peft(cfg, rank)
    args = common_args(cfg)
    args["beta"] = float(cfg.get("beta") or 0.1)
    if valid is not None:
        args["eval_strategy"] = "epoch" if not cfg.get("iters") else "no"
    callback = make_progress_callback(TrainerCallback)
    # Con un adapter LoRA il modello di riferimento è il modello base senza adapter: nessuna copia in memoria.
    trainer = DPOTrainer(
        model=model,
        args=DPOConfig(**args),
        train_dataset=train,
        eval_dataset=valid,
        processing_class=tokenizer,
        peft_config=peft_config,
        callbacks=[callback],
    )
    started = time.time()
    result = trainer.train()
    trainer.save_model(cfg["output_dir"])
    tokenizer.save_pretrained(cfg["output_dir"])
    metrics = {
        "train_loss": callback.last_loss if callback.last_loss is not None else result.training_loss,
        "steps": int(trainer.state.global_step),
        "duration_s": round(time.time() - started, 1),
    }
    if callback.last_val is not None:
        metrics["val_loss"] = callback.last_val
    write_metrics(cfg, metrics)


METHODS = {"sft": run_sft, "dpo": run_dpo}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="file di configurazione JSON scritto da AIW")
    args = parser.parse_args()
    with open(args.config, encoding="utf-8") as f:
        cfg = json.load(f)
    method = cfg.get("method", "sft")
    if method not in METHODS:
        sys.exit(f"metodo non supportato da questo script: {method}")
    os.makedirs(cfg["output_dir"], exist_ok=True)
    METHODS[method](cfg)
    print("Addestramento completato.", flush=True)


if __name__ == "__main__":
    main()
