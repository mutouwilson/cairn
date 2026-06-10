"""LoRA fine-tune Qwen 2.5-1.5B-Instruct on the synth dataset.

Output format is the OpenAI-style tool-call message, so the trained model can
be served via any tool-calling-compatible runtime (vLLM, MLX, llama.cpp).
"""

from __future__ import annotations

import json
import logging
import math
import os
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import click
import orjson
import torch
from datasets import Dataset
from peft import LoraConfig, PeftModel, TaskType, get_peft_model
from rich.console import Console
from transformers import (
    AutoModelForCausalLM,
    AutoTokenizer,
    DataCollatorForLanguageModeling,
    Trainer,
    TrainingArguments,
)

from .prompts import SERVING_SYSTEM
from .schema import EXTRACTION_SCHEMA

log = logging.getLogger(__name__)
console = Console(stderr=True)

TOOL_NAME = "save_extraction"


@dataclass
class TrainConfig:
    base_model: str
    train_path: Path
    test_path: Path | None
    output_dir: Path
    epochs: int = 3
    batch_size: int = 4
    grad_accum: int = 4
    lr: float = 2e-4
    max_seq_len: int = 2048
    warmup_ratio: float = 0.03
    lora_r: int = 32
    lora_alpha: int = 64
    lora_dropout: float = 0.05
    target_modules: tuple[str, ...] = (
        "q_proj",
        "k_proj",
        "v_proj",
        "o_proj",
        "gate_proj",
        "up_proj",
        "down_proj",
    )
    seed: int = 42


def _format_record(rec: dict[str, Any]) -> dict[str, str]:
    """Render one training row as a chat-formatted text the model will see.

    Format matches Qwen's chat template + an explicit ``<tool_call>`` block,
    which is what llama-server / MLX and vLLM parse out of the assistant
    response to surface as a tool call.
    """
    note = rec["note"]
    extraction = rec["extraction"]
    tool_call = {
        "name": TOOL_NAME,
        "arguments": extraction,
    }
    messages = [
        {"role": "system", "content": SERVING_SYSTEM},
        {"role": "user", "content": note},
        {
            "role": "assistant",
            "content": "<tool_call>\n" + json.dumps(tool_call, ensure_ascii=False) + "\n</tool_call>",
        },
    ]
    return {"messages": messages}


def _load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("rb") as f:
        for line in f:
            if not line.strip():
                continue
            rows.append(orjson.loads(line))
    return rows


def _apply_chat_template(tokenizer, sample: dict[str, Any], max_len: int) -> dict[str, Any]:
    rendered = tokenizer.apply_chat_template(
        sample["messages"],
        tokenize=False,
        add_generation_prompt=False,
    )
    enc = tokenizer(
        rendered,
        truncation=True,
        max_length=max_len,
        padding=False,
        return_attention_mask=True,
    )
    enc["labels"] = enc["input_ids"].copy()
    return enc


def build_datasets(tokenizer, cfg: TrainConfig) -> tuple[Dataset, Dataset | None]:
    train_rows = [_format_record(r) for r in _load_jsonl(cfg.train_path)]
    train_ds = Dataset.from_list(train_rows).map(
        lambda b: _apply_chat_template(tokenizer, b, cfg.max_seq_len),
        remove_columns=["messages"],
        desc="tokenize train",
    )

    eval_ds: Dataset | None = None
    if cfg.test_path is not None:
        eval_rows = [_format_record(r) for r in _load_jsonl(cfg.test_path)]
        eval_ds = Dataset.from_list(eval_rows).map(
            lambda b: _apply_chat_template(tokenizer, b, cfg.max_seq_len),
            remove_columns=["messages"],
            desc="tokenize eval",
        )
    return train_ds, eval_ds


def run_training(cfg: TrainConfig) -> Path:
    cfg.output_dir.mkdir(parents=True, exist_ok=True)
    torch.manual_seed(cfg.seed)

    console.rule("[bold]Load base model")
    tokenizer = AutoTokenizer.from_pretrained(cfg.base_model, trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    dtype = torch.bfloat16 if torch.cuda.is_available() else torch.float32
    base = AutoModelForCausalLM.from_pretrained(
        cfg.base_model,
        trust_remote_code=True,
        torch_dtype=dtype,
    )
    base.config.use_cache = False

    console.rule("[bold]Attach LoRA")
    lora_cfg = LoraConfig(
        r=cfg.lora_r,
        lora_alpha=cfg.lora_alpha,
        lora_dropout=cfg.lora_dropout,
        bias="none",
        target_modules=list(cfg.target_modules),
        task_type=TaskType.CAUSAL_LM,
    )
    model = get_peft_model(base, lora_cfg)
    model.print_trainable_parameters()

    console.rule("[bold]Build datasets")
    train_ds, eval_ds = build_datasets(tokenizer, cfg)
    console.print(f"train rows = {len(train_ds)}; eval rows = {len(eval_ds) if eval_ds else 0}")

    collator = DataCollatorForLanguageModeling(tokenizer=tokenizer, mlm=False)

    args = TrainingArguments(
        output_dir=str(cfg.output_dir),
        num_train_epochs=cfg.epochs,
        per_device_train_batch_size=cfg.batch_size,
        per_device_eval_batch_size=cfg.batch_size,
        gradient_accumulation_steps=cfg.grad_accum,
        learning_rate=cfg.lr,
        bf16=torch.cuda.is_available(),
        logging_steps=20,
        save_steps=500,
        save_total_limit=3,
        warmup_ratio=cfg.warmup_ratio,
        eval_strategy="steps" if eval_ds is not None else "no",
        eval_steps=200 if eval_ds is not None else None,
        report_to=[],
        seed=cfg.seed,
        gradient_checkpointing=True,
        optim="adamw_torch_fused" if torch.cuda.is_available() else "adamw_torch",
    )

    trainer = Trainer(
        model=model,
        args=args,
        train_dataset=train_ds,
        eval_dataset=eval_ds,
        tokenizer=tokenizer,
        data_collator=collator,
    )

    console.rule("[bold]Train")
    started = time.time()
    trainer.train()
    elapsed = time.time() - started

    adapter_dir = cfg.output_dir / "adapter"
    trainer.model.save_pretrained(adapter_dir)
    tokenizer.save_pretrained(adapter_dir)

    manifest = {
        "base_model": cfg.base_model,
        "train_path": str(cfg.train_path),
        "test_path": str(cfg.test_path) if cfg.test_path else None,
        "epochs": cfg.epochs,
        "batch_size": cfg.batch_size,
        "grad_accum": cfg.grad_accum,
        "lr": cfg.lr,
        "max_seq_len": cfg.max_seq_len,
        "lora_r": cfg.lora_r,
        "lora_alpha": cfg.lora_alpha,
        "target_modules": list(cfg.target_modules),
        "elapsed_seconds": math.floor(elapsed),
        "torch_version": torch.__version__,
        "cuda_available": torch.cuda.is_available(),
    }
    (cfg.output_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))

    console.print(f"[green]LoRA saved → {adapter_dir}[/green]  ({elapsed:.0f}s)")
    return adapter_dir


@click.command()
@click.option("--train", "train_path", type=click.Path(exists=True, dir_okay=False, path_type=Path), required=True)
@click.option("--test", "test_path", type=click.Path(exists=True, dir_okay=False, path_type=Path), default=None)
@click.option("--base", "base_model", default="Qwen/Qwen2.5-1.5B-Instruct")
@click.option("--output", "output_dir", type=click.Path(file_okay=False, path_type=Path), required=True)
@click.option("--epochs", type=int, default=3)
@click.option("--batch-size", type=int, default=4)
@click.option("--grad-accum", type=int, default=4)
@click.option("--lr", type=float, default=2e-4)
@click.option("--max-seq-len", type=int, default=2048)
@click.option("--seed", type=int, default=42)
@click.option("--log-level", default="INFO")
def main(
    train_path: Path,
    test_path: Path | None,
    base_model: str,
    output_dir: Path,
    epochs: int,
    batch_size: int,
    grad_accum: int,
    lr: float,
    max_seq_len: int,
    seed: int,
    log_level: str,
) -> None:
    logging.basicConfig(level=log_level.upper(), format="%(asctime)s %(levelname)s %(message)s")
    cfg = TrainConfig(
        base_model=base_model,
        train_path=train_path,
        test_path=test_path,
        output_dir=output_dir,
        epochs=epochs,
        batch_size=batch_size,
        grad_accum=grad_accum,
        lr=lr,
        max_seq_len=max_seq_len,
        seed=seed,
    )
    os.makedirs(output_dir, exist_ok=True)
    run_training(cfg)


if __name__ == "__main__":
    main()
