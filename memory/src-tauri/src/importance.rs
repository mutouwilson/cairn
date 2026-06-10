//! Importance scoring inspired by the Ebbinghaus forgetting curve.
//!
//! Each entity tracks:
//!   - `strength` (`S`) — how durable the memory is.
//!   - `last_accessed` — when it was last touched.
//!   - `access_count` — how many times it has been reinforced.
//!   - `base_importance` — a type-dependent prior (Person > Event, etc.).
//!
//! Retention probability at time `t` since last access:
//!     R(t) = exp(-t / S)
//!
//! Reinforcement (when the entity is mentioned/queried again):
//!     S' = S + bonus(access_count)
//! with diminishing returns so repeated access doesn't blow `S` up.
//!
//! Importance for ranking:
//!     I = clamp01(base_importance + frequency_bonus) × R
//!
//! This is intentionally simple. Phase 2 plugs in a learned model that uses
//! emotional weight, recency-vs-importance trade-off, person-count, etc.

const DAY_MS: f64 = 1000.0 * 60.0 * 60.0 * 24.0;

/// Base importance for a freshly extracted entity, by type.
/// Higher = more likely to be returned without explicit query.
pub fn base_importance_for_type(entity_type: &str) -> f64 {
    match entity_type {
        "Person" => 0.80,
        "Belief" => 0.70,
        "Goal" => 0.75,
        "Preference" => 0.65,
        "Event" => 0.50,
        "Asset" => 0.40,
        "Skill" => 0.55,
        "Location" => 0.45,
        _ => 0.50,
    }
}

/// Reinforce the strength when an entity is accessed/re-mentioned.
/// Diminishing returns via 1 / log(1+n).
pub fn reinforce(strength: f64, access_count: u32) -> f64 {
    let bonus = 2.0 / (1.0 + (access_count as f64 + 1.0).ln());
    (strength + bonus).max(0.1)
}

/// Probability of retention given strength and elapsed time.
pub fn retention(strength: f64, last_accessed_ms: i64, now_ms: i64) -> f64 {
    let elapsed_ms = (now_ms - last_accessed_ms).max(0) as f64;
    let elapsed_days = elapsed_ms / DAY_MS;
    (-elapsed_days / strength.max(0.1)).exp()
}

/// Composite importance for ranking.
///
/// Inputs come from `entities` table columns.
pub fn importance(
    base_importance: f64,
    access_count: i64,
    strength: f64,
    last_accessed_ms: i64,
    now_ms: i64,
) -> f64 {
    let frequency = ((access_count as f64).max(1.0).log10()) / 5.0;
    let r = retention(strength, last_accessed_ms, now_ms);
    ((base_importance + frequency).clamp(0.0, 1.0)) * r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshly_accessed_high_retention() {
        let now = 1_000_000_000_000;
        let r = retention(1.0, now, now);
        assert!(r > 0.99);
    }

    #[test]
    fn week_old_low_strength_decays() {
        let now = 1_000_000_000_000;
        let week_ago = now - 7 * DAY_MS as i64;
        let r = retention(1.0, week_ago, now);
        assert!(r < 0.01); // 7 days with S=1 -> exp(-7) ~ 0.0009
    }

    #[test]
    fn reinforcement_diminishes() {
        let mut s = 1.0;
        let s1 = reinforce(s, 0);
        s = s1;
        let s2 = reinforce(s, 1);
        let s3 = reinforce(s2, 50);
        // Each step adds less.
        assert!((s2 - s1) > (s3 - s2));
    }

    #[test]
    fn person_more_important_than_asset() {
        assert!(base_importance_for_type("Person") > base_importance_for_type("Asset"));
    }

    #[test]
    fn importance_combines_factors() {
        let now = 1_000_000_000_000;
        let fresh_low = importance(0.5, 1, 1.0, now, now);
        let fresh_high = importance(0.8, 50, 5.0, now, now);
        assert!(fresh_high > fresh_low);
    }
}
