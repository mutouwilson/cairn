"""Personal-embedding fine-tune (Phase 3d).

Reads the ``retrieval_signals`` table out of the local Cairn SQLite DB,
turns it into contrastive (query, positive, negative) triples, and trains a
small **projection adapter** that sits on top of the base embedding model.

The adapter is a single residual linear layer::

    final_emb = normalize(base_emb + alpha * tanh(W @ base_emb + b))

so when no adapter is loaded the runtime degrades cleanly to the base model.
``alpha`` is initialised at 0 so the very first epoch behaves like the
identity and learning is gradual.

Loss = triplet margin loss on cosine similarity. Positives = clicked +
corrected entities (corrected count is doubled because it is a stronger
signal). Negatives = dismissed first, then in-result-but-unclicked as soft
negatives.

Output (``--output``)::

    adapter.safetensors           # W, b, alpha
    adapter.meta.json             # base_model id, dim, n_train, eval metrics

Runtime loading: see ``memory/src-tauri/src/embed/`` — adapter loading is
filed for Phase 3d-v2; this script ships the offline half today.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import pathlib
import sqlite3
from dataclasses import dataclass
from typing import Any

import click
import httpx
import orjson
from rich.console import Console
from tenacity import retry, retry_if_exception_type, stop_after_attempt, wait_exponential

log = logging.getLogger(__name__)
console = Console(stderr=True)

DEFAULT_GATEWAY = "https://ai-gateway.vercel.sh/v1"
DEFAULT_EMBED_MODEL = "openai/text-embedding-3-small"
DEFAULT_EMBED_DIM = 1024


@dataclass
class Signal:
    id: str
    query: str
    types_filter: list[str] | None
    returned_ids: list[str]
    clicked_ids: list[str]
    dismissed_ids: list[str]
    corrected_ids: list[str]
    embedding_model: str | None


def load_signals(db_path: pathlib.Path) -> list[Signal]:
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    rows = conn.execute(
        "SELECT id, query_text, types_filter, returned_ids, clicked_ids, "
        "dismissed_ids, corrected_ids, embedding_model FROM retrieval_signals"
    ).fetchall()
    conn.close()

    def js(v: str | None, default: Any) -> Any:
        if not v:
            return default
        try:
            return orjson.loads(v)
        except orjson.JSONDecodeError:
            return default

    out: list[Signal] = []
    for r in rows:
        clicked = js(r[4], [])
        dismissed = js(r[5], [])
        corrected = js(r[6], [])
        if not clicked and not corrected and not dismissed:
            # nothing to learn from
            continue
        out.append(
            Signal(
                id=r[0],
                query=r[1],
                types_filter=js(r[2], None),
                returned_ids=js(r[3], []),
                clicked_ids=clicked,
                dismissed_ids=dismissed,
                corrected_ids=corrected,
                embedding_model=r[7],
            )
        )
    return out


def load_entity_text(db_path: pathlib.Path, entity_ids: set[str]) -> dict[str, str]:
    if not entity_ids:
        return {}
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    placeholders = ",".join("?" for _ in entity_ids)
    rows = conn.execute(
        f"SELECT id, type, name, properties FROM entities WHERE id IN ({placeholders})",
        tuple(entity_ids),
    ).fetchall()
    conn.close()
    return {r[0]: f"{r[1]}: {r[2]}\n{r[3]}" for r in rows}


class GatewayEmbedder:
    def __init__(self, model: str, dim: int) -> None:
        self.model = model
        self.dim = dim
        key = os.environ.get("AI_GATEWAY_API_KEY") or os.environ.get("AI_GATEWAY_TOKEN")
        if not key:
            raise click.ClickException(
                "AI_GATEWAY_API_KEY (or AI_GATEWAY_TOKEN) is required for embedding"
            )
        self.client = httpx.AsyncClient(
            base_url=os.environ.get("AI_GATEWAY_BASE_URL", DEFAULT_GATEWAY),
            timeout=httpx.Timeout(60.0, connect=10.0),
            headers={"Authorization": f"Bearer {key}"},
            http2=True,
        )

    async def aclose(self) -> None:
        await self.client.aclose()

    @retry(
        retry=retry_if_exception_type((httpx.HTTPError, asyncio.TimeoutError)),
        stop=stop_after_attempt(4),
        wait=wait_exponential(multiplier=0.5, min=0.5, max=8.0),
    )
    async def embed(self, inputs: list[str]) -> list[list[float]]:
        if not inputs:
            return []
        r = await self.client.post(
            "/embeddings",
            json={
                "model": self.model,
                "input": inputs,
                "dimensions": self.dim,
                "encoding_format": "float",
            },
        )
        r.raise_for_status()
        data = r.json()["data"]
        return [item["embedding"] for item in data]


async def build_pairs(
    signals: list[Signal],
    db_path: pathlib.Path,
    embedder: GatewayEmbedder,
) -> tuple[list[list[float]], list[list[float]], list[list[float]], list[float]]:
    # Resolve all entity texts in one DB scan.
    all_ids: set[str] = set()
    for s in signals:
        for ids in (s.returned_ids, s.clicked_ids, s.dismissed_ids, s.corrected_ids):
            for x in ids:
                all_ids.add(x)
    texts = load_entity_text(db_path, all_ids)

    queries: list[str] = []
    positives: list[str] = []
    negatives: list[str] = []
    weights: list[float] = []

    for s in signals:
        pos_ids: list[tuple[str, float]] = []
        for eid in s.clicked_ids:
            pos_ids.append((eid, 1.0))
        for eid in s.corrected_ids:
            pos_ids.append((eid, 2.0)) # corrected is a stronger signal
        if not pos_ids:
            continue
        neg_ids = list(s.dismissed_ids)
        # If no explicit negatives, fall back to returned-but-unclicked as soft negs.
        if not neg_ids:
            taken = set(s.clicked_ids) | set(s.corrected_ids)
            neg_ids = [x for x in s.returned_ids if x not in taken]
        for pid, w in pos_ids:
            for nid in neg_ids:
                if pid not in texts or nid not in texts:
                    continue
                queries.append(s.query)
                positives.append(texts[pid])
                negatives.append(texts[nid])
                weights.append(w)

    if not queries:
        return [], [], [], []

    console.print(f"[dim]embedding {len(queries)} (q, p, n) triples...[/dim]")
    # Batch through embedder in chunks of 96 to stay under provider limits.
    q_emb = await embed_in_batches(embedder, queries)
    p_emb = await embed_in_batches(embedder, positives)
    n_emb = await embed_in_batches(embedder, negatives)
    return q_emb, p_emb, n_emb, weights


async def embed_in_batches(embedder: GatewayEmbedder, inputs: list[str], batch: int = 96):
    out: list[list[float]] = []
    for i in range(0, len(inputs), batch):
        out.extend(await embedder.embed(inputs[i : i + batch]))
    return out


def train_adapter(
    q_emb: list[list[float]],
    p_emb: list[list[float]],
    n_emb: list[list[float]],
    weights: list[float],
    *,
    dim: int,
    epochs: int,
    lr: float,
    margin: float,
    output_dir: pathlib.Path,
    base_model: str,
) -> dict[str, Any]:
    import numpy as np
    import torch
    from safetensors.torch import save_file
    from torch import nn

    q = torch.tensor(np.array(q_emb), dtype=torch.float32)
    p = torch.tensor(np.array(p_emb), dtype=torch.float32)
    n = torch.tensor(np.array(n_emb), dtype=torch.float32)
    w = torch.tensor(weights, dtype=torch.float32)

    class Adapter(nn.Module):
        def __init__(self, d: int) -> None:
            super().__init__()
            self.linear = nn.Linear(d, d, bias=True)
            self.alpha = nn.Parameter(torch.zeros(1))
            # Zero-init weight so initial behaviour is identity.
            nn.init.zeros_(self.linear.weight)
            nn.init.zeros_(self.linear.bias)

        def forward(self, x: torch.Tensor) -> torch.Tensor:
            delta = torch.tanh(self.linear(x))
            return torch.nn.functional.normalize(x + self.alpha * delta, dim=-1)

    model = Adapter(dim)
    opt = torch.optim.AdamW(model.parameters(), lr=lr)
    triplet = nn.TripletMarginWithDistanceLoss(
        distance_function=lambda a, b: 1.0 - torch.nn.functional.cosine_similarity(a, b),
        margin=margin,
    )

    losses: list[float] = []
    for epoch in range(epochs):
        opt.zero_grad()
        qa = model(q)
        pa = model(p)
        na = model(n)
        loss_each = triplet(qa, pa, na)
        # PyTorch returns scalar by default; for weighted we need reduction='none'.
        loss_each = (
            torch.relu(
                (1.0 - torch.nn.functional.cosine_similarity(qa, pa))
                - (1.0 - torch.nn.functional.cosine_similarity(qa, na))
                + margin
            )
        )
        loss = (loss_each * w).mean()
        loss.backward()
        opt.step()
        losses.append(loss.item())
        if epoch % max(1, epochs // 10) == 0:
            console.print(f"  epoch {epoch:>3}: loss={loss.item():.4f}")

    # Eval: triplet accuracy = fraction where cos(qa, pa) > cos(qa, na).
    with torch.no_grad():
        qa = model(q)
        pa = model(p)
        na = model(n)
        cp = torch.nn.functional.cosine_similarity(qa, pa)
        cn = torch.nn.functional.cosine_similarity(qa, na)
        triplet_acc = (cp > cn).float().mean().item()

    output_dir.mkdir(parents=True, exist_ok=True)
    save_file(
        {
            "linear.weight": model.linear.weight.detach(),
            "linear.bias": model.linear.bias.detach(),
            "alpha": model.alpha.detach(),
        },
        str(output_dir / "adapter.safetensors"),
    )
    meta = {
        "base_model": base_model,
        "dim": dim,
        "n_pairs": len(q_emb),
        "epochs": epochs,
        "lr": lr,
        "margin": margin,
        "loss_final": losses[-1] if losses else None,
        "triplet_accuracy": triplet_acc,
    }
    (output_dir / "adapter.meta.json").write_text(json.dumps(meta, indent=2))
    return meta


@click.command()
@click.option(
    "--db",
    "db_path",
    type=click.Path(exists=True, dir_okay=False, path_type=pathlib.Path),
    default=pathlib.Path.home() / "Library" / "Application Support" / "Cairn" / "memory.db",
)
@click.option(
    "--output",
    "output_dir",
    type=click.Path(file_okay=False, path_type=pathlib.Path),
    default="runs/embed-adapter",
)
@click.option("--base-model", default=DEFAULT_EMBED_MODEL)
@click.option("--dim", default=DEFAULT_EMBED_DIM, type=int)
@click.option("--epochs", default=100, type=int)
@click.option("--lr", default=5e-4, type=float)
@click.option("--margin", default=0.1, type=float)
@click.option("--min-pairs", default=20, type=int, help="refuse to train below this many triples")
@click.option("--log-level", default="INFO")
def main(
    db_path: pathlib.Path,
    output_dir: pathlib.Path,
    base_model: str,
    dim: int,
    epochs: int,
    lr: float,
    margin: float,
    min_pairs: int,
    log_level: str,
) -> None:
    logging.basicConfig(level=log_level.upper(), format="%(asctime)s %(levelname)s %(message)s")
    signals = load_signals(db_path)
    console.print(f"[bold]signals with feedback:[/bold] {len(signals)}")
    if len(signals) == 0:
        raise click.ClickException("no usable signals — interact with search results first")

    async def run() -> dict[str, Any]:
        embedder = GatewayEmbedder(base_model, dim)
        q, p, n, w = await build_pairs(signals, db_path, embedder)
        await embedder.aclose()
        if len(q) < min_pairs:
            raise click.ClickException(
                f"too few triples ({len(q)} < min_pairs={min_pairs})"
            )
        return train_adapter(
            q,
            p,
            n,
            w,
            dim=dim,
            epochs=epochs,
            lr=lr,
            margin=margin,
            output_dir=output_dir,
            base_model=base_model,
        )

    meta = asyncio.run(run())
    console.print(f"[green]adapter saved → {output_dir}[/green]")
    console.print(f"  triplet_accuracy = {meta['triplet_accuracy']:.3f}")
    console.print(f"  meta: {output_dir}/adapter.meta.json")


if __name__ == "__main__":
    main()
