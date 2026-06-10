"""Generate synthetic (note, extraction) pairs via a teacher LLM.

Two-stage pipeline per record:

1. **Brainstorm** — small model writes one plausible personal note.
2. **Extract**    — teacher model (Claude Opus / Sonnet) returns the gold
   extraction via the same ``save_extraction`` tool the student will eventually
   learn.

Both calls go through the Vercel AI Gateway so we can swap teacher / brainstorm
models via env without code changes. Outputs are validated against the
JSON Schema and rejected on failure.

Resumable: each output JSONL line is keyed by a stable hash of the seed tuple,
so re-running with the same seed picks up where it left off.
"""

from __future__ import annotations

import asyncio
import hashlib
import logging
import os
import random
import time
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import click
import httpx
import orjson
from rich.console import Console
from tenacity import (
    retry,
    retry_if_exception_type,
    stop_after_attempt,
    wait_exponential,
)
from tqdm.asyncio import tqdm

from .prompts import SYNTH_BRAINSTORM_SYSTEM, TEACHER_SYSTEM
from .schema import EXTRACTION_SCHEMA, validate_payload
from .seeds import ALL_AXES

log = logging.getLogger(__name__)
console = Console(stderr=True)

DEFAULT_GATEWAY = "https://ai-gateway.vercel.sh/v1"
DEFAULT_BRAINSTORM_MODEL = "openai/gpt-5-mini"
DEFAULT_TEACHER_MODEL = "anthropic/claude-opus-4-7"
TOOL_NAME = "save_extraction"


def _seed_to_key(seed_tuple: tuple[str, ...]) -> str:
    h = hashlib.sha256("|".join(seed_tuple).encode("utf-8")).hexdigest()
    return h[:16]


def _sample_seed(rng: random.Random) -> tuple[str, ...]:
    return tuple(rng.choice(axis.values) for axis in ALL_AXES)


class GatewayClient:
    """Thin async client around the Vercel AI Gateway."""

    def __init__(self, api_key: str, base_url: str = DEFAULT_GATEWAY, concurrency: int = 8) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self._sem = asyncio.Semaphore(concurrency)
        self.client = httpx.AsyncClient(
            timeout=httpx.Timeout(60.0, connect=10.0),
            headers={"Authorization": f"Bearer {api_key}"},
            http2=True,
        )

    async def aclose(self) -> None:
        await self.client.aclose()

    @retry(
        retry=retry_if_exception_type((httpx.HTTPError, asyncio.TimeoutError)),
        stop=stop_after_attempt(4),
        wait=wait_exponential(multiplier=0.5, min=0.5, max=8.0),
    )
    async def chat(
        self,
        *,
        model: str,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        tool_choice: dict[str, Any] | str | None = None,
        max_tokens: int = 1024,
        temperature: float = 0.7,
    ) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        }
        if tools is not None:
            body["tools"] = tools
        if tool_choice is not None:
            body["tool_choice"] = tool_choice

        async with self._sem:
            r = await self.client.post(f"{self.base_url}/chat/completions", json=body)
            r.raise_for_status()
            return r.json()


async def brainstorm_note(client: GatewayClient, model: str, seed: tuple[str, ...]) -> str:
    """Ask the brainstorm model for one personal note matching the seed."""
    instructions = (
        "Axes for this generation (use them as soft guidance, do not echo them):\n"
        + "\n".join(
            f"- {axis.name}: {value}" for axis, value in zip(ALL_AXES, seed, strict=True)
        )
        + "\nProduce ONLY the note text (no preamble, no explanation)."
    )
    resp = await client.chat(
        model=model,
        messages=[
            {"role": "system", "content": SYNTH_BRAINSTORM_SYSTEM},
            {"role": "user", "content": instructions},
        ],
        max_tokens=300,
        temperature=0.95,
    )
    content = resp["choices"][0]["message"].get("content", "").strip()
    return content


async def teach_extract(
    client: GatewayClient, model: str, note: str
) -> dict[str, Any] | None:
    tools = [
        {
            "type": "function",
            "function": {
                "name": TOOL_NAME,
                "description": "Save extracted entities and relations from the user's note.",
                "parameters": EXTRACTION_SCHEMA,
            },
        }
    ]
    resp = await client.chat(
        model=model,
        messages=[
            {"role": "system", "content": TEACHER_SYSTEM},
            {"role": "user", "content": note},
        ],
        tools=tools,
        tool_choice={"type": "function", "function": {"name": TOOL_NAME}},
        max_tokens=2048,
        temperature=0.0,
    )
    msg = resp["choices"][0]["message"]
    tool_calls = msg.get("tool_calls") or []
    if not tool_calls:
        return None
    args = tool_calls[0]["function"]["arguments"]
    if isinstance(args, str):
        try:
            payload = orjson.loads(args)
        except orjson.JSONDecodeError:
            return None
    else:
        payload = args
    errors = validate_payload(payload)
    if errors:
        log.debug("rejected payload: %s", errors[:3])
        return None
    return payload


async def generate_one(
    client: GatewayClient,
    brainstorm_model: str,
    teacher_model: str,
    rng: random.Random,
) -> dict[str, Any] | None:
    seed = _sample_seed(rng)
    key = _seed_to_key(seed)
    try:
        note = await brainstorm_note(client, brainstorm_model, seed)
        if not note or len(note) > 2000:
            return None
        extraction = await teach_extract(client, teacher_model, note)
        if extraction is None:
            return None
        return {
            "id": key,
            "note": note,
            "extraction": extraction,
            "meta": {
                "seed": list(seed),
                "brainstorm_model": brainstorm_model,
                "teacher_model": teacher_model,
            },
        }
    except Exception as exc:  # pragma: no cover - network noise
        log.warning("generate_one failed for seed %s: %s", key, exc)
        return None


def _existing_ids(path: Path) -> set[str]:
    if not path.exists():
        return set()
    out: set[str] = set()
    with path.open("rb") as f:
        for line in f:
            if not line.strip():
                continue
            try:
                row = orjson.loads(line)
                if "id" in row:
                    out.add(row["id"])
            except orjson.JSONDecodeError:
                continue
    return out


async def _run(
    output: Path,
    n: int,
    parallel: int,
    seed: int,
    brainstorm_model: str,
    teacher_model: str,
) -> None:
    api_key = os.environ.get("AI_GATEWAY_API_KEY") or os.environ.get("AI_GATEWAY_TOKEN")
    if not api_key:
        raise click.ClickException("AI_GATEWAY_API_KEY (or AI_GATEWAY_TOKEN) must be set")

    output.parent.mkdir(parents=True, exist_ok=True)
    seen = _existing_ids(output)
    if seen:
        console.print(f"[dim]Resuming: skipping {len(seen)} already-written ids[/dim]")

    rng = random.Random(seed)
    client = GatewayClient(api_key, concurrency=parallel)

    async def worker() -> dict[str, Any] | None:
        attempts = 0
        while attempts < 5:
            attempts += 1
            row = await generate_one(client, brainstorm_model, teacher_model, rng)
            if row is None:
                continue
            if row["id"] in seen:
                continue
            seen.add(row["id"])
            return row
        return None

    pending = max(0, n - len(seen))
    if pending == 0:
        console.print(f"[green]Already have {len(seen)} rows in {output}[/green]")
        return

    started = time.time()
    with output.open("ab") as out_fh:
        tasks: set[asyncio.Task[dict[str, Any] | None]] = set()
        produced = 0
        pbar = tqdm(total=pending, desc="synth", smoothing=0.05)

        async def submit_one() -> None:
            tasks.add(asyncio.create_task(worker()))

        for _ in range(min(parallel, pending)):
            await submit_one()

        while produced < pending and tasks:
            done, _ = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for fut in done:
                tasks.discard(fut)
                row = fut.result()
                if row is not None:
                    out_fh.write(orjson.dumps(row))
                    out_fh.write(b"\n")
                    out_fh.flush()
                    produced += 1
                    pbar.update(1)
                if produced + len(tasks) < pending:
                    await submit_one()
        pbar.close()

    await client.aclose()
    elapsed = time.time() - started
    console.print(
        f"[green]wrote {produced} rows[/green] in {elapsed:.1f}s "
        f"→ {output} (total {len(seen)})"
    )


@click.command()
@click.option("--output", "-o", type=click.Path(dir_okay=False, path_type=Path), required=True)
@click.option("--n", type=int, default=20_000, help="target number of rows")
@click.option("--parallel", type=int, default=8, help="concurrent gateway requests")
@click.option("--seed", type=int, default=20260513)
@click.option("--brainstorm-model", default=DEFAULT_BRAINSTORM_MODEL)
@click.option("--teacher-model", default=DEFAULT_TEACHER_MODEL)
@click.option("--log-level", default="INFO")
def main(
    output: Path,
    n: int,
    parallel: int,
    seed: int,
    brainstorm_model: str,
    teacher_model: str,
    log_level: str,
) -> None:
    logging.basicConfig(level=log_level.upper(), format="%(asctime)s %(levelname)s %(message)s")
    asyncio.run(
        _run(
            output=output,
            n=n,
            parallel=parallel,
            seed=seed,
            brainstorm_model=brainstorm_model,
            teacher_model=teacher_model,
        )
    )


if __name__ == "__main__":
    main()
