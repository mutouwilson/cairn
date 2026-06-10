"""Schema round-trip + validator tests."""

from __future__ import annotations

import pytest

from distill.schema import ENTITY_TYPES, ExtractionResult, validate_payload


def test_valid_payload_no_errors() -> None:
    payload = {
        "entities": [
            {"type": "Person", "name": "妈", "properties": {"relationship": "mother"}, "confidence": 0.99}
        ],
        "relations": [],
    }
    assert validate_payload(payload) == []


def test_unknown_entity_type_rejected() -> None:
    payload = {
        "entities": [
            {"type": "Vehicle", "name": "car", "confidence": 0.9},
        ],
        "relations": [],
    }
    errors = validate_payload(payload)
    assert any("enum" in e.lower() or "vehicle" in e.lower() for e in errors)


@pytest.mark.parametrize("t", ENTITY_TYPES)
def test_each_entity_type_round_trips(t: str) -> None:
    payload = {
        "entities": [{"type": t, "name": "x", "confidence": 0.9}],
        "relations": [],
    }
    assert validate_payload(payload) == []
    parsed = ExtractionResult.model_validate(payload)
    assert parsed.entities[0].type == t


def test_low_confidence_filtered_by_model() -> None:
    payload = {
        "entities": [{"type": "Person", "name": "x", "confidence": 0.3}],
        "relations": [],
    }
    parsed = ExtractionResult.model_validate(payload)
    assert parsed.entities == []


def test_relations_must_reference_entities() -> None:
    payload = {
        "entities": [{"type": "Person", "name": "a", "confidence": 0.9}],
        "relations": [{"from": "a", "to": "b", "type": "knows"}],
    }
    parsed = ExtractionResult.model_validate(payload)
    assert not parsed.relation_names_resolve()
