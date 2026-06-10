//! Calendar ICS feed poll source (Phase 5c).
//!
//! Config JSON shape:
//!
//! ```json
//! { "url": "https://calendar.google.com/calendar/ical/.../basic.ics" }
//! ```
//!
//! The poller fetches the full ICS document over HTTPS (no auth — relies on
//! the URL being secret for non-public calendars), parses with `icalendar`,
//! and inserts one note per `VEVENT` we haven't already imported. Dedupe
//! key is the event UID.

use crate::capture::{CaptureSource, PollOutcome};
use crate::db::Db;
use anyhow::{anyhow, Context, Result};
use icalendar::{Calendar, Component, EventLike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcsConfig {
    pub url: String,
}

pub async fn poll_once(db: &Db, source: &CaptureSource) -> Result<PollOutcome> {
    let cfg: IcsConfig = serde_json::from_str(&source.config).context("parse ICS config")?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client.get(&cfg.url).send().await.context("GET ICS feed")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("ICS fetch HTTP {status}"));
    }
    let body = resp.text().await.context("decode ICS body")?;

    let cal: Calendar = body.parse().map_err(|e| anyhow!("parse ICS: {e}"))?;

    let mut captured = 0_u32;
    let mut skipped = 0_u32;

    for component in cal.components.iter() {
        let Some(event) = component.as_event() else {
            continue;
        };
        let uid = event.get_uid().map(|s| s.to_string());
        let Some(uid) = uid else { continue };

        if db.has_seen(&source.id, &uid).await? {
            skipped += 1;
            continue;
        }

        let summary = event.get_summary().unwrap_or("(no title)").trim();
        let start = event
            .get_start()
            .map(|s| format!("{s:?}"))
            .unwrap_or_default();
        let end = event
            .get_end()
            .map(|s| format!("{s:?}"))
            .unwrap_or_default();
        let location = event.get_location().unwrap_or("").trim();
        let description = event.get_description().unwrap_or("").trim();

        let note_text = format!(
            "[calendar] {summary}\nwhen: {start} → {end}\n{}{}",
            if !location.is_empty() {
                format!("where: {location}\n")
            } else {
                String::new()
            },
            if !description.is_empty() {
                format!("\n{}", description)
            } else {
                String::new()
            }
        );
        db.insert_note(&note_text, &format!("calendar:{uid}"))
            .await?;
        db.mark_seen(&source.id, &uid).await?;
        captured += 1;
    }

    Ok(PollOutcome {
        captured,
        seen_skipped: skipped,
    })
}
