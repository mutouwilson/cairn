//! Discovers consolidation candidates from the episodic layer.
//!
//! Strategies (combined per run):
//!   1. **Person-centric**: each `Person` entity that has ≥3 other entities
//!      pointing at it via `relations` becomes a topic `person:<name>`.
//!   2. **Preference-domain-centric**: each unique `properties.domain` across
//!      Preferences with ≥3 entries becomes a topic `domain:<value>`.
//!   3. **Goal-centric**: each `Goal` entity with ≥2 related events/beliefs
//!      becomes a topic `goal:<name>`.
//!
//! Each candidate carries the supporting `evidence` entity ids — the
//! consolidation prompt receives those directly.

use crate::db::Db;
use crate::schema::Entity;
use anyhow::Result;
use serde_json::Value;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct TopicCandidate {
    pub key: String,   // e.g. "person:妈"
    pub title: String, // short UI label
    pub center_entity: Option<Entity>,
    pub evidence: Vec<Entity>, // supporting entities, including the center if any
}

pub async fn discover(db: &Db) -> Result<Vec<TopicCandidate>> {
    let mut out = Vec::new();
    out.extend(person_topics(db).await?);
    out.extend(preference_domain_topics(db).await?);
    out.extend(goal_topics(db).await?);
    Ok(out)
}

async fn person_topics(db: &Db) -> Result<Vec<TopicCandidate>> {
    // Find all Person entities, then for each count related entities.
    let people = db.list_entities(Some("Person"), 500).await?;
    let mut out = Vec::new();
    for person in people {
        // Gather entities related to this person (either direction).
        let related_ids: Vec<String> = sqlx::query(
            "SELECT DISTINCT CASE WHEN from_entity = ? THEN to_entity ELSE from_entity END AS other \
             FROM relations WHERE from_entity = ? OR to_entity = ?",
        )
        .bind(&person.id)
        .bind(&person.id)
        .bind(&person.id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|r| r.try_get::<String, _>("other").unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();

        if related_ids.len() < 3 {
            continue;
        }

        let mut evidence: Vec<Entity> = Vec::with_capacity(related_ids.len() + 1);
        evidence.push(person.clone());
        for id in &related_ids {
            if let Some(e) = db.get_entity(id).await? {
                evidence.push(e);
            }
        }
        out.push(TopicCandidate {
            key: format!("person:{}", person.name),
            title: format!("Notes about {}", person.name),
            center_entity: Some(person.clone()),
            evidence,
        });
    }
    Ok(out)
}

async fn preference_domain_topics(db: &Db) -> Result<Vec<TopicCandidate>> {
    let prefs = db.list_entities(Some("Preference"), 500).await?;
    let mut by_domain: std::collections::HashMap<String, Vec<Entity>> = Default::default();
    for p in prefs {
        let props: Value = serde_json::from_str(&p.properties).unwrap_or(Value::Null);
        let domain = props
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("misc")
            .to_string();
        by_domain.entry(domain).or_default().push(p);
    }
    let mut out = Vec::new();
    for (domain, evidence) in by_domain {
        if evidence.len() < 3 {
            continue;
        }
        out.push(TopicCandidate {
            key: format!("domain:{}", domain),
            title: format!("Preferences in {}", domain),
            center_entity: None,
            evidence,
        });
    }
    Ok(out)
}

async fn goal_topics(db: &Db) -> Result<Vec<TopicCandidate>> {
    let goals = db.list_entities(Some("Goal"), 500).await?;
    let mut out = Vec::new();
    for goal in goals {
        let related_ids: Vec<String> = sqlx::query(
            "SELECT DISTINCT CASE WHEN from_entity = ? THEN to_entity ELSE from_entity END AS other \
             FROM relations WHERE from_entity = ? OR to_entity = ?",
        )
        .bind(&goal.id)
        .bind(&goal.id)
        .bind(&goal.id)
        .fetch_all(db.pool())
        .await?
        .into_iter()
        .map(|r| r.try_get::<String, _>("other").unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();
        if related_ids.len() < 2 {
            continue;
        }
        let mut evidence = vec![goal.clone()];
        for id in &related_ids {
            if let Some(e) = db.get_entity(id).await? {
                evidence.push(e);
            }
        }
        out.push(TopicCandidate {
            key: format!("goal:{}", goal.name),
            title: format!("Progress on goal: {}", goal.name),
            center_entity: Some(goal),
            evidence,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ExtractedEntity, ExtractedRelation, ExtractionResult};
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn preference_domain_topic_emerges_at_three_entries() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();

        // Two preferences in "coffee" — should NOT emerge as a topic.
        seed_prefs(&db, &[("coffee", "拿铁糖少"), ("coffee", "拿铁不要太热")])
            .await
            .unwrap();
        let topics = discover(&db).await.unwrap();
        assert!(
            !topics.iter().any(|t| t.key == "domain:coffee"),
            "should not emerge with only 2 entries"
        );

        seed_prefs(&db, &[("coffee", "豆子要中烘")]).await.unwrap();
        let topics = discover(&db).await.unwrap();
        let coffee = topics
            .iter()
            .find(|t| t.key == "domain:coffee")
            .expect("coffee topic should emerge at 3 entries");
        assert_eq!(coffee.evidence.len(), 3);
    }

    #[tokio::test]
    async fn person_topic_requires_three_related_entities() {
        let dir = tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();

        let note_id = db.insert_note("seed", "test").await.unwrap();
        let extraction = ExtractionResult {
            entities: vec![
                ExtractedEntity {
                    r#type: "Person".into(),
                    name: "妈".into(),
                    properties: json!({}),
                    confidence: 0.99,
                },
                ExtractedEntity {
                    r#type: "Event".into(),
                    name: "膝盖疼".into(),
                    properties: json!({}),
                    confidence: 0.95,
                },
                ExtractedEntity {
                    r#type: "Belief".into(),
                    name: "妈拒绝就医".into(),
                    properties: json!({}),
                    confidence: 0.85,
                },
                ExtractedEntity {
                    r#type: "Event".into(),
                    name: "妈喜欢晨练".into(),
                    properties: json!({}),
                    confidence: 0.9,
                },
            ],
            relations: vec![
                ExtractedRelation {
                    from: "妈".into(),
                    to: "膝盖疼".into(),
                    r#type: "experienced".into(),
                },
                ExtractedRelation {
                    from: "妈".into(),
                    to: "妈拒绝就医".into(),
                    r#type: "holds_belief".into(),
                },
                ExtractedRelation {
                    from: "妈".into(),
                    to: "妈喜欢晨练".into(),
                    r#type: "preference_of".into(),
                },
            ],
        };
        db.save_extracted(&note_id, &extraction).await.unwrap();

        let topics = discover(&db).await.unwrap();
        let person_topic = topics
            .iter()
            .find(|t| t.key == "person:妈")
            .expect("person:妈 topic should emerge with 3 related entities");
        assert!(person_topic.evidence.len() >= 4);
        assert_eq!(person_topic.center_entity.as_ref().unwrap().name, "妈");
    }

    async fn seed_prefs(db: &Db, items: &[(&str, &str)]) -> anyhow::Result<()> {
        let note_id = db.insert_note("seed", "test").await?;
        let entities = items
            .iter()
            .map(|(domain, name)| ExtractedEntity {
                r#type: "Preference".into(),
                name: name.to_string(),
                properties: json!({ "domain": domain }),
                confidence: 0.95,
            })
            .collect();
        db.save_extracted(
            &note_id,
            &ExtractionResult {
                entities,
                relations: vec![],
            },
        )
        .await?;
        Ok(())
    }
}
