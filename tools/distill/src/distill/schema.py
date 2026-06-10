"""Extraction schema mirroring ``memory/src-tauri/src/extract/prompt.rs``.

This is the single source of truth used by:

* ``synth.py`` — to validate teacher outputs before keeping them as labels.
* ``train.py`` — to verify the dataset before fine-tuning.
* ``eval.py``  — to grade student outputs.

Schema drift between the Rust side and this file is a real bug. Whenever
the Rust JSON Schema changes, this module must change in the same commit and
the dataset must be regenerated.
"""

from __future__ import annotations

from typing import Any, Literal

from jsonschema import Draft202012Validator
from pydantic import BaseModel, ConfigDict, Field, field_validator

ENTITY_TYPES: tuple[str, ...] = (
    "Person",
    "Event",
    "Preference",
    "Belief",
    "Goal",
    "Asset",
    "Skill",
    "Location",
)

EXTRACTION_SCHEMA: dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": ["entities", "relations"],
    "properties": {
        "entities": {
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": False,
                "required": ["type", "name", "confidence"],
                "properties": {
                    "type": {"type": "string", "enum": list(ENTITY_TYPES)},
                    "name": {"type": "string", "minLength": 1, "maxLength": 200},
                    "properties": {"type": "object", "additionalProperties": True},
                    "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                },
            },
        },
        "relations": {
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": False,
                "required": ["from", "to", "type"],
                "properties": {
                    "from": {"type": "string", "minLength": 1},
                    "to": {"type": "string", "minLength": 1},
                    "type": {"type": "string", "minLength": 1},
                },
            },
        },
    },
}

VALIDATOR = Draft202012Validator(EXTRACTION_SCHEMA)


class ExtractedEntity(BaseModel):
    model_config = ConfigDict(extra="forbid")

    type: Literal["Person", "Event", "Preference", "Belief", "Goal", "Asset", "Skill", "Location"]
    name: str = Field(min_length=1, max_length=200)
    properties: dict[str, Any] = Field(default_factory=dict)
    confidence: float = Field(ge=0.0, le=1.0)


class ExtractedRelation(BaseModel):
    model_config = ConfigDict(extra="forbid")

    from_: str = Field(alias="from", min_length=1)
    to: str = Field(min_length=1)
    type: str = Field(min_length=1)


class ExtractionResult(BaseModel):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    entities: list[ExtractedEntity] = Field(default_factory=list)
    relations: list[ExtractedRelation] = Field(default_factory=list)

    @field_validator("entities")
    @classmethod
    def _drop_low_confidence(cls, v: list[ExtractedEntity]) -> list[ExtractedEntity]:
        return [e for e in v if e.confidence >= 0.6]

    def relation_names_resolve(self) -> bool:
        """Every relation endpoint references an entity in this result."""
        names = {e.name for e in self.entities}
        return all(r.from_ in names and r.to in names for r in self.relations)


def validate_payload(payload: dict[str, Any]) -> list[str]:
    """Return all schema-validation errors as strings; empty list = valid."""
    return [f"{e.message} at {list(e.path)}" for e in VALIDATOR.iter_errors(payload)]
