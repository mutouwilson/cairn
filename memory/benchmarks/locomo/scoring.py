"""Scoring primitives for LoCoMo.

Pure functions; no I/O. SQuAD-style normalization + token F1, plus an
LLM-judge stub so it's obvious what's missing.
"""

import re
import string
from collections import Counter


# https://github.com/allenai/bi-att-flow/blob/master/squad/evaluate-v1.1.py
_ARTICLES = re.compile(r"\b(a|an|the)\b", re.UNICODE)
_PUNCT = set(string.punctuation)


def normalize(text):
    """Lowercase, drop articles + punctuation, collapse whitespace."""
    if text is None:
        return ""
    text = str(text).lower()
    text = "".join(ch for ch in text if ch not in _PUNCT)
    text = _ARTICLES.sub(" ", text)
    text = " ".join(text.split())
    return text


def _tokens(text):
    return normalize(text).split()


def exact_match(pred, gold):
    """1 if normalized pred == normalized gold else 0."""
    return int(normalize(pred) == normalize(gold))


def f1_token(pred, gold):
    """SQuAD-style token-level F1."""
    pred_toks = _tokens(pred)
    gold_toks = _tokens(gold)
    if not pred_toks and not gold_toks:
        return 1.0
    if not pred_toks or not gold_toks:
        return 0.0
    common = Counter(pred_toks) & Counter(gold_toks)
    overlap = sum(common.values())
    if overlap == 0:
        return 0.0
    precision = overlap / len(pred_toks)
    recall = overlap / len(gold_toks)
    return 2 * precision * recall / (precision + recall)


def score_record(retrieved_texts, golden):
    """Best-of-retrieved scoring.

    `retrieved_texts` is a list of strings (entity names, note bodies, etc.).
    We score each candidate against the gold answer and return the best.
    LoCoMo's adversarial category expects "I don't know" — handled by the
    caller (treat empty retrieval as the correct answer there).
    """
    best_exact = 0
    best_f1 = 0.0
    best_idx = -1
    for i, cand in enumerate(retrieved_texts):
        em = exact_match(cand, golden)
        f1 = f1_token(cand, golden)
        if f1 > best_f1 or (f1 == best_f1 and em > best_exact):
            best_exact = em
            best_f1 = f1
            best_idx = i
    return {"exact": best_exact, "f1": best_f1, "best_retrieval_idx": best_idx}


def score_adversarial(retrieved_texts, golden):
    """LoCoMo adversarial questions: gold is typically a refusal phrase.

    Heuristic: if the retriever returned nothing relevant, that counts as a
    correct "I don't know". We approximate by giving full credit when the
    retrieval set is empty, and otherwise falling back to token-F1.
    """
    if not retrieved_texts:
        return {"exact": 1, "f1": 1.0, "best_retrieval_idx": -1}
    return score_record(retrieved_texts, golden)


def llm_judge(question, predicted, golden, model=None):
    """Placeholder for LLM-as-judge scoring.

    LoCoMo's reference eval feeds (question, gold, prediction) to an LLM and
    asks for a 0/1 judgment with paraphrase tolerance. We do NOT implement
    that here — this stub exists so callers fail loudly rather than silently
    returning bogus numbers.
    """
    raise NotImplementedError(
        "LLM-judge scoring is not implemented. "
        "Wire up an Anthropic/OpenAI client and follow the LoCoMo paper's "
        "Appendix B judge prompt before publishing any 'judge F1' numbers."
    )
