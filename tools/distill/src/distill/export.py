"""Merge LoRA into the base model and export to a serving-ready format.

Two export targets:

* **MLX 4-bit** (macOS, Apple Silicon) — produces a directory the `mlx_lm`
  server can load directly. Tested on M2 Pro / M2 Max / M3 Max.
* **GGUF / Q4_K_M** (everywhere) — for llama.cpp. Requires `llama.cpp` cloned
  somewhere; we shell out to `convert-hf-to-gguf.py` and `quantize`.
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Literal

import click
import torch
from peft import PeftModel
from rich.console import Console
from transformers import AutoModelForCausalLM, AutoTokenizer

log = logging.getLogger(__name__)
console = Console(stderr=True)


def _read_base(run_dir: Path) -> str:
    manifest = run_dir / "manifest.json"
    if manifest.exists():
        return json.loads(manifest.read_text())["base_model"]
    raise click.ClickException(f"manifest missing at {manifest}")


def merge_adapter(run_dir: Path, merged_dir: Path) -> None:
    base_id = _read_base(run_dir)
    console.rule(f"[bold]Merge LoRA into {base_id}")
    tokenizer = AutoTokenizer.from_pretrained(run_dir / "adapter", trust_remote_code=True)
    base = AutoModelForCausalLM.from_pretrained(
        base_id,
        trust_remote_code=True,
        torch_dtype=torch.bfloat16 if torch.cuda.is_available() else torch.float32,
    )
    model = PeftModel.from_pretrained(base, run_dir / "adapter")
    merged = model.merge_and_unload()
    merged_dir.mkdir(parents=True, exist_ok=True)
    merged.save_pretrained(merged_dir, safe_serialization=True)
    tokenizer.save_pretrained(merged_dir)
    console.print(f"[green]merged HF model → {merged_dir}[/green]")


def export_mlx(merged_dir: Path, output_dir: Path, quant: int) -> None:
    if sys.platform != "darwin":
        raise click.ClickException("MLX export is macOS-only; use --target gguf elsewhere")
    try:
        from mlx_lm.convert import convert  # type: ignore
    except ImportError as exc:
        raise click.ClickException(
            "install the mlx extra: `pip install 'cairn-distill[mlx]'`"
        ) from exc

    console.rule(f"[bold]Convert → MLX (Q{quant})")
    output_dir.parent.mkdir(parents=True, exist_ok=True)
    convert(
        hf_path=str(merged_dir),
        mlx_path=str(output_dir),
        quantize=True,
        q_bits=quant,
        q_group_size=64,
    )
    console.print(f"[green]MLX model → {output_dir}[/green]")


def export_gguf(
    merged_dir: Path,
    output_path: Path,
    quant: str,
    llama_cpp_dir: Path,
) -> None:
    convert_script = llama_cpp_dir / "convert_hf_to_gguf.py"
    quantize_bin = llama_cpp_dir / "build" / "bin" / "llama-quantize"
    if not convert_script.exists() or not quantize_bin.exists():
        raise click.ClickException(
            f"llama.cpp tools not found in {llama_cpp_dir}; "
            "build llama.cpp first and pass --llama-cpp-dir"
        )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    f16_path = output_path.with_suffix(".f16.gguf")

    console.rule(f"[bold]Convert HF → GGUF (f16)")
    subprocess.check_call(
        [
            sys.executable,
            str(convert_script),
            str(merged_dir),
            "--outfile",
            str(f16_path),
            "--outtype",
            "f16",
        ]
    )

    console.rule(f"[bold]Quantize GGUF → {quant}")
    subprocess.check_call([str(quantize_bin), str(f16_path), str(output_path), quant])
    if f16_path.exists():
        os.remove(f16_path)
    console.print(f"[green]GGUF model → {output_path}[/green]")


@click.command()
@click.option("--run", "run_dir", type=click.Path(file_okay=False, exists=True, path_type=Path), required=True)
@click.option("--output", "output", type=click.Path(path_type=Path), required=True)
@click.option(
    "--target",
    type=click.Choice(["mlx", "gguf"]),
    default="mlx",
    help="serving format. mlx → Apple Silicon, gguf → llama.cpp anywhere.",
)
@click.option("--quant", default="4", help="bits (mlx) or GGUF quant string (gguf, e.g. Q4_K_M)")
@click.option(
    "--llama-cpp-dir",
    type=click.Path(file_okay=False, exists=False, path_type=Path),
    default=None,
    help="path to a built llama.cpp checkout (required for --target gguf)",
)
@click.option("--keep-merged", is_flag=True, help="don't delete the intermediate merged HF dir")
@click.option("--log-level", default="INFO")
def main(
    run_dir: Path,
    output: Path,
    target: Literal["mlx", "gguf"],
    quant: str,
    llama_cpp_dir: Path | None,
    keep_merged: bool,
    log_level: str,
) -> None:
    logging.basicConfig(level=log_level.upper(), format="%(asctime)s %(levelname)s %(message)s")
    merged_dir = run_dir / "merged"
    if not merged_dir.exists():
        merge_adapter(run_dir, merged_dir)

    if target == "mlx":
        export_mlx(merged_dir, output, int(quant))
    else:
        if llama_cpp_dir is None:
            raise click.ClickException("--llama-cpp-dir is required for gguf target")
        export_gguf(merged_dir, output, quant, llama_cpp_dir)

    if not keep_merged:
        shutil.rmtree(merged_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
