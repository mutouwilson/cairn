"""Seed concepts used by ``synth.py`` to diversify generated notes.

The synth pipeline samples one row from each axis to compose a prompt for the
brainstorm model. This guarantees coverage of entity types, languages, and
moods without relying on the model to self-diversify (which it does poorly).
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Axis:
    name: str
    values: tuple[str, ...]


LANGUAGE = Axis("language", ("zh", "en", "mixed-zh-en"))

DOMAIN = Axis(
    "domain",
    (
        "work / colleagues / meetings",
        "family / parents / siblings / kids",
        "health / body / sleep / exercise",
        "food / cooking / eating-out / preferences",
        "travel / commute / destinations",
        "money / spending / saving / subscriptions",
        "hobby / making / reading / music / games",
        "relationships / friends / partner / dating",
        "learning / studying / courses / skills",
        "tech / tools / setup / gripes",
    ),
)

MOOD = Axis(
    "mood",
    (
        "neutral observation",
        "mildly anxious",
        "frustrated",
        "happy / proud",
        "curious / exploring",
        "tired / venting",
        "decisive / planning",
        "regretful",
    ),
)

ENTITY_FOCUS = Axis(
    "entity_focus",
    (
        "primarily Person",
        "primarily Event",
        "primarily Preference",
        "primarily Belief",
        "primarily Goal",
        "mostly Skill",
        "mostly Asset",
        "mostly Location",
        "mix of Person + Event",
        "mix of Goal + Preference",
        "essentially empty — should extract 0 or 1 entities",
    ),
)

COMPLEXITY = Axis(
    "complexity",
    (
        "one sentence",
        "two sentences with two entities",
        "three sentences with relations",
        "longer paragraph with 4–6 entities",
    ),
)

ALL_AXES: tuple[Axis, ...] = (LANGUAGE, DOMAIN, MOOD, ENTITY_FOCUS, COMPLEXITY)
